{-# LANGUAGE OverloadedStrings #-}
-- | §4 The store.
--
-- What a mutator writes through and a query reads from, as a pure value.
-- A runtime's store is an ordered key-value backend with atomic batch
-- commit — SQLite's B-tree, LMDB, IndexedDB, a map — under an interface
-- of five operations ('get', 'scan', 'put', 'delete', and the commit a
-- batch of them makes). This module is the meaning of those operations;
-- it is not a description of a file format, and no SQL appears in it.
--
-- Two things a backend is never asked: what a constraint is, and what an
-- index is for. Not-null, uniqueness and references are enforced here, in
-- the generated code, identically everywhere, as deterministic refusals
-- (Petros leaned on SQLite's @foreign_keys=ON@ for the last of those; a
-- design that does not require SQLite cannot). And an index in this
-- program changes nothing: 'scan' filters and sorts, so a runtime that
-- seeks a declared index must give the same rows in the same order.
--
-- __The overlay.__ A peer's optimistic state is a store derived from its
-- confirmed store by applying its pending intents. Here that is simply a
-- second 'Store' value, because the structure is persistent; dropping it
-- is dropping a reference, and a rebase is recomputing it. A runtime
-- implements the same semantics as an in-memory overlay of
-- @(table, key) -> Maybe row@ over a durable base, consulted first on every
-- read; what it must reproduce is that reads see the overlay, that the
-- base is untouched until confirmation, and that dropping the overlay
-- reports no changes — which is why a view is told @Rebuilt@ after one.
module Ark.Store
  ( Row
  , Key
  , Store (..)
  , Change (..)
  , Refusal (..)
  , empty
  , get
  , exists
  , scan
  , put
  , delete
  , changeTable
  , applyChange
  , applyChanges
  , merge
  , rows
  , tableNames
  ) where

import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust)
import Data.Text (Text)
import qualified Data.Text as T

import Ark.Schema
import Ark.Value

-- | A row: every column of its table, by name. Never partial.
type Row = Map FieldName Value

-- | The key columns' values, in key order.
type Key = [Value]

-- | The rows of every table, each keyed by its key. Tables that have no
-- rows may be absent from the map; 'rows' treats them as empty.
data Store = Store
  { stSchema :: Schema
  , stTables :: Map TableName (Map Key Row)
  }
  deriving (Eq, Show)

-- | §4.1 What a write reports.
--
-- Every write says which row it touched and how, because that is the only
-- thing an incrementally maintained view can work from, and because it is
-- what the authority keeps beside each entry as the entry's /facts/
-- (§6). A statement that reported a row count would be no use for either.
data Change
  = Add TableName Row
  | Remove TableName Row
  | Edit TableName Row Row -- ^ old, new
  deriving (Eq, Show)

changeTable :: Change -> TableName
changeTable (Add t _) = t
changeTable (Remove t _) = t
changeTable (Edit t _ _) = t

-- | §4.2 A refusal is a verdict about the write, reached identically by
-- every replica; it travels in the same channel as a mutator's own
-- 'Ark.IR.SRefuse' and a mutator body propagates it. A refusal is never
-- a failure of the machine applying the entry.
data Refusal
  = NoSuchTable TableName
  | -- | The row is not a full, well-typed row of the table.
    MalformedRow TableName Text
  | NotNull TableName FieldName
  | UniqueViolation TableName [FieldName]
  | -- | A reference names a parent row that does not exist. 'VNull' in a
    -- nullable reference column references nothing and is allowed.
    MissingParent TableName FieldName TableName
  | -- | Deleting a row that other rows reference.
    StillReferenced TableName TableName
  | -- | An explicit 'Ark.IR.SRefuse' from a mutator, or a checked
    -- arithmetic fault; the text is the mutator's, or the fault's name.
    Refused Text
  deriving (Eq, Show)

empty :: Schema -> Store
empty sch = Store sch M.empty

rows :: Store -> TableName -> Map Key Row
rows st t = fromMaybe M.empty (M.lookup t (stTables st))

tableNames :: Store -> [TableName]
tableNames st = [tName t | sc <- schScopes (stSchema st), t <- sTables sc]

get :: Store -> TableName -> Key -> Maybe Row
get st t k = M.lookup k (rows st t)

exists :: Store -> TableName -> Key -> Bool
exists st t k = isJust (get st t k)

-- | Every row of a table, in key order under 'compareValue'. Filtering,
-- ordering and limiting are 'Ark.Eval.select''s business, so that the
-- store's one read is the simplest thing three backends can agree on.
scan :: Store -> TableName -> [Row]
scan st t = M.elems (rows st t)

-- | §4.3 Write a full row.
--
-- In order: the table must exist; the row must be exactly the table's
-- columns with values of their types ('wellTyped'); no non-nullable column
-- may be 'VNull'; every unique index must stay unique against every
-- /other/ row; every reference must find its parent. Then:
--
-- * a row whose key is new is an 'Add';
-- * a row equal to what is there is __no change at all__ — a rescan that
--   writes the same thousand rows reports nothing, and a maintained view
--   is not woken for a write that moved nothing;
-- * anything else is an 'Edit' carrying both versions.
put :: Store -> TableName -> Row -> Either Refusal (Store, Maybe Change)
put st tn row = do
  tbl <- maybe (Left (NoSuchTable tn)) Right (lookupTable (stSchema st) tn)
  wellTyped tbl row
  let k = keyOf tbl row
      here = rows st tn
      others = M.delete k here
  mapM_ (unique tbl others row) (filter ixUnique (tIndexes tbl))
  mapM_ (parentExists st tbl row) (tRefs tbl)
  let st' = st {stTables = M.insert tn (M.insert k row here) (stTables st)}
  pure $ case M.lookup k here of
    Nothing -> (st', Just (Add tn row))
    Just old
      | old == row -> (st, Nothing)
      | otherwise -> (st', Just (Edit tn old row))

-- | §4.4 Delete by key. A missing row is a no-op reporting nothing; a row
-- that another row still references is a refusal, because a dangling
-- reference is a row the schema said could not exist.
delete :: Store -> TableName -> Key -> Either Refusal (Store, Maybe Change)
delete st tn k = do
  tbl <- maybe (Left (NoSuchTable tn)) Right (lookupTable (stSchema st) tn)
  case get st tn k of
    Nothing -> pure (st, Nothing)
    Just row -> do
      mapM_ (noChild st tbl k) (childrenOf (stSchema st) tn)
      let st' = st {stTables = M.adjust (M.delete k) tn (stTables st)}
      pure (st', Just (Remove tn row))

-- A row is exactly the table's columns, each holding a value of the
-- column's type ('VNull' only where nullable).
wellTyped :: Table -> Row -> Either Refusal ()
wellTyped tbl row = do
  let want = map colName (tColumns tbl)
      have = M.keys row
  if M.keysSet row /= M.keysSet (M.fromList [(c, ()) | c <- want])
    then Left (MalformedRow (tName tbl) ("columns " <> T.pack (show have) <> " are not " <> T.pack (show want)))
    else mapM_ check (tColumns tbl)
  where
    check c = case M.lookup (colName c) row of
      Nothing -> Left (MalformedRow (tName tbl) (colName c))
      Just VNull
        | colNullable c -> Right ()
        | otherwise -> Left (NotNull (tName tbl) (colName c))
      Just v
        | ofType (colTy c) v -> Right ()
        | otherwise -> Left (MalformedRow (tName tbl) (colName c <> " has the wrong type"))

-- Whether a value inhabits a scalar type. Ids are untyped at run time, so
-- any sixteen bytes inhabit any 'TId'; the static checker holds a column
-- to the table it names.
ofType :: Ty -> Value -> Bool
ofType t v = case (t, v) of
  (TBool, VBool _) -> True
  (TInt, VInt _) -> True
  (TText, VText _) -> True
  (TBytes, VBytes _) -> True
  (TId _, VId _) -> True
  (TEnum vs, VText x) -> x `elem` vs
  (TOption _, VNull) -> True
  (TOption t', v') -> ofType t' v'
  _ -> False

unique :: Table -> Map Key Row -> Row -> Index -> Either Refusal ()
unique tbl others row ix
  | any clash (M.elems others) = Left (UniqueViolation (tName tbl) (ixColumns ix))
  | otherwise = Right ()
  where
    proj r = [M.lookup c r | c <- ixColumns ix]
    mine = proj row
    -- A NULL is not equal to anything, itself included, so two rows that
    -- are both NULL in a unique column do not clash. This is the one
    -- place the store follows SQL's reading of NULL, because it is also
    -- the only reading under which an optional unique column is usable.
    clash r = proj r == mine && all (maybe False (not . isNull)) mine

parentExists :: Store -> Table -> Row -> Ref -> Either Refusal ()
parentExists st tbl row r =
  case M.lookup (refColumn r) row of
    Just VNull -> Right ()
    Just v
      | exists st (refTable r) [v] -> Right ()
      | otherwise -> Left (MissingParent (tName tbl) (refColumn r) (refTable r))
    Nothing -> Left (MalformedRow (tName tbl) (refColumn r))

noChild :: Store -> Table -> Key -> Relation -> Either Refusal ()
noChild st tbl k rel =
  case k of
    [kv]
      | any (\r -> M.lookup (relColumn rel) r == Just kv) (scan st (relChild rel)) ->
          Left (StillReferenced (tName tbl) (relChild rel))
    _ -> Right ()

-- | §4.5 Apply a change as a fact.
--
-- A 'Change' an authority recorded is the effect an entry had, so applying
-- it needs no constraint check — the constraints held when the authority
-- applied the intent, and this is that same write arriving as a row.
-- This is the second way to apply an entry (§6 of the design), and the
-- one a peer takes for an entry whose function it does not hold. It is
-- deliberately raw: a fact is not re-judged.
applyChange :: Store -> Change -> Store
applyChange st ch = case ch of
  Add t row -> insert t row
  Edit t _ row -> insert t row
  Remove t row -> case lookupTable (stSchema st) t of
    Just tbl -> st {stTables = M.adjust (M.delete (keyOf tbl row)) t (stTables st)}
    Nothing -> st
  where
    insert t row = case lookupTable (stSchema st) t of
      Just tbl -> st {stTables = M.insertWith M.union t (M.singleton (keyOf tbl row) row) (stTables st)}
      Nothing -> st

applyChanges :: Store -> [Change] -> Store
applyChanges = foldl applyChange

-- | The rows of two stores over one schema, together. A peer holds one
-- store per scope it replicates, each with rows only in that scope's
-- tables; a query that reads across scopes reads their merge. Where both
-- hold a table, the left one wins, which never arises between scopes.
merge :: Store -> Store -> Store
merge a b = a {stTables = M.unionWith M.union (stTables a) (stTables b)}
