-- | §8 Hashes.
--
-- Two quantities every runtime must reproduce byte for byte, both SHA-256
-- over canonical CBOR so that there is one encoder and one hash in the
-- whole system.
module Ark.Hash
  ( stateHash
  , functionHash
  , moduleHash
  ) where

import qualified Data.ByteString as B
import qualified Data.Map.Strict as M

import Ark.Canon (encode)
import Ark.Encode (functionValue, normalizeModule, toValue)
import Ark.IR (Function, Module)
import Ark.Sha256 (sha256)
import Ark.Store (Store, rows, tableNames)
import Ark.Value

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
-- excluded. This is the @fn@ an entry records, the key an authority stores
-- bodies under, and the thing two builders in two languages must agree on
-- for one function.
functionHash :: Function -> B.ByteString
functionHash = sha256 . encode . functionValue

-- | The hash of a whole module, normalised.
moduleHash :: Module -> B.ByteString
moduleHash = sha256 . encode . toValue . normalizeModule
