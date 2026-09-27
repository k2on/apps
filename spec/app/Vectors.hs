{-# LANGUAGE OverloadedStrings #-}
-- | Emits the conformance vectors, and in doing so runs the whole
-- specification end to end on a small domain: build a module, verify it,
-- apply a mutator, hash the result.
--
-- What is here today is the shape and a first case per directory; the
-- fuzzing generator and the remaining directories arrive with the modules
-- README.md lists as not yet written.
module Main (main) where

import qualified Data.ByteString as B
import Data.Char (toLower)
import Data.Int (Int64)
import Data.List (intercalate, sortBy)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.IO (IOMode (WriteMode), hPutStr, hSetEncoding, stdout, utf8, withFile)

import Ark.Canon (encode)
import Ark.Encode (toValue)
import Ark.Eval
import Ark.Hash
import Ark.IR
import Ark.Log
import Ark.Peer
import Ark.Schema
import Ark.Std (textOfId)
import Ark.Store (Change, Store)
import qualified Ark.Store as S
import Ark.Value
import Ark.Verify

main :: IO ()
main = do
  hSetEncoding stdout utf8
  args <- getArgs
  let out = case args of
        (d : _) -> d
        [] -> "vectors"
  mapM_ (createDirectoryIfMissing True . ((out ++ "/") ++)) ["codec", "codec/falsify", "order", "hash", "eval", "verify", "rebase"]
  codecVectors out
  orderVectors out
  demo out
  rebase out
  putStrLn "vectors written"

-- JSON, with the wrappers README.md describes ---------------------------

json :: Value -> String
json v = case v of
  VNull -> "null"
  VBool b -> if b then "true" else "false"
  VInt n -> "{\"$int\":\"" ++ show n ++ "\"}"
  VText t -> str t
  VBytes b -> "{\"$bytes\":\"" ++ hex b ++ "\"}"
  VId i -> "{\"$id\":\"" ++ T.unpack (textOfId i) ++ "\"}"
  VList xs -> "[" ++ intercalate "," (map json xs) ++ "]"
  VStruct m -> "{" ++ intercalate "," [str k ++ ":" ++ json x | (k, x) <- M.toList m] ++ "}"
  where
    str t = "\"" ++ concatMap esc (T.unpack t) ++ "\""
    esc c
      | c == '"' = "\\\""
      | c == '\\' = "\\\\"
      | c < ' ' = "\\u" ++ pad4 (showHex (fromEnum c) "")
      | otherwise = [c]
    pad4 s = replicate (4 - length s) '0' ++ s

hex :: B.ByteString -> String
hex = concatMap (\w -> let s = showHex w "" in if length s == 1 then '0' : s else s) . B.unpack

obj :: [(String, String)] -> String
obj kvs = "{\n" ++ intercalate ",\n" ["  \"" ++ k ++ "\": " ++ v | (k, v) <- kvs] ++ "\n}\n"

-- Files are written as UTF-8 whatever the locale says; a vector must be
-- the same bytes on every machine that emits it.
write :: FilePath -> String -> IO ()
write path s = do
  withFile path WriteMode (\h -> hSetEncoding h utf8 >> hPutStr h s)
  putStrLn ("  " ++ path)

-- A JSON string from a plain String.
quoted :: String -> String
quoted = json . VText . T.pack

-- codec/ ---------------------------------------------------------------

codecVectors :: FilePath -> IO ()
codecVectors out = do
  putStrLn "codec/"
  mapM_ one cases
  -- A wrong expectation the runner must fail on.
  write (out ++ "/codec/falsify/int-1-wrong-bytes.json") (obj [("value", json (VInt 1)), ("bytes", quoted "02"), ("expect", quoted "fail")])
  where
    one (name, v) =
      write (out ++ "/codec/" ++ name ++ ".json") (obj [("value", json v), ("bytes", quoted (hex (encode v)))])
    cases =
      [ ("null", VNull)
      , ("true", VBool True)
      , ("int-0", VInt 0)
      , ("int-24", VInt 24)
      , ("int-neg-1", VInt (-1))
      , ("int-min", VInt minBound)
      , ("int-max", VInt maxBound)
      , ("text-water", VText "水")
      , ("text-astral", VText "𐅑")
      , ("bytes", VBytes (B.pack [1, 2, 3, 4]))
      , ("id-nil", VId nilId)
      , ("list", VList [VInt 1, VList [VInt 2, VInt 3]])
      , ("struct-key-order", VStruct (M.fromList [("aa", VInt 1), ("b", VInt 2)]))
      ]

nilId :: IdBytes
nilId = case mkId (B.replicate 16 0) of
  Just i -> i
  Nothing -> error "sixteen zero bytes are an id"

-- order/ ---------------------------------------------------------------

orderVectors :: FilePath -> IO ()
orderVectors out = do
  putStrLn "order/"
  let vals =
        [ VText "e\x0301" -- e + combining acute: NOT equal to é
        , VText "é"
        , VText "\xFF5E" -- fullwidth tilde: after U+1F3B5 in UTF-16 units, before in code points
        , VText "\x1F3B5"
        , VInt 3
        , VNull
        , VBool False
        , VList []
        , VList [VInt 1]
        , VBytes B.empty
        , VStruct M.empty
        ]
      sorted = sortBy compareValue vals
  write (out ++ "/order/mixed.json") (obj [("input", json (VList vals)), ("sorted", json (VList sorted))])

-- eval/ and hash/: a small domain, end to end ---------------------------

-- The demo schema: one scope, a playlist and its items, keyed as harken's.
demoSchema :: Schema
demoSchema =
  Schema
    [ Scope
        "playlists"
        [ Table
            "playlist"
            [Column "id" (TId "playlist") False, Column "name" TText False, Column "user_id" TText False]
            ["id"]
            []
            []
        , Table
            "playlist_item"
            [ Column "playlist_id" (TId "playlist") False
            , Column "media_id" TBytes False
            , Column "pos" TInt False
            , Column "added_ms" TInt False
            , Column "user_id" TText False
            ]
            ["playlist_id", "media_id"]
            []
            [Ref "playlist_id" "playlist"]
        ]
    ]

-- | @add_to_playlist@, as a builder would emit it: the worked example of
-- docs/arkdb.md §3.4, in IR. Symbols are written as the builder numbered
-- them; the verifier renumbers.
addToPlaylist :: Function
addToPlaylist =
  Function
    { fnName = "add_to_playlist"
    , fnKind = Mutator
    , fnScope = Just "playlists"
    , fnAutos = [("added_ms", Now)]
    , fnArgs = [("playlist_id", TId "playlist"), ("media_id", TBytes)]
    , fnRet = Nothing
    , fnNames = M.fromList [(0, "rows"), (1, "r"), (2, "last")]
    , fnBody =
        [ SLet 10 (EExists "playlist" [EArg "playlist_id"])
        , SIf (EOp Not [EVar 10]) [SReturn Nothing] []
        , SLet 11 (EExists "playlist_item" [EArg "playlist_id", EArg "media_id"])
        , SIf (EVar 11) [SReturn Nothing] []
        , SLet
            0
            ( ESelect
                Plan
                  { pTable = "playlist_item"
                  , pFilter = Just (PCmp "playlist_id" Eq (EArg "playlist_id"))
                  , pOrder = [("pos", Desc)]
                  , pLimit = Just 1
                  , pRelated = []
                  }
            )
        , SLet 2 (EStd UnwrapOr [EMatch (EStd First [EVar 0]) 1 (ESome (EField (EVar 1) "pos")) (ENone TInt), ELit (VInt 0)])
        , SPut
            "playlist_item"
            ( EStruct
                ( M.fromList
                    [ ("playlist_id", EArg "playlist_id")
                    , ("media_id", EArg "media_id")
                    , ("pos", EOp Add [EVar 2, ELit (VInt 1)])
                    , ("added_ms", EAuto "added_ms")
                    , ("user_id", ECtxUser)
                    ]
                )
            )
        ]
    }

-- | @create_playlist@: the mutator whose auto is a fresh id.
createPlaylist :: Function
createPlaylist =
  Function
    { fnName = "create_playlist"
    , fnKind = Mutator
    , fnScope = Just "playlists"
    , fnAutos = [("id", NewId "playlist")]
    , fnArgs = [("name", TText)]
    , fnRet = Nothing
    , fnNames = M.empty
    , fnBody =
        [ SIf (EStd IsEmpty [EStd Trim [EArg "name"]]) [SRefuse (ELit (VText "a playlist needs a name"))] []
        , SLet 0 (EExists "playlist" [EAuto "id"])
        , SIf (EVar 0) [SReturn Nothing] []
        , SPut "playlist" (EStruct (M.fromList [("id", EAuto "id"), ("name", EStd Trim [EArg "name"]), ("user_id", ECtxUser)]))
        ]
    }

demoModule :: Module
demoModule = Module specVersion demoSchema [createPlaylist, addToPlaylist] []

demo :: FilePath -> IO ()
demo out = do
  putStrLn "verify/ eval/ hash/"
  m <- case verify demoModule of
    Left es -> error ("the demo module does not verify: " ++ show es)
    Right m -> pure m
  write (out ++ "/verify/demo-ok.json") (obj [("module", json (toValue m)), ("verifies", "true")])
  let pid = maybe (error "an id") id (mkId (B.pack (replicate 15 0 ++ [1])))
      playlistRow = M.fromList [("id", VId pid), ("name", VText "Favorites"), ("user_id", VText "alice")]
      st0 = either (error . show) fst (S.put (S.empty demoSchema) "playlist" playlistRow)
      ctx = Ctx "alice" "session-1"
      autos = M.fromList [("added_ms", VInt (1577836800000 :: Int64))]
      args k = M.fromList [("playlist_id", VId pid), ("media_id", VBytes (B.pack [k]))]
      step st k = case apply m "add_to_playlist" ctx autos (args k) st of
        Right (Right (st', chs)) -> (st', chs)
        other -> error ("apply: " ++ show other)
      (st1, ch1) = step st0 7
      (st2, ch2) = step st1 9
      (st3, ch3) = step st2 7 -- already there: a no-op
  write
    (out ++ "/eval/add-to-playlist.json")
    ( obj
        [ ("module", json (toValue m))
        , ("function", quoted "add_to_playlist")
        , ("function_hash", quoted (hex (functionHash (closure m addToPlaylist))))
        , ("store_before", json (storeValue st0))
        , ("ctx", json (VStruct (M.fromList [("user", VText "alice"), ("session", VText "session-1")])))
        , ("autos", json (VStruct autos))
        , ("steps", "[" ++ intercalate "," [stepJson (args 7) ch1 st1, stepJson (args 9) ch2 st2, stepJson (args 7) ch3 st3] ++ "]")
        ]
    )
  write (out ++ "/hash/demo-state.json") (obj [("store", json (storeValue st3)), ("hash", quoted (hex (stateHash st3)))])
  putStrLn ("  function hash " ++ hex (functionHash (closure m addToPlaylist)))
  putStrLn ("  state hash    " ++ hex (stateHash st3))
  putStrLn ("  changes       " ++ map toLower (show (length ch1, length ch2, length ch3)))
  where
    stepJson a chs st =
      obj
        [ ("args", json (VStruct a))
        , ("changes", json (VList (map changeValue chs)))
        , ("store_after", json (storeValue st))
        , ("hash_after", quoted (hex (stateHash st)))
        ]

storeValue :: Store -> Value
storeValue st = VStruct (M.fromList [(t, VList (map VStruct (M.elems (S.rows st t)))) | t <- S.tableNames st])

changeValue :: Change -> Value
changeValue = \c -> case c of
  S.Add t r -> VStruct (M.fromList [("t", VText "add"), ("table", VText t), ("row", VStruct r)])
  S.Remove t r -> VStruct (M.fromList [("t", VText "remove"), ("table", VText t), ("row", VStruct r)])
  S.Edit t o n -> VStruct (M.fromList [("t", VText "edit"), ("table", VText t), ("old", VStruct o), ("new", VStruct n)])


-- rebase/: three peers and an authority, then the roads not taken ---------

-- | The whole of §11 on one scenario, asserted as it goes: the file is
-- written only if every claim below holds, so a spec change that breaks
-- one fails `ark-vectors` rather than emitting a wrong vector.
rebase :: FilePath -> IO ()
rebase out = do
  putStrLn "rebase/"
  m <- either (error . show) pure (verify demoModule)
  let sch = modSchema m
      bodies = closures m
      hashOf name = head [h | (h, c) <- M.toList bodies, fnName (cFn c) == name]
      hCreate = hashOf "create_playlist"
      hAdd = hashOf "add_to_playlist"
      idN k = maybe (error "id") id (mkId (B.pack (replicate 15 0 ++ [k])))
      pid = idN 1
      ctx who = Ctx who (who <> "-session")
      now = M.fromList [("added_ms", VInt 1577836800000)]
      addArgs k = M.fromList [("playlist_id", VId pid), ("media_id", VBytes (B.pack [k]))]
      must what = either (\e -> error (what ++ ": " ++ show e)) id
      claim what ok = if ok then pure () else error ("rebase: " ++ what)
      -- an authority, and three replicas that hold the generated code
      auth0 = authority sch "playlists" bodies
      fresh = open sch "playlists" bodies (S.empty sch) 0 []
      alice0 = fresh
      bob0 = fresh
      -- the authority answers one pushed entry and both connected peers hear it
      push a e = case sequenceEntry a e of
        (a', Appended n facts) -> (a', n, facts)
        (_, other) -> error ("push: " ++ show other)
      -- step 1: alice creates the playlist; everybody sees it
      (alice1, e1) = must "create" (mutate alice0 (idN 101) (ctx "alice") hCreate (M.fromList [("id", VId pid)]) (M.fromList [("name", VText " Favorites ")]))
      (auth1, s1, _) = push auth0 e1
      alice2 = ack alice1 (eId e1) s1
      bob1 = receive bob0 s1 e1
  claim "the playlist was sequenced first" (s1 == 1)
  claim "alice's name was trimmed" ((M.lookup "name" =<< S.get (rView alice2) "playlist" [VId pid]) == Just (VText "Favorites"))
  claim "alice and bob agree after step 1" (verifyAt alice2 == verifyAt bob1)
  let -- step 2: alice goes dark. bob adds two tracks; alice adds one alone.
      (bob2, e2) = must "bob adds 1" (mutate bob1 (idN 102) (ctx "bob") hAdd now (addArgs 1))
      (auth2, s2, _) = push auth1 e2
      bob3 = ack bob2 (eId e2) s2
      (bob4, e3) = must "bob adds 2" (mutate bob3 (idN 103) (ctx "bob") hAdd now (addArgs 2))
      (auth3, s3, _) = push auth2 e3
      bob5 = ack bob4 (eId e3) s3
      (_, alice2') = takeChanges alice2 -- the ack rebuilt her view; a screen has drawn it since
      (alice3, e9) = must "alice adds 9 alone" (mutate alice2' (idN 109) (ctx "alice") hAdd now (addArgs 9))
      posOf r k = M.lookup "pos" =<< S.get (rView r) "playlist_item" [VId pid, VBytes (B.pack [k])]
      (chA, alice3') = takeChanges alice3
  claim "alone, alice's track is first on her view" (posOf alice3 9 == Just (VInt 1))
  claim "a local mutation reports its changes, not a rebuild" (case chA of Applied [_] -> True; _ -> False)
  claim "bob's tracks are 1 and 2 on his view" (posOf bob5 1 == Just (VInt 1) && posOf bob5 2 == Just (VInt 2))
  let -- step 3: alice comes back. bob's entries land; her pending replays on top.
      alice4 = receive (receive alice3' s2 e2) s3 e3
      (chB, alice4') = takeChanges alice4
  claim "the rebase is reported as a rebuild" (chB == Rebuilt)
  claim "after the rebase alice's track is third" (posOf alice4 9 == Just (VInt 3))
  claim "alice's confirmed state is bob's" (verifyAt alice4' == verifyAt bob5)
  let -- alice pushes what she did alone; it lands after everything that happened while she was away
      (auth4, s9, f9) = push auth3 e9
      alice5 = ack (receiveFacts alice4' s9 f9) (eId e9) s9
      bob6 = receive bob5 s9 e9
      (chC, alice5') = takeChanges alice5
  claim "nothing is pending on alice once acked" (null (rPending alice5))
  claim "with nothing pending the ack costs no rebuild" (case chC of Applied _ -> True; Rebuilt -> False)
  claim "alice's view is her confirmed store" (rView alice5 == rConfirmed alice5)
  claim "the authority's facts say pos 3 too" (any (\c -> case c of S.Add _ row -> M.lookup "pos" row == Just (VInt 3); _ -> False) f9)
  claim "three replicas, one hash" (verifyAt alice5' == verifyAt bob6 && snd (verifyAt bob6) == stateHash (aStore auth4))
  let -- a duplicate delivery changes nothing
      bob7 = receive bob6 s2 e2
  claim "a duplicate delivery is a no-op" (bob7 == bob6)
  let -- carol holds no generated code at all: she applies by facts
      carol0 = open sch "playlists" M.empty (S.empty sch) 0 []
      carol1 = foldl (\r (n, e) -> receive r n e) carol0 [(s1, e1), (s2, e2), (s3, e3), (s9, e9)]
  claim "without closures carol asks for every entry's facts" (needs carol1 == [1, 2, 3, 4])
  let factsOf n = case M.lookup n (lEntries (aLog auth4)) of Just (_, f) -> f; Nothing -> error "no facts"
      carol2 = foldl (\r n -> receiveFacts r n (factsOf n)) carol1 [1 .. 4]
  claim "by facts alone carol reaches the same state" (verifyAt carol2 == verifyAt bob6 && null (rDiverged carol2))
  let -- dave's build of add_to_playlist is wrong: it steps by two. Facts catch it.
      wrong = (bodies M.! hAdd) {cFn = (cFn (bodies M.! hAdd)) {fnBody = map stepByTwo (fnBody (cFn (bodies M.! hAdd)))}}
      stepByTwo st = case st of
        SPut t (EStruct fs) -> SPut t (EStruct (M.adjust (const (EOp Add [EVar 4, ELit (VInt 2)])) "pos" fs))
        other -> other
      dave0 = open sch "playlists" (M.insert hAdd wrong bodies) (S.empty sch) 0 []
      dave1 = foldl (\r (n, e) -> receiveWith r n e (factsOf n)) dave0 [(s1, e1), (s2, e2), (s3, e3), (s9, e9)]
  claim "a divergent runtime is detected" (rDiverged dave1 == [2, 3, 4])
  claim "and healed by the facts" (verifyAt dave1 == verifyAt bob6)
  let -- eve has no server: she is her own authority, and later hands the scope over
      eve0 = fresh
      eveAuth0 = authority sch "playlists" bodies
      (eve1, _) = must "eve creates" (mutate eve0 (idN 201) (ctx "eve") hCreate (M.fromList [("id", VId (idN 2))]) (M.fromList [("name", VText "Road")]))
      (eve2, _) = must "eve adds" (mutate eve1 (idN 202) (ctx "eve") hAdd now (M.fromList [("playlist_id", VId (idN 2)), ("media_id", VBytes (B.pack [5]))]))
      (eveAuth1, eve3) = localCommit eveAuth0 eve2
  claim "alone, eve confirms her own intents" (rCursor eve3 == 2 && null (rPending eve3) && rView eve3 == rConfirmed eve3)
  claim "and her state is her authority's" (snd (verifyAt eve3) == stateHash (aStore eveAuth1))
  let adopted = adopt sch "playlists" bodies (aLog eveAuth1)
  claim "a server adopts her scope by replaying it" (either (const False) (\a -> stateHash (aStore a) == snd (verifyAt eve3)) adopted)
  let tampered = (aLog eveAuth1) {lEntries = M.adjust (\(e, f) -> (e, map bump f)) 2 (lEntries (aLog eveAuth1))}
      bump c = case c of S.Add t row -> S.Add t (M.insert "pos" (VInt 99) row); other -> other
  claim "a log whose facts were touched is refused" (adopt sch "playlists" bodies tampered == Left (FactsDiffer 2))
  let -- compaction: the authority moves its horizon to 2
      auth5 = maybe (error "compact") id (compact auth4 2)
  claim "a peer at 0 is sent the snapshot" (case page auth5 0 10 of BelowHorizon sn -> snSeq sn == 2; _ -> False)
  claim "a peer at 2 is sent the tail" (case page auth5 2 10 of Entries es False -> map (\(n, _, _) -> n) es == [3, 4]; _ -> False)
  claim "the state at the head, from facts, is the head state" (fmap stateHash (stateAt (aLog auth5) 4) == Just (stateHash (aStore auth5)))
  write
    (out ++ "/rebase/three-peers.json")
    ( obj
        [ ("module", json (toValue m))
        , ("entries", json (VList [entryValue n e | (n, (e, _)) <- M.toList (lEntries (aLog auth4))]))
        , ("facts", json (VList [VList (map changeValue f) | (_, (_, f)) <- M.toList (lEntries (aLog auth4))]))
        , ("alice_alone_pos_of_9", json (VInt 1))
        , ("alice_after_rebase_pos_of_9", json (VInt 3))
        , ("final_hash", quoted (hex (stateHash (aStore auth4))))
        , ("final_store", json (storeValue (aStore auth4)))
        ]
    )
  putStrLn ("  final hash    " ++ hex (stateHash (aStore auth4)))
  where
    entryValue n e =
      VStruct
        ( M.fromList
            [ ("seq", VInt n)
            , ("id", VId (eId e))
            , ("actor", VText (eActor e))
            , ("session", VText (eSession e))
            , ("fn", VBytes (eFn e))
            , ("args", VStruct (eArgs e))
            , ("autos", VStruct (eAutos e))
            ]
        )
