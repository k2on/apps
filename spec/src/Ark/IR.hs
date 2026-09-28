{-# LANGUAGE OverloadedStrings #-}
-- | §3 Ark IR.
--
-- The program a domain is. A module carries a schema, routers, functions
-- and the types of its live frames. A function is a mutator, a query, a
-- helper, or a piece of middleware (a guard or a provider); its body is a
-- statement list in a deliberately small imperative core over a pure
-- expression language. Nobody writes this IR by hand: a domain is written
-- in Rust, Swift or Kotlin against one vocabulary (@spec/AUTHORING.md@),
-- running that program under @Emit@ yields this, 'Ark.Verify' checks it,
-- and running the same program under @Native@ must agree with 'Ark.Eval'
-- over it — which is the meaning.
--
-- Three properties are designed in rather than checked afterwards:
--
-- * __total__: no @while@, no recursion; 'SFor' iterates a list; a helper
--   may call only helpers declared before it. Every function terminates,
--   so an authority can run a stranger's module with no timeout to tune.
-- * __deterministic__: no clock, no randomness, no I/O and no floats exist
--   in the language; the only non-determinism a mutator sees arrives in its
--   'fnAutos', chosen once at the originating peer and frozen in the log.
-- * __scoped__: a router names one scope and every procedure on it reads
--   and writes only that scope's tables.
--
-- Local variables are 'Sym's — small integers assigned in order of
-- binding — so that two authors' choices of names never reach the hash of
-- a function. The names an author wrote travel in 'fnNames', which is not
-- part of the canonical form.
module Ark.IR
  ( Module (..)
  , Router (..)
  , Function (..)
  , FnKind (..)
  , Auto (..)
  , Field (..)
  , Check (..)
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
  , lookupRouter
  , fnArgs
  , isProcedure
  , isMiddleware
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

-- | Version 2: routers, middleware, input schemas with checks, and the
-- three table writes (@insert@, @upsert@, @update@) in place of @put@.
specVersion :: SpecVersion
specVersion = 2

data Module = Module
  { modSpec :: SpecVersion
  , modSchema :: Schema
  , -- | In declaration order. A helper may be called only by functions
    -- after it in this list, which is what makes every call graph a DAG.
    modFunctions :: [Function]
  , -- | §3.10 Routers, in declaration order. Every mutator and query is on
    -- exactly one.
    modRouters :: [Router]
  , -- | §3.9 The live section: the frame types an app's realtime channel
    -- carries, by name. Declared here so that every runtime generates them;
    -- the engine never looks inside a frame and none of the log's rules
    -- bind one.
    modLive :: [(Text, Ty)]
  }
  deriving (Eq, Show)

-- | §3.10 A router: a scope, and the middleware declared on it.
--
-- A procedure belongs to one router and inherits its scope. What a
-- procedure /runs/ before its body is its own 'fnUses' — the chain it was
-- built from, a subsequence of the router's 'rtUses' — so that
-- @signed_in.mutation(..)@ and @owned.mutation(..)@ on one router run
-- different chains, as tRPC's builders do.
data Router = Router
  { rtName :: Text
  , rtScope :: ScopeName
  , -- | Every middleware function declared on this router, in declaration
    -- order, by name.
    rtUses :: [Text]
  }
  deriving (Eq, Show)

data FnKind
  = -- | Writes. Takes a context and autos; reads and writes its router's
    -- scope; may 'SRefuse'. Its effect is what the log records.
    Mutator
  | -- | Reads. Takes an input; may 'ESelect' from its router's scope;
    -- returns a value; may refuse (a check or a guard) but cannot write.
    -- Not in the log, so not held to permanence.
    Query
  | -- | Pure. No store access at all, no refusal; returns a value. This
    -- is where a domain's @slug@ and @art_to_write@ live.
    Helper
  | -- | Middleware that runs before a procedure's body and may refuse; it
    -- returns nothing. Reads its scope; writes nothing.
    Guard
  | -- | Middleware that runs before a procedure's body, may refuse, and
    -- returns a value of its 'fnRet', which the body reads as
    -- 'EProvided' under the middleware's name.
    Provide
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

-- | §3.11 One field of a function's input: its type and the checks that
-- run on it, in order, before anything else does.
data Field = Field
  { fTy :: Ty
  , fChecks :: [Check]
  }
  deriving (Eq, Show)

-- | §3.11 A check on one input field. Every constructor but 'CTrim' carries
-- an optional message; 'Nothing' means the default 'Ark.Eval.defaultMessage'
-- gives. A failing check is a refusal with that message. On a 'TOption'
-- field the checks run only when the value is present.
data Check
  = -- | Text: strip White_Space from both ends, before every later check
    -- and before the body. Normalisation, not a test.
    CTrim
  | -- | Text: at least @n@ code points.
    CMinLen Int (Maybe Text)
  | -- | Text: at most @n@ code points.
    CMaxLen Int (Maybe Text)
  | -- | Int: @lo <= v <= hi@, either bound optional.
    CRange (Maybe Int) (Maybe Int) (Maybe Text)
  | -- | List: at least one element.
    CNonEmpty (Maybe Text)
  | -- | Id: a row with that key exists, in the procedure's scope.
    CExists (Maybe Text)
  | -- | Any type: the expression, over @EArg <this field>@, is true.
    CRefine Expr (Maybe Text)
  deriving (Eq, Show)

data Function = Function
  { fnName :: Text
  , fnKind :: FnKind
  , -- | The scope: a procedure's is its router's; middleware names its
    -- own; 'Nothing' for a helper.
    fnScope :: Maybe ScopeName
  , -- | The router a mutator or query is on; 'Nothing' otherwise.
    fnRouter :: Maybe Text
  , -- | The middleware this procedure runs before its body, in order; a
    -- subsequence of its router's 'rtUses'. Empty for helpers and
    -- middleware.
    fnUses :: [Text]
  , fnAutos :: [(Text, Auto)]
  , -- | The input, field by field, in declaration order. For middleware,
    -- the fields of the procedure's input it reads (the verifier requires
    -- every procedure using it to have those fields at those types); for
    -- a helper, its parameters, with no checks.
    fnInput :: [(Text, Field)]
  , -- | Checks over the whole input, after every field's own, each with
    -- its optional message.
    fnRefine :: [(Expr, Maybe Text)]
  , -- | The result type of a query, helper or provider; 'Nothing' for a
    -- mutator or guard.
    fnRet :: Maybe Ty
  , fnBody :: Block
  , -- | The author's names for symbols; not hashed, not required, only
    -- for the printable form and for the authoring form to read well.
    fnNames :: Map Sym Text
  }
  deriving (Eq, Show)

-- | The input's names and types alone, which is all most rules need.
fnArgs :: Function -> [(Text, Ty)]
fnArgs fn = [(n, fTy f) | (n, f) <- fnInput fn]

isProcedure :: Function -> Bool
isProcedure fn = fnKind fn `elem` [Mutator, Query]

isMiddleware :: Function -> Bool
isMiddleware fn = fnKind fn `elem` [Guard, Provide]

lookupFunction :: Module -> Text -> Maybe Function
lookupFunction m n = find ((== n) . fnName) (modFunctions m)

lookupRouter :: Module -> Text -> Maybe Router
lookupRouter m n = find ((== n) . rtName) (modRouters m)

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
  | -- | Write a full row unless one matches: on the columns named, which
    -- must be a declared unique index of the table, or on the key when
    -- none are named. A match is a no-op reporting nothing; an insert
    -- never edits. Otherwise reports an @Add@, or refuses on a
    -- constraint. See 'Ark.Store.insertOn'.
    SInsert TableName Expr [FieldName]
  | -- | Write a full row; where one matches on the columns named (or the
    -- key), keep the matching row's key columns and take the rest from
    -- the new row. Reports @Add@, @Edit@ or nothing. @SUpsert t e []@ is
    -- exactly a put. See 'Ark.Store.upsertOn'.
    SUpsert TableName Expr [FieldName]
  | -- | Rewrite the row at a key: the existing row is bound to the symbol
    -- for the new row's expression, whose key columns are then the
    -- existing ones. A missing row is a no-op. See 'Ark.Store.update'.
    SUpdate TableName [Expr] Sym Expr
  | -- | Delete by key (a list of the key columns' values). A missing row is
    -- a no-op; a row another row references is a refusal.
    SDelete TableName [Expr]
  | -- | End the function with a deterministic verdict. Every replica
    -- reaches the same one, so it is a fact about the entry and not a
    -- failure.
    SRefuse Expr
  | -- | Leave the function. A mutator or guard returns nothing; a query,
    -- helper or provider returns a value of its 'fnRet'.
    SReturn (Maybe Expr)
  deriving (Eq, Show)

-- | §3.2 Expressions. Pure, apart from the three reads, which the verifier
-- confines to 'SLet' in everything but a helper.
data Expr
  = ELit Value
  | EArg Text
  | EAuto Text
  | EVar Sym
  | -- | The user the authority verified for the entry's connection.
    ECtxUser
  | -- | The login the entry was authored under.
    ECtxSession
  | -- | What a 'Provide' middleware of that name returned, before the body
    -- ran.
    EProvided Text
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
  , -- | The verifier makes every order total by appending the key columns
    -- ascending, because a @limit 1@ over a partial order is exactly the
    -- kind of thing two backends answer differently.
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
  | Unwrap -- ^ the value, or the refusal @unwrapped none@; what @or_refuse@ reads after its check
  deriving (Eq, Ord, Show, Enum, Bounded)
