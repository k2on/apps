{-# LANGUAGE OverloadedStrings #-}
-- | §11 The peer.
--
-- The log machine every peer runs, per scope, and the authority role a
-- peer takes for a scope it sequences. Both are pure: they take what
-- arrived and give back what follows, and a transport is a loop around
-- them. Nothing here knows what a socket is, which is what makes the
-- simulation ('Ark.Sim') and the vectors possible, and is Petros's own
-- shape for Petros's own reason.
--
-- __A replica__ ('Replica') holds one scope: a confirmed store at a
-- cursor, the intents it authored that no authority has answered yet, and
-- the optimistic store those intents produce on top of the confirmed one.
-- Its view is always @replay(confirmed) then replay(pending)@. Confirmed
-- state moves only forward; the only thing ever undone is the replica's
-- own pending, replayed on top when confirmed entries land. That is the
-- rebase, and it is the whole concurrency story.
--
-- __An authority__ ('Authority') sequences one scope: it applies each
-- pushed intent in order to its own store, records the facts, appends,
-- and answers with a verdict. "Server" is a peer doing this for others;
-- a peer alone does it for itself ('localCommit'), and is then a database
-- with one replica rather than a client in a mode.
--
-- __Two ways to apply an entry, one state.__ A replica that holds the
-- closure an entry names replays the intent; one that does not asks for
-- the entry's facts and applies those. When it holds both it replays and
-- compares, and a disagreement is recorded ('rDiverged') and resolved in
-- the authority's favour — the cure for divergence rather than its
-- silence.
module Ark.Peer
  ( -- * A replica
    Replica (..)
  , Inbox (..)
  , Changes (..)
  , open
  , mutate
  , receive
  , receiveFacts
  , receiveWith
  , ack
  , reject
  , needs
  , takeChanges
  , verifyAt

    -- * An authority
  , Authority (..)
  , authority
  , Sequenced (..)
  , sequenceEntry
  , page
  , compact
  , retire
  , AdoptError (..)
  , adopt

    -- * Both at once
  , localCommit
  ) where

import qualified Data.ByteString as B
import Data.List (foldl')
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (isNothing)
import Data.Set (Set)
import qualified Data.Set as Set
import qualified Data.Text as T

import Ark.Eval (Args, Ctx (..), applyClosure)
import Ark.Hash (Closure (..), FnHash, stateHash)
import Ark.Log
import Ark.Schema (Schema, ScopeName)
import Ark.Std (hexText)
import Ark.Store (Change, Refusal (..), Store)
import qualified Ark.Store as S
import Ark.Value

-- ---------------------------------------------------------------------
-- A replica

-- | One peer's copy of one scope.
data Replica = Replica
  { rScope :: ScopeName
  , rSchema :: Schema
  , -- | The closures this peer can run: its generated code's, plus any it
    -- was sent. Keyed by the hash an entry names.
    rBodies :: Map FnHash Closure
  , -- | The confirmed store: the scope exactly as the authority had it at
    -- 'rCursor'. Durable; moves only forward.
    rConfirmed :: Store
  , rCursor :: Seq
  , -- | Intents authored here that no verdict has answered, in authoring
    -- order. Durable (committed before 'mutate' reports success).
    rPending :: [Entry]
  , -- | The optimistic store: 'rConfirmed' with 'rPending' replayed. Never
    -- durable; recomputed on every rebase.
    rView :: Store
  , -- | Confirmed entries received and not yet applied: out of order, or
    -- waiting for facts.
    rInbox :: Map Seq Inbox
  , -- | Verdicts against this peer's own intents, newest last.
    rRejections :: [(IdBytes, Refusal)]
  , -- | Sequences at which this peer's replay disagreed with the
    -- authority's facts, or refused what the authority accepted. Empty on a
    -- conformant runtime; never empty silently.
    rDiverged :: [Seq]
  , -- | What has happened to 'rView' since 'takeChanges' last asked.
    rRebuilt :: Bool
  , rChanges :: [Change] -- ^ newest first
  }
  deriving (Eq, Show)

data Inbox = Inbox
  { ibEntry :: Maybe Entry
  , ibFacts :: Maybe Facts
  }
  deriving (Eq, Show)

-- | What a view is told: the changes to the optimistic store since it
-- last asked, or that it was rebuilt and must re-hydrate. A rollback
-- reports nothing, so no list of changes describes a rebase; 'Rebuilt'
-- says so instead of lying.
data Changes = Applied [Change] | Rebuilt
  deriving (Eq, Show)

-- | §11.1 Open a replica from what was durable: the confirmed store and
-- cursor, and the pending intents, which are replayed on top.
open :: Schema -> ScopeName -> Map FnHash Closure -> Store -> Seq -> [Entry] -> Replica
open sch scope bodies confirmed cursor pending =
  replay
    Replica
      { rScope = scope
      , rSchema = sch
      , rBodies = bodies
      , rConfirmed = confirmed
      , rCursor = cursor
      , rPending = pending
      , rView = confirmed
      , rInbox = M.empty
      , rRejections = []
      , rDiverged = []
      , rRebuilt = True
      , rChanges = []
      }

-- | §11.2 Author an intent: apply it forward into the optimistic store,
-- and if it is not refused, record it as pending. The autos were drawn by
-- the caller, once, and are frozen in the entry from here on. A refusal
-- changes nothing and records nothing — an intent this peer's own view
-- refuses is not worth a round trip.
mutate :: Replica -> IdBytes -> Ctx -> FnHash -> Args -> Args -> Either Refusal (Replica, Entry)
mutate r i ctx fh autos args = do
  c <- maybe (Left (Refused ("unknown function " <> hexText fh))) Right (M.lookup fh (rBodies r))
  case applyClosure (rSchema r) c ctx autos args (rView r) of
    Left bug -> Left (Refused ("bug: " <> T.pack (show bug)))
    Right (Left refusal) -> Left refusal
    Right (Right (view', chs)) ->
      let e = Entry i (ctxUser ctx) (ctxSession ctx) fh args autos
       in Right (r {rView = view', rPending = rPending r ++ [e], rChanges = reverse chs ++ rChanges r}, e)

-- | §11.3 A confirmed entry arrives, at its sequence. It waits in the
-- inbox until everything before it has been applied and it can be applied
-- itself, then 'advance' takes it.
receive :: Replica -> Seq -> Entry -> Replica
receive r n e
  | n <= rCursor r = r -- already applied: a duplicate delivery
  | otherwise = advance r {rInbox = M.insertWith merge n (Inbox (Just e) Nothing) (rInbox r)}
  where
    merge new old = old {ibEntry = ibEntry new}

-- | An entry and its facts arrive together, as a batch from an authority
-- delivers them. A replica that holds the closure replays and compares;
-- one that does not takes the facts. Delivering the entry first and the
-- facts later would apply by intent unchecked, which is why a batch is one
-- call.
receiveWith :: Replica -> Seq -> Entry -> Facts -> Replica
receiveWith r n e f
  | n <= rCursor r = r
  | otherwise = advance r {rInbox = M.insert n (Inbox (Just e) (Just f)) (rInbox r)}

-- | The facts of an entry arrive, at its sequence.
receiveFacts :: Replica -> Seq -> Facts -> Replica
receiveFacts r n f
  | n <= rCursor r = r
  | otherwise = advance r {rInbox = M.insertWith merge n (Inbox Nothing (Just f)) (rInbox r)}
  where
    merge new old = old {ibFacts = ibFacts new}

-- | §11.4 An acknowledgement: this peer's own intent was sequenced at
-- @n@. The entry is already in hand, so it goes to the inbox as if it had
-- arrived, and is applied at its turn — through the confirmed store, not
-- the view it was probed against, which is the rebase. It stays pending
-- until then, so a view drawn in between still shows it.
ack :: Replica -> IdBytes -> Seq -> Replica
ack r i n = case [e | e <- rPending r, eId e == i] of
  (e : _) -> receive r n e
  [] -> r

-- | §11.5 A verdict against this peer's own intent: it is dropped, the
-- verdict is kept for the app to show, and the view is rebuilt without it.
reject :: Replica -> IdBytes -> Refusal -> Replica
reject r i why =
  replay r {rPending = filter ((/= i) . eId) (rPending r), rRejections = rRejections r ++ [(i, why)]}

-- | The sequences the replica is waiting on facts for: the next entries
-- in order whose closures it does not hold, or which it could not apply
-- as the authority did. A transport answers this with @Facts@.
needs :: Replica -> [Seq]
needs r =
  [ n
  | (n, ib) <- M.toAscList (rInbox r)
  , Just e <- [ibEntry ib]
  , isNothing (ibFacts ib)
  , not (M.member (eFn e) (rBodies r)) || n `elem` rDiverged r
  ]

-- | What a view is told, and the slate wiped.
takeChanges :: Replica -> (Changes, Replica)
takeChanges r =
  ( if rRebuilt r then Rebuilt else Applied (reverse (rChanges r))
  , r {rRebuilt = False, rChanges = []}
  )

-- | This replica's claim: its cursor and the hash of its confirmed state
-- there. What @Verify { scope, seq, hash }@ carries.
verifyAt :: Replica -> (Seq, B.ByteString)
verifyAt r = (rCursor r, stateHash (rConfirmed r))

-- §11.6 Advancing --------------------------------------------------------

-- Apply from the inbox in order for as long as the next entry can be
-- applied; then decide what the view owes. Three cases, and the third is
-- the rebase:
--
-- * nothing was pending before or after: the confirmed changes are the
--   view's changes, and there is nothing to rebuild — the common online
--   case costs the rows that moved;
-- * every entry applied was this peer's own next pending intent, in
--   order: the view already showed exactly this (it was probed against
--   the same state the authority applied it to), so nothing is reported
--   and nothing is rebuilt — an ack is free;
-- * anything else landed under pending intents: the view is rebuilt from
--   the new confirmed store, and says so.
advance :: Replica -> Replica
advance r0 = finish (go r0 [] False False)
  where
    go r acc moved others =
      let n = rCursor r + 1
       in case M.lookup n (rInbox r) of
            Just (Inbox (Just e) mf) | Just (st', chs, diverged) <- applyOne r n e mf ->
              let ownNext = case rPending r of
                    (p : _) -> eId p == eId e
                    [] -> False
               in go
                    r
                      { rConfirmed = st'
                      , rCursor = n
                      , rInbox = M.delete n (rInbox r)
                      , rPending = filter ((/= eId e) . eId) (rPending r)
                      , rDiverged = if diverged then rDiverged r ++ [n] else rDiverged r
                      }
                    (reverse chs ++ acc)
                    True
                    (others || not ownNext || diverged)
            _ -> (r, acc, moved, others)
    finish (r, acc, moved, others)
      | not moved = r
      | null (rPending r0) = r {rView = rConfirmed r, rChanges = acc ++ rChanges r}
      | not others = r {rView = if null (rPending r) then rConfirmed r else rView r}
      | otherwise = replay r

-- One entry against the confirmed store: by intent when the closure is
-- held, by facts otherwise; both when both are present, comparing them.
-- 'Nothing' means it cannot be applied yet (no closure, no facts) — or,
-- when replay refused or disagreed and no facts are in hand, that facts
-- must be fetched before this sequence can pass.
applyOne :: Replica -> Seq -> Entry -> Maybe Facts -> Maybe (Store, [Change], Bool)
applyOne r n e mf =
  case (M.lookup (eFn e) (rBodies r), mf) of
    (Just c, _) | n `notElem` rDiverged r ->
      case applyClosure (rSchema r) c (Ctx (eActor e) (eSession e)) (eAutos e) (eArgs e) (rConfirmed r) of
        Right (Right (st', chs)) -> case mf of
          Just f | f /= chs -> Just (S.applyChanges (rConfirmed r) f, f, True)
          _ -> Just (st', chs, False)
        _ -> case mf of
          Just f -> Just (S.applyChanges (rConfirmed r) f, f, True)
          Nothing -> Nothing
    (_, Just f) -> Just (S.applyChanges (rConfirmed r) f, f, False)
    (_, Nothing) -> Nothing

-- Rebuild the view: the confirmed store, then every pending intent in
-- order. One that is now refused is dropped and recorded, exactly as the
-- authority would answer it.
replay :: Replica -> Replica
replay r0 = go r0 {rView = rConfirmed r0, rRebuilt = True, rChanges = []} (rPending r0) []
  where
    go r [] kept = r {rPending = reverse kept}
    go r (e : es) kept = case M.lookup (eFn e) (rBodies r) of
      Nothing -> go r {rRejections = rRejections r ++ [(eId e, Refused "no closure for a pending intent")]} es kept
      Just c -> case applyClosure (rSchema r) c (Ctx (eActor e) (eSession e)) (eAutos e) (eArgs e) (rView r) of
        Right (Right (view', _)) -> go r {rView = view'} es (e : kept)
        Right (Left why) -> go r {rRejections = rRejections r ++ [(eId e, why)]} es kept
        Left bug -> go r {rRejections = rRejections r ++ [(eId e, Refused ("bug: " <> T.pack (show bug)))]} es kept

-- ---------------------------------------------------------------------
-- An authority

-- | The peer that sequences a scope.
data Authority = Authority
  { aScope :: ScopeName
  , aSchema :: Schema
  , -- | Every closure ever accepted for this scope, by hash: the current
    -- module's and every historical version a retained entry names.
    aBodies :: Map FnHash Closure
  , aLog :: Log
  , -- | The state at the head of the log.
    aStore :: Store
  }
  deriving (Eq, Show)

authority :: Schema -> ScopeName -> Map FnHash Closure -> Authority
authority sch scope bodies = Authority scope sch bodies (emptyLog sch) (S.empty sch)

-- | The answer to a pushed intent.
data Sequenced
  = -- | Applied and appended, at this sequence, with these facts.
    Appended Seq Facts
  | -- | Seen before: re-acknowledged with the sequence it already has, and
    -- nothing applied. Delivery is idempotent by construction.
    Duplicate Seq
  | -- | The verdict. Reached identically by every replica that applies the
    -- same intent to the same state, which is what makes it a fact about
    -- the entry.
    Rejected Refusal
  deriving (Eq, Show)

-- | §11.7 Sequence an intent: dedupe by id, apply to the head state, and
-- append with the facts. An intent naming a closure the authority does not
-- hold is refused, not stalled — a peer is never left waiting on a verdict.
sequenceEntry :: Authority -> Entry -> (Authority, Sequenced)
sequenceEntry a e = case seqOf (aLog a) (eId e) of
  Just n -> (a, Duplicate n)
  Nothing -> case M.lookup (eFn e) (aBodies a) of
    Nothing -> (a, Rejected (Refused ("unknown function " <> hexText (eFn e))))
    Just c -> case applyClosure (aSchema a) c (Ctx (eActor e) (eSession e)) (eAutos e) (eArgs e) (aStore a) of
      Left bug -> (a, Rejected (Refused ("bug: " <> T.pack (show bug))))
      Right (Left why) -> (a, Rejected why)
      Right (Right (st', facts)) ->
        let (l', n) = append (aLog a) e facts
         in (a {aLog = l', aStore = st'}, Appended n facts)

-- | What a peer at a cursor is sent: a page of entries with their facts,
-- or the snapshot if it is below the horizon.
page :: Authority -> Seq -> Int -> Page
page a = entriesAfter (aLog a)

-- | Move the horizon; 'Nothing' if the sequence is not retained.
compact :: Authority -> Seq -> Maybe Authority
compact a n = (\l -> a {aLog = l}) <$> compactTo (aLog a) n

-- | Drop every closure that neither the current module nor a retained
-- entry names. The first argument is the current module's hashes.
retire :: Set FnHash -> Authority -> Authority
retire current a = a {aBodies = M.filterWithKey (\h _ -> h `Set.member` keep) (aBodies a)}
  where
    keep = current `Set.union` namedHashes (aLog a)

-- | Why a log offered for adoption was turned away.
data AdoptError
  = -- | Only a log from sequence 1 can be verified; one standing on a
    -- snapshot asks to be trusted, and adoption does not trust.
    NotFromTheBeginning
  | Gap
  | MissingClosure Seq FnHash
  | -- | The authority's own application refused what the offering peer
    -- accepted.
    RefusedAt Seq Refusal
  | -- | Replaying the intent did not produce the facts the peer recorded.
    FactsDiffer Seq
  | -- | The state after replay does not match what the peer's facts say.
    HashDiffers
  deriving (Eq, Show)

-- | §11.8 Adopt a scope a peer sequenced alone: replay every intent from
-- the beginning through this authority's own closures, holding each to the
-- facts the peer recorded, and become its authority. A peer cannot smuggle
-- rows it did not derive — this is the property only exact replicas have,
-- and the reason intents rather than facts are the log. The closures are
-- supplied by the adopter (the peer may send its own; the adopter decides
-- which it trusts).
adopt :: Schema -> ScopeName -> Map FnHash Closure -> Log -> Either AdoptError Authority
adopt sch scope bodies l = do
  if horizon l /= 0 || not (S.stTables (snStore (lBase l)) == M.empty) then Left NotFromTheBeginning else Right ()
  if contiguous l then Right () else Left Gap
  let a0 = authority sch scope bodies
  a <- foldlM' step a0 (M.toAscList (lEntries l))
  claimed <- maybe (Left HashDiffers) Right (stateAt l (headSeq l))
  if stateHash (aStore a) == stateHash claimed then Right a else Left HashDiffers
  where
    step a (n, (e, recorded)) = case sequenceEntry a e of
      (a', Appended n' facts)
        | n' /= n -> Left Gap
        | facts /= recorded -> Left (FactsDiffer n)
        | otherwise -> Right a'
      (_, Rejected why) -> Left (RefusedAt n why)
      (_, Duplicate _) -> Left Gap
    foldlM' _ z [] = Right z
    foldlM' f z (x : xs) = f z x >>= \z' -> foldlM' f z' xs

-- ---------------------------------------------------------------------
-- Both at once

-- | §11.9 A peer that is its own authority: everything pending is
-- sequenced, and every answer is delivered back, in order. After it,
-- nothing is pending and the view is the confirmed store. This is the
-- whole of "offline mode": there is no mode, only a loop with no socket
-- in it — and it is also, without the fan-out, what a server does for one
-- peer's push.
localCommit :: Authority -> Replica -> (Authority, Replica)
localCommit a0 r0 = foldl' step (a0, r0) (rPending r0)
  where
    step (a, r) e = case sequenceEntry a e of
      (a', Appended n facts) -> (a', ack (receiveFacts r n facts) (eId e) n)
      (a', Duplicate n) -> (a', ack r (eId e) n)
      (a', Rejected why) -> (a', reject r (eId e) why)

