-- | §2 The canonical encoding.
--
-- A value has one encoding, and this module is it. The reason is the hash:
-- 'Ark.Hash' takes SHA-256 over the bytes a value encodes to, and a hash of
-- a value is meaningful only if its bytes are — two runtimes that agree
-- about a value but not about its bytes disagree about every state hash,
-- every index key and every signature built on one. So the bytes are
-- specified, not the value.
--
-- The format is CBOR (RFC 8949), in the core deterministic encoding of
-- §4.2.1, rather than a format of Ark's own. CBOR has a library in every
-- language a runtime will be written in, and its deterministic profile is a
-- short list of rules over what the base format leaves optional: which of
-- the several heads an integer may carry, whether a length is stated up
-- front, in what order a map's keys appear. A bespoke format would have to
-- settle the same questions and then be implemented from nothing three
-- times. Here the spec's whole job is to forbid what the RFC leaves open —
-- and to pin the mapping from 'Value' onto CBOR's own types, which the RFC
-- cannot do for it.
--
-- 'decode' is as strict as 'encode' is exact: it accepts canonical bytes and
-- nothing else, so that an encoding a runtime could not have produced is
-- rejected rather than silently normalised. Rejecting is what keeps
-- @decode . encode = id@ /and/ @encode . decode = id@ true at once, and the
-- second of those is what makes it safe to store, hash and compare encoded
-- bytes without decoding them ('roundTrip' is the check).
module Ark.Canon
  ( encode
  , decode
  , DecodeError (..)
  , roundTrip
  ) where

import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import Data.Int (Int64)
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text.Encoding as TE
import Data.Word (Word64, Word8)

import Ark.Value

-- * Encoding

-- | The canonical bytes of a value. Total: every 'Value' has an encoding.
--
-- The mapping onto CBOR's major types (§3.1), constructor by constructor,
-- is in 'build'. The rules that make it deterministic (§4.2.1) are:
--
-- * every head is the shortest that holds its argument ('header');
-- * every length is definite — no indefinite strings, arrays or maps
--   (§3.2 is never used);
-- * a map's keys are sorted by the bytes they encode to ('build' on
--   'VStruct');
-- * floats and other simple values do not arise, because the value model
--   has none.
encode :: Value -> B.ByteString
encode = BL.toStrict . BB.toLazyByteString . build

-- | The head of a data item (§3): a major type in the top three bits of the
-- initial byte and an argument in the rest. §4.2.1 requires the argument in
-- the shortest form that holds it — in the initial byte for 0–23, then one,
-- two, four or eight following bytes with additional information 24, 25, 26
-- or 27. Every length and every integer here goes through this.
header :: Word8 -> Word64 -> BB.Builder
header major n
  | n < 24 = BB.word8 (mt .|. fromIntegral n)
  | n < 0x100 = BB.word8 (mt .|. 24) <> BB.word8 (fromIntegral n)
  | n < 0x10000 = BB.word8 (mt .|. 25) <> BB.word16BE (fromIntegral n)
  | n < 0x100000000 = BB.word8 (mt .|. 26) <> BB.word32BE (fromIntegral n)
  | otherwise = BB.word8 (mt .|. 27) <> BB.word64BE n
  where
    mt = major `shiftL` 5

-- | How each constructor is spelled in CBOR.
build :: Value -> BB.Builder
-- 'VNull' is the simple value @null@ (§3.3): major type 7, argument 22,
-- the single byte 0xf6. Not @undefined@ (23), which CBOR also has and the
-- value model does not.
build VNull = BB.word8 0xf6
-- 'VBool' is the simple value @false@ (20, 0xf4) or @true@ (21, 0xf5).
build (VBool False) = BB.word8 0xf4
build (VBool True) = BB.word8 0xf5
-- 'VInt' is an unsigned integer (major type 0) when it is not negative and a
-- negative integer (major type 1) when it is; major type 1 carries @-1 - n@
-- (§3.1), so @-1@ is argument 0 and 'minBound' is argument @2^63 - 1@,
-- which is why the subtraction is done in 'Int64' and cannot overflow:
-- @-1 - n@ is in range exactly when @n@ is negative.
build (VInt n)
  | n >= 0 = header 0 (fromIntegral n)
  | otherwise = header 1 (fromIntegral ((-1) - n))
-- 'VText' is a text string (major type 3): the UTF-8 bytes of the text,
-- behind a definite length in bytes. Not characters — the length is the
-- byte count, which is what @"\62c3bc"@ for @ü@ says.
build (VText t) = string 3 (TE.encodeUtf8 t)
-- 'VBytes' is a byte string (major type 2), behind a definite length.
build (VBytes b) = string 2 b
-- 'VId' is a byte string of exactly sixteen bytes under tag 37, which the
-- IANA tag registry assigns to a binary UUID (RFC 9562 §6.7 asks for
-- exactly this tag over exactly these bytes). The tag is what tells an id
-- from a 'VBytes' that happens to be sixteen long: without it the two would
-- encode alike and decode as whichever the reader guessed. The bytes are
-- always @0xd8 0x25 0x50@ and then the id, because 37 needs one following
-- byte and 16 fits the initial byte.
build (VId i) = header 6 37 <> string 2 (idBytes i)
-- 'VList' is an array (major type 4) of its elements in order, behind a
-- definite count.
build (VList xs) = header 4 (fromIntegral (length xs)) <> foldMap build xs
-- 'VStruct' is a map (major type 5) from field names, each a 'VText', to
-- values, behind a definite count of pairs. The pairs are sorted by the
-- bytes the /key/ encodes to, compared bytewise (§4.2.1: "the keys in every
-- map MUST be sorted in the bytewise lexicographic order of their
-- deterministic encodings"). That order is not the order of the names: an
-- encoded key starts with its head, and the head carries the length, so a
-- shorter name sorts before every longer one whatever its letters — @"b"@
-- before @"aa"@ — and only names of one length are in alphabetical (UTF-8
-- byte) order among themselves. §4.2.3 calls this "length-first" and notes
-- it is what bytewise order over encodings comes to; for text keys under
-- shortest heads the two rules are the same rule, which is why 'decode'
-- can check it by comparing the raw key bytes it read.
--
-- The 'M.Map' the struct is held in is ordered by 'Text', which is code
-- point order and disagrees with this for names of different lengths, so
-- the pairs are re-sorted rather than taken as the map yields them.
build (VStruct m) =
  header 5 (fromIntegral (M.size m))
    <> foldMap (\(k, v) -> BB.byteString k <> build v) (sortOn fst pairs)
  where
    pairs = [(encode (VText k), v) | (k, v) <- M.toList m]

-- | A string of either kind: a definite byte length, then the bytes.
string :: Word8 -> B.ByteString -> BB.Builder
string major b = header major (fromIntegral (B.length b)) <> BB.byteString b

-- * Decoding

-- | Why some bytes are not the canonical encoding of any value. The
-- constructor names the first rule the input broke, reading left to right.
data DecodeError
  = -- | A head longer than the shortest that holds its argument (§4.2.1),
    -- or one of the reserved additional-information values 28–30 (§3),
    -- which no well-formed item has.
    NonCanonicalHead
  | -- | Additional information 31: an indefinite-length string, array or
    -- map (§3.2), or a stray @break@. §4.2.1 forbids indefinite lengths;
    -- 31 under major type 0, 1 or 6 is not well-formed at all and is
    -- reported the same way.
    IndefiniteLength
  | -- | A map key that sorts before the key preceding it.
    UnsortedKeys
  | -- | A map key equal to the key preceding it (§5.6: a decoder need not
    -- accept duplicates, and a canonical one must not).
    DuplicateKey
  | -- | A map key that is not a text string; a 'VStruct' has field names
    -- for keys and nothing else.
    NonTextKey
  | -- | A tag other than 37. Tags are how CBOR extends itself and the value
    -- model has exactly one extension.
    BadTag
  | -- | Tag 37 over something other than a byte string of exactly sixteen
    -- bytes.
    BadId
  | -- | A half, single or double float (major type 7, additional
    -- information 25, 26 or 27). The value model has no floats.
    Float
  | -- | A simple value other than @false@, @true@ and @null@: @undefined@,
    -- the unassigned ones, and the one-byte form (§3.3).
    BadSimple
  | -- | A text string whose bytes are not valid UTF-8 (§3.1 requires it of
    -- major type 3, and the value's 'Text' could not hold it).
    BadUtf8
  | -- | An integer outside 'Int64': an unsigned argument above @2^63 - 1@,
    -- or a negative one whose argument is above @2^63 - 1@ (so the value
    -- would be below @-2^63@). CBOR's integers are 65 bits wide and the
    -- value model's are 64.
    IntOutOfRange
  | -- | Bytes after the value. One value is the whole input.
    Trailing
  | -- | The input ended inside a value.
    Truncated
  deriving (Eq, Show)

-- | A value from its canonical bytes, and only from those: for every input
-- either @decode b = Left _@ or @encode <$> decode b = Right b@.
decode :: B.ByteString -> Either DecodeError Value
decode input = do
  (v, rest) <- runP value input
  if B.null rest then Right v else Left Trailing

-- | Whether some bytes are a canonical encoding: they decode, and what they
-- decode to encodes back to exactly them. The second half is what a
-- decoder that normalised instead of rejecting would fail.
roundTrip :: B.ByteString -> Bool
roundTrip b = case decode b of
  Left _ -> False
  Right v -> encode v == b

-- | A parser over the remaining input, failing with the first rule broken.
newtype P a = P {runP :: B.ByteString -> Either DecodeError (a, B.ByteString)}

instance Functor P where
  fmap f (P p) = P $ \s -> case p s of
    Left e -> Left e
    Right (a, s') -> Right (f a, s')

instance Applicative P where
  pure a = P $ \s -> Right (a, s)
  P pf <*> P pa = P $ \s -> case pf s of
    Left e -> Left e
    Right (f, s') -> case pa s' of
      Left e -> Left e
      Right (a, s'') -> Right (f a, s'')

instance Monad P where
  P p >>= k = P $ \s -> case p s of
    Left e -> Left e
    Right (a, s') -> runP (k a) s'

refuse :: DecodeError -> P a
refuse e = P $ \_ -> Left e

-- | The next byte.
byte :: P Word8
byte = P $ \s -> case B.uncons s of
  Nothing -> Left Truncated
  Just (b, s') -> Right (b, s')

-- | The next byte without consuming it.
peek :: P Word8
peek = P $ \s -> case B.uncons s of
  Nothing -> Left Truncated
  Just (b, _) -> Right (b, s)

-- | The next @n@ bytes. The count is compared as a 'Word64' rather than
-- converted, because a length near @2^64@ is a legal argument and a
-- ludicrous 'Int'.
chunk :: Word64 -> P B.ByteString
chunk n = P $ \s ->
  if n > fromIntegral (B.length s)
    then Left Truncated
    else Right (B.splitAt (fromIntegral n) s)

-- | Run a parser and also return the bytes it consumed.
spanned :: P a -> P (a, B.ByteString)
spanned (P p) = P $ \s -> case p s of
  Left e -> Left e
  Right (a, s') -> Right ((a, B.take (B.length s - B.length s') s), s')

-- | The argument of a head whose initial byte carried additional
-- information @ai@, insisting on the shortest form (§4.2.1): each longer
-- form is accepted only for an argument the next shorter one could not
-- hold.
argument :: Word8 -> P Word64
argument ai
  | ai < 24 = pure (fromIntegral ai)
  | ai == 24 = wide 1 24
  | ai == 25 = wide 2 0x100
  | ai == 26 = wide 4 0x10000
  | ai == 27 = wide 8 0x100000000
  | ai == 31 = refuse IndefiniteLength
  | otherwise = refuse NonCanonicalHead
  where
    wide :: Int -> Word64 -> P Word64
    wide k least = do
      n <- bigEndian k
      if n < least then refuse NonCanonicalHead else pure n

-- | An unsigned big-endian integer of @k@ bytes (§3: network byte order).
bigEndian :: Int -> P Word64
bigEndian k = go k 0
  where
    go 0 acc = pure acc
    go i acc = do
      b <- byte
      go (i - 1) ((acc `shiftL` 8) .|. fromIntegral b)

majorOf :: Word8 -> Word8
majorOf ib = ib `shiftR` 5

infoOf :: Word8 -> Word8
infoOf ib = ib .&. 0x1f

-- | The largest argument an 'Int64' holds under either integer major type.
int64Limit :: Word64
int64Limit = fromIntegral (maxBound :: Int64)

-- | One data item, as a value. The inverse of 'build', case for case.
value :: P Value
value = do
  ib <- byte
  let ai = infoOf ib
  case majorOf ib of
    -- Major type 0: an unsigned integer, refused above 'Int64'.
    0 -> do
      n <- argument ai
      if n > int64Limit then refuse IntOutOfRange else pure (VInt (fromIntegral n))
    -- Major type 1: the value is @-1 - n@; refused where that is below
    -- 'minBound', which is where @n@ exceeds @2^63 - 1@. The subtraction is
    -- done in 'Int64' after the bound is checked, so it cannot overflow.
    1 -> do
      n <- argument ai
      if n > int64Limit
        then refuse IntOutOfRange
        else pure (VInt ((-1) - fromIntegral n))
    -- Major type 2: a byte string.
    2 -> VBytes <$> (argument ai >>= chunk)
    -- Major type 3: a text string, which must be UTF-8.
    3 -> VText <$> (argument ai >>= text)
    -- Major type 4: an array, of exactly the counted elements.
    4 -> do
      n <- argument ai
      VList <$> count n value
    -- Major type 5: a map of text keys in canonical order to values.
    5 -> do
      n <- argument ai
      VStruct . M.fromList <$> pairs n
    -- Major type 6: a tag, of which only 37 over a sixteen-byte string is
    -- an id. The tag's own argument is subject to the shortest-head rule
    -- like any other, so @0xd9 0x00 0x25@ is a non-canonical head, not a
    -- bad tag.
    6 -> do
      t <- argument ai
      if t /= 37 then refuse BadTag else VId <$> identifier
    -- Major type 7: simple values and floats (§3.3). Only three simple
    -- values are values; 25–27 are floats; 24 is the two-byte simple form,
    -- which is not canonical for anything the value model has; 31 is a
    -- @break@, which belongs inside an indefinite-length item and there
    -- are none.
    _ -> case ai of
      20 -> pure (VBool False)
      21 -> pure (VBool True)
      22 -> pure VNull
      25 -> refuse Float
      26 -> refuse Float
      27 -> refuse Float
      31 -> refuse IndefiniteLength
      _ -> refuse BadSimple

-- | The text of a string of @n@ bytes.
text :: Word64 -> P Text
text n = do
  b <- chunk n
  case TE.decodeUtf8' b of
    Left _ -> refuse BadUtf8
    Right t -> pure t

-- | @n@ of something, one at a time, so that an absurd count fails on the
-- input running out rather than on allocation.
count :: Word64 -> P a -> P [a]
count 0 _ = pure []
count n p = do
  x <- p
  xs <- count (n - 1) p
  pure (x : xs)

-- | @n@ key–value pairs whose keys are text strings in strictly increasing
-- order of their encoded bytes, which is the order 'build' writes them in.
-- The comparison is over the bytes as read — the head included — because
-- that is what §4.2.1 orders by; and since each key is itself a canonical
-- item, bytes that are equal are keys that are equal, so a repeat is a
-- 'DuplicateKey' and anything else out of order is 'UnsortedKeys'.
pairs :: Word64 -> P [(FieldName, Value)]
pairs = go Nothing
  where
    go _ 0 = pure []
    go prev n = do
      (k, raw) <- spanned key
      case prev of
        Just p | raw == p -> refuse DuplicateKey
        Just p | raw < p -> refuse UnsortedKeys
        _ -> pure ()
      v <- value
      rest <- go (Just raw) (n - 1)
      pure ((k, v) : rest)

-- | A map key: a text string, and nothing else.
key :: P FieldName
key = do
  ib <- peek
  if majorOf ib /= 3
    then refuse NonTextKey
    else byte >> argument (infoOf ib) >>= text

-- | What follows tag 37: a byte string of exactly sixteen bytes.
identifier :: P IdBytes
identifier = do
  ib <- byte
  if majorOf ib /= 2
    then refuse BadId
    else do
      n <- argument (infoOf ib)
      if n /= 16 then refuse BadId else pure ()
      b <- chunk n
      case mkId b of
        Nothing -> refuse BadId
        Just i -> pure i
