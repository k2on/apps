{-# LANGUAGE OverloadedStrings #-}
-- | §5 The standard library.
--
-- Everything a function may compute that is not an operator or a read. The
-- admission rule is: an exact definition here, vectors for it, and an
-- implementation in each language's pinned @ArkStd@ that calls nothing of
-- the platform's. Because the IR is compiled rather than interpreted, a new
-- function costs one mapping per generator and one implementation per
-- @ArkStd@; because everything here is in the log forever, the library
-- still grows slowly.
--
-- Unicode is the trap, and it is closed by data rather than by rule:
-- 'Trim', 'Lower' and 'IsAlnum' consult 'Ark.Std.Unicode', three tables at
-- a pinned Unicode version generated from the character database, which
-- every runtime embeds. Rust's @char::to_lowercase@, Swift's @lowercased()@
-- and Kotlin's @lowercase()@ disagree at the edges and move with the
-- platform; a domain's @slug@ keys its log through these three functions
-- and cannot afford either. Moving the Unicode version is a spec version.
module Ark.Std
  ( StdError (..)
  , std
  , trim
  , fnv1a64
  , hexText
  , textOfId
  , idOfText
  ) where

import Data.Bits (shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word64, Word8)

import Ark.IR (StdFn (..))
import Ark.Sha256 (sha256)
import Ark.Std.Unicode (isAlphanumeric, isWhiteSpace, toLowerSimple)
import Ark.Value

-- | A fault a standard function can report. 'Arity' and 'TypeMismatch' are
-- bugs — a verified module never produces them; 'Fault' is a deterministic
-- verdict a mutator turns into a refusal (an overflow, a clamp with its
-- bounds crossed).
data StdError
  = Arity StdFn Int
  | TypeMismatch StdFn
  | Fault Text
  deriving (Eq, Show)

-- | Apply a standard function to already-evaluated arguments.
std :: StdFn -> [Value] -> Either StdError Value
std f args = case (f, args) of
  -- text --------------------------------------------------------------
  (Trim, [VText t]) -> ok (VText (trim t))
  (IsEmpty, [VText t]) -> ok (VBool (T.null t))
  (Concat, [VList xs]) -> VText . T.concat <$> mapM text xs
  (Lower, [VText t]) -> ok (VText (T.map toLowerSimple t))
  (IsAlnum, [VText t]) -> ok (VBool (not (T.null t) && T.all isAlphanumeric t))
  (Chars, [VText t]) -> ok (VList [VText (T.singleton c) | c <- T.unpack t])
  (TextLen, [VText t]) -> ok (VInt (fromIntegral (T.length t)))
  (StartsWith, [VText t, VText p]) -> ok (VBool (p `T.isPrefixOf` t))
  (SplitOnce, [VText t, VText sep])
    | T.null sep -> ok VNull
    | otherwise ->
        let (before, rest) = T.breakOn sep t
         in ok $
              if T.null rest
                then VNull
                else VStruct (M.fromList [("before", VText before), ("after", VText (T.drop (T.length sep) rest))])
  (TextOfInt, [VInt n]) -> ok (VText (T.pack (show n)))
  (Hex, [VBytes b]) -> ok (VText (T.pack (concatMap hexByte (B.unpack b))))
  -- int ---------------------------------------------------------------
  (Min, [VInt a, VInt b]) -> ok (VInt (min a b))
  (Max, [VInt a, VInt b]) -> ok (VInt (max a b))
  (Clamp, [VInt x, VInt lo, VInt hi])
    | lo > hi -> Left (Fault "clamp: lower bound above upper bound")
    | otherwise -> ok (VInt (max lo (min hi x)))
  (Abs, [VInt n])
    | n == minBound -> Left (Fault "integer overflow")
    | otherwise -> ok (VInt (abs n))
  -- hash --------------------------------------------------------------
  (Fnv1a64, [VText t]) -> ok (VInt (fromIntegral (fnv1a64 (TE.encodeUtf8 t))))
  (Sha256, [VBytes b]) -> ok (VBytes (sha256 b))
  -- id ----------------------------------------------------------------
  (IdOfText, [VText t]) -> ok (maybe VNull VId (idOfText t))
  (TextOfId, [VId i]) -> ok (VText (textOfId i))
  (NilId, []) -> maybe (Left (TypeMismatch f)) (Right . VId) (mkId (B.replicate 16 0))
  (Utf8, [VText t]) -> ok (VBytes (TE.encodeUtf8 t))
  -- list and option ---------------------------------------------------
  (First, [VList xs]) -> ok (option (safeHead xs))
  (Last, [VList xs]) -> ok (option (safeHead (reverse xs)))
  (Len, [VList xs]) -> ok (VInt (fromIntegral (length xs)))
  (Contains, [VList xs, v]) -> ok (VBool (any ((== EQ) . compareValue v) xs))
  (Reverse, [VList xs]) -> ok (VList (reverse xs))
  (IsSome, [v]) -> ok (VBool (not (isNull v)))
  (UnwrapOr, [VNull, d]) -> ok d
  (Unwrap, [VNull]) -> Left (Fault "unwrapped none")
  (Unwrap, [v]) -> ok v
  (UnwrapOr, [v, _]) -> ok v
  _ | length args /= arity f -> Left (Arity f (length args))
  _ -> Left (TypeMismatch f)
  where
    ok = Right
    text (VText t) = Right t
    text _ = Left (TypeMismatch f)
    option = maybe VNull id
    safeHead (x : _) = Just x
    safeHead [] = Nothing

-- | How many arguments each function takes.
-- | 'Trim' as a function of text, which the input checks apply directly.
trim :: Text -> Text
trim = T.dropAround isWhiteSpace

arity :: StdFn -> Int
arity f = case f of
  Trim -> 1
  IsEmpty -> 1
  Concat -> 1
  Lower -> 1
  IsAlnum -> 1
  Chars -> 1
  TextLen -> 1
  StartsWith -> 2
  SplitOnce -> 2
  TextOfInt -> 1
  Hex -> 1
  Min -> 2
  Max -> 2
  Clamp -> 3
  Abs -> 1
  Fnv1a64 -> 1
  Sha256 -> 1
  IdOfText -> 1
  TextOfId -> 1
  NilId -> 0
  Utf8 -> 1
  First -> 1
  Last -> 1
  Len -> 1
  Contains -> 2
  Reverse -> 1
  IsSome -> 1
  UnwrapOr -> 2
  Unwrap -> 1

-- | FNV-1a, 64-bit: offset basis @0xcbf29ce484222325@, prime
-- @0x00000100000001b3@, over the bytes in order. Harken's @key_part@ uses
-- it as the key of a name with no letters in it, and any runtime must
-- produce the same 64 bits — which is why it is here and not written with
-- a wrapping multiply in a mutator.
fnv1a64 :: B.ByteString -> Word64
fnv1a64 = B.foldl' step 0xcbf29ce484222325
  where
    step h b = (h `xor` fromIntegral b) * 0x00000100000001b3

-- | Bytes as lowercase hex, two digits each; what 'Hex' computes.
hexText :: B.ByteString -> Text
hexText = T.pack . concatMap hexByte . B.unpack

hexByte :: Word8 -> String
hexByte w = [digit (w `shiftR` 4), digit (w .&. 0x0f)]
  where
    digit n = "0123456789abcdef" !! fromIntegral n

-- | The canonical text of an id: lowercase 8-4-4-4-12.
textOfId :: IdBytes -> Text
textOfId i =
  T.pack (concat [take 8 h, "-", take 4 (drop 8 h), "-", take 4 (drop 12 h), "-", take 4 (drop 16 h), "-", drop 20 h])
  where
    h = concatMap hexByte (B.unpack (idBytes i))

-- | Parse 8-4-4-4-12 hex in either case; anything else is 'Nothing'.
idOfText :: Text -> Maybe IdBytes
idOfText t =
  case map T.unpack (T.splitOn "-" t) of
    [a, b, c, d, e]
      | map length [a, b, c, d, e] == [8, 4, 4, 4, 12]
      , all (all isHexDigit) [a, b, c, d, e] ->
          mkId (B.pack (pairs (concat [a, b, c, d, e])))
    _ -> Nothing
  where
    pairs (x : y : rest) = fromIntegral (hexVal x `shiftL` 4 .|. hexVal y) : pairs rest
    pairs _ = []
    hexVal ch
      | ch >= '0' && ch <= '9' = fromEnum ch - fromEnum '0'
      | ch >= 'a' && ch <= 'f' = fromEnum ch - fromEnum 'a' + 10
      | otherwise = fromEnum ch - fromEnum 'A' + 10

