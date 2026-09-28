{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §12 The protocol.
--
-- The frames two peers exchange, as values (so that 'Ark.Canon.encode' is
-- their wire form), and the two state machines around them: a 'Client'
-- holding replicas of the scopes it subscribes to, and a 'Server' holding
-- authorities for the scopes it hosts, the connections it has identified,
-- and the live rooms. Both are sans-io: a transport feeds them frames and
-- drains what they queue, and nothing here knows what a socket is.
--
-- The rules, as Petros had them and as the design keeps them:
--
-- * identity is asked once, at 'Hello', through the server's authenticator;
--   every pushed entry is held to it, and a 'Hello' that proves nothing is
--   answered 'Denied' and nothing else;
-- * the authority applies before it appends, so a 'Reject' is a verdict;
-- * after every message the server sends every connection every entry
--   above what it has been sent, a page at a time — one rule for first
--   sync, resume and broadcast, and a client sometimes receives its own
--   entry twice, which is harmless because delivery is idempotent;
-- * a page carries facts for a peer that asked to be fed by facts, and
--   not for one that replays; the latter asks 'NeedFacts' for what it
--   cannot replay and 'Verify' for what it can;
-- * a connection below a scope's horizon is sent the snapshot instead of a
--   page, and continues from there with its pending intact;
-- * 'Say' and 'Heard' ride the same connection and touch nothing else.
module Ark.Protocol
  ( -- * Frames
    Mode (..)
  , Subscription (..)
  , ClientMsg (..)
  , ServerMsg (..)
  , clientValue
  , serverValue
  , clientFromValue
  , serverFromValue
  , entryValue
  , entryFromValue
  , changeValue
  , changeFromValue
  , batchLimit

    -- * The client
  , Client (..)
  , openClient
  , subscribe
  , connected
  , disconnected
  , clientMutate
  , clientRecv
  , say
  , verifyAll
  , takeOutgoing
  , takeHeard

    -- * The server
  , Identity (..)
  , Authenticate
  , trusting
  , Server (..)
  , Conn (..)
  , openServer
  , withOwns
  , host
  , serverRecv
  , disconnect
  , takeServerOutgoing
  ) where

import qualified Data.ByteString as B
import Data.Int (Int64)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Text (Text)
import qualified Data.Text as T

import Ark.Decode (DecodeError (..), closureFromValue)
import Ark.Eval (Args, Ctx (..))
import Ark.Hash (Closure, FnHash, closureValue, stateHash)
import qualified Ark.Live as L
import Ark.Log
import Ark.Peer
import Ark.Schema (Schema, ScopeName)
import Ark.Store (Change (..), Refusal (..))
import qualified Ark.Store as S
import Ark.Value

-- ---------------------------------------------------------------------
-- Frames

-- | How a client holds a scope. 'Whole' replays intents and is exact;
-- 'ByFacts' is fed the facts of every entry and holds no generated code
-- for the scope.
data Mode = Whole | ByFacts
  deriving (Eq, Show)

data Subscription = Subscription
  { subScope :: ScopeName
  , subSince :: Seq -- ^ the cursor: the last sequence applied
  , subMode :: Mode
  }
  deriving (Eq, Show)

data ClientMsg
  = Hello {hSubs :: [Subscription], hToken :: Maybe Text, hSpec :: Int}
  | Push {pScope :: ScopeName, pEntries :: [Entry]}
  | NeedFacts {nfScope :: ScopeName, nfSeqs :: [Seq]}
  | NeedClosures {ncHashes :: [FnHash]}
  | Verify {vScope :: ScopeName, vSeq :: Seq, vHash :: B.ByteString}
  | Say {sayFrame :: B.ByteString}
  deriving (Eq, Show)

data ServerMsg
  = Batch {bScope :: ScopeName, bItems :: [(Seq, Entry, Maybe Facts)], bHasMore :: Bool}
  | FactsFor {fScope :: ScopeName, fItems :: [(Seq, Facts)]}
  | SnapshotOf {snScope :: ScopeName, snAt :: Seq, snStateHash :: B.ByteString, snRows :: Map TableName [Value]}
  | Ack {ackScope :: ScopeName, ackIds :: [IdBytes], ackSeqs :: [Seq]}
  | Reject {rjScope :: ScopeName, rjId :: IdBytes, rjReason :: Text}
  | Denied {dReason :: Text}
  | Closures {cItems :: [(FnHash, Closure)]}
  | Agree {agScope :: ScopeName, agSeq :: Seq, agHash :: B.ByteString, agOk :: Bool}
  | Heard {heardFrame :: B.ByteString}
  deriving (Eq, Show)

-- | Entries per page.
batchLimit :: Int
batchLimit = 256

node :: Text -> [(FieldName, Value)] -> Value
node t fs = VStruct (M.fromList (("t", VText t) : fs))

int :: Integral a => a -> Value
int = VInt . fromIntegral

entryValue :: Entry -> Value
entryValue e =
  VStruct
    ( M.fromList
        [ ("id", VId (eId e))
        , ("actor", VText (eActor e))
        , ("session", VText (eSession e))
        , ("fn", VBytes (eFn e))
        , ("args", VStruct (eArgs e))
        , ("autos", VStruct (eAutos e))
        ]
    )

changeValue :: Change -> Value
changeValue = \case
  Add t r -> node "add" [("table", VText t), ("row", VStruct r)]
  Remove t r -> node "remove" [("table", VText t), ("row", VStruct r)]
  Edit t o n -> node "edit" [("table", VText t), ("old", VStruct o), ("new", VStruct n)]

factsValue :: Facts -> Value
factsValue = VList . map changeValue

clientValue :: ClientMsg -> Value
clientValue = \case
  Hello subs tok spec ->
    node
      "hello"
      [ ("scopes", VList [node "sub" [("scope", VText s), ("since", int n), ("mode", VText (if m == Whole then "whole" else "facts"))] | Subscription s n m <- subs])
      , ("token", maybe VNull VText tok)
      , ("spec", int spec)
      ]
  Push s es -> node "push" [("scope", VText s), ("entries", VList (map entryValue es))]
  NeedFacts s ns -> node "need_facts" [("scope", VText s), ("seqs", VList (map int ns))]
  NeedClosures hs -> node "need_closures" [("hashes", VList (map VBytes hs))]
  Verify s n h -> node "verify" [("scope", VText s), ("seq", int n), ("hash", VBytes h)]
  Say f -> node "say" [("say", VBytes f)]

serverValue :: ServerMsg -> Value
serverValue = \case
  Batch s items more ->
    node
      "batch"
      [ ("scope", VText s)
      , ("items", VList [VStruct (M.fromList [("seq", int n), ("entry", entryValue e), ("facts", maybe VNull factsValue f)]) | (n, e, f) <- items])
      , ("has_more", VBool more)
      ]
  FactsFor s items -> node "facts" [("scope", VText s), ("items", VList [VStruct (M.fromList [("seq", int n), ("facts", factsValue f)]) | (n, f) <- items])]
  SnapshotOf s n h rows -> node "snapshot" [("scope", VText s), ("seq", int n), ("hash", VBytes h), ("rows", VStruct (M.map VList rows))]
  Ack s ids ns -> node "ack" [("scope", VText s), ("ids", VList (map VId ids)), ("seqs", VList (map int ns))]
  Reject s i why -> node "reject" [("scope", VText s), ("id", VId i), ("reason", VText why)]
  Denied why -> node "denied" [("reason", VText why)]
  Closures cs -> node "closures" [("items", VList [VStruct (M.fromList [("hash", VBytes h), ("closure", closureValue c)]) | (h, c) <- cs])]
  Agree s n h ok -> node "agree" [("scope", VText s), ("seq", int n), ("hash", VBytes h), ("ok", VBool ok)]
  Heard f -> node "heard" [("hear", VBytes f)]

-- Decoding ------------------------------------------------------------

type D a = Either DecodeError a

bad :: Text -> D a
bad = Left . DecodeError ["frame"]

struct :: Value -> D (Map FieldName Value)
struct (VStruct m) = pure m
struct _ = bad "expected a struct"

need :: Map FieldName Value -> FieldName -> D Value
need m k = maybe (bad ("missing " <> k)) pure (M.lookup k m)

text :: Value -> D Text
text (VText t) = pure t
text _ = bad "expected text"

bytes :: Value -> D B.ByteString
bytes (VBytes b) = pure b
bytes _ = bad "expected bytes"

int64 :: Value -> D Int64
int64 (VInt n) = pure n
int64 _ = bad "expected an int"

list :: (Value -> D a) -> Value -> D [a]
list f (VList xs) = mapM f xs
list _ _ = bad "expected a list"

ident :: Value -> D IdBytes
ident (VId i) = pure i
ident _ = bad "expected an id"

entryFromValue :: Value -> D Entry
entryFromValue v = do
  m <- struct v
  Entry
    <$> (need m "id" >>= ident)
    <*> (need m "actor" >>= text)
    <*> (need m "session" >>= text)
    <*> (need m "fn" >>= bytes)
    <*> (need m "args" >>= struct)
    <*> (need m "autos" >>= struct)

changeFromValue :: Value -> D Change
changeFromValue v = do
  m <- struct v
  t <- need m "t" >>= text
  tbl <- need m "table" >>= text
  case t of
    "add" -> Add tbl <$> (need m "row" >>= struct)
    "remove" -> Remove tbl <$> (need m "row" >>= struct)
    "edit" -> Edit tbl <$> (need m "old" >>= struct) <*> (need m "new" >>= struct)
    other -> bad ("unknown change " <> other)

clientFromValue :: Value -> D ClientMsg
clientFromValue v = do
  m <- struct v
  t <- need m "t" >>= text
  case t of
    "hello" ->
      Hello
        <$> (need m "scopes" >>= list sub)
        <*> (need m "token" >>= \case VNull -> pure Nothing; x -> Just <$> text x)
        <*> (fromIntegral <$> (need m "spec" >>= int64))
    "push" -> Push <$> (need m "scope" >>= text) <*> (need m "entries" >>= list entryFromValue)
    "need_facts" -> NeedFacts <$> (need m "scope" >>= text) <*> (need m "seqs" >>= list int64)
    "need_closures" -> NeedClosures <$> (need m "hashes" >>= list bytes)
    "verify" -> Verify <$> (need m "scope" >>= text) <*> (need m "seq" >>= int64) <*> (need m "hash" >>= bytes)
    "say" -> Say <$> (need m "say" >>= bytes)
    other -> bad ("unknown client frame " <> other)
  where
    sub x = do
      m <- struct x
      md <- need m "mode" >>= text >>= \case
        "whole" -> pure Whole
        "facts" -> pure ByFacts
        other -> bad ("unknown mode " <> other)
      Subscription <$> (need m "scope" >>= text) <*> (need m "since" >>= int64) <*> pure md

serverFromValue :: Value -> D ServerMsg
serverFromValue v = do
  m <- struct v
  t <- need m "t" >>= text
  case t of
    "batch" -> Batch <$> (need m "scope" >>= text) <*> (need m "items" >>= list item) <*> (need m "has_more" >>= bool)
    "facts" -> FactsFor <$> (need m "scope" >>= text) <*> (need m "items" >>= list factsItem)
    "snapshot" -> SnapshotOf <$> (need m "scope" >>= text) <*> (need m "seq" >>= int64) <*> (need m "hash" >>= bytes) <*> (need m "rows" >>= struct >>= traverse (list pure))
    "ack" -> Ack <$> (need m "scope" >>= text) <*> (need m "ids" >>= list ident) <*> (need m "seqs" >>= list int64)
    "reject" -> Reject <$> (need m "scope" >>= text) <*> (need m "id" >>= ident) <*> (need m "reason" >>= text)
    "denied" -> Denied <$> (need m "reason" >>= text)
    "closures" -> Closures <$> (need m "items" >>= list closureItem)
    "agree" -> Agree <$> (need m "scope" >>= text) <*> (need m "seq" >>= int64) <*> (need m "hash" >>= bytes) <*> (need m "ok" >>= bool)
    "heard" -> Heard <$> (need m "hear" >>= bytes)
    other -> bad ("unknown server frame " <> other)
  where
    bool (VBool b) = pure b
    bool _ = bad "expected a bool"
    item x = do
      m <- struct x
      (,,) <$> (need m "seq" >>= int64) <*> (need m "entry" >>= entryFromValue) <*> (need m "facts" >>= \case VNull -> pure Nothing; f -> Just <$> list changeFromValue f)
    factsItem x = do
      m <- struct x
      (,) <$> (need m "seq" >>= int64) <*> (need m "facts" >>= list changeFromValue)
    closureItem x = do
      m <- struct x
      (,) <$> (need m "hash" >>= bytes) <*> (need m "closure" >>= closureFromValue)

-- ---------------------------------------------------------------------
-- The client

-- | A peer's end of one connection: its replicas, and what it has queued.
data Client = Client
  { clSchema :: Schema
  , clScopes :: Map ScopeName (Replica, Mode)
  , clToken :: Maybe Text
  , clLinked :: Bool
  , -- | Counts connections, so a live room that has never heard of this
    -- device (a new connection) can be told apart from one that has.
    clEpoch :: Int
  , clOut :: [ClientMsg] -- ^ newest first
  , clHeard :: [B.ByteString] -- ^ newest first
  , clDenied :: Maybe Text
  , clAgreed :: [(ScopeName, Seq, Bool)]
  }

openClient :: Schema -> Maybe Text -> Client
openClient sch tok = Client sch M.empty tok False 0 [] [] Nothing []

-- | Hold a scope, with the replica as opened from what was durable.
subscribe :: Client -> Mode -> Replica -> Client
subscribe c md r = c {clScopes = M.insert (rScope r) (r, md) (clScopes c)}

emit :: ClientMsg -> Client -> Client
emit m c
  | clLinked c = c {clOut = m : clOut c}
  | otherwise = c -- unlinked: nothing is queued; 'connected' says it all again

-- | §12.1 A connection opened: say hello for every scope at its cursor,
-- then push everything pending. What was queued before is dropped, since
-- the hello resends it all.
connected :: Client -> Client
connected c0 =
  let c = c0 {clLinked = True, clEpoch = clEpoch c0 + 1, clOut = [], clHeard = []}
      subs = [Subscription s (rCursor r) md | (s, (r, md)) <- M.toList (clScopes c)]
      c1 = emit (Hello subs (clToken c) 1) c
   in foldl (\acc (s, (r, _)) -> if null (rPending r) then acc else emit (Push s (rPending r)) acc) c1 (M.toList (clScopes c))

disconnected :: Client -> Client
disconnected c = c {clLinked = False, clOut = [], clHeard = []}

-- | Author an intent into a scope and push it if linked.
clientMutate :: Client -> ScopeName -> IdBytes -> Ctx -> FnHash -> Args -> Args -> Either Refusal (Client, Entry)
clientMutate c s i ctx fh autos args = do
  (r, md) <- maybe (Left (Refused ("not holding scope " <> s))) Right (M.lookup s (clScopes c))
  (r', e) <- mutate r i ctx fh autos args
  pure (emit (Push s [e]) c {clScopes = M.insert s (r', md) (clScopes c)}, e)

-- | §12.2 A frame from the server.
clientRecv :: Client -> ServerMsg -> Client
clientRecv c = \case
  Heard f -> c {clHeard = f : clHeard c}
  Denied why -> c {clDenied = Just why, clLinked = False, clOut = []}
  Batch s items more -> withScope s $ \r md ->
    let r' = foldl (\acc (n, e, mf) -> case mf of Just f -> receiveWith acc n e f; Nothing -> receive acc n e) r items
        c' = put s r' md
        c'' = if null (needs r') then c' else emit (NeedFacts s (needs r')) c'
     in if more then emit (Hello [Subscription s (rCursor r') md] (clToken c) 1) c'' else c''
  FactsFor s items -> withScope s $ \r md -> put s (foldl (\acc (n, f) -> receiveFacts acc n f) r items) md
  SnapshotOf s n _ rows -> withScope s $ \r md ->
    -- Below the horizon: the confirmed store is replaced by the snapshot
    -- and the cursor moves to it; pending intents are kept and replay on
    -- top, as they always did.
    let st = foldl (\acc (t, vs) -> foldl (\a v -> case v of VStruct row -> S.applyChange a (Add t row); _ -> a) acc vs) (S.empty (clSchema c)) (M.toList rows)
        r' = open (rSchema r) s (rBodies r) st n (rPending r)
     in put s r' md
  Ack s ids ns -> withScope s $ \r md -> put s (foldl (\acc (i, n) -> ack acc i n) r (zip ids ns)) md
  Reject s i why -> withScope s $ \r md -> put s (reject r i (Refused why)) md
  Closures cs ->
    -- New closures may unblock entries waiting in an inbox, so every
    -- replica is asked to try again.
    c {clScopes = M.map (\(r, md) -> (retry r {rBodies = M.union (M.fromList cs) (rBodies r)}, md)) (clScopes c)}
  Agree s n _ ok -> c {clAgreed = clAgreed c ++ [(s, n, ok)]}
  where
    withScope s f = case M.lookup s (clScopes c) of
      Just (r, md) -> f r md
      Nothing -> c
    put s r md = c {clScopes = M.insert s (r, md) (clScopes c)}

-- | A live frame; dropped while unlinked, never queued.
say :: Client -> B.ByteString -> Client
say c f = emit (Say f) c

-- | Ask the authority whether it agrees with every replica's confirmed
-- state.
verifyAll :: Client -> Client
verifyAll c = foldl (\acc (s, (r, _)) -> let (n, h) = verifyAt r in emit (Verify s n h) acc) c (M.toList (clScopes c))

takeOutgoing :: Client -> ([ClientMsg], Client)
takeOutgoing c = (reverse (clOut c), c {clOut = []})

takeHeard :: Client -> ([B.ByteString], Client)
takeHeard c = (reverse (clHeard c), c {clHeard = []})

-- ---------------------------------------------------------------------
-- The server

-- | Who a connection is: the user, and the login (one login on one
-- device). Every entry the connection pushes is held to both.
data Identity = Identity
  { idUser :: Text
  , idSession :: Text
  }
  deriving (Eq, Show)

-- | What a token proves. The engine asks this once, at 'Hello', and never
-- looks at a token again.
type Authenticate = Maybe Text -> Maybe Identity

-- | Dev auth: anyone is whoever they say, and the token is their name.
-- Said loudly by whatever starts a server with it.
trusting :: Authenticate
trusting tok = Just (Identity (fromMaybe "anonymous" tok) "dev")

data Conn = Conn
  { cnWho :: Identity
  , -- | Per scope: the mode, and the sequence the connection has been
    -- sent up to (not what it has applied — that is its own business).
    cnScopes :: Map ScopeName (Mode, Seq)
  }

data Server s = Server
  { svAuth :: Authenticate
  , -- | Does this user own this session? A session outlives its token: an
    -- entry authored offline under one login and pushed after the same
    -- person signs in again carries the old session, and is still theirs.
    -- Only ever asked about the connection's own user. By default, no.
    svOwns :: Text -> Text -> Bool
  , -- | May this identity receive this scope? The scope-level read rule.
    svAccess :: Identity -> ScopeName -> Bool
  , svScopes :: Map ScopeName Authority
  , svConns :: Map L.ConnId Conn
  , svMachine :: L.Machine s
  , svRooms :: L.Rooms s
  , svOut :: [(L.ConnId, ServerMsg)] -- ^ newest first
  }

openServer :: Authenticate -> (Identity -> ScopeName -> Bool) -> L.Machine s -> Server s
openServer auth access m = Server auth (\_ _ -> False) access M.empty M.empty m L.emptyRooms []

-- | Install the sessions a user owns, which the authenticator's session
-- store knows and the engine does not.
withOwns :: (Text -> Text -> Bool) -> Server s -> Server s
withOwns owns sv = sv {svOwns = owns}

-- | Host a scope: become its authority.
host :: Server s -> Authority -> Server s
host sv a = sv {svScopes = M.insert (aScope a) a (svScopes sv)}

send :: L.ConnId -> ServerMsg -> Server s -> Server s
send c m sv = sv {svOut = (c, m) : svOut sv}

-- | §12.3 A frame from a connection.
serverRecv :: Server s -> L.ConnId -> ClientMsg -> Server s
serverRecv sv0 c msg = case msg of
  Hello subs tok _ -> case svAuth sv0 tok of
    Nothing -> send c (Denied "not signed in") sv0
    Just who ->
      let allowed = [s | s <- subs, svAccess sv0 who (subScope s), M.member (subScope s) (svScopes sv0)]
          named = M.fromList [(subScope s, (subMode s, subSince s)) | s <- allowed]
          -- A second Hello from the same identity on one connection is the
          -- log paging one scope: it names that scope, and every other
          -- scope the connection holds is kept. A different identity is a
          -- new connection's worth of scopes.
          conn = case M.lookup c (svConns sv0) of
            Just old | cnWho old == who -> Conn who (M.union named (cnScopes old))
            _ -> Conn who named
          sv1 = sv0 {svConns = M.insert c conn (svConns sv0)}
          (rooms, post) = L.arrive (svMachine sv1) (svRooms sv1) (L.Peer c (idUser who) (idSession who))
       in fanout (deliver post sv1 {svRooms = rooms})
  Push s es -> withConn $ \conn -> case M.lookup s (svScopes sv0) of
    Nothing -> send c (Denied ("unknown scope " <> s)) sv0
    Just a ->
      let (a', acks, sv') = foldl (one conn) (a, [], sv0) es
          sv'' = sv' {svScopes = M.insert s a' (svScopes sv')}
       in fanout (if null acks then sv'' else send c (Ack s (map fst acks) (map snd acks)) sv'')
    where
      one conn (a, acks, sv) e
        | eActor e /= idUser (cnWho conn)
            || (eSession e /= idSession (cnWho conn) && not (svOwns sv0 (eActor e) (eSession e))) =
            (a, acks, send c (Reject s (eId e) "not yours") sv)
        | otherwise = case sequenceEntry a e of
            (a', Appended n _) -> (a', acks ++ [(eId e, n)], sv)
            (a', Duplicate n) -> (a', acks ++ [(eId e, n)], sv)
            (a', Rejected why) -> (a', acks, send c (Reject s (eId e) (T.pack (show why))) sv)
  NeedFacts s ns -> withConn $ \_ -> case M.lookup s (svScopes sv0) of
    Nothing -> sv0
    Just a ->
      let items = [(n, f) | n <- ns, Just (_, f) <- [M.lookup n (lEntries (aLog a))]]
       in send c (FactsFor s items) sv0
  NeedClosures hs -> withConn $ \_ ->
    let known = M.unions (map aBodies (M.elems (svScopes sv0)))
     in send c (Closures [(h, cl) | h <- hs, Just cl <- [M.lookup h known]]) sv0
  Verify s n h -> withConn $ \_ -> case M.lookup s (svScopes sv0) of
    Nothing -> sv0
    Just a -> send c (Agree s n h (fmap stateHash (stateAt (aLog a) n) == Just h)) sv0
  Say f -> withConn $ \_ ->
    let (rooms, post) = L.speak (svMachine sv0) (svRooms sv0) c f
     in deliver post sv0 {svRooms = rooms}
  where
    withConn f = case M.lookup c (svConns sv0) of
      Just conn -> f conn
      Nothing -> send c (Denied "hello first") sv0

-- | A connection closed: the room hears it, the cursors are forgotten.
disconnect :: Server s -> L.ConnId -> Server s
disconnect sv c =
  let (rooms, post) = L.depart (svMachine sv) (svRooms sv) c
   in deliver post sv {svRooms = rooms, svConns = M.delete c (svConns sv)}

deliver :: L.Post -> Server s -> Server s
deliver post sv = foldl (\acc (to, f) -> send to (Heard f) acc) sv (L.postOut post)

-- | §12.4 Fan-out: every connection, every scope it holds, everything above
-- what it has been sent, a page at a time; a snapshot for one below the
-- horizon. Run after every message.
fanout :: Server s -> Server s
fanout sv0 = foldl perConn sv0 (M.toList (svConns sv0))
  where
    perConn sv (c, conn) = foldl (perScope c) sv (M.toList (cnScopes conn))
    perScope c sv (s, (md, sent)) = case M.lookup s (svScopes sv) of
      Nothing -> sv
      Just a
        | sent >= headSeq (aLog a) -> sv
        | otherwise -> case page a sent batchLimit of
            BelowHorizon sn ->
              let rows = M.fromList [(t, map VStruct (M.elems (S.rows (snStore sn) t))) | t <- S.tableNames (snStore sn)]
               in advanceSent c s (snSeq sn) (send c (SnapshotOf s (snSeq sn) (snHash sn) rows) sv)
            Entries items more ->
              let withFacts = [(n, e, if md == ByFacts then Just f else Nothing) | (n, e, f) <- items]
                  lastSeq = maximum (sent : [n | (n, _, _) <- items])
               in advanceSent c s lastSeq (send c (Batch s withFacts more) sv)
    advanceSent c s n sv = sv {svConns = M.adjust (\cn -> cn {cnScopes = M.adjust (\(md, _) -> (md, n)) s (cnScopes cn)}) c (svConns sv)}

takeServerOutgoing :: Server s -> ([(L.ConnId, ServerMsg)], Server s)
takeServerOutgoing sv = (reverse (svOut sv), sv {svOut = []})

