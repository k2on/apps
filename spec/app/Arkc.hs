{-# LANGUAGE OverloadedStrings #-}
-- | @arkc@: everything that is about a module rather than about running one.
--
-- @
-- arkc verify    M                 type-check and print the module hash
-- arkc hash      M                 the module hash and every function's hash
-- arkc print     M                 the diagnostic text form (what a diff shows)
-- arkc check     OLD NEW           log compatibility: every break, or nothing
-- arkc gen       rust|swift|kotlin M OUTDIR [OPTIONS]
--                                  the authoring form: the schema, a file per
--                                  router, and the module file
-- arkc roundtrip rust|swift|kotlin M SRCDIR [OPTIONS]
--                                  print into a temporary directory and compare
--                                  with SRCDIR, comment-only lines aside; exit 1
--                                  on any difference
-- arkc demo      OUT               write the demo module
--
-- OPTIONS: --only f,g   print only these procedures and what they reach
--          --name N     the struct of tables every router is over
--          --package p  the Kotlin package (default "domain")
--          --fmt CMD    run CMD over the printed files (a formatter: the
--                       canonical text is its output)
-- @
--
-- A @.ark@ file is the module's canonical CBOR ('Ark.Encode.toValue' through
-- 'Ark.Canon.encode'); @arkc@ verifies before it does anything else, and
-- what it prints or hashes is the verified, normalised module.
module Main (main) where

import Control.Monad (forM, forM_, unless)
import qualified Data.ByteString as B
import Data.Char (toLower)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Text.IO as TIO
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing, doesFileExist, getTemporaryDirectory, removeDirectoryRecursive)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, hSetEncoding, stderr, stdout, utf8)
import System.Process (callCommand)

import Ark.Canon (decode, encode)
import Ark.Compat (check)
import Ark.Decode (fromValue)
import Ark.Demo (demoModule)
import Ark.Encode (toValue)
import Ark.Gen
import Ark.Hash (closure, closures, functionHash, moduleHash)
import Ark.IR
import Ark.Print (printModule)
import Ark.Verify

main :: IO ()
main = do
  hSetEncoding stdout utf8
  args <- getArgs
  case args of
    ["verify", path] -> load path >>= \m -> putStrLn (hex (moduleHash m))
    ["print", path] -> load path >>= TIO.putStr . printModule
    ["check", old, new] -> do
      a <- load old
      b <- load new
      case check a b of
        [] -> putStrLn "additive: every retained entry still applies"
        breaks -> mapM_ (hPutStrLn stderr . show) breaks >> exitFailure
    ["hash", path] -> do
      m <- load path
      putStrLn ("module   " ++ hex (moduleHash m))
      mapM_ (\fn -> putStrLn (hex (functionHash (closure m fn)) ++ "  " ++ T.unpack (fnName fn))) (modFunctions m)
    ("gen" : target : path : out : rest) -> do
      written <- gen target path out rest
      mapM_ putStrLn written
    ("roundtrip" : target : path : src : rest) -> do
      tmp <- getTemporaryDirectory
      let out = tmp ++ "/arkc-roundtrip-" ++ map toLower target
      removeIfThere out
      written <- gen target path out rest
      diffs <- forM written $ \file -> do
        let name = drop (length out + 1) file
            theirs = src ++ "/" ++ name
        present <- doesFileExist theirs
        if not present
          then pure [name ++ ": not in " ++ src]
          else do
            a <- TIO.readFile file
            b <- TIO.readFile theirs
            pure (compareText name (uncomment a) (uncomment b))
      removeIfThere out
      case concat diffs of
        [] -> putStrLn ("roundtrip " ++ target ++ ": " ++ show (length written) ++ " files, identical")
        ds -> mapM_ (hPutStrLn stderr) ds >> exitFailure
    ["demo", out] -> do
      m <- either (die . show) pure (verify demoModule)
      B.writeFile out (encode (toValue m))
      putStrLn (out ++ "  " ++ hex (moduleHash m) ++ "  " ++ show (M.size (closures m)) ++ " functions")
    _ -> die "usage: arkc verify M | print M | hash M | check OLD NEW | gen LANG M OUTDIR [--only f,g] [--package p] [--name T] [--fmt CMD] | roundtrip LANG M SRCDIR [...] | demo OUT"

-- Print a module's authoring form into a directory, format it, and name
-- the files written.
gen :: String -> FilePath -> FilePath -> [String] -> IO [FilePath]
gen target path out rest = do
  t <- case map toLower target of
    "rust" -> pure Rust
    "swift" -> pure Swift
    "kotlin" -> pure Kotlin
    other -> die ("unknown target " ++ other)
  let opts = pairs rest
      only = fmap (T.splitOn "," . T.pack) (lookup "--only" opts)
      options = Options (maybe "domain" T.pack (lookup "--package" opts)) (maybe "Tables" T.pack (lookup "--name" opts))
  whole <- load path
  let m = maybe whole (`restrict` whole) only
  fs <- either (die . T.unpack) pure (files t options m)
  createDirectoryIfMissing True out
  written <- forM fs $ \(name, text) -> do
    let file = out ++ "/" ++ name
    TIO.writeFile file text
    pure file
  forM_ (lookup "--fmt" opts) $ \cmd -> callCommand (cmd ++ concatMap (\f -> " '" ++ f ++ "'") written)
  pure written

-- The first line at which two texts part, or nothing.
compareText :: String -> T.Text -> T.Text -> [String]
compareText name a b
  | a == b = []
  | otherwise =
      let la = T.lines a
          lb = T.lines b
          firstDiff = length (takeWhile id (zipWith (==) la lb))
          at xs = if firstDiff < length xs then T.unpack (xs !! firstDiff) else "<end of file>"
       in [ name ++ ":" ++ show (firstDiff + 1) ++ ": the print and the source part here"
          , "  printed: " ++ at la
          , "  source:  " ++ at lb
          ]

removeIfThere :: FilePath -> IO ()
removeIfThere dir = do
  here <- doesFileExist (dir ++ "/.")
  unless (not here) (removeDirectoryRecursive dir)

pairs :: [String] -> [(String, String)]
pairs (k : v : rest) = (k, v) : pairs rest
pairs _ = []

-- | Read, decode and verify a module; anything wrong is fatal and named.
load :: FilePath -> IO Module
load path = do
  bytes <- B.readFile path
  v <- either (die . ("not canonical CBOR: " ++) . show) pure (decode bytes)
  m <- either (die . ("not a module: " ++) . show) pure (fromValue v)
  either (die . unlines . map show) pure (verify m)

die :: String -> IO a
die s = hPutStrLn stderr ("arkc: " ++ s) >> exitFailure

hex :: B.ByteString -> String
hex = concatMap (\w -> let s = showHex w "" in if length s == 1 then '0' : s else s) . B.unpack
