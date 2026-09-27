{-# LANGUAGE OverloadedStrings #-}
-- | §3 Ark IR.
--
-- The program a domain is. A module carries a schema, functions and the
-- types of its live frames. A function is a mutator, a query or a helper;
-- its body is a statement list in a deliberately small imperative core over
-- a pure expression language. Nobody runs this IR in production: a builder
-- in Rust, Swift or Kotlin constructs it, 'Ark.Verify' checks it, and a
-- generator writes native source from it. This module defines what that
-- source must mean, and 'Ark.Eval' is the meaning.
--
-- Three properties are designed in rather than checked afterwards:
--
-- * __total__: no @while@, no recursion; 'SFor' iterates a list; a helper
--   may call only helpers declared before it. Every function terminates,
--   so an authority can run a stranger's module with no timeout to tune.
-- * __deterministic__: no clock, no randomness, no I/O and no floats exist
--   in the language; the only non-determinism a mutator sees arrives in its
--   'fnAutos', chosen once at the originating peer and frozen in the log.
-- * __scoped__: a mutator names one scope and touches only its tables.
--
-- Local variables are 'Sym's — small integers assigned in order of
-- binding — so that two authors' choices of names never reach the hash of
-- a function. The names an author wrote travel in 'fnNames', which is not
-- part of the canonical form.
module Ark.IR
  ( Module (..)
  , Function (..)
  , FnKind (..)
  , Auto (..)
  , Sym
  , Block
  , Stmt (..)
  , Expr (..)
  , Op (..)
  , CmpOp (..)
  , Plan (..)
  , Related (..)
  , Pred (..)
  , StdFn (..)
  , SpecVersion
  , specVersion
  , lookupFunction
  ) where

import Data.List (find)
import Data.Map.Strict (Map)
import Data.Text (Text)

import Ark.Schema
import Ark.Value

-- | The version of this specification a module was written against. A
-- runtime states the versions it implements in 'Hello'; an authority whose
-- module needs a newer one answers 'Denied', which a client shows as
-- "update required". It is about the runtime alone, never about the app.
type SpecVersion = Int

specVersion :: SpecVersion
specVersion = 1

data Module = Module
  { modSpec :: SpecVersion
  , modSchema :: Schema
  , -- | In declaration order. A helper may be called only by functions
    -- after it in this list, which is what makes every call graph a DAG.
    modFunctions :: [Function]
  , -- | §3.9 The live section: the frame types an app's realtime channel
    -- carries, by name. Declared here so that every runtime generates them;
    -- the engine never looks inside a frame and none of the log's rules
    -- bind one.
    modLive :: [(Text, Ty)]
  }
  deriving (Eq, Show)

data FnKind
  = -- | Writes. Takes a context and autos; reads and writes one scope;
    -- may 'SRefuse'. Its effect is what the log records.
    Mutator
  | -- | Reads. Takes arguments; may 'ESelect' from any scope the peer
    -- holds; returns a value; cannot write or refuse. Not in the log, so
    -- not held to permanence.
    Query
  | -- | Pure. No store access at all, no refusal; returns a value. This
    -- is where a domain's @slug@ and @art_to_write@ live.
    Helper
  deriving (Eq, Show)

-- | The non-determinism a mutator is allowed, by type. Each is one value,
-- drawn once at the originating peer by @fill_auto@ and frozen in the
-- entry; the mutator reads it as 'EAuto'. A mutator may declare several
-- 'NewId's, since the one-uuid limit was Petros's @fill_auto@ and never the
-- log's.
data Auto
  = NewId TableName -- ^ a fresh 'VId' naming a row of this table
  | Now -- ^ milliseconds since the Unix epoch, as 'VInt'
  deriving (Eq, Show)

data Function = Function
  { fnName :: Text
  , fnKind :: FnKind
  , -- | The scope a mutator belongs to; 'Nothing' for queries and helpers.
    fnScope :: Maybe ScopeName
  , fnAutos :: [(Text, Auto)]
  , fnArgs :: [(Text, Ty)]
  , -- | The result type of a query or helper; 'Nothing' for a mutator.
    fnRet :: Maybe Ty
  , fnBody :: Block
  , -- | The author's names for symbols; not hashed, not required, only
    -- for the printable form and for generated code to read well.
    fnNames :: Map Sym Text
  }
  deriving (Eq, Show)

lookupFunction :: Module -> Text -> Maybe Function
lookupFunction m n = find ((== n) . fnName) (modFunctions m)

-- | A local variable, alpha-normalised: the @n@th binding in a function,
-- counting from 0 in evaluation order. 'Ark.Verify' renumbers.
type Sym = Int

type Block = [Stmt]

-- | §3.1 Statements.
data Stmt
  = -- | Bind a value. Reads ('ESelect', 'EGet', 'EExists') may appear only
    -- as the whole right-hand side of a 'SLet', so a read runs exactly once
    -- wherever a builder's host-level reuse of the expression might
    -- otherwise have duplicated it.
    SLet Sym Expr
  | SIf Expr Block Block
  | -- | Iterate a list, binding each element. Finite by construction.
    SFor Sym Expr Block
  | -- | Write a full row. Reports an @Add@, an @Edit@, or nothing if the
    -- row is already exactly that; refuses on a constraint. See
    -- 'Ark.Store.put'.
    SPut TableName Expr
  | -- | Delete by key (a list of the key columns' values). A missing row is
    -- a no-op; a row another row references is a refusal.
    SDelete TableName [Expr]
  | -- | End the mutator with a deterministic verdict. Every replica reaches
    -- the same one, so it is a fact about the entry and not a failure.
    SRefuse Expr
  | -- | Leave the function. A mutator returns nothing; a query or helper
    -- returns a value of its 'fnRet'.
    SReturn (Maybe Expr)
  deriving (Eq, Show)

-- | §3.2 Expressions. Pure, apart from the three reads, which the verifier
-- confines to 'SLet' in mutators and queries.
data Expr
  = ELit Value
  | EArg Text
  | EAuto Text
  | EVar Sym
  | -- | The user the authority verified for the entry's connection.
    ECtxUser
  | -- | The login the entry was authored under.
    ECtxSession
  | EField Expr FieldName
  | EStruct (Map FieldName Expr)
  | EList [Expr]
  | -- | @Some e@.
    ESome Expr
  | -- | @None@, at the type given, so that the expression is typed alone.
    ENone Ty
  | -- | @match e { Some x -> a; None -> b }@; @x@ is bound in @a@.
    EMatch Expr Sym Expr Expr
  | EIf Expr Expr Expr
  | EOp Op [Expr]
  | ECmp CmpOp Expr Expr
  | -- | A call to a helper declared earlier in the module.
    ECall Text [Expr]
  | EStd StdFn [Expr]
  | -- | @map@, @filter@, @any@, @all@, @sort_by@: the element is bound in
    -- the body.
    EMap Expr Sym Expr
  | EFilter Expr Sym Expr
  | EAny Expr Sym Expr
  | EAll Expr Sym Expr
  | -- | Stable sort by a key expression, under 'compareValue'.
    ESortBy Expr Sym Expr
  | -- | @fold xs init (acc, x -> body)@.
    EFold Expr Expr Sym Sym Expr
  | ESelect Plan
  | EGet TableName [Expr]
  | EExists TableName [Expr]
  deriving (Eq, Show)

-- | Arithmetic and boolean operators. Integer arithmetic is __checked__:
-- overflow, division by zero and @minBound / -1@ are refusals, never
-- wrapping — a hash is a standard-library function, and a wrapping
-- multiply is not something three languages spell the same way by
-- accident.
data Op
  = Add
  | Sub
  | Mul
  | Div -- ^ truncating toward zero, as every target's integer division does
  | Mod -- ^ the remainder with the dividend's sign, as every target's @%@ does
  | Neg
  | And
  | Or
  | Not
  deriving (Eq, Ord, Show)

-- | Comparison under 'compareValue'. Both sides must have the same type.
data CmpOp = Eq | Ne | Lt | Le | Gt | Ge
  deriving (Eq, Ord, Show)

-- | §3.3 Plans. A plan is a query's maintainable half: what
-- 'Ark.Store.select' pulls today and what a view maintains incrementally.
-- Its filter, order and limit are data, so one plan is a statement to a
-- backend and a pipeline to a view, and the two must agree.
data Plan = Plan
  { pTable :: TableName
  , pFilter :: Maybe Pred
  , -- | The verifier makes every mutator's order total by appending the
    -- key columns ascending, because a @limit 1@ over a partial order is
    -- exactly the kind of thing two backends answer differently.
    pOrder :: [(FieldName, Dir)]
  , pLimit :: Maybe Int
  , -- | Relationships to read beneath each row, each appearing as a field
    -- of that name holding a list of child rows (reading down, so a
    -- childless parent is still a row).
    pRelated :: [Related]
  }
  deriving (Eq, Show)

data Related = Related
  { rName :: FieldName
  , rRelation :: Relation
  , rPlan :: Plan
  }
  deriving (Eq, Show)

-- | A filter over one row. The right-hand sides are expressions that may
-- not mention the row (they are evaluated once, before the scan).
data Pred
  = PCmp FieldName CmpOp Expr
  | PIn FieldName [Expr]
  | PAll [Pred]
  | PAny [Pred]
  | PNot Pred
  deriving (Eq, Show)

-- | §3.4 The standard library, by name. Its semantics are 'Ark.Std'; its
-- admission rule is an exact definition plus vectors, and Unicode is
-- pinned as data ('Ark.Std.Unicode'). Anything a runtime cannot promise to
-- compute identically is not here.
data StdFn
  = -- text
    Trim -- ^ strip White_Space from both ends
  | IsEmpty
  | Concat -- ^ of a list of texts
  | Lower -- ^ simple lowercase mapping, code point by code point
  | IsAlnum -- ^ non-empty and every code point Alphabetic or numeric
  | Chars -- ^ the code points, each as a one-character text
  | TextLen -- ^ in code points
  | StartsWith
  | SplitOnce -- ^ at the first occurrence: @Some {before, after}@ or @None@
  | TextOfInt -- ^ decimal, with a leading @-@ for negatives
  | Hex -- ^ lowercase, two digits per byte
    -- int
  | Min
  | Max
  | Clamp
  | Abs
    -- hash
  | Fnv1a64 -- ^ of a text's UTF-8 bytes, as 'VInt' (the u64 reinterpreted)
  | Sha256 -- ^ of bytes, as 32 bytes
    -- id
  | IdOfText -- ^ parse 8-4-4-4-12 hex; @None@ if not an id
  | TextOfId -- ^ canonical lowercase 8-4-4-4-12
  | NilId
  | Utf8 -- ^ a text's bytes
    -- list and option
  | First
  | Last
  | Len
  | Contains
  | Reverse
  | IsSome
  | UnwrapOr
  deriving (Eq, Ord, Show, Enum, Bounded)
