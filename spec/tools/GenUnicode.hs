-- | Generates @src/Ark/Std/Unicode.hs@ - and the same tables for the
-- runtimes in other languages - from the Unicode Character Database.
--
-- Usage:
--
-- > ghc -O1 tools/GenUnicode.hs -o gen
-- > ./gen <directory holding UnicodeData.txt, DerivedCoreProperties.txt,
-- >        PropList.txt> <output file> [haskell|rust|swift|kotlin]
--
-- The target defaults to @haskell@, which writes the spec's own module.
-- The other three write one source file each - @unicode_tables.rs@,
-- @UnicodeTables.swift@, @UnicodeTables.kt@ - defining the same three
-- functions over the same data, so a runtime need not carry a UCD of
-- its own and cannot disagree with the spec about a code point.
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
-- How a table is written down is decided per compiler, because each has a
-- literal it is slow on:
--
--  * Haskell: one string literal of hex numbers, parsed at start-up.
--    GHC compiles a long 'String' in a moment and a long @[(Int, Int)]@
--    literal does not: 0.19s at -O0 for the whole module.
--  * Rust: @static@ arrays of @(u32, u32)@, which rustc compiles fast.
--  * Swift: flat @[UInt32]@ arrays, pairs interleaved, with an explicit
--    type annotation. The type checker is very slow on a long literal
--    of tuples and acceptable on a homogeneous one it need not infer.
--  * Kotlin: one string of hex tokens parsed once into an @IntArray@,
--    because an array literal in a field initializer is bytecode in
--    @<clinit>@ and the JVM caps a method at 64KB.
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
    [dir, out] -> generate dir out Haskell
    [dir, out, name] | Just target <- lookup name targets -> generate dir out target
    _ -> do
      hPutStrLn stderr
        "usage: GenUnicode <ucd-directory> <output> [haskell|rust|swift|kotlin]"
      exitFailure

-- | The languages a table can be written for.
data Target = Haskell | Rust | Swift | Kotlin

targets :: [(String, Target)]
targets = [("haskell", Haskell), ("rust", Rust), ("swift", Swift), ("kotlin", Kotlin)]

generate :: FilePath -> FilePath -> Target -> IO ()
generate dir out target = do
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
      render = case target of
        Haskell -> renderHaskell
        Rust -> renderRust
        Swift -> renderSwift
        Kotlin -> renderKotlin
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

-- ** Haskell

renderHaskell :: [Range] -> [Range] -> [(Int, Int)] -> String
renderHaskell white alnum lower = unlines $
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

-- | The Unicode version every table is generated from, and the one every
-- generated file names.
version :: String
version = "16.0.0"

-- | Items joined by @sep@ and packed onto lines of at most @width@
-- columns, each line indented by @indent@; an item longer than a line
-- gets one to itself.
pack :: String -> Int -> String -> [String] -> [String]
pack sep width indent = map (indent ++) . go
  where
    limit = width - length indent
    go [] = []
    go (x : xs) = let (line, rest) = fill x xs in line : go rest
    fill acc (y : ys)
      | length acc + length sep + length y <= limit = fill (acc ++ sep ++ y) ys
    fill acc ys = (acc, ys)

-- | Every code point of a table in order: a range as its two ends, a
-- mapping as source then target. The flat form Swift and Kotlin hold.
flat :: [(Int, Int)] -> [Int]
flat ps = concat [[a, b] | (a, b) <- ps]

-- | The header every generated file opens with, in that language's line
-- comment.
header :: String -> [String]
header comment = map (\l -> if null l then comment else comment ++ " " ++ l)
  [ "Unicode character properties, pinned to UCD " ++ version ++ "."
  , ""
  , "GENERATED by tools/GenUnicode.hs from the Unicode " ++ version
  , "Character Database. Do not edit: regenerate."
  , ""
  , "A runtime is conformant when its predicates agree with the spec's on"
  , "every code point, which is why the Unicode version is part of the spec"
  , "rather than whatever a platform's library happens to carry: two"
  , "runtimes on two operating systems must sort, match and lowercase the"
  , "same text the same way. These tables are the spec's, so they cannot"
  , "disagree with it."
  , ""
  , "The tables are inclusive code-point ranges, sorted and coalesced, and"
  , "the simple lowercase mapping as (from, to) pairs sorted by source, each"
  , "looked up by binary search."
  ]

-- ** Rust

renderRust :: [Range] -> [Range] -> [(Int, Int)] -> String
renderRust white alnum lower = unlines $
  header "//!" ++
  [ "//!"
  , "//! Core only: nothing here needs `std`."
  , ""
  , "use core::cmp::Ordering;"
  , ""
  , "/// The Unicode version every table here was generated from."
  , "pub const UNICODE_VERSION: &str = " ++ show version ++ ";"
  , ""
  , "/// Property `White_Space` (PropList.txt)."
  , "pub fn is_white_space(c: char) -> bool {"
  , "    in_ranges(&WHITE_SPACE, c as u32)"
  , "}"
  , ""
  , "/// `Alphabetic` (DerivedCoreProperties.txt) or a general category of"
  , "/// `Nd`, `Nl` or `No` (UnicodeData.txt) - the definition of"
  , "/// `char::is_alphanumeric`, pinned to this Unicode version rather than"
  , "/// to the one the compiler's `core` was built with."
  , "pub fn is_alphanumeric(c: char) -> bool {"
  , "    in_ranges(&ALPHANUMERIC, c as u32)"
  , "}"
  , ""
  , "/// The simple lowercase mapping (UnicodeData.txt field 13); identity"
  , "/// where the database gives none."
  , "///"
  , "/// Simple, not full: SpecialCasing.txt is not consulted, so U+0130"
  , "/// becomes U+0069, one code point, as the spec has it."
  , "pub fn to_lower_simple(c: char) -> char {"
  , "    let n = c as u32;"
  , "    match LOWER.binary_search_by(|&(from, _)| from.cmp(&n)) {"
  , "        Ok(i) => char::from_u32(LOWER[i].1).unwrap_or(c),"
  , "        Err(_) => c,"
  , "    }"
  , "}"
  , ""
  , "/// Is the code point inside one of the sorted, disjoint ranges?"
  , "fn in_ranges(table: &[(u32, u32)], n: u32) -> bool {"
  , "    table"
  , "        .binary_search_by(|&(lo, hi)| {"
  , "            if hi < n {"
  , "                Ordering::Less"
  , "            } else if lo > n {"
  , "                Ordering::Greater"
  , "            } else {"
  , "                Ordering::Equal"
  , "            }"
  , "        })"
  , "        .is_ok()"
  , "}"
  , ""
  , "/// " ++ show (length white) ++ " ranges."
  , "static WHITE_SPACE: [(u32, u32); " ++ show (length white) ++ "] = ["
  ] ++ pack " " 80 "    " (map pair white) ++
  [ "];"
  , ""
  , "/// " ++ show (length alnum) ++ " ranges."
  , "static ALPHANUMERIC: [(u32, u32); " ++ show (length alnum) ++ "] = ["
  ] ++ pack " " 80 "    " (map pair alnum) ++
  [ "];"
  , ""
  , "/// " ++ show (length lower) ++ " mappings, sorted by source code point."
  , "static LOWER: [(u32, u32); " ++ show (length lower) ++ "] = ["
  ] ++ pack " " 80 "    " (map pair lower) ++
  [ "];"
  ]
  where
    pair (a, b) = "(0x" ++ hex4 a ++ ", 0x" ++ hex4 b ++ "),"

-- ** Swift

renderSwift :: [Range] -> [Range] -> [(Int, Int)] -> String
renderSwift white alnum lower = unlines $
  header "//" ++
  [ "//"
  , "// Each table is a flat `[UInt32]` with its pairs interleaved - element"
  , "// 2i is the first of pair i and 2i+1 the second - because the type"
  , "// checker is very slow on a long literal of tuples and acceptable on an"
  , "// annotated homogeneous one."
  , ""
  , "public enum UnicodeTables {"
  , "  /// The Unicode version every table here was generated from."
  , "  public static let version = " ++ show version
  , ""
  , "  /// Property `White_Space` (PropList.txt)."
  , "  public static func isWhiteSpace(_ c: Unicode.Scalar) -> Bool {"
  , "    return inRanges(whiteSpace, c.value)"
  , "  }"
  , ""
  , "  /// `Alphabetic` (DerivedCoreProperties.txt) or a general category of"
  , "  /// `Nd`, `Nl` or `No` (UnicodeData.txt) - Rust's `char::is_alphanumeric`,"
  , "  /// pinned to this Unicode version rather than to the platform's ICU."
  , "  public static func isAlphanumeric(_ c: Unicode.Scalar) -> Bool {"
  , "    return inRanges(alphanumeric, c.value)"
  , "  }"
  , ""
  , "  /// The simple lowercase mapping (UnicodeData.txt field 13); identity"
  , "  /// where the database gives none."
  , "  ///"
  , "  /// Simple, not full: SpecialCasing.txt is not consulted, so U+0130"
  , "  /// becomes U+0069, one code point, as the spec has it."
  , "  public static func toLowerSimple(_ c: Unicode.Scalar) -> Unicode.Scalar {"
  , "    guard let i = floorIndex(lower, c.value), lower[2 * i] == c.value,"
  , "          let to = Unicode.Scalar(lower[2 * i + 1])"
  , "    else { return c }"
  , "    return to"
  , "  }"
  , ""
  , "  /// Is the code point inside one of the sorted, disjoint ranges?"
  , "  static func inRanges(_ table: [UInt32], _ n: UInt32) -> Bool {"
  , "    guard let i = floorIndex(table, n) else { return false }"
  , "    return n <= table[2 * i + 1]"
  , "  }"
  , ""
  , "  /// The index of the last pair whose first component is at most `n`."
  , "  static func floorIndex(_ table: [UInt32], _ n: UInt32) -> Int? {"
  , "    var lo = 0"
  , "    var hi = table.count / 2 - 1"
  , "    if hi < 0 || table[0] > n { return nil }"
  , "    // invariant: table[2 * lo] <= n, and every pair above hi is > n"
  , "    while lo < hi {"
  , "      let m = (lo + hi + 1) / 2"
  , "      if table[2 * m] <= n { lo = m } else { hi = m - 1 }"
  , "    }"
  , "    return lo"
  , "  }"
  , ""
  , "  /// " ++ show (length white) ++ " ranges."
  , "  static let whiteSpace: [UInt32] = ["
  ] ++ pack " " 80 "    " (map point (flat white)) ++
  [ "  ]"
  , ""
  , "  /// " ++ show (length alnum) ++ " ranges."
  , "  static let alphanumeric: [UInt32] = ["
  ] ++ pack " " 80 "    " (map point (flat alnum)) ++
  [ "  ]"
  , ""
  , "  /// " ++ show (length lower) ++ " mappings, sorted by source code point."
  , "  static let lower: [UInt32] = ["
  ] ++ pack " " 80 "    " (map point (flat lower)) ++
  [ "  ]"
  , "}"
  ]
  where
    point n = "0x" ++ hex4 n ++ ","

-- ** Kotlin

renderKotlin :: [Range] -> [Range] -> [(Int, Int)] -> String
renderKotlin white alnum lower = unlines $
  header "//" ++
  [ "//"
  , "// Each table is one string of hex tokens parsed once into an `IntArray`"
  , "// with its pairs interleaved - element 2i is the first of pair i and"
  , "// 2i+1 the second. An array literal in a field initializer is bytecode"
  , "// in `<clinit>`, and the JVM caps a method at 64KB; a string constant"
  , "// is one entry in the constant pool."
  , "//"
  , "// Code points are `Int`, because `Char` is a UTF-16 code unit."
  , ""
  , "package dev.arkdb.std"
  , ""
  , "object UnicodeTables {"
  , "    /** The Unicode version every table here was generated from. */"
  , "    const val VERSION = " ++ show version
  , ""
  , "    /** Property `White_Space` (PropList.txt). */"
  , "    fun isWhiteSpace(cp: Int): Boolean = inRanges(WHITE_SPACE, cp)"
  , ""
  , "    /**"
  , "     * `Alphabetic` (DerivedCoreProperties.txt) or a general category of"
  , "     * `Nd`, `Nl` or `No` (UnicodeData.txt) - Rust's `char::is_alphanumeric`,"
  , "     * pinned to this Unicode version rather than to the JVM's."
  , "     */"
  , "    fun isAlphanumeric(cp: Int): Boolean = inRanges(ALPHANUMERIC, cp)"
  , ""
  , "    /**"
  , "     * The simple lowercase mapping (UnicodeData.txt field 13); identity"
  , "     * where the database gives none."
  , "     *"
  , "     * Simple, not full: SpecialCasing.txt is not consulted, so U+0130"
  , "     * becomes U+0069, one code point, as the spec has it."
  , "     */"
  , "    fun toLowerSimple(cp: Int): Int {"
  , "        val i = floorIndex(LOWER, cp)"
  , "        return if (i >= 0 && LOWER[2 * i] == cp) LOWER[2 * i + 1] else cp"
  , "    }"
  , ""
  , "    /** Is the code point inside one of the sorted, disjoint ranges? */"
  , "    private fun inRanges(table: IntArray, n: Int): Boolean {"
  , "        val i = floorIndex(table, n)"
  , "        return i >= 0 && n <= table[2 * i + 1]"
  , "    }"
  , ""
  , "    /**"
  , "     * The index of the last pair whose first component is at most `n`,"
  , "     * or -1 when there is none."
  , "     */"
  , "    private fun floorIndex(table: IntArray, n: Int): Int {"
  , "        var lo = 0"
  , "        var hi = table.size / 2 - 1"
  , "        if (hi < 0 || table[0] > n) return -1"
  , "        // invariant: table[2 * lo] <= n, and every pair above hi is > n"
  , "        while (lo < hi) {"
  , "            val m = (lo + hi + 1) / 2"
  , "            if (table[2 * m] <= n) lo = m else hi = m - 1"
  , "        }"
  , "        return lo"
  , "    }"
  , ""
  , "    /** Comma-separated hex code points. */"
  , "    private fun parse(s: String): IntArray ="
  , "        s.split(',').map { it.toInt(16) }.toIntArray()"
  , ""
  , "    /** " ++ show (length white) ++ " ranges. */"
  , "    private val WHITE_SPACE: IntArray = parse("
  ] ++ chunks (flat white) ++
  [ "    )"
  , ""
  , "    /** " ++ show (length alnum) ++ " ranges. */"
  , "    private val ALPHANUMERIC: IntArray = parse("
  ] ++ chunks (flat alnum) ++
  [ "    )"
  , ""
  , "    /** " ++ show (length lower) ++ " mappings, sorted by source code point. */"
  , "    private val LOWER: IntArray = parse("
  ] ++ chunks (flat lower) ++
  [ "    )"
  , "}"
  ]
  where
    -- One string literal per line, joined with `+`; kotlinc folds the
    -- concatenation of constants into one constant.
    chunks ns = joinPlus (map quote (pack "" 66 "" (tokens ns)))
    tokens [] = []
    tokens [n] = [hex4 n]
    tokens (n : ns) = (hex4 n ++ ",") : tokens ns
    quote l = "        \"" ++ l ++ "\""
    joinPlus [] = []
    joinPlus [l] = [l]
    joinPlus (l : ls) = (l ++ " +") : joinPlus ls
