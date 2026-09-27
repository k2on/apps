{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §16 The printable form.
--
-- What @arkc print@ writes: a module as text a person reads and a diff
-- shows. It replaces Petros's @mutations.txt@ — the recorded surface the
-- log-compatibility check compared — as the thing a reviewer looks at when
-- a function changes, and it is the only place the author's names
-- ('fnNames') are ever read back.
--
-- It is __not__ canonical, and nothing parses it. The canonical form is
-- 'Ark.Encode.toValue' under 'Ark.Canon.encode'; a hash is taken of that
-- and never of this. Two things follow. Readability wins every trade here
-- — infix operators, the fewest parentheses that keep the meaning, names
-- over numbers — because no decoder has to undo any of it. And it is still
-- deterministic: the same module prints the same text, every construct of
-- the IR has a spelling (nothing falls back to @show@), and a diff of two
-- prints is a diff of two modules.
--
-- The shape, informally: the spec version, the schema as one @scope@ block
-- per scope with a @table@ line per table, the live frame types, and then
-- one function per block — its signature on the first line, its statements
-- indented two spaces per level beneath it. Arguments are @$name@, autos
-- @\@name@, locals their author's name or @v<n>@ where the author gave
-- none, and the context is @ctx.user@ and @ctx.session@. Symbols are
-- printed as the function carries them; a caller who wants the numbering
-- the hash saw normalises first ('Ark.Encode.normalize'), which also
-- carries the names across.
module Ark.Print
  ( printModule
  , printFunction
  , printSchema
  , printTy
  , printValue
  ) where

import Data.Char (ord)
import Data.List (intercalate)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text as T
import Numeric (showHex)

import Ark.IR
import Ark.Schema
import Ark.Std (hexText, textOfId)
import Ark.Value

-- The module ------------------------------------------------------------

-- | The whole module: version, schema, live types, functions, each part
-- separated from the next by one blank line, ending in a newline.
printModule :: Module -> Text
printModule m =
  T.unlines (intercalate [""] (filter (not . null) sections))
  where
    sections =
      [["spec " <> tshow (modSpec m)]]
        ++ [schemaLines (modSchema m)]
        ++ [[live n t | (n, t) <- modLive m]]
        ++ [T.lines (printFunction f) | f <- modFunctions m]
    live n t = "live " <> n <> ": " <> printTy t

-- The schema ------------------------------------------------------------

-- | The schema: one block per scope, one line per table, no trailing
-- newline.
printSchema :: Schema -> Text
printSchema = T.intercalate "\n" . schemaLines

schemaLines :: Schema -> [Text]
schemaLines (Schema scopes) = concatMap scope scopes
  where
    scope s
      | null (sTables s) = ["scope " <> sName s <> " {}"]
      | otherwise = ("scope " <> sName s <> " {") : map ((indent 1 <>) . table) (sTables s) ++ ["}"]
    table t =
      T.unwords
        ( ["table " <> tName t <> "(" <> commas (map col (tColumns t)) <> ")"]
            ++ ["key (" <> commas (tKey t) <> ")"]
            ++ map index (tIndexes t)
            ++ map ref (tRefs t)
        )
    col c = colName c <> ": " <> printTy (colTy c) <> (if colNullable c then "?" else "")
    index i = (if ixUnique i then "unique (" else "index (") <> commas (ixColumns i) <> ")"
    ref r = "ref " <> refColumn r <> " -> " <> refTable r

-- Types -----------------------------------------------------------------

printTy :: Ty -> Text
printTy = \case
  TBool -> "Bool"
  TInt -> "Int"
  TText -> "Text"
  TBytes -> "Bytes"
  TId t -> "Id(" <> t <> ")"
  TEnum vs -> "Enum(" <> T.intercalate "|" vs <> ")"
  TOption t -> "Option(" <> printTy t <> ")"
  TList t -> "List(" <> printTy t <> ")"
  TStruct fs -> "Struct{" <> commas [k <> ": " <> printTy t | (k, t) <- M.toAscList fs] <> "}"

-- Values ----------------------------------------------------------------

-- | A literal. Text is quoted with @\\@ escapes for the quote, the
-- backslash, newline, return and tab, and @\\u{…}@ for any other control
-- character; everything else is written as itself, so a title reads as a
-- title. Bytes are @0x@ and lowercase hex; an id is its uuid text; the
-- absent case of an option is @none@.
printValue :: Value -> Text
printValue = \case
  VNull -> "none"
  VBool b -> if b then "true" else "false"
  VInt n -> tshow n
  VText t -> quote t
  VBytes b -> "0x" <> hexText b
  VId i -> textOfId i
  VList vs -> list (map printValue vs)
  VStruct fs -> struct [(k, printValue v) | (k, v) <- M.toAscList fs]

quote :: Text -> Text
quote t = "\"" <> T.concatMap esc t <> "\""
  where
    esc c = case c of
      '"' -> "\\\""
      '\\' -> "\\\\"
      '\n' -> "\\n"
      '\r' -> "\\r"
      '\t' -> "\\t"
      _
        | ord c < 0x20 || ord c == 0x7f -> "\\u{" <> T.pack (showHex (ord c) "") <> "}"
        | otherwise -> T.singleton c

-- Functions -------------------------------------------------------------

-- | The author's names, by symbol.
type Names = Map Sym Text

-- | One function: its signature, then its body indented beneath it, no
-- trailing newline. Autos come before arguments in the signature, each as
-- @name: Now@ or @name: NewId(table)@; a mutator's scope follows as
-- @in scope@ and a result type as @-> Ty@.
printFunction :: Function -> Text
printFunction fn = T.intercalate "\n" (sig : block (fnNames fn) 1 (fnBody fn))
  where
    sig =
      kind (fnKind fn)
        <> " "
        <> fnName fn
        <> "("
        <> commas (map auto (fnAutos fn) ++ map arg (fnArgs fn))
        <> ")"
        <> maybe "" (" in " <>) (fnScope fn)
        <> maybe "" ((" -> " <>) . printTy) (fnRet fn)
    kind Mutator = "mutation"
    kind Query = "query"
    kind Helper = "helper"
    auto (n, Now) = n <> ": Now"
    auto (n, NewId t) = n <> ": NewId(" <> t <> ")"
    arg (n, t) = n <> ": " <> printTy t

sym :: Names -> Sym -> Text
sym names x = M.findWithDefault ("v" <> tshow x) x names

block :: Names -> Int -> Block -> [Text]
block names d = concatMap (stmt names d)

stmt :: Names -> Int -> Stmt -> [Text]
stmt names d = \case
  SLet x e -> [line ("let " <> sym names x <> " = " <> top e)]
  SIf c a b ->
    line ("if " <> top c)
      : block names (d + 1) a
      ++ (if null b then [] else line "else" : block names (d + 1) b)
  SFor x xs b -> line ("for " <> sym names x <> " in " <> top xs) : block names (d + 1) b
  SPut t e -> [line ("put(" <> t <> ", " <> top e <> ")")]
  SDelete t ks -> [line ("delete(" <> commas (t : map top ks) <> ")")]
  SRefuse e -> [line ("refuse " <> top e)]
  SReturn Nothing -> [line "return"]
  SReturn (Just e) -> [line ("return " <> top e)]
  where
    line = (indent d <>)
    top = expr names pTop

-- Expressions -----------------------------------------------------------

-- Precedence, loosest first. An expression is parenthesised where it
-- appears in a position that binds tighter than it does.
pTop, pIf, pOr, pAnd, pCmp, pAdd, pMul, pUnary, pField, pAtom :: Int
pTop = 0
pIf = 1
pOr = 2
pAnd = 3
pCmp = 4
pAdd = 5
pMul = 6
pUnary = 7
pField = 8
pAtom = 9

prec :: Expr -> Int
prec = \case
  EIf {} -> pIf
  ECmp {} -> pCmp
  EField {} -> pField
  EOp op es -> case (op, es) of
    (Or, _ : _ : _) -> pOr
    (And, _ : _ : _) -> pAnd
    (Add, [_, _]) -> pAdd
    (Sub, [_, _]) -> pAdd
    (Mul, [_, _]) -> pMul
    (Div, [_, _]) -> pMul
    (Mod, [_, _]) -> pMul
    (Neg, [_]) -> pUnary
    (Not, [_]) -> pUnary
    _ -> pAtom
  _ -> pAtom

-- | An expression in a position of precedence @p@.
expr :: Names -> Int -> Expr -> Text
expr names p e
  | prec e < p = "(" <> body <> ")"
  | otherwise = body
  where
    body = exprBody names e

exprBody :: Names -> Expr -> Text
exprBody names = \case
  ELit v -> printValue v
  EArg a -> "$" <> a
  EAuto a -> "@" <> a
  EVar x -> sym names x
  ECtxUser -> "ctx.user"
  ECtxSession -> "ctx.session"
  EField e f -> at pField e <> "." <> f
  EStruct fs -> struct [(k, at pTop v) | (k, v) <- M.toAscList fs]
  EList es -> list (map (at pTop) es)
  ESome e -> "some(" <> at pTop e <> ")"
  ENone t -> "none(" <> printTy t <> ")"
  EMatch e x a b ->
    "match " <> at pField e <> " { some " <> sym names x <> " -> " <> at pTop a <> ", none -> " <> at pTop b <> " }"
  EIf c a b -> "if " <> at pOr c <> " then " <> at pOr a <> " else " <> at pIf b
  EOp op es -> case (op, es) of
    (Or, _ : _ : _) -> T.intercalate " or " (map (at pOr) es)
    (And, _ : _ : _) -> T.intercalate " and " (map (at pAnd) es)
    (Add, [a, b]) -> binary pAdd "+" a b
    (Sub, [a, b]) -> binary pAdd "-" a b
    (Mul, [a, b]) -> binary pMul "*" a b
    (Div, [a, b]) -> binary pMul "/" a b
    (Mod, [a, b]) -> binary pMul "%" a b
    (Neg, [a]) -> "-" <> at pField a
    (Not, [a]) -> "not " <> at pField a
    -- An arity the operator has no infix spelling for: the name, applied.
    _ -> call (opName op) es
  ECmp op a b -> at pAdd a <> " " <> cmpName op <> " " <> at pAdd b
  ECall n es -> call n es
  EStd f es -> call (stdName f) es
  EMap xs x b -> lambda "map" xs x b
  EFilter xs x b -> lambda "filter" xs x b
  EAny xs x b -> lambda "any" xs x b
  EAll xs x b -> lambda "all" xs x b
  ESortBy xs x k -> lambda "sort_by" xs x k
  EFold xs z acc x b ->
    "fold(" <> at pTop xs <> ", " <> at pTop z <> ", (" <> sym names acc <> ", " <> sym names x <> ") -> " <> at pTop b <> ")"
  ESelect p -> "select(" <> plan names p <> ")"
  EGet t ks -> "get(" <> commas (t : map (at pTop) ks) <> ")"
  EExists t ks -> "exists(" <> commas (t : map (at pTop) ks) <> ")"
  where
    at = expr names
    -- Left-associative: the left operand may be another of the same level,
    -- the right may not.
    binary lvl s a b = at lvl a <> " " <> s <> " " <> at (lvl + 1) b
    call n es = n <> "(" <> commas (map (at pTop) es) <> ")"
    lambda n xs x b = n <> "(" <> at pTop xs <> ", " <> sym names x <> " -> " <> at pTop b <> ")"

opName :: Op -> Text
opName = \case
  Add -> "add"
  Sub -> "sub"
  Mul -> "mul"
  Div -> "div"
  Mod -> "mod"
  Neg -> "neg"
  And -> "and"
  Or -> "or"
  Not -> "not"

cmpName :: CmpOp -> Text
cmpName = \case
  Eq -> "="
  Ne -> "!="
  Lt -> "<"
  Le -> "<="
  Gt -> ">"
  Ge -> ">="

-- | The standard library, spelt as a generator would name it.
stdName :: StdFn -> Text
stdName = \case
  Trim -> "trim"
  IsEmpty -> "is_empty"
  Concat -> "concat"
  Lower -> "lower"
  IsAlnum -> "is_alnum"
  Chars -> "chars"
  TextLen -> "text_len"
  StartsWith -> "starts_with"
  SplitOnce -> "split_once"
  TextOfInt -> "text_of_int"
  Hex -> "hex"
  Min -> "min"
  Max -> "max"
  Clamp -> "clamp"
  Abs -> "abs"
  Fnv1a64 -> "fnv1a64"
  Sha256 -> "sha256"
  IdOfText -> "id_of_text"
  TextOfId -> "text_of_id"
  NilId -> "nil_id"
  Utf8 -> "utf8"
  First -> "first"
  Last -> "last"
  Len -> "len"
  Contains -> "contains"
  Reverse -> "reverse"
  IsSome -> "is_some"
  UnwrapOr -> "unwrap_or"

-- Plans -----------------------------------------------------------------

-- | @from t where … order c desc, k asc limit n with name: (… via child.col -> parent)@.
-- Each clause appears only when the plan has it.
plan :: Names -> Plan -> Text
plan names p =
  T.unwords
    ( ["from " <> pTable p]
        ++ ["where " <> predicate names f | Just f <- [pFilter p]]
        ++ ["order " <> commas [c <> " " <> dir d | (c, d) <- pOrder p] | not (null (pOrder p))]
        ++ ["limit " <> tshow n | Just n <- [pLimit p]]
        ++ map related (pRelated p)
    )
  where
    dir Asc = "asc"
    dir Desc = "desc"
    related r =
      let rel = rRelation r
       in "with "
            <> rName r
            <> ": ("
            <> plan names (rPlan r)
            <> " via "
            <> relChild rel
            <> "."
            <> relColumn rel
            <> " -> "
            <> relParent rel
            <> ")"

-- | A filter. @and@ and @or@ are written flat, since each is associative;
-- one nested inside the other is parenthesised, and so is anything under
-- @not@. An empty conjunction is @true@ and an empty disjunction @false@,
-- which is what each means.
predicate :: Names -> Pred -> Text
predicate names = go qTop
  where
    go q = \case
      PCmp c op e -> c <> " " <> cmpName op <> " " <> expr names pAdd e
      PIn c es -> c <> " in " <> list (map (expr names pTop) es)
      PAll [] -> "true"
      PAll ps -> paren (q > qAnd) (T.intercalate " and " (map (go qAnd) ps))
      PAny [] -> "false"
      PAny ps -> paren (q > qOr) (T.intercalate " or " (map (go qOr) ps))
      PNot r -> "not (" <> go qTop r <> ")"
    paren b t = if b then "(" <> t <> ")" else t
    qTop, qOr, qAnd :: Int
    qTop = 0
    qOr = 1
    qAnd = 2

-- Small pieces ----------------------------------------------------------

indent :: Int -> Text
indent d = T.replicate (2 * d) " "

commas :: [Text] -> Text
commas = T.intercalate ", "

list :: [Text] -> Text
list xs = "[" <> commas xs <> "]"

struct :: [(Text, Text)] -> Text
struct [] = "{}"
struct fs = "{ " <> commas [k <> ": " <> v | (k, v) <- fs] <> " }"

tshow :: Show a => a -> Text
tshow = T.pack . show
