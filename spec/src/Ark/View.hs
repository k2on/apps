{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §13 Incremental views.
--
-- A view is a plan kept up to date. 'hydrate' pulls the answer
-- 'Ark.Eval.select' gives; 'push' is told each 'Change' the store made and
-- moves that answer to what 'select' would give now, reporting what it did
-- to its own list as positions ('Patch'). The contract is exactly that, and
-- it is what the vectors hold ('contract'): after any sequence of changes
-- @rows view@ equals a fresh 'hydrate', and splicing the patches into the
-- old list ('splice') gives the new one. A runtime may maintain a view
-- however it likes — Petros composes source, filter, join and take as a
-- pipeline — and must produce these rows and these patches.
--
-- The design is Zero's (@rocicorp/mono@, @zql@), read through
-- @petros-ivm@; @docs/ivm.md@ in Petros has the notes. Three ideas from it
-- are load-bearing here:
--
-- * __Push and pull.__ An operator is told what changed /and/ may ask the
--   store for more. That is what makes @ORDER BY … LIMIT@ tractable: a
--   take keeps a bound — the last row it admitted — and when a removal
--   opens a gap it pulls the next admitted row beyond the bound, in O(1)
--   state and one seek. Here the bound is read off the list (its last row)
--   and the seek is a filter over 'Ark.Store.scan', because the spec store
--   has one read; a runtime seeks an index for @(order, key) > bound@ and
--   must find the same row ('refill', §13.5).
-- * __Results are trees.__ A node is a row plus one field per relationship
--   holding the child nodes, exactly as 'Ark.Eval.attach' builds it, and a
--   change in a child table is an /update of its parent node/ rather than
--   a remove and an add (§13.6): hearting a song moves no song, and a
--   library view still has to move without flickering.
-- * __The store is already at the new state__ when a change arrives. So a
--   pull can see rows the push has not reported yet, and an edit that
--   lands at the bound has to ask the store whether a row the bound was
--   hiding now comes first. Six hand-written tests passed with that wrong
--   in Petros and a random session found it at step 26; the case is
--   'edit' in §13.5.
--
-- What is deliberately not here. A filter's right-hand sides are
-- expressions that may name a query's arguments, so a view takes the plan
-- with them already evaluated ('ViewPlan', from 'evalPlan'). A rebase rolls
-- the optimistic store back and a rollback reports nothing, so a view told
-- 'Ark.Peer.Rebuilt' does not patch — it hydrates again ('rebuild'). And a
-- maintained count is the length of a view, so there is no separate tally.
module Ark.View
  ( -- * The plan a view maintains
    Filter (..)
  , ViewPlan (..)
  , evalPlan
  , admits
  , compareRows

    -- * The view
  , View (..)
  , hydrate
  , rebuild
  , rows

    -- * Patches
  , Patch (..)
  , splice
  , push

    -- * The contract
  , contract
  ) where

import Data.List (findIndex, foldl', sortBy)
import qualified Data.Map.Strict as M
import qualified Data.Set as Set

import Ark.IR (CmpOp (..), Expr, Plan (..), Pred (..), Related (..))
import Ark.Schema
import Ark.Store (Change, Row, Store, changeTable)
import qualified Ark.Store as S
import Ark.Value

-- ---------------------------------------------------------------------
-- §13.1 The plan a view maintains

-- | A filter with its right-hand sides evaluated. 'Ark.IR.Pred' carries
-- expressions, which may name a query's arguments and are evaluated once
-- before the scan ('Ark.Eval.predicate'); a view lives longer than the
-- call that opened it, so it holds the values and never the expressions.
-- One constructor per 'Pred' constructor, and 'admits' is 'predicate' over
-- the values.
data Filter
  = FCmp FieldName CmpOp Value
  | FIn FieldName [Value]
  | FAll [Filter]
  | FAny [Filter]
  | FNot Filter
  deriving (Eq, Show)

-- | 'Ark.IR.Plan' with every right-hand side evaluated, to any depth. The
-- four clauses are Zero's four operators — a source, a filter, a take and
-- a join per relationship — as data rather than as types, because the
-- spec has no cost to model and one shape to agree on.
data ViewPlan = ViewPlan
  { vpTable :: TableName
  , vpFilter :: Maybe Filter
  , -- | The verifier has already made this total by appending the key
    -- columns ascending; 'compareRows' appends them again, which is
    -- harmless when they are there and what makes the order total when a
    -- caller hands the view a plan the verifier never saw.
    vpOrder :: [(FieldName, Dir)]
  , vpLimit :: Maybe Int
  , -- | Each relationship read beneath a row: the field it appears as, the
    -- relationship, and the child plan (whose limit is per parent).
    vpRelated :: [(FieldName, Relation, ViewPlan)]
  }
  deriving (Eq, Show)

-- | Resolve a plan with an evaluator for its right-hand sides. The caller
-- supplies the evaluation — a query's arguments are its business — and
-- the first failure is the answer, in the plan's own order: the filter's
-- right-hand sides left to right and depth first, then each relationship's
-- child plan in turn. 'Ark.Eval.select' evaluates a child plan's
-- right-hand sides once per parent instead; a right-hand side cannot
-- mention the row, so the values are the same and only the first fault is
-- ever seen either way.
evalPlan :: (Expr -> Either e Value) -> Plan -> Either e ViewPlan
evalPlan ev p = do
  f <- traverse (evalPred ev) (pFilter p)
  rels <- mapM related (pRelated p)
  pure
    ViewPlan
      { vpTable = pTable p
      , vpFilter = f
      , vpOrder = pOrder p
      , vpLimit = pLimit p
      , vpRelated = rels
      }
  where
    related r = do
      child <- evalPlan ev (rPlan r)
      pure (rName r, rRelation r, child)

evalPred :: (Expr -> Either e Value) -> Pred -> Either e Filter
evalPred ev = \case
  PCmp c op e -> FCmp c op <$> ev e
  PIn c es -> FIn c <$> mapM ev es
  PAll ps -> FAll <$> mapM (evalPred ev) ps
  PAny ps -> FAny <$> mapM (evalPred ev) ps
  PNot q -> FNot <$> evalPred ev q

-- | Whether a row passes the filter; no filter admits every row. A column
-- the row lacks reads as 'VNull', as 'Ark.Eval.predicate' reads it.
admits :: Maybe Filter -> Row -> Bool
admits Nothing _ = True
admits (Just f) row = go f
  where
    field c = M.findWithDefault VNull c row
    go = \case
      FCmp c op v -> cmp op (field c) v
      FIn c vs -> any (cmp Eq (field c)) vs
      FAll fs -> all go fs
      FAny fs -> any go fs
      FNot g -> not (go g)

-- | Comparison under the one total order, so @NULL = NULL@ is true and
-- @NULL < 0@ is true. This __must equal__ 'Ark.Eval'@.cmp@, which is not
-- exported; it is the same six lines over 'compareValue', and a view that
-- disagreed with 'select' about one row would fail 'contract' on it.
cmp :: CmpOp -> Value -> Value -> Bool
cmp op a b = case op of
  Eq -> o == EQ
  Ne -> o /= EQ
  Lt -> o == LT
  Le -> o /= GT
  Gt -> o == GT
  Ge -> o /= LT
  where
    o = compareValue a b

-- | §13.2 The order a view keeps its rows in: the plan's order, then the
-- key ascending. 'select' sorts the scan stably by the plan's order alone,
-- and the scan is in key order, so ties fall in key order there; keys are
-- unique within a table, so this is the same order written as a total
-- comparison — which is what an insertion position and a bound need, and
-- what a runtime's index key @(order columns…, key columns…)@ is.
compareRows :: Table -> [(FieldName, Dir)] -> Row -> Row -> Ordering
compareRows tbl cols a b =
  mconcat [dir d (compareValue (field c a) (field c b)) | (c, d) <- cols]
    <> compare (keyOf tbl a) (keyOf tbl b)
  where
    field c = M.findWithDefault VNull c
    dir Asc o = o
    dir Desc o = case o of LT -> GT; GT -> LT; EQ -> EQ

-- ---------------------------------------------------------------------
-- §13.3 The view

-- | A maintained plan: the plan and its nodes, in the plan's order. Each
-- node is kept beside the row it was built over, because a node's fields
-- are the row's columns /and/ the relationships (a relationship named as a
-- column is named would shadow it, as in 'Ark.Eval.attach'), and 'push' finds a
-- row by its key.
--
-- This is also every piece of take state there is: Zero keeps @{size,
-- bound}@ per partition, and here the size is the list's length and the
-- bound is its last row. A child plan's take is per parent (§13.6), and a
-- child change recomputes the parent's list, so no bound is kept for one.
data View = View
  { vPlan :: ViewPlan
  , vNodes :: [(Row, Value)]
  }
  deriving (Eq, Show)

-- | Pull everything: what 'Ark.Eval.select' answers for the plan, as a
-- view. This is what a view does when it opens and when it is told
-- 'Ark.Peer.Rebuilt'.
hydrate :: Schema -> ViewPlan -> Store -> View
hydrate sch vp st = View vp (pull sch vp st)

-- | A rebase rolled the optimistic store back, and a rollback reports no
-- changes, so there is nothing to push: the view hydrates again against
-- the store as it now stands. It costs one query, in the one case where
-- the authority spoke while something of ours was pending.
rebuild :: Schema -> Store -> View -> View
rebuild sch st view = hydrate sch (vPlan view) st

-- | The current nodes, in order. Equal to what 'select' returns for the
-- plan against the store the view has been told about.
rows :: View -> [Value]
rows = map snd . vNodes

-- 'select', over an evaluated plan: scan, filter, sort, take, attach. A
-- table the schema lacks is a bug the verifier refuses ('UnknownTable' in
-- 'Ark.Eval'); here it is empty, so that a view is total.
pull :: Schema -> ViewPlan -> Store -> [(Row, Value)]
pull sch vp st = case lookupTable sch (vpTable vp) of
  Nothing -> []
  Just tbl ->
    let admitted = filter (admits (vpFilter vp)) (S.scan st (vpTable vp))
        ordered = sortBy (compareRows tbl (vpOrder vp)) admitted
        taken = maybe ordered (`take` ordered) (vpLimit vp)
     in [(row, nodeOf sch vp st tbl row) | row <- taken]

-- A node over a row: the row's columns, plus one field per relationship
-- holding the child nodes, as 'Ark.Eval.attach' builds it (the
-- relationship's field wins a name clash). A child plan is pulled with its
-- join column pinned to the parent's key and its own filter beneath, so
-- its limit is per parent; a runtime that pulls a page of parents' children
-- in one statement must still cut each parent's list to that limit.
nodeOf :: Schema -> ViewPlan -> Store -> Table -> Row -> Value
nodeOf sch vp st tbl row = VStruct (M.union (M.fromList fields) row)
  where
    pk = parentKey tbl row
    fields =
      [ (name, VList (map snd (pull sch (pinned rel child) st)))
      | (name, rel, child) <- vpRelated vp
      ]
    pinned rel child =
      let pin = FCmp (relColumn rel) Eq pk
       in child {vpFilter = Just (maybe pin (\f -> FAll [pin, f]) (vpFilter child))}

-- The value a child's join column holds for this parent. A relationship's
-- parent has a single-column key ('Ark.Schema.RefToCompositeKey' refuses
-- the other case), so the list case never arises for a verified module;
-- it is given a value rather than an error so that the view is total.
parentKey :: Table -> Row -> Value
parentKey tbl row = case keyOf tbl row of
  [k] -> k
  ks -> VList ks

-- ---------------------------------------------------------------------
-- §13.4 Patches

-- | What 'push' did to the view's list, as positions into the list /as it
-- stands when the patch is applied/, in order. The node travels with the
-- patch rather than being looked up afterwards, because by then the view
-- has applied the rest of them. 'Insert' puts the node before the element
-- at @at@ (so @at == length@ appends); 'Remove' and 'Update' name an
-- element that is there.
data Patch
  = Insert {at :: Int, node :: Value}
  | Remove {at :: Int}
  | Update {at :: Int, node :: Value}
  deriving (Eq, Show)

-- | What a patch means to whoever holds a copy of the list: apply each in
-- order. This is the definition a client's list is held to, and
-- @splice ps (rows before) == rows after@ is the second half of the
-- contract.
splice :: [Patch] -> [Value] -> [Value]
splice ps xs = foldl' step xs ps
  where
    step vs = \case
      Insert i v -> take i vs ++ v : drop i vs
      Remove i -> take i vs ++ drop (i + 1) vs
      Update i v -> take i vs ++ v : drop (i + 1) vs

-- | §13.5 A change arrives. The store is the store /after/ the change — a
-- peer applies a change to its store and then tells every view — so a
-- pull sees the new state. A change in the plan's own table moves the
-- list ('pushTop'); a change in a table read beneath it updates the parent
-- nodes it hangs under ('pushBelow'); both run when the table is both,
-- which a self-reference makes possible; any other table is nothing.
push :: Schema -> Store -> Change -> View -> (View, [Patch])
push sch st ch view = case lookupTable sch (vpTable vp) of
  Nothing -> (view, [])
  Just tbl ->
    let (v1, ps1)
          | changeTable ch == vpTable vp = pushTop sch st tbl ch view
          | otherwise = (view, [])
        (v2, ps2) = pushBelow sch st tbl ch v1
     in (v2, ps1 ++ ps2)
  where
    vp = vPlan view

-- A change in the plan's own table. A row is in the view or it is not,
-- and a new version is admitted or it is not; the four cases are an add,
-- a remove, an edit in place or across, and nothing.
--
-- Under a limit the view is a window, and the rows admitted but beyond
-- its last row (the bound) are not held. Two things follow. An add that
-- would land past a full window is not admitted and then evicted — it is
-- nothing, which a test that only compares rows cannot see and a test
-- that counts patches can. And whenever a row leaves a full window, or a
-- row moves to its end, the store is asked what comes next ('refill'):
-- that is the pull, and the one place a view reads the store on a change
-- to its own table.
pushTop :: Schema -> Store -> Table -> Change -> View -> (View, [Patch])
pushTop sch st tbl ch view = case ch of
  S.Add _ row -> add row
  S.Remove _ row -> maybe (view, []) removeAt (position row)
  S.Edit _ old new -> case (position old, keep new) of
    (Nothing, False) -> (view, [])
    (Nothing, True) -> add new
    (Just i, False) -> removeAt i
    (Just i, True) -> edit i new
  where
    vp = vPlan view
    nodes = vNodes view
    keep = admits (vpFilter vp)
    key = keyOf tbl
    order = compareRows tbl (vpOrder vp)
    -- Where a row of this key sits, if it is in the view. A runtime keeps
    -- a map from key to position or searches by the order; both answer
    -- this.
    position row = findIndex ((== key row) . key . fst) nodes
    -- Where a row belongs among nodes it is not in: before the first row
    -- that orders after it.
    insertPos row ns = length (takeWhile (\(r, _) -> order r row == LT) ns)
    build row = (row, nodeOf sch vp st tbl row)
    -- A window is full when it holds exactly its limit; only then can the
    -- store hold admitted rows the view does not.
    full = vpLimit vp == Just (length nodes)
    with ns = view {vNodes = ns}

    -- An admitted row goes in at its position; past a full window it is
    -- nothing; into a full window it pushes the last row out.
    add row
      | not (keep row) = (view, [])
      | Just lim <- vpLimit vp, j >= lim = (view, [])
      | otherwise = case vpLimit vp of
          Just lim | length ns > lim -> (with (take lim ns), [Insert j (snd n), Remove lim])
          _ -> (with ns, [Insert j (snd n)])
      where
        j = insertPos row nodes
        n = build row
        ns = insertAt j n nodes

    -- The row at @i@ leaves; a full window refills from the store, at the
    -- end, because everything admitted before the bound was already held.
    removeAt i =
      let ns = deleteAt i nodes
       in case (if full then refill ns else Nothing) of
            Just n -> (with (ns ++ [n]), [Remove i, Insert (length ns) (snd n)])
            Nothing -> (with ns, [Remove i])

    -- The row at @i@ is now @new@, and still admitted. Taken out, it
    -- belongs at @j@ among the rest; if that is where it was, the node is
    -- updated in place, otherwise it is removed and inserted. The one
    -- case that has to ask the store: a full window whose edited row now
    -- orders last. The bound was hiding rows, the store may hold one
    -- between the remaining rows and @new@, and the first admitted row
    -- beyond the remaining rows is the answer — @new@ itself, or a hidden
    -- row that takes its place while @new@ falls past the bound. This is
    -- the bug a random session found in Petros: remove-then-refill pulled
    -- the edited row in and then added it a second time.
    edit i new =
      let ns = deleteAt i nodes
          j = insertPos new ns
          n = build new
          hidden = if full && j == length ns then refill ns else Nothing
       in case hidden of
            Just h
              | key (fst h) /= key new -> (with (ns ++ [h]), [Remove i, Insert j (snd h)])
            _
              | j == i -> (with (insertAt j n ns), [Update i (snd n)])
              | otherwise -> (with (insertAt j n ns), [Remove i, Insert j (snd n)])

    -- The pull: the first admitted row beyond the bound, the bound being
    -- the last row the window still holds (none, and it is the first
    -- admitted row). Here that is a filter over the scan and a sort; a
    -- runtime seeks its index from the bound and reads one row, and must
    -- find the same one, which 'compareRows' being total guarantees.
    refill ns =
      let bound = fst <$> lastMaybe ns
          beyond r = maybe True (\b -> order b r == LT) bound
          candidates = [r | r <- S.scan st (vpTable vp), keep r, beyond r]
       in case sortBy order candidates of
            (r : _) -> Just (build r)
            [] -> Nothing

-- | §13.6 A change beneath the plan. A node's relationship fields are a
-- function of the store, so a change in a table read beneath the plan is
-- an update of every parent node it could have moved: the node is rebuilt
-- from the store and, if it differs, reported as an 'Update' at the
-- parent's position. A child row that the child plan does not admit, or a
-- child edit that its parent's list comes out the same under, moves
-- nothing and reports nothing — as a 'put' of an unchanged row does.
--
-- Which parents: for a table a /direct/ relationship reaches, those whose
-- key equals the changed row's join column (both the old and the new value
-- of an edit, since a child that moved between parents moves two lists);
-- a runtime finds each by one key lookup and rebuilds one list from the
-- join column's index. For a table reached only deeper — a grandchild or
-- beyond — every parent is rebuilt, because the join key that names the
-- affected parent is a row away in a table this module does not look up;
-- a runtime may narrow it by following the keys up, and must update the
-- same nodes. Positions do not move under an 'Update', so the patches are
-- in position order.
pushBelow :: Schema -> Store -> Table -> Change -> View -> (View, [Patch])
pushBelow sch st tbl ch view
  | null direct && not deeper = (view, [])
  | otherwise = (view {vNodes = ns}, concat pss)
  where
    t = changeTable ch
    vp = vPlan view
    direct = [relColumn rel | (_, rel, _) <- vpRelated vp, relChild rel == t]
    joins =
      Set.fromList
        [v | col <- direct, row <- changedRows ch, Just v <- [M.lookup col row]]
    descendants c =
      [vpTable g | (_, _, g) <- vpRelated c]
        ++ concatMap (\(_, _, g) -> descendants g) (vpRelated c)
    deeper = t `elem` concatMap (\(_, _, c) -> descendants c) (vpRelated vp)
    affected row = deeper || parentKey tbl row `Set.member` joins
    (ns, pss) = unzip (zipWith step [0 ..] (vNodes view))
    step i (row, old)
      | affected row && new /= old = ((row, new), [Update i new])
      | otherwise = ((row, old), [])
      where
        new = nodeOf sch vp st tbl row

-- The rows a change is about: both versions of an edit.
changedRows :: Change -> [Row]
changedRows = \case
  S.Add _ r -> [r]
  S.Remove _ r -> [r]
  S.Edit _ o n -> [o, n]

-- ---------------------------------------------------------------------
-- §13.7 The contract

-- | A maintained view is right when it is indistinguishable from one
-- hydrated now: the same rows, over the same raw rows, in the same order.
-- This is what every @view/@ vector asserts after every step of a session,
-- and what a runtime's own random session should assert too, because the
-- cases that matter (§13.5) are the ones a hand-written test does not
-- think to write.
contract :: Schema -> ViewPlan -> Store -> View -> Bool
contract sch vp st view = view == hydrate sch vp st

-- ---------------------------------------------------------------------
-- List helpers

insertAt :: Int -> a -> [a] -> [a]
insertAt i x xs = take i xs ++ x : drop i xs

deleteAt :: Int -> [a] -> [a]
deleteAt i xs = take i xs ++ drop (i + 1) xs

lastMaybe :: [a] -> Maybe a
lastMaybe [] = Nothing
lastMaybe xs = Just (last xs)
