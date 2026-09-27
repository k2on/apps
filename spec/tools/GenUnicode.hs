-- | Generates @src/Ark/Std/Unicode.hs@ from the Unicode Character Database.
--
-- Usage:
--
-- > ghc -O1 tools/GenUnicode.hs -o gen
-- > ./gen <directory holding UnicodeData.txt, DerivedCoreProperties.txt,
-- >        PropList.txt> src/Ark/Std/Unicode.hs
--
-- The generated module is committed, so that building the spec needs no
-- UCD download; this program is how it moves when the pinned Unicode
-- version does. It is deterministic: the same three files produce a
-- byte-identical module, so a regeneration that changes nothing shows
-- nothing in a diff.
--
-- Three things are read:
--
--  * @PropList.txt@, property @White_Space@;
--  * @DerivedCoreProperties.txt@, property @Alphabetic@;
--  * @UnicodeData.txt@, general categories @Nd@, @Nl@, @No@ (which, with
--    @Alphabetic@, is Rust's @char::is_alphanumeric@) and field 13, the
--    simple lowercase mapping.
--
-- @UnicodeData.txt@ writes a large block of identical characters as two
-- lines, @<…, First>@ and @<…, Last>@; the category of such a pair covers
-- the whole range. None of those blocks is @Nd@, @Nl@ or @No@ today, but
-- the pairing is honoured rather than assumed away.
--
-- Each table is emitted as one string literal of hex numbers and parsed at
-- start-up, because GHC compiles a long 'String' in a moment and a long
-- @[(Int, Int)]@ literal does not: 0.19s at -O0 for the whole module.
module Main (main) where

import Data.Char (isSpace, toUpper)
import Data.List (foldl', intercalate, sortOn)
import Numeric (readHex, showHex)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO
  ( IOMode (ReadMode, WriteMode), hGetContents, hPutStr, hPutStrLn
  , hSetEncoding, stderr, utf8, withFile )

main :: IO ()
main = do
  args <- getArgs
  case args of
    [dir, out] -> generate dir out
    _ -> do
      hPutStrLn stderr "usage: GenUnicode <ucd-directory> <output.hs>"
      exitFailure

generate :: FilePath -> FilePath -> IO ()
generate dir out = do
  propList <- readUtf8 (dir ++ "/PropList.txt")
  derived <- readUtf8 (dir ++ "/DerivedCoreProperties.txt")
  unicodeData <- readUtf8 (dir ++ "/UnicodeData.txt")
  let white = merge (property "White_Space" propList)
      alphabetic = property "Alphabetic" derived
      rows = unicodeRows unicodeData
      numeric = [r | (r, cat, _) <- rows, cat `elem` ["Nd", "Nl", "No"]]
      alnum = merge (alphabetic ++ numeric)
      lower = sortOn fst
        [ (lo, to) | ((lo, hi), _, Just to) <- rows, lo == hi ]
  writeUtf8 out (render white alnum lower)
  putStrLn ("wrote " ++ out)
  putStrLn ("  White_Space ranges:        " ++ show (length white))
  putStrLn ("  Alphabetic ranges (input): " ++ show (length alphabetic))
  putStrLn ("  Nd|Nl|No ranges (input):   " ++ show (length numeric))
  putStrLn ("  alphanumeric ranges:       " ++ show (length alnum))
  putStrLn ("  simple lowercase mappings: " ++ show (length lower))

-- | The UCD is UTF-8 whatever the locale says, and so is the output.
readUtf8 :: FilePath -> IO String
readUtf8 path = withFile path ReadMode $ \h -> do
  hSetEncoding h utf8
  s <- hGetContents h
  length s `seq` return s

writeUtf8 :: FilePath -> String -> IO ()
writeUtf8 path s = withFile path WriteMode $ \h -> do
  hSetEncoding h utf8
  hPutStr h s

-- * Parsing

type Range = (Int, Int)

-- | The ranges a property covers in a @PropList@-style file: lines of
-- @XXXX[..YYYY] ; Property # comment@.
property :: String -> String -> [Range]
property name file =
  [ r
  | line <- lines file
  , let body = takeWhile (/= '#') line
  , (range : prop : _) <- [map trim (splitOn ';' body)]
  , prop == name
  , Just r <- [parseRange range]
  ]

parseRange :: String -> Maybe Range
parseRange s = case breakOn ".." s of
  (a, Just b) -> (,) <$> hex a <*> hex b
  (a, Nothing) -> (\c -> (c, c)) <$> hex a

-- | Every row of @UnicodeData.txt@ as (range, general category, simple
-- lowercase mapping), with @First@/@Last@ pairs folded into one range.
unicodeRows :: String -> [(Range, String, Maybe Int)]
unicodeRows file = go (map (splitOn ';') (filter (not . null) (lines file)))
  where
    go (a : b : rest)
      | isFirst (name a), isLast (name b), Just lo <- code a, Just hi <- code b =
          ((lo, hi), category a, Nothing) : go rest
    go (a : rest)
      | Just c <- code a = ((c, c), category a, lowerOf a) : go rest
      | otherwise = go rest
    go [] = []
    code fs = hex (field 0 fs)
    name fs = field 1 fs
    category fs = field 2 fs
    lowerOf fs = case field 13 fs of
      "" -> Nothing
      h -> hex h
    field i fs = if i < length fs then fs !! i else ""
    isFirst n = ", First>" `isSuffixOf'` n
    isLast n = ", Last>" `isSuffixOf'` n
    isSuffixOf' suf s = reverse suf == take (length suf) (reverse s)

-- | Sort and coalesce ranges; adjacent and overlapping ranges become one.
merge :: [Range] -> [Range]
merge = reverse . foldl' step [] . sortOn fst
  where
    step ((lo, hi) : acc) (lo', hi')
      | lo' <= hi + 1 = (lo, max hi hi') : acc
    step acc r = r : acc

hex :: String -> Maybe Int
hex s = case readHex s of
  [(n, "")] -> Just n
  _ -> Nothing

trim :: String -> String
trim = dropWhile isSpace . reverse . dropWhile isSpace . reverse

splitOn :: Char -> String -> [String]
splitOn c s = case break (== c) s of
  (a, []) -> [a]
  (a, _ : rest) -> a : splitOn c rest

breakOn :: String -> String -> (String, Maybe String)
breakOn pat = go []
  where
    n = length pat
    go acc s
      | take n s == pat = (reverse acc, Just (drop n s))
      | otherwise = case s of
          [] -> (reverse acc, Nothing)
          (c : rest) -> go (c : acc) rest

-- * Rendering

hex4 :: Int -> String
hex4 n = let h = map toUpper (showHex n "") in replicate (4 - length h) '0' ++ h

rangeText :: [Range] -> String
rangeText rs = intercalate ","
  [ if lo == hi then hex4 lo else hex4 lo ++ "-" ++ hex4 hi | (lo, hi) <- rs ]

mapText :: [(Int, Int)] -> String
mapText ms = intercalate "," [ hex4 from ++ ":" ++ hex4 to | (from, to) <- ms ]

-- | A string literal wrapped with string gaps, so the module stays under
-- eighty columns without the literal being anything but one string.
literal :: String -> String
literal s = "  \"" ++ intercalate "\\\n  \\" (chunks 72 s) ++ "\""
  where
    chunks n xs
      | null xs = [""]
      | otherwise = go xs
      where
        go [] = []
        go ys = let (a, b) = splitAt n ys in a : go b

render :: [Range] -> [Range] -> [(Int, Int)] -> String
render white alnum lower = unlines $
  [ "-- | Unicode character properties, pinned to UCD " ++ version ++ "."
  , "--"
  , "-- GENERATED by tools/GenUnicode.hs from the Unicode " ++ version
  , "-- Character Database. Do not edit: regenerate."
  , "--"
  , "-- A runtime in another language is conformant when its predicates agree"
  , "-- with these on every code point, which is why the Unicode version is"
  , "-- part of the spec rather than whatever a platform's library happens to"
  , "-- carry: two runtimes on two operating systems must sort, match and"
  , "-- lowercase the same text the same way."
  , "--"
  , "-- The tables are inclusive code-point ranges, sorted and coalesced, held"
  , "-- as one string of hex numbers that is parsed once into an array and"
  , "-- looked up by binary search. A string literal this long compiles in"
  , "-- a fraction of a second at -O0; a list of tuples this long does not."
  , "module Ark.Std.Unicode"
  , "  ( unicodeVersion"
  , "  , isWhiteSpace"
  , "  , isAlphanumeric"
  , "  , toLowerSimple"
  , "  ) where"
  , ""
  , "import Data.Array (Array, bounds, listArray, (!))"
  , "import Data.Char (chr, ord)"
  , "import Numeric (readHex)"
  , ""
  , "-- | The Unicode version every table here was generated from."
  , "unicodeVersion :: String"
  , "unicodeVersion = " ++ show version
  , ""
  , "-- | Property @White_Space@ (PropList.txt)."
  , "isWhiteSpace :: Char -> Bool"
  , "isWhiteSpace c = inRanges whiteSpaceTable (ord c)"
  , ""
  , "-- | @Alphabetic@ (DerivedCoreProperties.txt) or a general category of"
  , "-- @Nd@, @Nl@ or @No@ (UnicodeData.txt) - the definition of Rust's"
  , "-- @char::is_alphanumeric@, and deliberately not GHC's 'Data.Char.isAlphaNum',"
  , "-- which is @L*@ or @N*@ by general category alone and disagrees on, for"
  , "-- instance, the combining marks and the Roman numeral letters."
  , "isAlphanumeric :: Char -> Bool"
  , "isAlphanumeric c = inRanges alphanumericTable (ord c)"
  , ""
  , "-- | The simple lowercase mapping (UnicodeData.txt field 13); identity"
  , "-- where the database gives none."
  , "--"
  , "-- Simple, not full: SpecialCasing.txt is not consulted. The full mapping"
  , "-- is one-to-many (@\\x130@ lowercases to two code points) and in places"
  , "-- locale-sensitive (Turkish and Lithuanian), and neither property belongs"
  , "-- in a function every runtime has to reproduce; the spec pins the simple"
  , "-- map on purpose. So @\\x130@ becomes @\\x69@ here, one code point, as"
  , "-- Rust's simple mapping and ICU's @u_tolower@ have it."
  , "toLowerSimple :: Char -> Char"
  , "toLowerSimple c = case lookupExact lowerTable (ord c) of"
  , "  Just to -> chr to"
  , "  Nothing -> c"
  , ""
  , "-- * Lookup"
  , ""
  , "-- | Is the code point inside one of the sorted, disjoint ranges?"
  , "inRanges :: Array Int (Int, Int) -> Int -> Bool"
  , "inRanges arr n = case floorIndex arr n of"
  , "  Just i -> n <= snd (arr ! i)"
  , "  Nothing -> False"
  , ""
  , "-- | The value paired with exactly this key, in a sorted array of pairs."
  , "lookupExact :: Array Int (Int, Int) -> Int -> Maybe Int"
  , "lookupExact arr n = case floorIndex arr n of"
  , "  Just i | fst (arr ! i) == n -> Just (snd (arr ! i))"
  , "  _ -> Nothing"
  , ""
  , "-- | The index of the last pair whose first component is at most @n@."
  , "floorIndex :: Array Int (Int, Int) -> Int -> Maybe Int"
  , "floorIndex arr n"
  , "  | hi < lo || fst (arr ! lo) > n = Nothing"
  , "  | otherwise = Just (go lo hi)"
  , "  where"
  , "    (lo, hi) = bounds arr"
  , "    -- invariant: fst (arr ! a) <= n, and every index above b is > n"
  , "    go a b"
  , "      | a == b = a"
  , "      | fst (arr ! m) <= n = go m b"
  , "      | otherwise = go a (m - 1)"
  , "      where m = (a + b + 1) `div` 2"
  , ""
  , "-- * Tables"
  , ""
  , "-- | Inclusive ranges @LO-HI@ or single points @LO@, comma-separated hex."
  , "parseRanges :: String -> Array Int (Int, Int)"
  , "parseRanges s = toArray (map range (splitOn ',' s))"
  , "  where"
  , "    range item = case break (== '-') item of"
  , "      (a, []) -> (hex a, hex a)"
  , "      (a, _ : b) -> (hex a, hex b)"
  , ""
  , "-- | Pairs @FROM:TO@, comma-separated hex."
  , "parsePairs :: String -> Array Int (Int, Int)"
  , "parsePairs s = toArray (map pair (splitOn ',' s))"
  , "  where"
  , "    pair item = case break (== ':') item of"
  , "      (a, _ : b) -> (hex a, hex b)"
  , "      (a, []) -> error (\"Ark.Std.Unicode: malformed pair \" ++ a)"
  , ""
  , "toArray :: [a] -> Array Int a"
  , "toArray xs = listArray (0, length xs - 1) xs"
  , ""
  , "hex :: String -> Int"
  , "hex h = case readHex h of"
  , "  [(n, \"\")] -> n"
  , "  _ -> error (\"Ark.Std.Unicode: malformed hex \" ++ h)"
  , ""
  , "splitOn :: Char -> String -> [String]"
  , "splitOn c s = case break (== c) s of"
  , "  (a, []) -> [a]"
  , "  (a, _ : rest) -> a : splitOn c rest"
  , ""
  , "-- | " ++ show (length white) ++ " ranges."
  , "whiteSpaceTable :: Array Int (Int, Int)"
  , "whiteSpaceTable = parseRanges"
  , literal (rangeText white)
  , ""
  , "-- | " ++ show (length alnum) ++ " ranges."
  , "alphanumericTable :: Array Int (Int, Int)"
  , "alphanumericTable = parseRanges"
  , literal (rangeText alnum)
  , ""
  , "-- | " ++ show (length lower) ++ " mappings, sorted by source code point."
  , "lowerTable :: Array Int (Int, Int)"
  , "lowerTable = parsePairs"
  , literal (mapText lower)
  ]
  where
    version = "16.0.0"
