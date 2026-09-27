{-# LANGUAGE OverloadedStrings #-}
-- | §17 Compatibility: what a module may become.
--
-- @arkc check old.ark new.ark@. The log is permanent and every retained
-- entry is replayed by every whole-scope peer, so a module is a promise to
-- bytes already on disk, and a proposed module is held to the one that
-- made the promise. The answer is a list of 'Break's; an empty list is
-- what lets a build through ('isAdditive').
--
-- __The rule is additive-only__, and it is additive-only for a reason
-- rather than by taste. An entry carries an intent, not a fact: a function
-- by hash, arguments by name, and the autos its peer drew. Nothing in it
-- can be read without the vocabulary it was written in. Adding to that
-- vocabulary costs the log nothing — a table nobody wrote to is empty at
-- every retained sequence, a nullable column reads as @None@ on every row
-- that predates it, a new function has no entries yet, an optional argument
-- is absent from every old intent and means @None@ there. Taking from it
-- always strands somebody: a mutator's name is what a peer that has not
-- updated still authors, and what its pending intents carry when it comes
-- back, so a name is a public promise once any entry may have named it; a
-- non-optional argument added to one is a value every existing caller and
-- every pending intent lacks; a column removed is a field a retained body
-- reads, and a row every retained entry wrote. So a schema grows and never
-- shrinks, a column is /retired/ (docs/arkdb.md §3.2) rather than removed,
-- and a change to a live column is a rebuild from snapshot plus replay,
-- never a check that passes. Helpers and queries may go: neither is named
-- by an entry, and a helper a retained mutator still reaches travels with
-- it in its 'Closure', in the version that was current when the mutator
-- was hashed.
--
-- __A retained body is type-checked, not diffed.__ Petros held the same
-- rule with @log-compat@ and a @mutations.txt@: the surface a build
-- exposed, recorded as text, and the next build's surface compared against
-- it. That says what the log may /carry/, never what @apply@ will /do/ —
-- it recorded a verb no build could run for the two commits it existed,
-- and it could not see a schema change at all, because a column is not
-- part of a verb's signature. Here the entries name their function by hash
-- and the authority keeps every closure a retained entry names (§8.3,
-- §11), so the question is not whether the old text still appears in the
-- new text; it is whether each closure the log still needs /verifies/
-- against the schema it would now run over. 'checkRetained' asks
-- 'Ark.Verify.verifyFunction' exactly that, closure by closure, with the
-- new schema and the closure's own helpers — the same judgement that
-- admitted the function in the first place, made again against the
-- proposal. A column a retained body reads, a table it writes, a key whose
-- arity moved: each is a 'Complaint' the verifier already knows how to
-- make, reported under the hash the entries carry, so that what is refused
-- is named by the thing that would have been stranded.
--
-- 'check' is the half a text diff could do and the verifier cannot: the
-- schema, table by table, and every mutator's name, scope, arguments and
-- autos. Argument /order/ is not compared — arguments travel keyed by name
-- ('Ark.Log.Entry') — and an auto added to a mutator is not a break,
-- because autos are drawn by whichever runtime holds the function rather
-- than supplied by a caller, so there is nobody to strand. An id argument
-- that names a different table is an 'ArgRetyped', since 'TId' carries the
-- table: the bytes are the same sixteen either way, which is exactly why
-- it has to be caught here (Petros: @ArgRetabled@).
--
-- Both modules are assumed to verify on their own ('Ark.Verify.verify');
-- this module compares, it does not re-admit.
module Ark.Compat
  ( Break (..)
  , check
  , checkRetained
  , isAdditive
  ) where

import Data.List (find)
import Data.Maybe (mapMaybe)
import Data.Text (Text)

import Ark.Hash (Closure (..), FnHash, functionHash)
import Ark.IR
import Ark.Schema
import Ark.Value (FieldName, TableName)
import Ark.Verify (VerifyError, verifyFunction)

-- | A change the log cannot survive. Each names what it is about, because
-- the list is read by somebody who has just made the change and needs to
-- know which of their edits was the problem.
data Break
  = -- | A mutator present before and absent now (or no longer a mutator).
    -- Entries naming it may be pending on a peer that has not updated, and
    -- a peer that has cannot author it. Helpers and queries may go.
    FunctionRemoved Text
  | -- | Mutator, argument. Old intents carry the value; nothing takes it.
    ArgRemoved Text Text
  | -- | Mutator, argument, was, now. The bytes in the log were written as
    -- one thing and would be read as another; an id naming another table
    -- is this too.
    ArgRetyped Text Text Ty Ty
  | -- | Mutator, argument. A new argument that is not a 'TOption': every
    -- existing caller and every pending intent lacks it, and there is no
    -- value to read where it is absent. An optional one reads as @None@.
    ArgAdded Text Text
  | -- | Mutator, auto. Old entries froze a value nothing reads.
    AutoRemoved Text Text
  | -- | Mutator, auto. A 'Now' that became a 'NewId', or an id of another
    -- table: the frozen value would be read as something it is not.
    AutoChanged Text Text
  | -- | A mutator moved to another scope: its entries are in one log and
    -- would now be applied against another's tables.
    ScopeChanged Text
  | -- | A table gone. Every retained entry that wrote to it is stranded,
    -- and the state hash at every retained sequence moves.
    TableRemoved TableName
  | -- | A table in another scope now: its rows are in one log's snapshots
    -- and would be another's.
    TableMovedScope TableName
  | -- | Table, column. Retired, never removed: a retained body reads it and
    -- every retained row has it.
    ColumnRemoved TableName FieldName
  | -- | Table, column, was, now. Rows already written hold the old type.
    ColumnRetyped TableName FieldName Ty Ty
  | -- | Table, column. A nullable column made required refuses every
    -- retained row that holds @None@ there. The other direction is fine.
    ColumnMadeNonNullable TableName FieldName
  | -- | Table, column. A new column with no default: every retained put
    -- writes a full row without it, and every retained row lacks it. Add
    -- it nullable.
    ColumnAddedNonNullable TableName FieldName
  | -- | The key columns changed. Rows are stored, deleted and referenced by
    -- key; a retained delete names the old one.
    KeyChanged TableName
  | -- | Table, column. A reference added, removed or re-pointed on a column
    -- that existed: a new constraint may refuse rows retained entries
    -- wrote, and a dropped or moved one changes what those rows mean.
    RefChanged TableName FieldName
  | -- | Table, columns. A new unique index is a new refusal, and retained
    -- entries' rows may already collide under it. A non-unique index says
    -- nothing about meaning and may be added freely.
    UniqueAdded TableName [FieldName]
  | -- | A closure a retained entry names no longer verifies against the
    -- proposed schema, with the verifier's own complaints.
    RetainedBodyBroken FnHash [VerifyError]
  deriving (Eq, Show)

-- | §17.1 Every way the new module breaks the promise the old one made:
-- the schema first, then the mutators. Empty is additive.
check :: Module -> Module -> [Break]
check old new = schemaBreaks (modSchema old) (modSchema new) ++ functionBreaks old new

-- | @'null' . 'check'@.
isAdditive :: Module -> Module -> Bool
isAdditive old new = null (check old new)

-- | §17.2 Re-verify every retained closure against the new module's
-- schema. The closure is complete — its function and every helper it
-- reaches, in declaration order — so the module it is checked in is
-- those helpers followed by the function, and the function is checked at
-- its own index, which is what lets it call them. Only the function is
-- verified: a helper has no store access, so no schema can break one.
checkRetained :: Module -> [Closure] -> [Break]
checkRetained new = mapMaybe retained
  where
    retained c =
      let fns = cHelpers c ++ [cFn c]
          m = new {modFunctions = fns}
       in case verifyFunction m (length (cHelpers c)) (cFn c) of
            Right () -> Nothing
            Left es -> Just (RetainedBodyBroken (functionHash c) es)

-- Schema ------------------------------------------------------------------

schemaBreaks :: Schema -> Schema -> [Break]
schemaBreaks old new = concatMap perTable oldTables
  where
    oldTables = [(sName sc, t) | sc <- schScopes old, t <- sTables sc]
    perTable (scope, t) = case lookupTable new (tName t) of
      Nothing -> [TableRemoved (tName t)]
      Just t' ->
        [TableMovedScope (tName t) | tableScope new (tName t) /= Just scope]
          ++ concatMap (perColumn t t') (tColumns t)
          ++ [ ColumnAddedNonNullable (tName t) (colName c)
             | c <- tColumns t'
             , not (colNullable c)
             , colName c `notElem` map colName (tColumns t)
             ]
          ++ [KeyChanged (tName t) | tKey t /= tKey t']
          ++ [ UniqueAdded (tName t) (ixColumns ix)
             | ix <- tIndexes t'
             , ixUnique ix
             , ix `notElem` tIndexes t
             ]
    perColumn t t' c = case column t' (colName c) of
      Nothing -> [ColumnRemoved (tName t) (colName c)]
      Just c' ->
        [ColumnRetyped (tName t) (colName c) (colTy c) (colTy c') | colTy c /= colTy c']
          ++ [ColumnMadeNonNullable (tName t) (colName c) | colNullable c && not (colNullable c')]
          ++ [RefChanged (tName t) (colName c) | refOn t (colName c) /= refOn t' (colName c)]
    refOn t c = refTable <$> find ((== c) . refColumn) (tRefs t)

-- Functions --------------------------------------------------------------

functionBreaks :: Module -> Module -> [Break]
functionBreaks old new = concatMap perMutator [f | f <- modFunctions old, fnKind f == Mutator]
  where
    perMutator f = case lookupFunction new (fnName f) of
      Just f' | fnKind f' == Mutator -> same f f'
      _ -> [FunctionRemoved (fnName f)]
    same f f' =
      [ScopeChanged n | fnScope f /= fnScope f']
        ++ concatMap perArg (fnArgs f)
        ++ [ ArgAdded n a
           | (a, t) <- fnArgs f'
           , a `notElem` map fst (fnArgs f)
           , not (optional t)
           ]
        ++ concatMap perAuto (fnAutos f)
      where
        n = fnName f
        perArg (a, t) = case lookup a (fnArgs f') of
          Nothing -> [ArgRemoved n a]
          Just t' -> [ArgRetyped n a t t' | t /= t']
        perAuto (a, u) = case lookup a (fnAutos f') of
          Nothing -> [AutoRemoved n a]
          Just u' -> [AutoChanged n a | u /= u']
    optional (TOption _) = True
    optional _ = False
