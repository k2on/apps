-- | SHA-256, FIPS 180-4.
--
-- Implemented here rather than imported because the spec depends on nothing
-- but a compiler, and a hash that every runtime has to reproduce byte for
-- byte is part of what the spec defines: an implementation that cannot be
-- read is not a specification of it.
module Ark.Sha256
  ( sha256
  , sha256Hex
  ) where

import Data.Bits (complement, rotateR, shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Unsafe as BU
import Data.List (foldl')
import Data.Word (Word32, Word64, Word8)

-- | The 32-byte digest of a message.
sha256 :: B.ByteString -> B.ByteString
sha256 msg = digest (foldl' compress initial (blocks (pad msg)))

-- | The digest as 64 lowercase hexadecimal digits.
sha256Hex :: B.ByteString -> String
sha256Hex = concatMap hex . B.unpack . sha256
  where
    hex w = [nibble (w `shiftR` 4), nibble (w .&. 0x0f)]
    nibble n = "0123456789abcdef" !! fromIntegral n

-- Padding (§5.1.1): a 1 bit, zeros to 56 mod 64 bytes, then the message
-- length in bits as a 64-bit big-endian integer.
pad :: B.ByteString -> B.ByteString
pad msg = B.concat [msg, B.singleton 0x80, B.replicate zeros 0, be64 bits]
  where
    len = B.length msg
    zeros = (55 - len) `mod` 64
    bits = fromIntegral len * 8 :: Word64
    be64 w = B.pack [fromIntegral (w `shiftR` (8 * i)) | i <- [7, 6 .. 0]]

-- The padded message as 512-bit blocks (§5.2.1).
blocks :: B.ByteString -> [B.ByteString]
blocks bs
  | B.null bs = []
  | otherwise = let (h, t) = B.splitAt 64 bs in h : blocks t

-- The eight working variables a..h, strict so that a fold over the message
-- never builds a thunk per round.
data State = State !Word32 !Word32 !Word32 !Word32
                   !Word32 !Word32 !Word32 !Word32

-- Initial hash value (§5.3.3): the first 32 bits of the fractional parts of
-- the square roots of the first 8 primes.
initial :: State
initial = State 0x6a09e667 0xbb67ae85 0x3c6ef372 0xa54ff53a
                0x510e527f 0x9b05688c 0x1f83d9ab 0x5be0cd19

-- Round constants (§4.2.2): the first 32 bits of the fractional parts of the
-- cube roots of the first 64 primes.
k :: [Word32]
k =
  [ 0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5
  , 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174
  , 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da
  , 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967
  , 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85
  , 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070
  , 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3
  , 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
  ]

-- Message schedule (§6.2.2 step 1): the block's sixteen big-endian words,
-- extended to sixty-four.
schedule :: B.ByteString -> [Word32]
schedule blk = ws
  where
    ws = [word (4 * i) | i <- [0 .. 15]]
      ++ zipWith4' (\a b c d -> s1 a + b + s0 c + d) (drop 14 ws) (drop 9 ws) (drop 1 ws) ws
    word i = (byte i `shiftL` 24) .|. (byte (i + 1) `shiftL` 16)
         .|. (byte (i + 2) `shiftL` 8) .|. byte (i + 3)
    byte i = fromIntegral (BU.unsafeIndex blk i) :: Word32
    s0 x = rotateR x 7 `xor` rotateR x 18 `xor` shiftR x 3
    s1 x = rotateR x 17 `xor` rotateR x 19 `xor` shiftR x 10
    zipWith4' f (a : as) (b : bs) (c : cs) (d : ds) = f a b c d : zipWith4' f as bs cs ds
    zipWith4' _ _ _ _ _ = []

-- Compression (§6.2.2 steps 2-4): sixty-four rounds over one block, added
-- to the incoming hash value.
compress :: State -> B.ByteString -> State
compress st@(State h0 h1 h2 h3 h4 h5 h6 h7) blk =
  case foldl' rnd st (zip k (take 64 (schedule blk))) of
    State a b c d e f g h ->
      State (h0 + a) (h1 + b) (h2 + c) (h3 + d) (h4 + e) (h5 + f) (h6 + g) (h7 + h)
  where
    rnd (State a b c d e f g h) (kt, wt) =
      let t1 = h + bigS1 e + ch e f g + kt + wt
          t2 = bigS0 a + maj a b c
      in State (t1 + t2) a b c (d + t1) e f g
    bigS0 x = rotateR x 2 `xor` rotateR x 13 `xor` rotateR x 22
    bigS1 x = rotateR x 6 `xor` rotateR x 11 `xor` rotateR x 25
    ch x y z = (x .&. y) `xor` (complement x .&. z)
    maj x y z = (x .&. y) `xor` (x .&. z) `xor` (y .&. z)

-- The hash value as thirty-two big-endian bytes.
digest :: State -> B.ByteString
digest (State a b c d e f g h) = B.pack (concatMap be32 [a, b, c, d, e, f, g, h])
  where
    be32 :: Word32 -> [Word8]
    be32 w = [fromIntegral (w `shiftR` s) | s <- [24, 16, 8, 0]]
