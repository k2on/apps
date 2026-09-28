{-# LANGUAGE OverloadedStrings #-}
-- | @arkc@: everything that is about a module rather than about running one.
--
-- @
-- arkc verify  m.ark               type-check and print the module hash
-- arkc hash    m.ark               the module hash and every function's hash
-- arkc print   m.ark               the diagnostic text form (what a diff shows)
-- arkc check   old.ark new.ark     log compatibility: every break, or nothing
-- arkc gen     rust|swift|kotlin  m.ark  OUTDIR  [--name Name] [--only f,g,h]
--                                  --only: generate only these functions (and
--                                  the helpers they reach); a peer applies the
--                                  rest by facts
-- arkc demo    OUT.ark             write the demo module (for the runtimes' first test)
-- @
--
-- A @.ark@ file is the module's canonical CBOR ('Ark.Encode.toValue' through
-- 'Ark.Canon.encode'); @arkc@ verifies before it does anything else, and
-- what it generates or hashes is the verified, normalised module.
module Main (main) where

import qualified Data.ByteString as B
import Data.Char (toLower)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Text.IO as TIO
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, hSetEncoding, stderr, stdout, utf8)

import Ark.Canon (decode, encode)
import Ark.Compat (check)
import Ark.Decode (fromValue)
import Ark.Demo (demoModule)
import Ark.Encode (toValue)
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
    ["demo", out] -> do
      m <- either (die . show) pure (verify demoModule)
      B.writeFile out (encode (toValue m))
      putStrLn (out ++ "  " ++ hex (moduleHash m) ++ "  " ++ show (M.size (closures m)) ++ " functions")
    _ -> die "usage: arkc verify M | print M | hash M | check OLD NEW | demo OUT"

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

