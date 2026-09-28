{-# LANGUAGE OverloadedStrings #-}
-- | §2 The schema.
--
-- A schema is a value in the module, not DDL. It says what Petros's
-- @tables!@ used to ask SQLite: which tables exist, their columns and key,
-- which indexes are unique, and which columns reference which table. From
-- it a runtime derives, identically in every language, the row types, the
-- key type, and both directions of every reference as a relationship.
--
-- A module has one set of tables and one log. (Spec version 2 split them
-- into /scopes/, each its own log; @docs/scopes.md@ says what those were
-- for and why they are gone.) So every reference is checked, and any
-- function may read any table.
module Ark.Schema
  ( Ty (..)
  , Column (..)
  , Index (..)
  , Ref (..)
  , Table (..)
  , Schema (..)
  , Relation (..)
  , Dir (..)
  , SchemaError (..)
  , isScalar
  , lookupTable
  , column
  , columnTy
  , rowTy
  , keyTy
  , keyOf
  , relations
  , childrenOf
  , parentOf
  , checkSchema
  ) where

import Data.List (find)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, mapMaybe)
import Data.Text (Text)

import Ark.Value

-- | §2.1 Types.
--
-- The static types of the IR. A column may have only a scalar type
-- ('isScalar'), optionally nullable; everything else exists so that
-- expressions, arguments and query results can be typed. 'TEnum' is a
-- closed set of names whose values are 'VText' at run time. 'TStruct' is
-- a record; a row of table @t@ is the struct 'rowTy' gives, in full.
data Ty
  = TBool
  | TInt
  | TText
  | TBytes
  | TId TableName
  | TEnum [Text]
  | TOption Ty
  | TList Ty
  | TStruct (Map FieldName Ty)
  deriving (Eq, Ord, Show)

-- | The types a column may have (before nullability).
isScalar :: Ty -> Bool
isScalar t = case t of
  TBool -> True
  TInt -> True
  TText -> True
  TBytes -> True
  TId _ -> True
  TEnum _ -> True
  _ -> False

-- | A column. A nullable column's values are 'VNull' or a value of 'colTy';
-- its static type is @'TOption' colTy@.
data Column = Column
  { colName :: FieldName
  , colTy :: Ty
  , colNullable :: Bool
  }
  deriving (Eq, Show)

-- | A declared index. In this specification an index is a /uniqueness
-- constraint/ when 'ixUnique' and otherwise only a statement of intent
-- about performance: the meaning of a query never depends on which indexes
-- exist, so the evaluator here scans and sorts, and a runtime that seeks an
-- index must give the same answer.
data Index = Index
  { ixColumns :: [FieldName]
  , ixUnique :: Bool
  }
  deriving (Eq, Show)

-- | @refColumn REFERENCES refTable(key)@. The parent's key must be a single
-- column, and the child column's type must be that column's type; where
-- the parent is keyed by an id the child column is @'TId' refTable@, which
-- is how a column comes to name the table it identifies (Petros: "an id
-- knows what it identifies").
data Ref = Ref
  { refColumn :: FieldName
  , refTable :: TableName
  }
  deriving (Eq, Show)

data Table = Table
  { tName :: TableName
  , tColumns :: [Column]
  , tKey :: [FieldName]
  , tIndexes :: [Index]
  , tRefs :: [Ref]
  }
  deriving (Eq, Show)

-- | The tables, in declaration order — which is also the order a state
-- hash walks them.
newtype Schema = Schema {schTables :: [Table]}
  deriving (Eq, Show)

-- | Ascending or descending, for an order.
data Dir = Asc | Desc
  deriving (Eq, Ord, Show)

lookupTable :: Schema -> TableName -> Maybe Table
lookupTable sch n = find ((== n) . tName) (schTables sch)

column :: Table -> FieldName -> Maybe Column
column t n = find ((== n) . colName) (tColumns t)

-- | The static type of a column, nullability included.
columnTy :: Column -> Ty
columnTy c
  | colNullable c = TOption (colTy c)
  | otherwise = colTy c

-- | The type of a whole row: a struct of every column.
rowTy :: Table -> Ty
rowTy t = TStruct (M.fromList [(colName c, columnTy c) | c <- tColumns t])

-- | The types of the key columns, in key order.
keyTy :: Table -> [Ty]
keyTy t = mapMaybe (fmap columnTy . column t) (tKey t)

-- | The key of a row, as the key columns' values in key order. A row is
-- always full, so every key column is present; this is total on rows the
-- store accepted.
keyOf :: Table -> Map FieldName Value -> [Value]
keyOf t row = [M.findWithDefault VNull k row | k <- tKey t]

-- | §2.2 Relationships.
--
-- A reference @child.col REFERENCES parent@ is two relationships, named as
-- Petros names them: from the parent, the /child table's name/ reaches
-- down to the children (a childless parent is still a row — the left
-- join); from the child, the /parent table's name/ reaches up to the one
-- parent (a child whose parent is missing is dropped — the inner join).
-- Neither is written by an author; both come from the DDL, and a query
-- names one through 'Ark.IR.Plan'.
data Relation = Relation
  { relParent :: TableName
  , relChild :: TableName
  , relColumn :: FieldName -- ^ the child column that holds the parent's key
  }
  deriving (Eq, Show)

-- | Every relationship in a schema.
relations :: Schema -> [Relation]
relations sch =
  [ Relation (refTable r) (tName t) (refColumn r)
  | t <- schTables sch
  , r <- tRefs t
  ]

-- | The relationships reaching down from a table, keyed by child table.
childrenOf :: Schema -> TableName -> [Relation]
childrenOf sch p = filter ((== p) . relParent) (relations sch)

-- | The relationships reaching up from a table, keyed by parent table.
parentOf :: Schema -> TableName -> [Relation]
parentOf sch c = filter ((== c) . relChild) (relations sch)

-- | §2.3 Well-formedness. A module whose schema fails any of these is
-- refused by the verifier before anything else is looked at.
data SchemaError
  = DuplicateTable TableName
  | DuplicateColumn TableName FieldName
  | NoKey TableName
  | UnknownKeyColumn TableName FieldName
  | NullableKey TableName FieldName
  | NonScalarColumn TableName FieldName
  | UnknownIndexColumn TableName FieldName
  | UnknownRefColumn TableName FieldName
  | UnknownRefTable TableName TableName
  | RefToCompositeKey TableName TableName
  | RefTypeMismatch TableName FieldName Ty Ty
  | -- | An id-typed column must be a key of its own table (@TId self@) or
    -- a reference. An id naming a table without a reference is a
    -- declaration the store could not hold to.
    IdColumnWithoutRef TableName FieldName
  | -- | A reference column's id type names a table other than the one it
    -- references.
    IdNamesWrongTable TableName FieldName
  deriving (Eq, Show)

checkSchema :: Schema -> [SchemaError]
checkSchema sch =
  dupTables ++ concatMap perTable tables
  where
    tables = schTables sch
    dupTables = [DuplicateTable (tName t) | t <- dups' tables]
    dups xs = [x | (x, n) <- M.toList (M.fromListWith (+) [(x, 1 :: Int) | x <- xs]), n > 1]
    dups' ts = [t | t <- ts, tName t `elem` dups (map tName ts)]
    perTable t =
      [DuplicateColumn (tName t) c | c <- dups (map colName (tColumns t))]
        ++ [NoKey (tName t) | null (tKey t)]
        ++ [UnknownKeyColumn (tName t) k | k <- tKey t, not (has t k)]
        ++ [NullableKey (tName t) k | k <- tKey t, Just c <- [column t k], colNullable c]
        ++ [NonScalarColumn (tName t) (colName c) | c <- tColumns t, not (isScalar (colTy c))]
        ++ [UnknownIndexColumn (tName t) c | ix <- tIndexes t, c <- ixColumns ix, not (has t c)]
        ++ concatMap (perRef t) (tRefs t)
        ++ [ IdColumnWithoutRef (tName t) (colName c)
           | c <- tColumns t
           , TId of' <- [colTy c]
           , not (isRef t (colName c))
           , not (of' == tName t && colName c `elem` tKey t)
           ]
    has t c = isJust (column t c)
    isRef t c = any ((== c) . refColumn) (tRefs t)
    perRef t r =
      case (column t (refColumn r), lookupTable sch (refTable r)) of
        (Nothing, _) -> [UnknownRefColumn (tName t) (refColumn r)]
        (_, Nothing) -> [UnknownRefTable (tName t) (refTable r)]
        (Just c, Just p) ->
          case keyTy p of
              [pk] ->
                let want = case pk of
                      TId _ -> TId (refTable r)
                      other -> other
                 in [ RefTypeMismatch (tName t) (refColumn r) want (colTy c)
                    | colTy c /= want
                    ]
                      ++ [ IdNamesWrongTable (tName t) (refColumn r)
                         | TId of' <- [colTy c]
                         , of' /= refTable r
                         ]
              _ -> [RefToCompositeKey (tName t) (refTable r)]

