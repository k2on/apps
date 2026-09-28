{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §12 The protocol.
--
-- The frames two peers exchange, as values (so that 'Ark.Canon.encode' is
-- their wire form), and the two state machines around them: a 'Client'
-- holding a replica of the log, and a 'Server' holding its authority, the
-- connections it has identified, and the live rooms. Both are sans-io: a transport feeds them frames and
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
-- * a connection below the horizon is sent the snapshot instead of a
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
  , clientSignIn
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
  , refusalText
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
import Ark.IR (specVersion)
import Ark.Log
import Ark.Peer
import Ark.Schema (Schema)
import Ark.Store (Change (..), Refusal (..))
import qualified Ark.Store as S
import Ark.Value

-- ---------------------------------------------------------------------
-- Frames

-- | How a client holds the log. 'Whole' replays intents and is exact;
-- 'ByFacts' is fed the facts of every entry and needs no closure.
data Mode = Whole | ByFacts
  deriving (Eq, Show)

data Subscription = Subscription
  { subSince :: Seq -- ^ the cursor: the last sequence applied
  , subMode :: Mode
  }
  deriving (Eq, Show)

data ClientMsg
  = Hello {hSub :: Subscription, hToken :: Maybe Text, hSpec :: Int}
  | Push {pEntries :: [Entry]}
  | NeedFacts {nfSeqs :: [Seq]}
  | NeedClosures {ncHashes :: [FnHash]}
  | Verify {vSeq :: Seq, vHash :: B.ByteString}
  | Say {sayFrame :: B.ByteString}
  deriving (Eq, Show)

data ServerMsg
  = Batch {bItems :: [(Seq, Entry, Maybe Facts)], bHasMore :: Bool}
  | FactsFor {fItems :: [(Seq, Facts)]}
  | SnapshotOf {snAt :: Seq, snStateHash :: B.ByteString, snRows :: Map TableName [Value]}
  | Ack {ackIds :: [IdBytes], ackSeqs :: [Seq]}
  | -- | A verdict against one entry, with the reason every replica would
    -- reach: what a screen shows beside the item that did not happen.
    Reject {rjId :: IdBytes, rjReason :: Text}
  | Denied {dReason :: Text}
  | Closures {cItems :: [(FnHash, Closure)]}
  | Agree {agSeq :: Seq, agHash :: B.ByteString, agOk :: Bool}
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
  Hello (Subscription n m) tok spec ->
    node
      "hello"
      [ ("since", int n)
      , ("mode", VText (if m == Whole then "whole" else "facts"))
      , ("token", maybe VNull VText tok)
      , ("spec", int spec)
      ]
  Push es -> node "push" [("entries", VList (map entryValue es))]
  NeedFacts ns -> node "need_facts" [("seqs", VList (map int ns))]
  NeedClosures hs -> node "need_closures" [("hashes", VList (map VBytes hs))]
  Verify n h -> node "verify" [("seq", int n), ("hash", VBytes h)]
  Say f -> node "say" [("say", VBytes f)]

serverValue :: ServerMsg -> Value
serverValue = \case
  Batch items more ->
    node
      "batch"
      [ ("items", VList [VStruct (M.fromList [("seq", int n), ("entry", entryValue e), ("facts", maybe VNull factsValue f)]) | (n, e, f) <- items])
      , ("has_more", VBool more)
      ]
  FactsFor items -> node "facts" [("items", VList [VStruct (M.fromList [("seq", int n), ("facts", factsValue f)]) | (n, f) <- items])]
  SnapshotOf n h rows -> node "snapshot" [("seq", int n), ("hash", VBytes h), ("rows", VStruct (M.map VList rows))]
  Ack ids ns -> node "ack" [("ids", VList (map VId ids)), ("seqs", VList (map int ns))]
  Reject i why -> node "reject" [("id", VId i), ("reason", VText why)]
  Denied why -> node "denied" [("reason", VText why)]
  Closures cs -> node "closures" [("items", VList [VStruct (M.fromList [("hash", VBytes h), ("closure", closureValue c)]) | (h, c) <- cs])]
  Agree n h ok -> node "agree" [("seq", int n), ("hash", VBytes h), ("ok", VBool ok)]
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
    "hello" -> do
      md <- need m "mode" >>= text >>= \case
        "whole" -> pure Whole
        "facts" -> pure ByFacts
        other -> bad ("unknown mode " <> other)
      Hello
        <$> (Subscription <$> (need m "since" >>= int64) <*> pure md)
        <*> (need m "token" >>= \case VNull -> pure Nothing; x -> Just <$> text x)
        <*> (fromIntegral <$> (need m "spec" >>= int64))
    "push" -> Push <$> (need m "entries" >>= list entryFromValue)
    "need_facts" -> NeedFacts <$> (need m "seqs" >>= list int64)
    "need_closures" -> NeedClosures <$> (need m "hashes" >>= list bytes)
    "verify" -> Verify <$> (need m "seq" >>= int64) <*> (need m "hash" >>= bytes)
    "say" -> Say <$> (need m "say" >>= bytes)
    other -> bad ("unknown client frame " <> other)

serverFromValue :: Value -> D ServerMsg
serverFromValue v = do
  m <- struct v
  t <- need m "t" >>= text
  case t of
    "batch" -> Batch <$> (need m "items" >>= list item) <*> (need m "has_more" >>= bool)
    "facts" -> FactsFor <$> (need m "items" >>= list factsItem)
    "snapshot" -> SnapshotOf <$> (need m "seq" >>= int64) <*> (need m "hash" >>= bytes) <*> (need m "rows" >>= struct >>= traverse (list pure))
    "ack" -> Ack <$> (need m "ids" >>= list ident) <*> (need m "seqs" >>= list int64)
    "reject" -> Reject <$> (need m "id" >>= ident) <*> (need m "reason" >>= text)
    "denied" -> Denied <$> (need m "reason" >>= text)
    "closures" -> Closures <$> (need m "items" >>= list closureItem)
    "agree" -> Agree <$> (need m "seq" >>= int64) <*> (need m "hash" >>= bytes) <*> (need m "ok" >>= bool)
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

-- | A peer's end of one connection: its replica, and what it has queued.
data Client = Client
  { clSchema :: Schema
  , clReplica :: Replica
  , clMode :: Mode
  , clToken :: Maybe Text
  , clLinked :: Bool
  , -- | Counts connections, so a live room that has never heard of this
    -- device (a new connection) can be told apart from one that has.
    clEpoch :: Int
  , clOut :: [ClientMsg] -- ^ newest first
  , clHeard :: [B.ByteString] -- ^ newest first
  , clDenied :: Maybe Text
  , clAgreed :: [(Seq, Bool)]
  }

-- | A client over the replica as opened from what was durable.
openClient :: Replica -> Mode -> Maybe Text -> Client
openClient r md tok = Client (rSchema r) r md tok False 0 [] [] Nothing []

-- | Somebody signed in: the token every later 'Hello' carries, and every
-- intent authored before anyone had signed in made theirs ('signIn'). A
-- peer used without an account has been saying nothing to any server —
-- 'connected' is only called once there is a token — so what it authored
-- is pending, and the first 'Hello' after this pushes all of it.
clientSignIn :: Ctx -> Maybe Text -> Client -> Client
clientSignIn who tok c = c {clReplica = signIn who (clReplica c), clToken = tok}

emit :: ClientMsg -> Client -> Client
emit m c
  | clLinked c = c {clOut = m : clOut c}
  | otherwise = c -- unlinked: nothing is queued; 'connected' says it all again

-- | §12.1 A connection opened: say hello at the cursor, then push
-- everything pending. What was queued before is dropped, since the hello
-- resends it all.
connected :: Client -> Client
connected c0 =
  let c = c0 {clLinked = True, clEpoch = clEpoch c0 + 1, clOut = [], clHeard = []}
      r = clReplica c
      c1 = emit (Hello (Subscription (rCursor r) (clMode c)) (clToken c) specVersion) c
   in if null (rPending r) then c1 else emit (Push (rPending r)) c1

disconnected :: Client -> Client
disconnected c = c {clLinked = False, clOut = [], clHeard = []}

-- | Author an intent and push it if linked. A refusal here is the
-- optimistic verdict, on the state this peer has; the authority's may
-- differ, and arrives as a 'Reject' with its own reason.
clientMutate :: Client -> IdBytes -> Ctx -> FnHash -> Args -> Args -> Either Refusal (Client, Entry)
clientMutate c i ctx fh autos args = do
  (r', e) <- mutate (clReplica c) i ctx fh autos args
  pure (emit (Push [e]) c {clReplica = r'}, e)

-- | §12.2 A frame from the server.
clientRecv :: Client -> ServerMsg -> Client
clientRecv c = \case
  Heard f -> c {clHeard = f : clHeard c}
  Denied why -> c {clDenied = Just why, clLinked = False, clOut = []}
  Batch items more ->
    let r' = foldl (\acc (n, e, mf) -> case mf of Just f -> receiveWith acc n e f; Nothing -> receive acc n e) r items
        c' = c {clReplica = r'}
        c'' = if null (needs r') then c' else emit (NeedFacts (needs r')) c'
     in if more then emit (Hello (Subscription (rCursor r') (clMode c)) (clToken c) specVersion) c'' else c''
  FactsFor items -> c {clReplica = foldl (\acc (n, f) -> receiveFacts acc n f) r items}
  SnapshotOf n _ rows ->
    -- Below the horizon: the confirmed store is replaced by the snapshot
    -- and the cursor moves to it; pending intents are kept and replay on
    -- top, as they always did.
    let st = foldl (\acc (t, vs) -> foldl (\a v -> case v of VStruct row -> S.applyChange a (Add t row); _ -> a) acc vs) (S.empty (clSchema c)) (M.toList rows)
     in c {clReplica = open (rSchema r) (rBodies r) st n (rPending r)}
  Ack ids ns -> c {clReplica = foldl (\acc (i, n) -> ack acc i n) r (zip ids ns)}
  Reject i why -> c {clReplica = reject r i (Refused why)}
  Closures cs ->
    -- New closures may unblock entries waiting in the inbox.
    c {clReplica = retry r {rBodies = M.union (M.fromList cs) (rBodies r)}}
  Agree n _ ok -> c {clAgreed = clAgreed c ++ [(n, ok)]}
  where
    r = clReplica c

-- | A live frame; dropped while unlinked, never queued.
say :: Client -> B.ByteString -> Client
say c f = emit (Say f) c

-- | Ask the authority whether it agrees with the replica's confirmed state.
verifyAll :: Client -> Client
verifyAll c = let (n, h) = verifyAt (clReplica c) in emit (Verify n h) c

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
  , cnMode :: Mode
  , -- | The sequence the connection has been sent up to (not what it has
    -- applied — that is its own business).
    cnSent :: Seq
  }

data Server s = Server
  { svAuth :: Authenticate
  , -- | Does this user own this session? A session outlives its token: an
    -- entry authored offline under one login and pushed after the same
    -- person signs in again carries the old session, and is still theirs.
    -- Only ever asked about the connection's own user. By default, no.
    svOwns :: Text -> Text -> Bool
  , -- | May this identity receive the log? The read rule.
    svAccess :: Identity -> Bool
  , svAuthority :: Authority
  , svConns :: Map L.ConnId Conn
  , svMachine :: L.Machine s
  , svRooms :: L.Rooms s
  , svOut :: [(L.ConnId, ServerMsg)] -- ^ newest first
  }

-- | A server that is the authority for the log.
openServer :: Authenticate -> (Identity -> Bool) -> L.Machine s -> Authority -> Server s
openServer auth access m a = Server auth (\_ _ -> False) access a M.empty m L.emptyRooms []

-- | Install the sessions a user owns, which the authenticator's session
-- store knows and the engine does not.
withOwns :: (Text -> Text -> Bool) -> Server s -> Server s
withOwns owns sv = sv {svOwns = owns}

send :: L.ConnId -> ServerMsg -> Server s -> Server s
send c m sv = sv {svOut = (c, m) : svOut sv}

-- | §12.3 A frame from a connection.
serverRecv :: Server s -> L.ConnId -> ClientMsg -> Server s
serverRecv sv0 c msg = case msg of
  Hello (Subscription since md) tok _ -> case svAuth sv0 tok of
    Nothing -> send c (Denied "not signed in") sv0
    Just who
      | idUser who == ctxUser nobody -> send c (Denied "not signed in") sv0
      | not (svAccess sv0 who) -> send c (Denied "not allowed") sv0
      | otherwise ->
          -- A second Hello on one connection is the log paging, and says
          -- where to continue from; the room already has this peer, and
          -- 'L.arrive' with the same peer changes nothing.
          let sv1 = sv0 {svConns = M.insert c (Conn who md since) (svConns sv0)}
              (rooms, post) = L.arrive (svMachine sv1) (svRooms sv1) (L.Peer c (idUser who) (idSession who))
           in fanout (deliver post sv1 {svRooms = rooms})
  Push es -> withConn $ \conn ->
    let (a', acks, sv') = foldl (one conn) (svAuthority sv0, [], sv0) es
        sv'' = sv' {svAuthority = a'}
     in fanout (if null acks then sv'' else send c (Ack (map fst acks) (map snd acks)) sv'')
    where
      one conn (a, acks, sv) e
        | eActor e /= idUser (cnWho conn)
            || (eSession e /= idSession (cnWho conn) && not (svOwns sv0 (eActor e) (eSession e))) =
            (a, acks, send c (Reject (eId e) "not yours") sv)
        | otherwise = case sequenceEntry a e of
            (a', Appended n _) -> (a', acks ++ [(eId e, n)], sv)
            (a', Duplicate n) -> (a', acks ++ [(eId e, n)], sv)
            (a', Rejected why) -> (a', acks, send c (Reject (eId e) (refusalText why)) sv)
  NeedFacts ns -> withConn $ \_ ->
    let items = [(n, f) | n <- ns, Just (_, f) <- [M.lookup n (lEntries (aLog (svAuthority sv0)))]]
     in send c (FactsFor items) sv0
  NeedClosures hs -> withConn $ \_ ->
    let known = aBodies (svAuthority sv0)
     in send c (Closures [(h, cl) | h <- hs, Just cl <- [M.lookup h known]]) sv0
  Verify n h -> withConn $ \_ ->
    send c (Agree n h (fmap stateHash (stateAt (aLog (svAuthority sv0)) n) == Just h)) sv0
  Say f -> withConn $ \_ ->
    let (rooms, post) = L.speak (svMachine sv0) (svRooms sv0) c f
     in deliver post sv0 {svRooms = rooms}
  where
    withConn f = case M.lookup c (svConns sv0) of
      Just conn -> f conn
      Nothing -> send c (Denied "hello first") sv0

-- | §12.5 The reason a 'Reject' carries: a mutator's own refusal is its
-- text, word for word, because that is what an author wrote for a person
-- to read; the store's constraint refusals are named in a sentence.
refusalText :: Refusal -> Text
refusalText = \case
  Refused t -> t
  NoSuchTable t -> "no table " <> t
  MalformedRow t why -> t <> ": " <> why
  NotNull t col -> t <> "." <> col <> " may not be empty"
  UniqueViolation t cols -> t <> ": another row has the same " <> T.intercalate ", " cols
  MissingParent t col p -> t <> "." <> col <> " names no " <> p
  StillReferenced t child -> t <> ": still referenced by " <> child

-- | A connection closed: the room hears it, the cursor is forgotten.
disconnect :: Server s -> L.ConnId -> Server s
disconnect sv c =
  let (rooms, post) = L.depart (svMachine sv) (svRooms sv) c
   in deliver post sv {svRooms = rooms, svConns = M.delete c (svConns sv)}

deliver :: L.Post -> Server s -> Server s
deliver post sv = foldl (\acc (to, f) -> send to (Heard f) acc) sv (L.postOut post)

-- | §12.4 Fan-out: every connection, everything above what it has been
-- sent, a page at a time; a snapshot for one below the horizon. Run after
-- every message.
fanout :: Server s -> Server s
fanout sv0 = foldl perConn sv0 (M.toList (svConns sv0))
  where
    perConn sv (c, conn)
      | cnSent conn >= headSeq (aLog a) = sv
      | otherwise = case page a (cnSent conn) batchLimit of
          BelowHorizon sn ->
            let rows = M.fromList [(t, map VStruct (M.elems (S.rows (snStore sn) t))) | t <- S.tableNames (snStore sn)]
             in advanceSent c (snSeq sn) (send c (SnapshotOf (snSeq sn) (snHash sn) rows) sv)
          Entries items more ->
            let withFacts = [(n, e, if cnMode conn == ByFacts then Just f else Nothing) | (n, e, f) <- items]
                lastSeq = maximum (cnSent conn : [n | (n, _, _) <- items])
             in advanceSent c lastSeq (send c (Batch withFacts more) sv)
      where
        a = svAuthority sv
    advanceSent c n sv = sv {svConns = M.adjust (\cn -> cn {cnSent = n}) c (svConns sv)}

takeServerOutgoing :: Server s -> ([(L.ConnId, ServerMsg)], Server s)
takeServerOutgoing sv = (reverse (svOut sv), sv {svOut = []})
