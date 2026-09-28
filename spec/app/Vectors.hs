{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
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
import Data.List (intercalate, sortBy)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.IO (IOMode (WriteMode), hPutStr, hSetEncoding, stdout, utf8, withFile)

import Ark.Canon (decode, encode)
import Ark.Decode (fromValue)
import Ark.Demo
import Ark.Encode (toValue)
import Ark.Eval
import Ark.Hash
import Ark.IR
import Ark.Log
import Ark.Peer
import Ark.Protocol
import Ark.Schema
import Ark.Std (textOfId)
import Ark.Store (Store)
import qualified Ark.Store as S
import Ark.Sim
import Ark.Value
import qualified Ark.View as V
import Ark.Verify

main :: IO ()
main = do
  hSetEncoding stdout utf8
  args <- getArgs
  let out = case args of
        (d : _) -> d
        [] -> "vectors"
  mapM_ (createDirectoryIfMissing True . ((out ++ "/") ++)) ["codec", "codec/falsify", "order", "hash", "eval", "verify", "rebase", "protocol", "module", "views"]
  codecVectors out
  orderVectors out
  demo out
  rebase out
  moduleVectors out
  protocolVectors out
  simVectors out
  viewVectors out
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

demo :: FilePath -> IO ()
demo out = do
  putStrLn "verify/ eval/ hash/"
  m <- case verify demoModule of
    Left es -> error ("the demo module does not verify: " ++ show es)
    Right m -> pure m
  write (out ++ "/verify/demo-ok.json") (obj [("module", json (toValue m)), ("verifies", "true")])
  -- Two modules that must not verify, each a one-line edit of the demo: an
  -- insert matched on columns that are not a declared unique index, and a
  -- procedure running middleware its router does not declare.
  let refused what m' want = case verify m' of
        Left es | any want es -> pure ()
        other -> error ("verify: " ++ what ++ " was not refused as it should be: " ++ take 300 (show other))
      editing name f = demoModule {modFunctions = [if fnName fn == name then f fn else fn | fn <- modFunctions demoModule]}
      onName = \fn -> fn {fnBody = [case st of SInsert t e _ -> SInsert t e ["name"]; other -> other | st <- fnBody fn]}
  refused "an insert on columns that are no unique index" (editing "create_playlist" onName) (\case In _ (OnNotUnique _ _) -> True; _ -> False)
  refused "a procedure running middleware its router lacks" (editing "items" (\fn -> fn {fnUses = ["nope"]})) (\case In _ (UsesNotOnRouter _) -> True; _ -> False)
  let pid = maybe (error "an id") id (mkId (B.pack (replicate 15 0 ++ [1])))
      playlistRow = M.fromList [("id", VId pid), ("name", VText "Favorites"), ("user_id", VText "alice")]
      st0 = either (error . show) fst (S.put (S.empty demoSchema) "playlist" playlistRow)
      ctx = Ctx "alice" "session-1"
      autos = M.empty
      args k = M.fromList [("playlist_id", VId pid), ("track_id", VText (T.pack ("t" ++ show k)))]
      step st k = case apply m "add_to_playlist" ctx autos (args k) st of
        Right (Right (st', chs)) -> (st', chs)
        other -> error ("apply: " ++ show other)
      -- The function as verified: orders completed. Hashing the authored
      -- form gave a hash no entry ever names, which two runtimes caught.
      verifiedAdd = maybe (error "add_to_playlist") id (lookupFunction m "add_to_playlist")
      (st1, ch1) = step st0 7
      (st2, ch2) = step st1 9
      (st3, ch3) = step st2 7 -- already there: a no-op
      (st4, ch4) = step st3 11 -- lands third: which is only true reading pos DESCENDING
  write
    (out ++ "/eval/add-to-playlist.json")
    ( obj
        [ ("module", json (toValue m))
        , ("function", quoted "add_to_playlist")
        , ("function_hash", quoted (hex (functionHash (closure m verifiedAdd))))
        , ("store_before", json (storeValue st0))
        , ("ctx", json (VStruct (M.fromList [("user", VText "alice"), ("session", VText "session-1")])))
        , ("autos", json (VStruct autos))
        , ("steps", "[" ++ intercalate "," [stepJson (args 7) ch1 st1, stepJson (args 9) ch2 st2, stepJson (args 7) ch3 st3, stepJson (args 11) ch4 st4] ++ "]")
        ]
    )
  -- A state hash depends on the schema's table order, so the vector
  -- carries the module the store belongs to.
  write (out ++ "/hash/demo-state.json") (obj [("module", json (toValue m)), ("store", json (storeValue st4)), ("hash", quoted (hex (stateHash st4)))])
  putStrLn ("  function hash " ++ hex (functionHash (closure m verifiedAdd)))
  putStrLn ("  state hash    " ++ hex (stateHash st4))
  putStrLn ("  changes       " ++ map toLower (show (length ch1, length ch2, length ch3, length ch4)))
  -- The checks, the middleware order and the form validator, as verdicts.
  let cid = maybe (error "an id") id (mkId (B.pack (replicate 15 0 ++ [2])))
      verdictOf name autos' args' st = case apply m name ctx autos' args' st of
        Right (Left r) -> case r of
          S.Refused t -> Just t
          other -> Just (T.pack (show other))
        Right (Right _) -> Nothing
        Left e -> error ("apply " ++ T.unpack name ++ ": " ++ show e)
      createAutos = M.fromList [("id", VId cid)]
      cases =
        [ ("trim-then-min", "create_playlist", createAutos, M.fromList [("name", VText "   ")], Just "a playlist needs a name")
        , ("trimmed-name-lands", "create_playlist", createAutos, M.fromList [("name", VText "  Road  ")], Nothing)
        , ("same-name-again-is-a-no-op", "create_playlist", createAutos, M.fromList [("name", VText "Favorites")], Nothing)
        , ("exists-check-default-message", "add_to_playlist", M.empty, M.fromList [("playlist_id", VId cid), ("track_id", VText "t1")], Just "playlist_id: no such playlist")
        , ("min-len-default-message", "add_to_playlist", M.empty, M.fromList [("playlist_id", VId pid), ("track_id", VText "")], Just "track_id: at least 1 characters")
        ]
  mapM_ (\(what, name, autos', args', want) -> if verdictOf name autos' args' st4 == want then pure () else error ("eval " ++ what ++ ": " ++ show (verdictOf name autos' args' st4))) cases
  case apply m "create_playlist" ctx createAutos (M.fromList [("name", VText "  Road  ")]) st4 of
    Right (Right (st', [S.Add "playlist" row])) | M.lookup "name" row == Just (VText "Road") && M.size (S.rows st' "playlist") == 2 -> pure ()
    other -> error ("eval trimmed-name-lands: " ++ show (fmap (fmap snd) other))
  case apply m "create_playlist" ctx createAutos (M.fromList [("name", VText "Favorites")]) st4 of
    Right (Right (_, [])) -> pure ()
    other -> error ("eval same-name-again: " ++ show (fmap (fmap snd) other))
  write
    (out ++ "/eval/checks.json")
    ( obj
        [ ("module", json (toValue m))
        , ("store_before", json (storeValue st4))
        , ("ctx", json (VStruct (M.fromList [("user", VText "alice"), ("session", VText "session-1")])))
        , ("cases", "[" ++ intercalate "," [obj [("name", quoted what), ("function", quoted (T.unpack name)), ("autos", json (VStruct autos')), ("args", json (VStruct args')), ("refused", maybe "null" (quoted . T.unpack) want)] | (what, name, autos', args', want) <- cases] ++ "]")
        ]
    )
  -- The form validator: every field's first failure, and the input as normalised.
  let formCases =
        [ ("empty-name", "create_playlist", M.fromList [("name", VText " ")])
        , ("trimmed", "create_playlist", M.fromList [("name", VText " Mix ")])
        , ("partial-input", "add_to_playlist", M.fromList [("track_id", VText "")])
        , ("unknown-playlist", "add_to_playlist", M.fromList [("playlist_id", VId cid), ("track_id", VText "t")])
        ]
      form name args' = either (\e -> error ("check: " ++ show e)) id (check m name ctx args' st4)
  if fst (form "create_playlist" (M.fromList [("name", VText " ")])) == [("name", "a playlist needs a name")] then pure () else error "check: empty name"
  if snd (form "create_playlist" (M.fromList [("name", VText " Mix ")])) == M.fromList [("name", VText "Mix")] then pure () else error "check: trim"
  if fst (form "add_to_playlist" (M.fromList [("track_id", VText "")])) == [("track_id", "track_id: at least 1 characters")] then pure () else error "check: partial"
  write
    (out ++ "/eval/form-check.json")
    ( obj
        [ ("module", json (toValue m))
        , ("store", json (storeValue st4))
        , ("cases", "[" ++ intercalate "," [obj [("name", quoted what), ("function", quoted (T.unpack name)), ("input", json (VStruct args')), ("messages", json (VList [VStruct (M.fromList [("field", VText f), ("message", VText w)]) | (f, w) <- fst (form name args')])), ("normalised", json (VStruct (snd (form name args'))))] | (what, name, args') <- formCases] ++ "]")
        ]
    )
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
      now = M.empty
      addArgs k = M.fromList [("playlist_id", VId pid), ("track_id", VText (T.pack ("t" ++ show k)))]
      must what = either (\e -> error (what ++ ": " ++ show e)) id
      claim what ok = if ok then pure () else error ("rebase: " ++ what)
      -- an authority, and three replicas that hold the generated code
      auth0 = authority sch bodies
      fresh = open sch bodies (S.empty sch) 0 []
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
      posOf r k = M.lookup "pos" =<< S.get (rView r) "item" [VId pid, VText (T.pack ("t" ++ show k))]
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
      carol0 = open sch M.empty (S.empty sch) 0 []
      carol1 = foldl (\r (n, e) -> receive r n e) carol0 [(s1, e1), (s2, e2), (s3, e3), (s9, e9)]
  claim "without closures carol asks for every entry's facts" (needs carol1 == [1, 2, 3, 4])
  let factsOf n = case M.lookup n (lEntries (aLog auth4)) of Just (_, f) -> f; Nothing -> error "no facts"
      carol2 = foldl (\r n -> receiveFacts r n (factsOf n)) carol1 [1 .. 4]
  claim "by facts alone carol reaches the same state" (verifyAt carol2 == verifyAt bob6 && null (rDiverged carol2))
  let -- dave's build of add_to_playlist is wrong: it steps by two. Facts catch it.
      wrong = (bodies M.! hAdd) {cFn = (cFn (bodies M.! hAdd)) {fnBody = map stepByTwo (fnBody (cFn (bodies M.! hAdd)))}}
      stepByTwo st = case st of
        SInsert t (EStruct fs) on -> SInsert t (EStruct (M.adjust (\e -> case e of EOp Add [a, _] -> EOp Add [a, ELit (VInt 2)]; other -> other) "pos" fs)) on
        other -> other
      dave0 = open sch (M.insert hAdd wrong bodies) (S.empty sch) 0 []
      dave1 = foldl (\r (n, e) -> receiveWith r n e (factsOf n)) dave0 [(s1, e1), (s2, e2), (s3, e3), (s9, e9)]
  claim "a divergent runtime is detected" (rDiverged dave1 == [2, 3, 4])
  claim "and healed by the facts" (verifyAt dave1 == verifyAt bob6)
  let -- eve has no server: she is her own authority, and later hands the log over
      eve0 = fresh
      eveAuth0 = authority sch bodies
      (eve1, _) = must "eve creates" (mutate eve0 (idN 201) (ctx "eve") hCreate (M.fromList [("id", VId (idN 2))]) (M.fromList [("name", VText "Road")]))
      (eve2, _) = must "eve adds" (mutate eve1 (idN 202) (ctx "eve") hAdd now (M.fromList [("playlist_id", VId (idN 2)), ("track_id", VText "t5")]))
      (eveAuth1, eve3) = localCommit eveAuth0 eve2
  claim "alone, eve confirms her own intents" (rCursor eve3 == 2 && null (rPending eve3) && rView eve3 == rConfirmed eve3)
  claim "and her state is her authority's" (snd (verifyAt eve3) == stateHash (aStore eveAuth1))
  let adopted = adopt sch bodies (aLog eveAuth1)
  claim "a server adopts her log by replaying it" (either (const False) (\a -> stateHash (aStore a) == snd (verifyAt eve3)) adopted)
  let tampered = (aLog eveAuth1) {lEntries = M.adjust (\(e, f) -> (e, map bump f)) 2 (lEntries (aLog eveAuth1))}
      bump c = case c of S.Add t row -> S.Add t (M.insert "pos" (VInt 99) row); other -> other
  claim "a log whose facts were touched is refused" (adopt sch bodies tampered == Left (FactsDiffer 2))
  let -- compaction: the authority moves its horizon to 2
      auth5 = maybe (error "compact") id (compact auth4 2)
  claim "a peer at 0 is sent the snapshot" (case page auth5 0 10 of BelowHorizon sn -> snSeq sn == 2; _ -> False)
  claim "a peer at 2 is sent the tail" (case page auth5 2 10 of Entries es False -> map (\(n, _, _) -> n) es == [3, 4]; _ -> False)
  claim "the state at the head, from facts, is the head state" (fmap stateHash (stateAt (aLog auth5) 4) == Just (stateHash (aStore auth5)))
  write
    (out ++ "/rebase/three-peers.json")
    ( obj
        [ ("module", json (toValue m))
        , ("entries", json (VList [entryWithSeq n e | (n, (e, _)) <- M.toList (lEntries (aLog auth4))]))
        , ("facts", json (VList [VList (map changeValue f) | (_, (_, f)) <- M.toList (lEntries (aLog auth4))]))
        , ("alice_alone_pos_of_9", json (VInt 1))
        , ("alice_after_rebase_pos_of_9", json (VInt 3))
        , ("final_hash", quoted (hex (stateHash (aStore auth4))))
        , ("final_store", json (storeValue (aStore auth4)))
        ]
    )
  putStrLn ("  final hash    " ++ hex (stateHash (aStore auth4)))
  where
    entryWithSeq n e = case entryValue e of
      VStruct m -> VStruct (M.insert "seq" (VInt n) m)
      other -> other

-- module/: the module as bytes, and back --------------------------------

moduleVectors :: FilePath -> IO ()
moduleVectors out = do
  putStrLn "module/"
  m <- either (error . show) pure (verify demoModule)
  let v = toValue m
      bytes = encode v
  case decode bytes >>= either (error . show) Right . fromValue of
    Right m' | m' == m {modFunctions = map (\f -> f {fnNames = M.empty}) (modFunctions m)} -> pure ()
    Right _ -> error "module: decode . encode is not the identity"
    Left e -> error ("module: " ++ show e)
  write (out ++ "/module/demo.json") (obj [("module", json v), ("bytes", quoted (hex bytes)), ("hash", quoted (hex (moduleHash m)))])

-- protocol/: every frame, as bytes, and back ------------------------------

protocolVectors :: FilePath -> IO ()
protocolVectors out = do
  putStrLn "protocol/"
  m <- either (error . show) pure (verify demoModule)
  let idN k = maybe (error "id") id (mkId (B.pack (replicate 15 0 ++ [k])))
      hAdd = head [h | (h, c) <- M.toList (closures m), fnName (cFn c) == "add_to_playlist"]
      entry = Entry (idN 9) "alice" "alice-dev" hAdd (M.fromList [("playlist_id", VId (idN 1)), ("track_id", VText "t7")]) M.empty
      row = M.fromList [("playlist_id", VId (idN 1)), ("track_id", VText "t7"), ("pos", VInt 1)]
      clientFrames =
        [ ("hello", Hello (Subscription 4 Whole) (Just "tok") 3)
        , ("hello-facts", Hello (Subscription 0 ByFacts) Nothing 3)
        , ("push", Push [entry])
        , ("need_facts", NeedFacts [2, 3])
        , ("need_closures", NeedClosures [hAdd])
        , ("verify", Verify 4 (B.replicate 32 0xab))
        , ("say", Say (B.pack [1, 2, 3]))
        ]
      serverFrames =
        [ ("batch", Batch [(5, entry, Nothing), (6, entry, Just [S.Add "item" row])] True)
        , ("facts", FactsFor [(2, [S.Add "item" row, S.Remove "item" row])])
        , ("snapshot", SnapshotOf 2 (B.replicate 32 0xcd) (M.fromList [("item", [VStruct row])]))
        , ("ack", Ack [idN 9] [5])
        , ("reject", Reject (idN 9) "a playlist needs a name")
        , ("denied", Denied "not signed in")
        , ("closures", Closures [(hAdd, closures m M.! hAdd)])
        , ("agree", Agree 4 (B.replicate 32 0xab) True)
        , ("heard", Heard (B.pack [4, 5]))
        ]
  -- An entry from an older session of the same user is accepted only where
  -- ownership is installed; a stranger's never is.
  let sv0 = openServer trusting (const True) silent (authority (modSchema m) (closures m))
      sv2 = serverRecv sv0 1 (Hello (Subscription 0 Whole) (Just "alice") 3)
  let hCreate = head [h | (h, c) <- M.toList (closures m), fnName (cFn c) == "create_playlist"]
      old = Entry (idN 20) "alice" "alice-old" hCreate (M.fromList [("name", VText "Road")]) (M.fromList [("id", VId (idN 21))])
      verdictOn sv = case [msg | (1, msg) <- fst (takeServerOutgoing (serverRecv (snd (takeServerOutgoing sv)) 1 (Push [old]))), isVerdict msg] of
        [Ack {}] -> "ack"
        [Reject _ why] -> T.unpack why
        other' -> show other'
      isVerdict = \case
        Ack {} -> True
        Reject {} -> True
        _ -> False
  if verdictOn sv2 == "not yours" then pure () else error ("protocol: an older session was accepted with no ownership: " ++ verdictOn sv2)
  if verdictOn (withOwns (\u _ -> u == "alice") sv2) == "ack" then pure () else error ("protocol: an owned older session was refused: " ++ verdictOn (withOwns (\u _ -> u == "alice") sv2))
  let bob = old {eActor = "bob"}
      strangers = [msg | (1, msg) <- fst (takeServerOutgoing (serverRecv (snd (takeServerOutgoing (withOwns (\_ _ -> True) sv2))) 1 (Push [bob]))), isVerdict msg] == [Reject (eId bob) "not yours"]
  if strangers then pure () else error "protocol: another user's entry was accepted"
  -- A peer used for a while with no account, then signed in: everything it
  -- authored as nobody is pushed as the person who signed in, every entry
  -- is accepted, and the rows say who they belong to.
  let hAddP = head [h | (h, c) <- M.toList (closures m), fnName (cFn c) == "add_to_playlist"]
      pidL = idN 30
      authorAll r0 =
        foldl
          (\r (i, fh, autos, args) -> either (error . show) fst (mutate r (idN i) nobody fh autos args))
          r0
          ( (31, hCreate, M.fromList [("id", VId pidL)], M.fromList [("name", VText "Offline")])
              : [(32 + k, hAddP, M.empty, M.fromList [("playlist_id", VId pidL), ("track_id", VText (T.pack ("t" ++ show k)))]) | k <- [0 .. 9]]
          )
      local = authorAll (open (modSchema m) (closures m) (S.empty (modSchema m)) 0 [])
      signedIn = connected (clientSignIn (Ctx "alice" "dev") (Just "alice") (openClient local Whole Nothing))
      (sent, _) = takeOutgoing signedIn
      svEnd = foldl (\sv f -> serverRecv sv 7 f) sv0 sent
      verdicts = [msg | (7, msg) <- fst (takeServerOutgoing svEnd), isVerdict msg]
      acked = sum [length ids | Ack ids _ <- verdicts]
      owners = [M.lookup "user_id" row | row <- M.elems (S.rows (aStore (svAuthority svEnd)) "playlist")]
      unsigned = [msg | (7, msg) <- fst (takeServerOutgoing (foldl (\sv f -> serverRecv sv 7 f) sv0 (fst (takeOutgoing (connected (openClient local Whole Nothing)))))), isVerdict msg]
  if acked == 11 && null [() | Reject {} <- verdicts] then pure () else error ("protocol: a signed-in peer's offline work was not all accepted: " ++ show verdicts)
  if owners == [Just (VText "alice")] then pure () else error ("protocol: the offline playlist belongs to " ++ show owners)
  if all (\case Reject _ why -> why == "not yours"; _ -> False) unsigned && length unsigned == 11
    then pure ()
    else error ("protocol: work authored as nobody was accepted without signing in: " ++ show unsigned)
  mapM_
    (\(name, f) -> do
      let v = clientValue f
      case decode (encode v) >>= either (error . show) Right . clientFromValue of
        Right f' | f' == f -> pure ()
        other -> error ("protocol client " ++ name ++ ": " ++ show other)
      write (out ++ "/protocol/client-" ++ name ++ ".json") (obj [("frame", json v), ("bytes", quoted (hex (encode v)))]))
    clientFrames
  mapM_
    (\(name, f) -> do
      let v = serverValue f
      case decode (encode v) >>= either (error . show) Right . serverFromValue of
        Right f' | sameFrame f' f -> pure ()
        other -> error ("protocol server " ++ name ++ ": " ++ show other)
      write (out ++ "/protocol/server-" ++ name ++ ".json") (obj [("frame", json v), ("bytes", quoted (hex (encode v)))]))
    serverFrames
  where
    -- Closures decode with empty symbol names, so compare through their values.
    sameFrame (Closures a) (Closures b) = map fst a == map fst b && map (closureValue . snd) a == map (closureValue . snd) b
    sameFrame a b = a == b

-- rebase/: the fleet ---------------------------------------------------

-- | A seeded fleet of three over the demo domain: adds, partitions, heals
-- and two hundred random deliveries with duplicates and drops; then
-- settle, and every replica must hash as the authority does.
simVectors :: FilePath -> IO ()
simVectors out = do
  putStrLn "rebase/ (fleet)"
  m <- either (error . show) pure (verify demoModule)
  let sch = modSchema m
      bodies = closures m
      hashOf name = head [h | (h, c) <- M.toList bodies, fnName (cFn c) == name]
      hCreate = hashOf "create_playlist"
      hAdd = hashOf "add_to_playlist"
      idOf :: Int -> IdBytes
      idOf k = maybe (error "id") id (mkId (B.pack [fromIntegral (k `div` 256), fromIntegral (k `mod` 256)] <> B.replicate 14 0))
      pid = idOf 1
      now = M.empty
      sim0 = newSim sch bodies 3 7
      -- one playlist, created by peer 0 and delivered to all
      sim1 = settle (simMutate sim0 0 (idOf 1000) hCreate (M.fromList [("id", VId pid)]) (M.fromList [("name", VText "Fleet")]))
      -- a scripted mess: adds from everyone, a partition, more adds, random deliveries
      script = concat [[Add' i k | i <- [0 .. 2]] | k <- [1 .. 4]] ++ [Part 2] ++ [Add' 2 k | k <- [5 .. 7]] ++ [Add' 0 8, Add' 1 9] ++ replicate 60 Step ++ [Part 0] ++ [Add' 1 10, Add' 0 11] ++ replicate 60 Step ++ [Heal 0, Heal 2] ++ replicate 80 Step
      run (sim, n) op = case op of
        Add' i k -> (simMutate sim i (idOf (2000 + n)) hAdd now (M.fromList [("playlist_id", VId pid), ("track_id", VText (T.pack (show i ++ "-" ++ show k)))]), n + 1)
        Part i -> (partition sim i, n)
        Heal i -> (heal sim i, n)
        Step -> (step sim, n)
      (sim2, _) = foldl run (sim1, 0 :: Int) script
      sim3 = settle sim2
      clients = clientHashes sim3
      (headN, serverH) = serverHash sim3
  if all (\(_, n, h) -> n == headN && h == serverH) clients then pure () else error ("fleet did not converge: " ++ show (map (\(i, n, h) -> (i, n, hex h)) clients) ++ " vs " ++ hex serverH)
  if quiet sim3 then pure () else error "fleet: something still pending after settle"
  let rejected = [(i, rj) | (i, c) <- M.toList (simClients sim3), rj <- rRejections (clReplica c)]
  if null rejected then pure () else error ("fleet: rejections: " ++ show rejected)
  if headN >= 12 then pure () else error ("fleet: too few entries landed: " ++ show headN)
  let a = svAuthority (simServer sim3)
      items = S.rows (aStore a) "item"
  putStrLn ("  " ++ show (M.size items) ++ " items on the playlist after " ++ show headN ++ " entries; hash " ++ hex serverH)
  write
    (out ++ "/rebase/fleet-seed-7.json")
    ( obj
        [ ("module", json (toValue m))
        , ("clients", json (VInt 3))
        , ("seed", json (VInt 7))
        , ("script", json (VList (map opValue script)))
        , ("expected_head", json (VInt headN))
        , ("expected_hash", quoted (hex serverH))
        , ("final_store", json (storeValue (aStore a)))
        ]
    )
  where
    opValue = \case
      Add' i k -> VStruct (M.fromList [("t", VText "add"), ("peer", VInt (fromIntegral i)), ("track", VText (T.pack (show i ++ "-" ++ show k)))])
      Part i -> VStruct (M.fromList [("t", VText "partition"), ("peer", VInt (fromIntegral i))])
      Heal i -> VStruct (M.fromList [("t", VText "heal"), ("peer", VInt (fromIntegral i))])
      Step -> VStruct (M.fromList [("t", VText "step")])

data Op = Add' Int Int | Part Int | Heal Int | Step

-- views/: a maintained plan through the three-peer scenario's facts --------

-- | The playlist's items, ordered by position, limited to two, maintained
-- through every change the fleet scenario produced; and the same with the
-- playlist's items hanging beneath the playlist row. The patches are what
-- a screen splices; the contract is that the rows equal a fresh hydrate
-- after every step.
viewVectors :: FilePath -> IO ()
viewVectors out = do
  putStrLn "views/"
  m <- either (error . show) pure (verify demoModule)
  let sch = modSchema m
      idN k = maybe (error "id") id (mkId (B.pack (replicate 15 0 ++ [k])))
      pid = idN 1
      now = M.empty
      hashOf name = head [h | (h, c) <- M.toList (closures m), fnName (cFn c) == name]
      -- a straight sequence of entries on one authority: create, add 1..4, remove one, add 5
      a0 = authority sch (closures m)
      addE k eid = Entry (idN eid) "alice" "dev" (hashOf "add_to_playlist") (M.fromList [("playlist_id", VId pid), ("track_id", VText (T.pack ("t" ++ show (k :: Int))))]) now
      entries =
        [ Entry (idN 100) "alice" "dev" (hashOf "create_playlist") (M.fromList [("name", VText "Viewed")]) (M.fromList [("id", VId pid)])
        , addE 1 101, addE 2 102, addE 3 103, addE 4 104
        ]
      (aN, factsList) = foldl (\(a, fs) e -> case sequenceEntry a e of (a', Appended _ f) -> (a', fs ++ [f]); other -> error ("view seq: " ++ show other)) (a0, []) entries
      -- a delete written by hand as a fact, then another add, to exercise a refill
      removeFirst = [S.Remove "item" (head (M.elems (S.rows (aStore aN) "item")))]
      (aM, moreFacts) = case sequenceEntry aN {aStore = S.applyChanges (aStore aN) removeFirst} (addE 5 105) of
        (a', Appended _ f) -> (a', [removeFirst, f])
        other -> error ("view seq 2: " ++ show other)
      allFacts = factsList ++ moreFacts
      plans =
        [ ("top-two-by-pos", V.ViewPlan "item" (Just (V.FCmp "playlist_id" Eq (VId pid))) [("pos", Asc)] (Just 2) [])
        , ("playlist-with-items", V.ViewPlan "playlist" Nothing [("name", Asc)] Nothing [("item", Relation "playlist" "item" "playlist_id", V.ViewPlan "item" Nothing [("pos", Desc)] (Just 3) [])])
        ]
      run vp =
        let step (st, view, acc) facts =
              let (st', view', patches) = foldl (\(s, v, ps) ch -> let s' = S.applyChange s ch; (v', p) = V.push sch s' ch v in (s', v', ps ++ p)) (st, view, []) facts
               in if V.contract sch vp st' view' then (st', view', acc ++ [(patches, V.rows view')]) else error "view contract broken"
            st0 = S.empty sch
            (_, _, steps) = foldl step (st0, V.hydrate sch vp st0, []) allFacts
         in steps
  mapM_
    (\(name, vp) -> do
      let steps = run vp
      -- The top-level plan must show a removal and the refill after it; the
      -- nested one must show a child change surfacing as an update of its
      -- parent.
      let ok = case name of
            "top-two-by-pos" -> any (\(ps, _) -> any isRemove ps) steps && any (\(ps, _) -> length ps >= 2) steps
            _ -> any (\(ps, _) -> any isUpdate ps) steps
      if ok then pure () else error ("view " ++ name ++ ": the patches do not show what the plan is for")
      write
        (out ++ "/views/" ++ name ++ ".json")
        ( obj
            [ ("module", json (toValue m))
            , ("plan", quoted (show vp))
            , ("changes", json (VList [VList (map changeValue f) | f <- allFacts]))
            , ("steps", "[" ++ intercalate "," [obj [("patches", json (VList (map patchValue ps))), ("rows", json (VList rows))] | (ps, rows) <- steps] ++ "]")
            ]
        ))
    plans
  putStrLn ("  " ++ show (length allFacts) ++ " changes through " ++ show (length plans) ++ " plans; " ++ show (aM == aM))
  where
    isRemove V.Remove {} = True
    isRemove _ = False
    isUpdate V.Update {} = True
    isUpdate _ = False
    patchValue = \case
      V.Insert i n -> VStruct (M.fromList [("t", VText "insert"), ("at", VInt (fromIntegral i)), ("node", n)])
      V.Remove i -> VStruct (M.fromList [("t", VText "remove"), ("at", VInt (fromIntegral i))])
      V.Update i n -> VStruct (M.fromList [("t", VText "update"), ("at", VInt (fromIntegral i)), ("node", n)])
