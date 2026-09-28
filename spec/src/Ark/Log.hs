{-# LANGUAGE OverloadedStrings #-}
-- | §10 The log.
--
-- The module's history: an append-only sequence of entries, each an intent
-- with what applying it changed kept beside it, standing on a snapshot.
--
-- __An entry is an intent.__ It records who authored it, the function by
-- hash, the arguments, and the autos the originating peer drew. It is
-- what the authority sequences and what a whole peer replays; it is
-- the only thing that can be adopted, verified and rebased later, which is
-- why it and not the rows is the log.
--
-- __The facts are kept beside it.__ 'Facts' are the 'Change's the
-- authority's own application produced. They are derived, never
-- authoritative over the intent, and they are what a peer takes for an
-- entry it cannot replay (an older build meeting a new function) — ending
-- in the same state, because the facts are what exact replay produced.
-- They are also what lets a snapshot be taken at any retained sequence
-- without any function at all ('stateAt').
--
-- __The horizon.__ A log stands on a 'Snapshot' at some sequence and keeps
-- the entries above it. 'compactTo' moves the snapshot up and drops what
-- is under it — entries, facts, and with them the claim on any function
-- version only they named. A peer whose cursor is below the horizon
-- restarts from the snapshot. Entry ids are kept below the horizon too
-- ('lIds'), because a re-pushed intent older than the horizon must still be
-- recognised rather than sequenced twice; sixteen bytes an entry is the
-- cheapest insurance in the system.
module Ark.Log
  ( Seq
  , Entry (..)
  , Facts
  , Snapshot (..)
  , snapshotOf
  , Log (..)
  , emptyLog
  , headSeq
  , horizon
  , append
  , seqOf
  , Page (..)
  , entriesAfter
  , stateAt
  , compactTo
  , namedHashes
  , contiguous
  ) where

import qualified Data.ByteString as B
import Data.Int (Int64)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Set (Set)
import qualified Data.Set as Set
import Data.Text (Text)

import Ark.Eval (Args)
import Ark.Hash (FnHash, stateHash)
import Ark.Schema (Schema)
import Ark.Store (Change, Store)
import qualified Ark.Store as S
import Ark.Value

-- | A position in the log. The first entry is 1; 0 is "nothing".
type Seq = Int64

-- | An intent, as recorded. The sequence is not a field: it is the key an
-- entry is stored under once the authority has assigned it, and an entry
-- has none before that.
data Entry = Entry
  { eId :: IdBytes -- ^ chosen by the originating peer; the dedupe key
  , eActor :: Text -- ^ the user the authority verified for the connection
  , eSession :: Text -- ^ the login it was authored under
  , eFn :: FnHash -- ^ the closure that authored it (§8.3)
  , eArgs :: Args
  , eAutos :: Args
  }
  deriving (Eq, Show)

-- | What applying an entry changed, in order.
type Facts = [Change]

-- | The state at a sequence, and its hash. Verifiable: any
-- peer holding the entries up to 'snSeq' can reproduce 'snHash', and an
-- authority adopting a log does exactly that.
data Snapshot = Snapshot
  { snSeq :: Seq
  , snStore :: Store
  , snHash :: B.ByteString
  }
  deriving (Eq, Show)

snapshotOf :: Seq -> Store -> Snapshot
snapshotOf n st = Snapshot n st (stateHash st)

data Log = Log
  { lBase :: Snapshot
  , lEntries :: Map Seq (Entry, Facts) -- ^ every sequence above 'lBase', contiguous
  , lIds :: Map IdBytes Seq -- ^ every entry id ever sequenced, kept below the horizon too
  }
  deriving (Eq, Show)

emptyLog :: Schema -> Log
emptyLog sch = Log (snapshotOf 0 (S.empty sch)) M.empty M.empty

-- | The last sequence assigned.
headSeq :: Log -> Seq
headSeq l = maybe (snSeq (lBase l)) (fst . fst) (M.maxViewWithKey (lEntries l))

-- | The sequence the log stands on: entries at or below it are gone.
horizon :: Log -> Seq
horizon = snSeq . lBase

-- | §10.1 Append an entry the authority has applied, with what it changed.
-- The sequence is @head + 1@ and nothing else ever assigns one. The caller
-- has already checked 'seqOf' for a duplicate.
append :: Log -> Entry -> Facts -> (Log, Seq)
append l e facts = (l', n)
  where
    n = headSeq l + 1
    l' = l {lEntries = M.insert n (e, facts) (lEntries l), lIds = M.insert (eId e) n (lIds l)}

-- | The sequence an entry id was given, if it ever was. A re-pushed
-- intent is answered with this rather than sequenced again.
seqOf :: Log -> IdBytes -> Maybe Seq
seqOf l i = M.lookup i (lIds l)

-- | What a peer at a cursor is sent next.
data Page
  = -- | The entries after the cursor, at most a batch, and whether more
    -- follow.
    Entries [(Seq, Entry, Facts)] Bool
  | -- | The cursor is below the horizon: start again from the snapshot.
    BelowHorizon Snapshot
  deriving (Eq, Show)

entriesAfter :: Log -> Seq -> Int -> Page
entriesAfter l cursor limit
  | cursor < horizon l = BelowHorizon (lBase l)
  | otherwise =
      let after = [(s, e, f) | (s, (e, f)) <- M.toAscList (snd (M.split cursor (lEntries l)))]
       in Entries (take limit after) (length after > limit)

-- | §10.2 The state at any retained sequence, from the snapshot and the
-- facts alone — no function is run. This is what makes compaction a
-- property of the log rather than of the authority's current store, and
-- what a facts-mode peer computes when it catches up.
stateAt :: Log -> Seq -> Maybe Store
stateAt l n
  | n < horizon l || n > headSeq l = Nothing
  | otherwise = Just (foldl S.applyChanges (snStore (lBase l)) [f | (s, (_, f)) <- M.toAscList (lEntries l), s <= n])

-- | §10.3 Move the horizon up to a sequence: snapshot the state there and
-- drop everything at or under it. Ids are kept.
compactTo :: Log -> Seq -> Maybe Log
compactTo l n = do
  st <- stateAt l n
  pure l {lBase = snapshotOf n st, lEntries = snd (M.split n (lEntries l))}

-- | The function hashes the retained entries name: the versions an
-- authority must keep closures for, and a generator must still emit.
namedHashes :: Log -> Set FnHash
namedHashes l = Set.fromList [eFn e | (e, _) <- M.elems (lEntries l)]

-- | Whether the entries run without a gap from the sequence after the
-- snapshot to the head. True of every log this module builds; checked on
-- one that arrives from elsewhere.
contiguous :: Log -> Bool
contiguous l = M.keys (lEntries l) == [horizon l + 1 .. headSeq l]
