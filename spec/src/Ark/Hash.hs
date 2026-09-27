-- | §8 Hashes.
--
-- Two quantities every runtime must reproduce byte for byte, both SHA-256
-- over canonical CBOR so that there is one encoder and one hash in the
-- whole system.
{-# LANGUAGE OverloadedStrings #-}
module Ark.Hash
  ( FnHash
  , Closure (..)
  , closure
  , closures
  , closureValue
  , stateHash
  , functionHash
  , moduleHash
  ) where

import qualified Data.ByteString as B
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)

import Ark.Canon (encode)
import Ark.Encode (calls, functionValue, normalize, normalizeModule, toValue)
import Ark.IR (Function (..), Module (..), lookupFunction)
import Ark.Sha256 (sha256)
import Ark.Store (Store, rows, tableNames)
import Ark.Value

-- | The 32-byte hash an entry names its function by.
type FnHash = B.ByteString

-- | §8.3 A function with the helpers it was verified against.
--
-- This is what an authority stores under a hash and what a peer receives
-- when it asks for one: not a body alone, because a body calls helpers by
-- name, and the name is not the meaning. 'cHelpers' holds every helper
-- the function reaches, directly or through other helpers, each in the
-- version that was current when the function was hashed, in declaration
-- order — so a closure is a complete, self-contained program that
-- 'Ark.Eval.applyClosure' runs without consulting any module.
data Closure = Closure
  { cFn :: Function
  , cHelpers :: [Function]
  }
  deriving (Eq, Show)

-- | The closure of a function within a module: the function normalised,
-- with the helpers it reaches, normalised, in the module's order.
closure :: Module -> Function -> Closure
closure m fn = Closure (normalize fn) [normalize h | h <- modFunctions m, fnName h `elem` reach]
  where
    reach = go [] (calls fn)
    go seen [] = seen
    go seen (n : ns)
      | n `elem` seen = go seen ns
      | otherwise = case lookupFunction m n of
          Just h -> go (n : seen) (calls h ++ ns)
          Nothing -> go seen ns

-- | A closure as a value, as an authority sends one: @{ t: "closure", fn,
-- helpers }@, each function in its normalised form. 'Ark.Decode.closureFromValue'
-- reads it back.
closureValue :: Closure -> Value
closureValue (Closure fn helpers) =
  VStruct
    ( M.fromList
        [ ("t", VText "closure")
        , ("fn", functionValue M.empty fn)
        , ("helpers", VList (map (functionValue M.empty) helpers))
        ]
    )

-- | Every function of a module, by the hash of its closure.
closures :: Module -> Map FnHash Closure
closures m = M.fromList [(functionHash c, c) | fn <- modFunctions m, let c = closure m fn]

-- | §8.1 The state hash of a store: the hash of the canonical encoding of
-- a list with, for every table of the schema in schema order, the table's
-- name and its rows in key order (under 'compareValue'). Tables with no
-- rows contribute their name and an empty list, so adding an empty table
-- to a schema moves the hash — which is right, because the schema is part
-- of what two replicas must agree on.
--
-- This is what @Verify { scope, seq, hash }@ carries and what a snapshot
-- claims. Two whole-scope replicas at the same sequence must agree on it
-- exactly; that is the definition of exact replication in this design.
stateHash :: Store -> B.ByteString
stateHash st =
  sha256 (encode (VList [VList [VText t, VList (map VStruct (M.elems (rows st t)))] | t <- tableNames st]))

-- | §8.2 The hash of a function: over its normalised canonical form, names
-- excluded, together with the hashes of the helpers it calls directly —
-- which cover theirs in turn, so the hash is of the whole meaning. This is
-- the @fn@ an entry records, the key an authority stores closures under,
-- and the thing two builders in two languages must agree on for one
-- function.
functionHash :: Closure -> FnHash
functionHash (Closure fn helpers) = sha256 (encode (functionValue deps fn))
  where
    deps = M.fromList (mapMaybe dep (calls fn))
    dep n = case [h | h <- helpers, fnName h == n] of
      (h : _) -> Just (n, VBytes (functionHash (Closure h helpers)))
      [] -> Nothing

-- | The hash of a whole module, normalised.
moduleHash :: Module -> B.ByteString
moduleHash = sha256 . encode . toValue . normalizeModule
