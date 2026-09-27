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
  mapM_ (createDirectoryIfMissing True . ((out ++ "/") ++)) ["codec", "codec/falsify", "order", "hash", "eval", "verify"]
  codecVectors out
  orderVectors out
  demo out
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

demoModule :: Module
demoModule = Module specVersion demoSchema [addToPlaylist] []

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
        , ("function_hash", quoted (hex (functionHash addToPlaylist)))
        , ("store_before", json (storeValue st0))
        , ("ctx", json (VStruct (M.fromList [("user", VText "alice"), ("session", VText "session-1")])))
        , ("autos", json (VStruct autos))
        , ("steps", "[" ++ intercalate "," [stepJson (args 7) ch1 st1, stepJson (args 9) ch2 st2, stepJson (args 7) ch3 st3] ++ "]")
        ]
    )
  write (out ++ "/hash/demo-state.json") (obj [("store", json (storeValue st3)), ("hash", quoted (hex (stateHash st3)))])
  putStrLn ("  function hash " ++ hex (functionHash addToPlaylist))
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

