{-# LANGUAGE OverloadedStrings #-}
-- | §1 Values.
--
-- The value model is deliberately small: every type here is one that three
-- generated codebases (Rust, Swift, Kotlin) have to agree about to the byte,
-- in memory, on the wire, in an index key and under a hash. Anything not in
-- this module does not exist in an Ark database.
--
-- There are no floats. A gain is an 'Int' in millibels, a position is
-- milliseconds. Adding a float type is a spec version bump with a defined
-- canonical NaN and no equality in the IR, and it waits for a domain that
-- needs one.
--
-- An enum variant is a 'VText' at run time and an enum only in the static
-- type ('Ark.IR.TEnum'); it therefore encodes, orders and hashes exactly as
-- text does. That keeps the run-time model to eight constructors.
module Ark.Value
  ( Value (..)
  , IdBytes
  , mkId
  , idBytes
  , compareValue
  , rank
  , isNull
  , TableName
  , FieldName
  ) where

import qualified Data.ByteString as B
import Data.Int (Int64)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text as T

-- | The name of a table, as declared in the schema.
type TableName = Text

-- | The name of a field of a struct, or a column of a row.
type FieldName = Text

-- | Sixteen bytes. The table an id names is a static type, never a run-time
-- tag: the bytes on the wire and in the store are exactly Petros's, so a log
-- written before there were typed ids still decodes. Construct only through
-- 'mkId', which enforces the length.
newtype IdBytes = IdBytes B.ByteString
  deriving (Eq, Ord, Show)

-- | An id from its bytes; 'Nothing' unless there are exactly sixteen.
mkId :: B.ByteString -> Maybe IdBytes
mkId b
  | B.length b == 16 = Just (IdBytes b)
  | otherwise = Nothing

idBytes :: IdBytes -> B.ByteString
idBytes (IdBytes b) = b

-- | A run-time value.
--
-- 'VNull' exists only as the absent case of an @Option@ type; a nullable
-- column is an @Option@. 'VStruct' is a named record, used for rows, query
-- results and arguments; its field names are its identity, so two structs
-- with the same fields and values are equal whatever order they were built
-- in. A row of table @t@ is a 'VStruct' whose fields are @t@'s columns, in
-- full — a row is never partial.
data Value
  = VNull
  | VBool !Bool
  | VInt !Int64
  | VText !Text
  | VBytes !B.ByteString
  | VId !IdBytes
  | VList [Value]
  | VStruct (Map FieldName Value)
  deriving (Eq, Show)

isNull :: Value -> Bool
isNull VNull = True
isNull _ = False

-- | §1.2 The total order.
--
-- One order over all values, because @ORDER BY@, index keys and the state
-- hash all need one and three runtimes must agree on it. Values of
-- different types order by 'rank'; within a type:
--
-- * 'VInt' numerically;
-- * 'VText' by Unicode code point — which is UTF-8 byte order, and which is
--   /not/ what a platform gives you by default. Swift's @<@ on @String@
--   uses Unicode canonical ordering and treats @é@ and @e◌́@ as equal;
--   Kotlin's @compareTo@ compares UTF-16 code units, which puts U+FF5E
--   before U+1F3B5 where code points put it after. A conformant runtime
--   compares code points (or, equivalently, UTF-8 bytes) explicitly. It is
--   spelled out here as a comparison of code point lists so that nothing
--   about the host's @Text@ instance is relied on;
-- * 'VBytes' and 'VId' lexicographically by byte;
-- * 'VList' lexicographically by element, a prefix first;
-- * 'VStruct' as the association list sorted by field name, comparing
--   field names before values.
--
-- This is also the 'Ord' instance, so a @Map Value _@ or a @sort@ in this
-- program is the spec's order by construction.
compareValue :: Value -> Value -> Ordering
compareValue a b
  | ra /= rb = compare ra rb
  | otherwise = case (a, b) of
      (VNull, VNull) -> EQ
      (VBool x, VBool y) -> compare x y
      (VInt x, VInt y) -> compare x y
      (VText x, VText y) -> compare (T.unpack x) (T.unpack y)
      (VBytes x, VBytes y) -> compare x y
      (VId x, VId y) -> compare (idBytes x) (idBytes y)
      (VList xs, VList ys) -> lexico xs ys
      (VStruct xs, VStruct ys) -> assoc (M.toAscList xs) (M.toAscList ys)
      _ -> error "compareValue: rank mismatch is impossible"
  where
    ra = rank a
    rb = rank b
    lexico [] [] = EQ
    lexico [] _ = LT
    lexico _ [] = GT
    lexico (x : xs) (y : ys) = compareValue x y <> lexico xs ys
    assoc [] [] = EQ
    assoc [] _ = LT
    assoc _ [] = GT
    assoc ((k, v) : xs) ((k', v') : ys) =
      compare (T.unpack k) (T.unpack k') <> compareValue v v' <> assoc xs ys

-- | The rank of a value's type in the total order:
-- @Null < Bool < Int < Text < Bytes < Id < List < Struct@.
rank :: Value -> Int
rank VNull = 0
rank (VBool _) = 1
rank (VInt _) = 2
rank (VText _) = 3
rank (VBytes _) = 4
rank (VId _) = 5
rank (VList _) = 6
rank (VStruct _) = 7

instance Ord Value where
  compare = compareValue
