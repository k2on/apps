{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §7.2 A module from a value: the inverse of 'Ark.Encode'.
--
-- What a peer does with a @Module@ or @Closure@ frame, and what @arkc@
-- does with a @.ark@ file: read the tagged structs back into the IR. It is
-- strict about shape — an unknown tag, a missing field or a field of the
-- wrong type is a 'DecodeError' naming the path — and lenient about
-- nothing, because a module that half-decodes is a module that will run
-- differently on two machines.
--
-- 'fromValue . toValue' is the identity on normalised modules, which the
-- vectors hold; and because 'toValue' does not carry symbol names, a
-- decoded function's 'fnNames' is empty.
module Ark.Decode
  ( DecodeError (..)
  , fromValue
  , functionFromValue
  , closureFromValue
  , tyFromValue
  , schemaFromValue
  ) where

import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text as T

import Ark.Hash (Closure (..))
import Ark.IR
import Ark.Schema
import Ark.Value

data DecodeError = DecodeError
  { dePath :: [Text]
  , deWhat :: Text
  }
  deriving (Eq, Show)

type D a = Either DecodeError a

-- | The whole module.
fromValue :: Value -> D Module
fromValue v = do
  fs <- tagged ["module"] "module" v
  spec <- field fs "spec" >>= int ["module", "spec"]
  sch <- field fs "schema" >>= schemaFromValue
  fns <- field fs "functions" >>= list ["module", "functions"] functionFromValue
  live <- field fs "live" >>= list ["module", "live"] frame
  pure (Module (fromIntegral spec) sch fns live)
  where
    frame x = do
      fs <- tagged ["frame"] "frame" x
      n <- field fs "name" >>= text ["frame", "name"]
      t <- field fs "ty" >>= tyFromValue
      pure (n, t)

-- | A closure as an authority stores or sends one: the function and the
-- helpers it reaches. Encoded as a struct @{ t: "closure", fn, helpers }@.
closureFromValue :: Value -> D Closure
closureFromValue v = do
  fs <- tagged ["closure"] "closure" v
  fn <- field fs "fn" >>= functionFromValue
  hs <- field fs "helpers" >>= list ["closure", "helpers"] functionFromValue
  pure (Closure fn hs)

schemaFromValue :: Value -> D Schema
schemaFromValue v = Schema <$> list ["schema"] scope v
  where
    scope x = do
      fs <- tagged ["scope"] "scope" x
      n <- field fs "name" >>= text ["scope", "name"]
      ts <- field fs "tables" >>= list ["scope", n] table
      pure (Scope n ts)
    table x = do
      fs <- tagged ["table"] "table" x
      n <- field fs "name" >>= text ["table", "name"]
      cs <- field fs "columns" >>= list ["table", n, "columns"] col
      k <- field fs "key" >>= list ["table", n, "key"] (text ["table", n, "key"])
      ixs <- field fs "indexes" >>= list ["table", n, "indexes"] index
      rs <- field fs "refs" >>= list ["table", n, "refs"] ref
      pure (Table n cs k ixs rs)
    col x = do
      fs <- tagged ["column"] "column" x
      n <- field fs "name" >>= text ["column", "name"]
      t <- field fs "ty" >>= tyFromValue
      nl <- field fs "nullable" >>= bool ["column", n, "nullable"]
      pure (Column n t nl)
    index x = do
      fs <- tagged ["index"] "index" x
      cs <- field fs "columns" >>= list ["index", "columns"] (text ["index", "columns"])
      u <- field fs "unique" >>= bool ["index", "unique"]
      pure (Index cs u)
    ref x = do
      fs <- tagged ["ref"] "ref" x
      c <- field fs "column" >>= text ["ref", "column"]
      t <- field fs "table" >>= text ["ref", "table"]
      pure (Ref c t)

tyFromValue :: Value -> D Ty
tyFromValue v = do
  (t, fs) <- taggedAny ["ty"] v
  case t of
    "bool" -> pure TBool
    "int" -> pure TInt
    "text" -> pure TText
    "bytes" -> pure TBytes
    "id" -> TId <$> (field fs "table" >>= text ["ty", "id"])
    "enum" -> TEnum <$> (field fs "variants" >>= list ["ty", "enum"] (text ["ty", "enum"]))
    "option" -> TOption <$> (field fs "of" >>= tyFromValue)
    "list" -> TList <$> (field fs "of" >>= tyFromValue)
    "struct" -> do
      m <- field fs "fields" >>= structMap ["ty", "struct"]
      TStruct <$> traverse tyFromValue m
    other -> Left (DecodeError ["ty"] ("unknown type tag " <> other))

functionFromValue :: Value -> D Function
functionFromValue v = do
  fs <- tagged ["fn"] "fn" v
  n <- field fs "name" >>= text ["fn", "name"]
  let here = ["fn", n]
  k <- field fs "kind" >>= text (here ++ ["kind"]) >>= \case
    "mutator" -> pure Mutator
    "query" -> pure Query
    "helper" -> pure Helper
    other -> Left (DecodeError here ("unknown kind " <> other))
  sc <- field fs "scope" >>= optional (text (here ++ ["scope"]))
  autos <- field fs "autos" >>= list (here ++ ["autos"]) auto
  args <- field fs "args" >>= list (here ++ ["args"]) arg
  ret <- field fs "ret" >>= optional tyFromValue
  body <- field fs "body" >>= list (here ++ ["body"]) (stmt here)
  pure (Function n k sc autos args ret body M.empty)
  where
    auto x = do
      (t, fs) <- taggedAny ["auto"] x
      n <- field fs "name" >>= text ["auto", "name"]
      case t of
        "new_id" -> (\tb -> (n, NewId tb)) <$> (field fs "table" >>= text ["auto", n])
        "now" -> pure (n, Now)
        other -> Left (DecodeError ["auto", n] ("unknown auto " <> other))
    arg x = do
      fs <- tagged ["arg"] "arg" x
      n <- field fs "name" >>= text ["arg", "name"]
      t <- field fs "ty" >>= tyFromValue
      pure (n, t)

stmt :: [Text] -> Value -> D Stmt
stmt here v = do
  (t, fs) <- taggedAny here v
  let p = here ++ [t]
  case t of
    "let" -> SLet <$> (field fs "sym" >>= sym p) <*> (field fs "e" >>= expr p)
    "if" -> SIf <$> (field fs "c" >>= expr p) <*> (field fs "then" >>= list p (stmt p)) <*> (field fs "else" >>= list p (stmt p))
    "for" -> SFor <$> (field fs "sym" >>= sym p) <*> (field fs "in" >>= expr p) <*> (field fs "body" >>= list p (stmt p))
    "put" -> SPut <$> (field fs "table" >>= text p) <*> (field fs "row" >>= expr p)
    "delete" -> SDelete <$> (field fs "table" >>= text p) <*> (field fs "key" >>= list p (expr p))
    "refuse" -> SRefuse <$> (field fs "e" >>= expr p)
    "return" -> SReturn <$> (field fs "e" >>= optional (expr p))
    other -> Left (DecodeError here ("unknown statement " <> other))

expr :: [Text] -> Value -> D Expr
expr here v = do
  (t, fs) <- taggedAny here v
  let p = here ++ [t]
      e k = field fs k >>= expr p
      s k = field fs k >>= sym p
  case t of
    "lit" -> ELit <$> field fs "v"
    "arg" -> EArg <$> (field fs "name" >>= text p)
    "auto" -> EAuto <$> (field fs "name" >>= text p)
    "var" -> EVar <$> s "sym"
    "ctx_user" -> pure ECtxUser
    "ctx_session" -> pure ECtxSession
    "field" -> EField <$> e "e" <*> (field fs "name" >>= text p)
    "struct" -> EStruct <$> (field fs "fields" >>= structMap p >>= traverse (expr p))
    "list" -> EList <$> (field fs "items" >>= list p (expr p))
    "some" -> ESome <$> e "e"
    "none" -> ENone <$> (field fs "ty" >>= tyFromValue)
    "match" -> EMatch <$> e "e" <*> s "sym" <*> e "some" <*> e "none"
    "ife" -> EIf <$> e "c" <*> e "then" <*> e "else"
    "op" -> EOp <$> (field fs "op" >>= text p >>= op p) <*> (field fs "args" >>= list p (expr p))
    "cmp" -> ECmp <$> (field fs "op" >>= text p >>= cmpOp p) <*> e "l" <*> e "r"
    "call" -> ECall <$> (field fs "fn" >>= text p) <*> (field fs "args" >>= list p (expr p))
    "std" -> EStd <$> (field fs "fn" >>= text p >>= stdFn p) <*> (field fs "args" >>= list p (expr p))
    "map" -> EMap <$> e "in" <*> s "sym" <*> e "body"
    "filter" -> EFilter <$> e "in" <*> s "sym" <*> e "body"
    "any" -> EAny <$> e "in" <*> s "sym" <*> e "body"
    "all" -> EAll <$> e "in" <*> s "sym" <*> e "body"
    "sort_by" -> ESortBy <$> e "in" <*> s "sym" <*> e "key"
    "fold" -> EFold <$> e "in" <*> e "init" <*> s "acc" <*> s "sym" <*> e "body"
    "select" -> ESelect <$> (field fs "plan" >>= plan p)
    "get" -> EGet <$> (field fs "table" >>= text p) <*> (field fs "key" >>= list p (expr p))
    "exists" -> EExists <$> (field fs "table" >>= text p) <*> (field fs "key" >>= list p (expr p))
    other -> Left (DecodeError here ("unknown expression " <> other))

plan :: [Text] -> Value -> D Plan
plan here v = do
  fs <- tagged here "plan" v
  tbl <- field fs "table" >>= text here
  f <- field fs "filter" >>= optional (pred' (here ++ [tbl]))
  o <- field fs "order" >>= list here by
  l <- field fs "limit" >>= optional (int here)
  rs <- field fs "related" >>= list here related
  pure (Plan tbl f o (fromIntegral <$> l) rs)
  where
    by x = do
      fs <- tagged here "by" x
      c <- field fs "column" >>= text here
      d <- field fs "dir" >>= text here >>= \case
        "asc" -> pure Asc
        "desc" -> pure Desc
        other -> Left (DecodeError here ("unknown direction " <> other))
      pure (c, d)
    related x = do
      fs <- tagged here "related" x
      n <- field fs "name" >>= text here
      parent <- field fs "parent" >>= text here
      child <- field fs "child" >>= text here
      col <- field fs "column" >>= text here
      pl <- field fs "plan" >>= plan (here ++ [n])
      pure (Related n (Relation parent child col) pl)

pred' :: [Text] -> Value -> D Pred
pred' here v = do
  (t, fs) <- taggedAny here v
  case t of
    "pcmp" -> PCmp <$> (field fs "column" >>= text here) <*> (field fs "op" >>= text here >>= cmpOp here) <*> (field fs "e" >>= expr here)
    "pin" -> PIn <$> (field fs "column" >>= text here) <*> (field fs "items" >>= list here (expr here))
    "pall" -> PAll <$> (field fs "items" >>= list here (pred' here))
    "pany" -> PAny <$> (field fs "items" >>= list here (pred' here))
    "pnot" -> PNot <$> (field fs "e" >>= pred' here)
    other -> Left (DecodeError here ("unknown predicate " <> other))

op :: [Text] -> Text -> D Op
op here = \case
  "add" -> pure Add
  "sub" -> pure Sub
  "mul" -> pure Mul
  "div" -> pure Div
  "mod" -> pure Mod
  "neg" -> pure Neg
  "and" -> pure And
  "or" -> pure Or
  "not" -> pure Not
  other -> Left (DecodeError here ("unknown operator " <> other))

cmpOp :: [Text] -> Text -> D CmpOp
cmpOp here = \case
  "eq" -> pure Eq
  "ne" -> pure Ne
  "lt" -> pure Lt
  "le" -> pure Le
  "gt" -> pure Gt
  "ge" -> pure Ge
  other -> Left (DecodeError here ("unknown comparison " <> other))

-- The standard library is named by its constructors' 'Show' spelling, as
-- 'Ark.Encode' writes it.
stdFn :: [Text] -> Text -> D StdFn
stdFn here name = case [f | f <- [minBound .. maxBound], T.pack (show f) == name] of
  (f : _) -> pure f
  [] -> Left (DecodeError here ("unknown standard function " <> name))

-- Primitives ------------------------------------------------------------

tagged :: [Text] -> Text -> Value -> D (Map FieldName Value)
tagged here want v = do
  (t, fs) <- taggedAny here v
  if t == want then pure fs else Left (DecodeError here ("expected " <> want <> ", found " <> t))

taggedAny :: [Text] -> Value -> D (Text, Map FieldName Value)
taggedAny here = \case
  VStruct fs -> case M.lookup "t" fs of
    Just (VText t) -> pure (t, fs)
    _ -> Left (DecodeError here "a node needs a text tag \"t\"")
  _ -> Left (DecodeError here "expected a struct")

field :: Map FieldName Value -> FieldName -> D Value
field fs k = maybe (Left (DecodeError [k] "missing field")) Right (M.lookup k fs)

optional :: (Value -> D a) -> Value -> D (Maybe a)
optional _ VNull = pure Nothing
optional f v = Just <$> f v

list :: [Text] -> (Value -> D a) -> Value -> D [a]
list here f = \case
  VList xs -> mapM f xs
  _ -> Left (DecodeError here "expected a list")

structMap :: [Text] -> Value -> D (Map FieldName Value)
structMap here = \case
  VStruct m -> pure m
  _ -> Left (DecodeError here "expected a struct")

text :: [Text] -> Value -> D Text
text here = \case
  VText t -> pure t
  _ -> Left (DecodeError here "expected text")

int :: [Text] -> Value -> D Integer
int here = \case
  VInt n -> pure (toInteger n)
  _ -> Left (DecodeError here "expected an int")

bool :: [Text] -> Value -> D Bool
bool here = \case
  VBool b -> pure b
  _ -> Left (DecodeError here "expected a bool")

sym :: [Text] -> Value -> D Sym
sym here v = fromIntegral <$> int here v

