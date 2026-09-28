{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §18 The authoring form: a module printed back as the source that
-- would emit it, in Rust, Swift or Kotlin.
--
-- A domain is written once in the vocabulary of @spec/AUTHORING.md@ and
-- emitted as a module; this is the other direction. 'files' decompiles a
-- verified module into the canonical text of that vocabulary — the
-- schema, one file per router, and the module file — and the property
-- the checks hold is the round trip: a source in canonical form prints
-- back as itself, and a print emits the module it was printed from.
--
-- There is one decompiler and three spellings. The decompiler reads each
-- function against the lowerings of AUTHORING §6 in reverse — a @first@
-- pair of reads is one @.first()@, a @get@ followed by its refusal is
-- @.or_refuse(..)@, an option matched three ways is @.filter@, @.map@ or
-- @.map_or@ — and produces a small source tree; a renderer per 'Target'
-- spells that tree. What none of them decides is where a line breaks:
-- each language has a formatter that owns its layout (@rustfmt@,
-- @swift-format@, @ktfmt@), and a canonical file is that formatter's
-- output over this module's. So the printers write one logical line per
-- statement and item, and the blank lines between items, and nothing
-- else about layout.
--
-- __Names are derived, never captured__ (AUTHORING §6), because no host
-- can see a @let@'s name or a closure parameter's: a read bound by a
-- @let@ is named after its table, a closure parameter over a row is
-- @row@, and an input type is named after the function that declares it.
-- A binding is inlined — written where it is used, with no @let@ — when
-- its single use heads the next statement: the receiver of the chain that
-- statement builds, or the value a function returns. That is the one
-- choice the IR cannot record (a host @let@ emits nothing), and this rule
-- is how it is made.
--
-- A router file also carries the module's helpers and records ('records'),
-- each in the file its first user is in ('homeOf'), and imports what it
-- names from another. Rust and Swift spell them; Kotlin not yet
-- (AUTHORING §2.5).
module Ark.Gen
  ( Target (..)
  , Options (..)
  , defaultOptions
  , files
  , restrict
  , uncomment
  , targetName
  ) where

import Data.Char (isAlphaNum, toUpper)
import Data.List (find, nub, sort)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, listToMaybe, mapMaybe)
import qualified Data.Set as Set
import Data.Text (Text)
import qualified Data.Text as T

import Ark.Encode (calls)
import Ark.IR
import Ark.Schema
import Ark.Value

data Target = Rust | Swift | Kotlin
  deriving (Eq, Show)

targetName :: Target -> Text
targetName = \case
  Rust -> "rust"
  Swift -> "swift"
  Kotlin -> "kotlin"

data Options = Options
  { -- | The Kotlin package the files declare.
    optPackage :: Text
  , -- | The name of the module's one struct of tables, which a body's @db@
    -- is and every router is over (harken's is @Harken@). The module does
    -- not carry it.
    optName :: Text
  }

defaultOptions :: Options
defaultOptions = Options "domain" "Tables"

-- | §18.1 Only what a peer calls.
--
-- A phone that never authors @add_track@ has no use for its body: the
-- scanner authors it on the server, and the phone receives those entries'
-- effects as facts. So a print may be restricted to the procedures a
-- program calls — every named procedure, plus every helper and middleware
-- one of them reaches — and a router with nothing left on it is dropped.
-- The schema is always whole.
restrict :: [Text] -> Module -> Module
restrict wanted m =
  m
    { modFunctions = [fn | fn <- modFunctions m, fnName fn `elem` keep]
    , modRouters = [r {rtUses = filter (`elem` keep) (rtUses r)} | r <- modRouters m, any (\fn -> fnRouter fn == Just (rtName r) && fnName fn `elem` keep) (modFunctions m)]
    }
  where
    keep = go [] wanted
    go seen [] = seen
    go seen (n : ns)
      | n `elem` seen = go seen ns
      | otherwise = case lookupFunction m n of
          Just fn -> go (n : seen) (calls fn ++ fnUses fn ++ ns)
          Nothing -> go seen ns

-- | Remove the lines that are nothing but a comment, which a canonical
-- source may carry and a print never does. Blank lines stay: they are
-- part of the form.
uncomment :: Text -> Text
uncomment = T.unlines . filter (not . commentOnly) . T.lines
  where
    commentOnly l = let s = T.stripStart l in any (`T.isPrefixOf` s) ["//", "/*", "* ", "*/"] || s == "*"

-- | Every file of the authoring form of a verified module, by name, before
-- the language's formatter has run over it.
files :: Target -> Options -> Module -> Either Text [(FilePath, Text)]
files t opts m = do
  routerFiles <- mapM (routerFile t opts m) (modRouters m)
  pure ([(fileName t "schema", schemaFile t opts m)] ++ routerFiles ++ [(fileName t "module", moduleFile t opts m)])

fileName :: Target -> Text -> FilePath
fileName t n = T.unpack $ case t of
  Rust -> n <> ".rs"
  Swift -> pascal n <> ".swift"
  Kotlin -> pascal n <> ".kt"

-- Headers ---------------------------------------------------------------

header :: Target -> Options -> [Text]
header t opts = case t of
  Rust -> ["use ark::authoring::*;"]
  Swift -> ["import ArkAuthoring"]
  Kotlin ->
    [ "package " <> optPackage opts
    , ""
    , "import dev.arkdb.authoring.*"
    , "import dev.arkdb.authoring.Int"
    , "import dev.arkdb.authoring.List"
    ]

-- | Items, each a block of lines, separated by one blank line.
layout :: [[Text]] -> Text
layout = T.unlines . go
  where
    go [] = []
    go [x] = x
    go (x : xs) = x ++ [""] ++ go xs

-- §18.2 The schema --------------------------------------------------------

schemaFile :: Target -> Options -> Module -> Text
schemaFile t opts m = layout (header t opts : tablesItem t (optName opts) tables : [rowItem t sch tbl | tbl <- tables])
  where
    sch = modSchema m
    tables = schTables sch

-- | The module's one struct of tables: what a body's @db@ is, in the order
-- the schema declares them, which @open()@ says once because no host can
-- enumerate a struct's fields.
tablesItem :: Target -> Text -> [Table] -> [Text]
tablesItem t name tables = case t of
  Rust ->
    ["pub struct " <> name <> " {"]
      ++ ["    pub " <> tName tb <> ": Table<" <> pascal (tName tb) <> ">," | tb <- tables]
      ++ [ "}"
         , "impl Tables for " <> name <> " {"
         , "    fn open() -> Self {"
         , "        " <> name <> " { " <> commas [tName tb <> ": table()" | tb <- tables] <> " }"
         , "    }"
         , "}"
         ]
  Swift ->
    ["public struct " <> name <> " {"]
      ++ ["    public var " <> camel (tName tb) <> ": Table<" <> pascal (tName tb) <> ">" | tb <- tables]
      ++ [ "}"
         , "extension " <> name <> ": Tables {"
         , "    public static func open() -> Self {"
         , "        " <> name <> "(" <> commas [camel (tName tb) <> ": table()" | tb <- tables] <> ")"
         , "    }"
         , "}"
         ]
  Kotlin ->
    ["class " <> name <> "("]
      ++ ["    val " <> camel (tName tb) <> ": Table<" <> pascal (tName tb) <> ">," | tb <- tables]
      ++ [") : Tables"]

rowItem :: Target -> Schema -> Table -> [Text]
rowItem t sch tbl = case t of
  Rust ->
    ["pub struct " <> row <> " {"]
      ++ ["    pub " <> colName c <> ": " <> fieldTy c <> "," | c <- tColumns tbl]
      ++ [ "}"
         , "impl Row for " <> row <> " {"
         , "    const NAME: &str = " <> str t (tName tbl) <> ";"
         , "    type Key = (" <> T.concat [keyTy k <> ", " | k <- init' keyCols] <> keyTy (last keyCols) <> (if length keyCols == 1 then "," else "") <> ");"
         , "    fn columns() -> Columns<Self> {"
         , "        columns()" <> T.concat (map colCall (tColumns tbl)) <> ".key(" <> rustTuple (map self (tKey tbl)) <> ")" <> T.concat (map ixCall (tIndexes tbl))
         , "    }"
         , "}"
         , "impl " <> row <> " {"
         ]
      ++ ["    pub const " <> colName c <> ": Col<Self, " <> colTyRust c <> "> = col(" <> str t (colName c) <> ");" | c <- tColumns tbl]
      ++ ["    pub const " <> n <> ": Rel<Self, " <> pascal child <> "> = rel(" <> str t n <> ");" | (n, child) <- rels]
      ++ ["}"]
  Swift ->
    ["public struct " <> row <> " {"]
      ++ ["    public var " <> camel (colName c) <> ": " <> fieldTy c | c <- tColumns tbl]
      ++ [ "}"
         , "extension " <> row <> ": Row {"
         , "    public static let NAME = " <> str t (tName tbl)
         , "    public typealias Key = " <> (case keyCols of [k] -> keyTy k; ks -> "(" <> commas (map keyTy ks) <> ")")
         , "    public static func columns() -> Columns<Self> {"
         , "        Columns<Self>()" <> T.concat ["\n            " <> seg | seg <- concatMap colSegs (tColumns tbl) ++ [".key(" <> commas (map self (tKey tbl)) <> ")"] ++ map ixCall (tIndexes tbl)]
         , "    }"
         , "}"
         , "extension " <> row <> " {"
         ]
      ++ ["    public static let " <> camel (colName c) <> " = col<" <> row <> ", " <> fieldTy c <> ">(" <> str t (colName c) <> ")" | c <- tColumns tbl]
      ++ ["    public static let " <> camel n <> " = rel<" <> row <> ", " <> pascal child <> ">(" <> str t n <> ")" | (n, child) <- rels]
      ++ ["}"]
  Kotlin ->
    ["class " <> row <> "("]
      ++ ["    val " <> camel (colName c) <> ": " <> fieldTy c <> "," | c <- tColumns tbl]
      ++ [ ") : Row<Key" <> tshow (length keyCols) <> "<" <> commas (map keyTy keyCols) <> ">> {"
         , "    companion object : Row.Of<" <> row <> "> {"
         , "        override val NAME = " <> str t (tName tbl)
         , "        override fun columns(): Columns<" <> row <> "> = columns<" <> row <> ">()" <> T.concat (map colCall (tColumns tbl)) <> ".key(" <> commas (map self (tKey tbl)) <> ")" <> T.concat (map ixCall (tIndexes tbl))
         ]
      ++ ["        val " <> camel (colName c) <> " = col<" <> row <> ", " <> fieldTy c <> ">(" <> str t (colName c) <> ")" | c <- tColumns tbl]
      ++ ["        val " <> camel n <> " = rel<" <> row <> ", " <> pascal child <> ">(" <> str t n <> ")" | (n, child) <- rels]
      ++ ["    }", "}"]
  where
    row = pascal (tName tbl)
    keyCols = [c | k <- tKey tbl, Just c <- [column tbl k]]
    init' xs = if null xs then [] else init xs
    keyTy c = langTy t (colTy c)
    fieldTy c = langTy t (if colNullable c then TOption (colTy c) else colTy c)
    -- Rust names a column's own table's id @Id<Self>@ in its constant.
    colTyRust c = case colTy c of
      TId x | x == tName tbl -> wrapOpt c "Id<Self>"
      _ -> fieldTy c
    wrapOpt c s = if colNullable c then "Opt<" <> s <> ">" else s
    self c = case t of
      Rust -> "Self::" <> c
      Swift -> "Self." <> camel c
      Kotlin -> camel c
    colCall = T.concat . colSegs
    colSegs c =
      ["." <> method (colTy c) <> "(" <> self (colName c) <> variants (colTy c) <> ")"]
        ++ [".nullable()" | colNullable c]
        ++ [refsCall (refTable r) | r <- tRefs tbl, refColumn r == colName c]
    method = \case
      TId _ -> "id"
      TText -> "text"
      TInt -> "int"
      TBool -> "bool"
      TBytes -> "bytes"
      TEnum _ -> "enum_"
      _ -> "text"
    variants = \case
      TEnum vs -> ", " <> (case t of Kotlin -> commas (map (str t) vs); _ -> "[" <> commas (map (str t) vs) <> "]")
      _ -> ""
    refsCall p = case t of
      Rust -> ".refs::<" <> pascal p <> ">()"
      Swift -> ".refs(" <> pascal p <> ".self)"
      Kotlin -> ".refs<" <> pascal p <> ">()"
    ixCall ix =
      (if ixUnique ix then ".unique(" else ".index(")
        <> (case t of Rust -> rustTuple (map self (ixColumns ix)); _ -> commas (map self (ixColumns ix)))
        <> ")"
    rels = relsOf sch (tName tbl)

-- | The relationships a table is the parent of, with the name each is read
-- by: the child table's, or @<child>_<column>@ when the child references
-- this parent through more than one column.
relsOf :: Schema -> TableName -> [(Text, TableName)]
relsOf sch parent =
  [ (if length [() | r' <- childrenOf sch parent, relChild r' == relChild r] > 1 then relChild r <> "_" <> relColumn r else relChild r, relChild r)
  | r <- childrenOf sch parent
  ]

relName :: Schema -> Relation -> Text
relName sch r = if length [() | r' <- childrenOf sch (relParent r), relChild r' == relChild r] > 1 then relChild r <> "_" <> relColumn r else relChild r

rustTuple :: [Text] -> Text
rustTuple [x] = "(" <> x <> ",)"
rustTuple xs = "(" <> commas xs <> ")"

-- §18.3 The module file ---------------------------------------------------

moduleFile :: Target -> Options -> Module -> Text
moduleFile t opts m = case t of
  Rust ->
    layout
      [ header t opts
      , ["use crate::" <> r <> "::" <> r <> ";" | r <- names]
      , ["pub fn module() -> Module {", "    Module::new(" <> rustTuple [r <> "()" | r <- names] <> ")", "}"]
      ]
  Swift -> layout [header t opts, ["public func module() -> Module {", "    Module(" <> commas [camel r <> "()" | r <- names] <> ")", "}"]]
  Kotlin -> layout [header t opts, ["fun module(): Module = Module(" <> commas [camel r <> "()" | r <- names] <> ")"]]
  where
    names = map rtName (modRouters m)

-- §18.4 A router file -----------------------------------------------------

routerFile :: Target -> Options -> Module -> Router -> Either Text (FilePath, Text)
routerFile t opts m r = do
  mws <- mapM (middleware t m r) middlewares
  procs <- mapM (procedure t m r) procedures
  recs <- mapM (recordItem t m) [rc | rc <- records m, here (recFn rc)]
  helps <- mapM (helperItem t m) helpers
  let inputs = concatMap (inputItem t m) (middlewares ++ procedures)
      body =
        [ open
        , indent 1 (routerDecl)
        ]
          ++ map (indent 1) mws
          ++ [indent 1 (routesOpen)]
          ++ [indent 2 (p <> sep) | (p, sep) <- zip procs (replicate (length procs - 1) "," ++ [lastSep])]
          ++ [indent 1 routesClose, "}"]
      items = inputs ++ recs ++ helps ++ [body]
      -- What this file names that another file declares: the helpers its
      -- functions call, and the records its text writes.
      called = nub [c | fn <- middlewares ++ procedures ++ helpers, c <- calls fn]
      helperImports = ["use crate::" <> h <> "::" <> c <> ";" | c <- called, not (here c), Just h <- [homeOf m c]]
      recordImports = ["use crate::" <> h <> "::" <> recName rc <> ";" | rc <- records m, not (here (recFn rc)), Just h <- [homeOf m (recFn rc)], mentions (recName rc) (layout items)]
      imports = case t of
        Rust -> [header t opts, sort (helperImports ++ recordImports ++ ["use crate::schema::*;"])]
        _ -> [header t opts]
  pure (fileName t (rtName r), layout (imports ++ items))
  where
    here n = homeOf m n == Just (rtName r)
    helpers = [fn | fn <- modFunctions m, fnKind fn == Helper, here (fnName fn)]
    mentions n txt = n `elem` T.split (\c -> not (isAlphaNum c || c == '_')) txt
    fns = [fn | fn <- modFunctions m, fnRouter fn == Just (rtName r)]
    procedures = fns
    middlewares = [fn | u <- rtUses r, Just fn <- [lookupFunction m u]]
    rv = ident t (rtName r)
    scopeTy = optName opts
    open = case t of
      Rust -> "pub fn " <> rtName r <> "() -> Router<" <> scopeTy <> "> {"
      Swift -> "public func " <> camel (rtName r) <> "() -> Router<" <> scopeTy <> "> {"
      Kotlin -> "fun " <> camel (rtName r) <> "(): Router<" <> scopeTy <> "> {"
    routerDecl = case t of
      Rust -> "let " <> rv <> " = router::<" <> scopeTy <> ">(" <> str t (rtName r) <> ");"
      Swift -> "let " <> rv <> " = router(" <> scopeTy <> ".self, " <> str t (rtName r) <> ")"
      Kotlin -> "val " <> rv <> " = router<" <> scopeTy <> ">(" <> str t (rtName r) <> ")"
    routesOpen = case t of
      Rust -> rv <> ".routes(("
      _ -> "return " <> rv <> ".routes("
    routesClose = case t of
      Rust -> "))"
      _ -> ")"
    lastSep = case t of
      Swift -> ""
      Rust | length procedures == 1 -> ","
      _ -> ","

-- The builder a function hangs off: the last middleware in its chain, or
-- the router itself.
builderOf :: Target -> Router -> [Text] -> Text
builderOf t r uses = ident t (if null uses then rtName r else last uses)

-- | Which middleware each middleware was built on: the one before it in any
-- procedure's chain, or the router.
baseOf :: Module -> Router -> Text -> [Text]
baseOf m r mw =
  case [takeWhile (/= mw) (fnUses fn) | fn <- modFunctions m, fnRouter fn == Just (rtName r), mw `elem` fnUses fn] of
    (chain : _) -> chain
    [] -> []

middleware :: Target -> Module -> Router -> Function -> Either Text Text
middleware t m r fn = do
  let base = builderOf t r (baseOf m r (fnName fn))
      cx0 = newCx m fn
  (items, _) <- block cx0 (fnBody fn)
  let b = B items
      ctxP = param t "ctx" (usesCtx fn)
      dbP = param t "db" (usesDb fn)
      inputTy = inputTypeName m fn
      inputP = case fnKind fn of
        Provide -> [typedParam t (param t "input" (usesInput fn)) inputTy True]
        _ -> []
      verb = case fnKind fn of
        Guard -> "guard"
        _ -> "provide"
  pure (declare t (fnName fn) (call t base verb [str t (fnName fn)] (closure t (ctxP : dbP : inputP) (fnKind fn == Provide && not (null inputP)) b)))

procedure :: Target -> Module -> Router -> Function -> Either Text Text
procedure t m r fn = do
  let base = builderOf t r (fnUses fn)
      cx0 = newCx m fn
  (items, _) <- block cx0 (fnBody fn)
  let hasInput = not (null (fnInput fn))
      provided = [providedName m fn u | u <- fnUses fn, Just mw <- [lookupFunction m u], fnKind mw == Provide]
      ps =
        [param t "ctx" (usesCtx fn), param t "db" (usesDb fn)]
          ++ [if hasInput then param t "input" (usesInput fn) else noInput t]
          ++ [param t p (usesProvided fn u) | (u, p) <- zip [u | u <- fnUses fn, Just mw <- [lookupFunction m u], fnKind mw == Provide] provided]
      verb = case fnKind fn of
        Mutator -> "mutation"
        _ -> "query"
      recv =
        if hasInput
          then case t of
            Rust -> base <> ".input::<" <> inputTypeName m fn <> ">()"
            Swift -> base <> ".input(" <> inputTypeName m fn <> ".self)"
            Kotlin -> base <> ".input<" <> inputTypeName m fn <> ">()"
          else base
  pure (call t recv verb [str t (fnName fn)] (closure t ps False (B items)))

noInput :: Target -> Text
noInput = \case
  Rust -> "_input: ()"
  _ -> "_"

param :: Target -> Text -> Bool -> Text
param t n used
  | used = ident t n
  | otherwise = case t of
      Rust -> "_" <> n
      _ -> "_"

typedParam :: Target -> Text -> Text -> Bool -> Text
typedParam t p ty _ = case t of
  Rust -> p <> ": &" <> ty
  _ -> p <> ": " <> ty

declare :: Target -> Text -> Text -> Text
declare t n e = case t of
  Rust -> "let " <> ident t n <> " = " <> e <> ";"
  Swift -> "let " <> ident t n <> " = " <> e
  Kotlin -> "val " <> ident t n <> " = " <> e

-- A builder call whose last argument is a handler closure.
call :: Target -> Text -> Text -> [Text] -> Text -> Text
call t recv verb args cl = case t of
  Rust -> recv <> "." <> verb <> "(" <> commas (args ++ [cl]) <> ")"
  _ -> recv <> "." <> verb <> "(" <> commas args <> ") " <> cl

closure :: Target -> [Text] -> Bool -> B -> Text
closure t ps typed b = case t of
  Rust -> "|" <> commas ps <> "| " <> renderBody t b
  Swift -> "{ " <> (if typed then "(" <> commas ps <> ")" else commas ps) <> " in " <> renderBody t b <> " }"
  Kotlin -> "{ " <> commas ps <> " -> " <> renderBody t b <> " }"

-- The procedure's input type, and a middleware's: named after the
-- function, with @Input@ appended where that would be a row's
-- name.
inputTypeName :: Module -> Function -> Text
inputTypeName m fn
  | base `elem` taken = base <> "Input"
  | otherwise = base
  where
    base = pascal (fnName fn)
    taken = [pascal (tName tb) | tb <- schTables (modSchema m)]

-- The provided value's parameter name: the table its row is of, else the
-- middleware's name.
providedName :: Module -> Function -> Text -> Text
providedName m _ u = fromMaybe u $ do
  mw <- lookupFunction m u
  ty <- fnRet mw
  tableOfTy (modSchema m) (case ty of TList x -> x; x -> x)

tableOfTy :: Schema -> Ty -> Maybe TableName
tableOfTy sch ty = tName <$> find (\tb -> rowTy tb == ty) (schTables sch)

-- | §18.4 A record: a struct type that is not a row, which the module
-- carries only as a type. It is named after the first function, in module
-- order, whose result it is — the function's name in PascalCase, with
-- @Entry@ appended when the result is a list of it — and declared in that
-- function's file. A name already taken (a row's, an input's, an earlier
-- record's) has @Record@ appended.
data Rec = Rec
  { recTy :: Ty
  , recName :: Text
  , recFn :: Text
  }

records :: Module -> [Rec]
records m = reverse (foldl step [] (modFunctions m))
  where
    rows = map rowTy (schTables (modSchema m))
    inputs = [inputTypeName m fn | fn <- modFunctions m, fnKind fn /= Helper, not (null (fnInput fn))]
    step acc fn = case fnRet fn >>= structOf of
      Just (ty, suffix)
        | ty `notElem` rows && ty `notElem` map recTy acc ->
            let base = pascal (fnName fn) <> suffix
                taken = [pascal (tName tb) | tb <- schTables (modSchema m)] ++ inputs ++ map recName acc
             in Rec ty (if base `elem` taken then base <> "Record" else base) (fnName fn) : acc
      _ -> acc
    structOf = \case
      TList s@(TStruct _) -> Just (s, "Entry")
      TOption s@(TStruct _) -> Just (s, "")
      s@(TStruct _) -> Just (s, "")
      _ -> Nothing

-- | What every struct type is called: a row by its table, a record by
-- 'records'.
structNames :: Module -> [(Ty, Text)]
structNames m = [(rowTy tb, pascal (tName tb)) | tb <- schTables (modSchema m)] ++ [(recTy r, recName r) | r <- records m]

-- | A type as the vocabulary writes it, with its structs named.
tyName :: [(Ty, Text)] -> Ty -> Text
tyName names = \case
  TOption x -> "Opt<" <> tyName names x <> ">"
  TList x -> "List<" <> tyName names x <> ">"
  s@(TStruct _) -> fromMaybe "Struct" (lookup s names)
  other -> langTy Rust other

-- | The router whose file a function is printed in: a procedure's own, a
-- middleware's (the router that declares it), and a helper's the file of
-- the first function after it in module order that is not a helper —
-- which is where its first caller is, since a helper is placed
-- immediately before that.
homeOf :: Module -> Text -> Maybe Text
homeOf m n = do
  fn <- lookupFunction m n
  case fnKind fn of
    Helper -> case [f | f <- drop 1 (dropWhile ((/= n) . fnName) (modFunctions m)), fnKind f /= Helper] of
      (f : _) -> homeOf m (fnName f)
      [] -> rtName <$> listToMaybe (modRouters m)
    _
      | isMiddleware fn -> rtName <$> find (\r -> n `elem` rtUses r) (modRouters m)
      | otherwise -> fnRouter fn

inputItem :: Target -> Module -> Function -> [[Text]]
inputItem t m fn
  | null (fnInput fn) = []
  | otherwise = [structLines ++ implLines]
  where
    name = inputTypeName m fn
    structLines = case t of
      Rust -> ["pub struct " <> name <> " {"] ++ ["    pub " <> n <> ": " <> langTy t (fTy f) <> "," | (n, f) <- fnInput fn] ++ ["}"]
      Swift -> ["public struct " <> name <> " {"] ++ ["    public var " <> camel n <> ": " <> langTy t (fTy f) | (n, f) <- fnInput fn] ++ ["}"]
      Kotlin -> ["class " <> name <> "("] ++ ["    val " <> camel n <> ": " <> langTy t (fTy f) <> "," | (n, f) <- fnInput fn]
    fields = [fieldBuilder t m fn n f | (n, f) <- fnInput fn]
    refines = T.concat [refineCall t m fn e w | (e, w) <- fnRefine fn]
    implLines = case t of
      Rust ->
        [ "impl Input for " <> name <> " {"
        , "    fn schema() -> Object<Self> {"
        , "        object()" <> T.concat [".field(" <> f <> ")" | f <- fields] <> refines
        , "    }"
        , "}"
        ]
      Swift ->
        [ "extension " <> name <> ": Input {"
        , "    public static var schema: Object<Self> {"
        , "        object()" <> T.concat [".field(" <> f <> ")" | f <- fields] <> refines
        , "    }"
        , "}"
        ]
      Kotlin ->
        [ ") : Input {"
        , "    companion object : Input.Of<" <> name <> "> {"
        , "        override fun schema(): Schema<" <> name <> "> = obj(" <> commas ["field(" <> f <> ")" | f <- fields] <> ")" <> refines
        , "    }"
        , "}"
        ]

-- | §18.4 A helper: an ordinary function of the vocabulary's types whose
-- body is @helper(..)@ — its name, its parameters paired with their names
-- (one parameter is a bare pair, several a tuple of pairs), and a closure
-- over the same names, typed.
helperItem :: Target -> Module -> Function -> Either Text [Text]
helperItem t m fn = case t of
  Rust -> do
    (items, _) <- block (newCx m fn) (fnBody fn)
    let names = structNames m
        ps = [(n, tyName names (fTy f)) | (n, f) <- fnInput fn]
        sig = commas [n <> ": " <> ty | (n, ty) <- ps]
        pair n = "(" <> str t n <> ", " <> n <> ")"
        params = case ps of
          [(n, _)] -> pair n
          _ -> "(" <> commas [pair n | (n, _) <- ps] <> ")"
        ret = maybe "()" (tyName names) (fnRet fn)
    pure
      [ "pub fn " <> fnName fn <> "(" <> sig <> ") -> " <> ret <> " {"
      , "    helper(" <> commas [str t (fnName fn), params, "|" <> sig <> "| " <> renderBody t (B items)] <> ")"
      , "}"
      ]
  Swift -> do
    (items, _) <- block (newCx m fn) (fnBody fn)
    let names = structNames m
        ps = [(n, tyName names (fTy f)) | (n, f) <- fnInput fn]
        ret = maybe "Void" (tyName names) (fnRet fn)
        -- One expression is the closure's value; several need its type
        -- said and a @return@, which Swift cannot infer.
        header = commas [camel n | (n, _) <- ps] <> (if single items then "" else " -> " <> ret) <> " in "
    pure
      [ "public func " <> camel (fnName fn) <> "(" <> commas ["_ " <> camel n <> ": " <> ty | (n, ty) <- ps] <> ") -> " <> ret <> " {"
      , "    helper(" <> commas (str t (fnName fn) : ["(" <> str t n <> ", " <> camel n <> ")" | (n, _) <- ps]) <> ") { " <> header <> renderBody t (B items) <> " }"
      , "}"
      ]
  _ -> Left ("a helper has no " <> targetName t <> " spelling yet: " <> fnName fn)
  where
    single = \case
      [IDo _] -> True
      _ -> False

-- | §18.4 A record: a struct of the vocabulary's values and its
-- @Record@ impl, the fields in the IR's order, which is alphabetical.
recordItem :: Target -> Module -> Rec -> Either Text [Text]
recordItem t m rc = case (t, recTy rc) of
  (Rust, TStruct fs) -> do
    builders <- mapM (\(f, ty) -> (\b -> ".field(" <> str t f <> ", " <> b <> ")") <$> build ty) (M.toList fs)
    pure $
      ["pub struct " <> recName rc <> " {"]
        ++ ["    pub " <> f <> ": " <> tyName names ty <> "," | (f, ty) <- M.toList fs]
        ++ [ "}"
           , "impl Record for " <> recName rc <> " {"
           , "    fn fields() -> Fields<Self> {"
           , "        fields()" <> T.concat builders
           , "    }"
           , "}"
           ]
  (Swift, TStruct fs) -> do
    builders <- mapM (\(f, ty) -> (\b -> ".field(" <> str t f <> ", " <> b <> ")") <$> build ty) (M.toList fs)
    pure $
      ["public struct " <> recName rc <> " {"]
        ++ ["    public var " <> camel f <> ": " <> tyName names ty | (f, ty) <- M.toList fs]
        ++ [ "}"
           , "extension " <> recName rc <> ": Record {"
           , "    public static func fields() -> Fields<Self> {"
           , "        Fields<Self>()" <> T.concat ["\n            " <> b | b <- builders]
           , "    }"
           , "}"
           ]
  _ -> Left ("a record has no " <> targetName t <> " spelling yet: " <> recName rc)
  where
    names = structNames m
    q = builder t (case recTy rc of TStruct fs -> M.keys fs; _ -> [])
    build = \case
      TText -> Right (q "text" <> "()")
      TInt -> Right (q "int" <> "()")
      TBool -> Right (if t == Rust then "bool_()" else q "bool" <> "()")
      TBytes -> Right (q "bytes" <> "()")
      TId x -> Right (if t == Rust then "id::<" <> pascal x <> ">()" else q "id" <> "(" <> pascal x <> ".self)")
      TEnum _ -> Right (q "text" <> "()")
      TOption x -> (\b -> q "opt" <> "(" <> b <> ")") <$> build x
      TList x -> (\b -> q "list" <> "(" <> b <> ")") <$> build x
      other -> Left ("a record field of type " <> tyName names other <> " has no spelling")

-- | A field builder's name, where a declaration's fields are these. In
-- Swift a builder is called inside the type's own extension, where a
-- field of the same name (@id@, @text@) hides the free function, so it is
-- qualified by its module there: @ArkAuthoring.id(Media.self)@.
builder :: Target -> [Text] -> Text -> Text
builder t fields f
  | t == Swift && f `elem` map camel fields = "ArkAuthoring." <> f
  | otherwise = f

fieldBuilder :: Target -> Module -> Function -> Text -> Field -> Text
fieldBuilder t m fn n (Field ty cs) = str t n <> ", " <> go ty
  where
    q = builder t (map fst (fnInput fn))
    go = \case
      TOption inner -> q "opt" <> "(" <> base inner <> checks <> ")"
      other -> base other <> checks
    base = \case
      TText -> q "text" <> "()"
      TInt -> q "int" <> "()"
      TBool -> case t of Rust -> "bool_()"; _ -> q "bool" <> "()"
      TBytes -> q "bytes" <> "()"
      TId x -> case t of
        Rust -> "id::<" <> pascal x <> ">()"
        Swift -> q "id" <> "(" <> pascal x <> ".self)"
        Kotlin -> "id<" <> pascal x <> ">()"
      TList inner -> q "list" <> "(" <> base inner <> ")"
      other -> "text() /* " <> T.pack (show other) <> " */"
    checks = T.concat (map check cs)
    check = \case
      CTrim -> ".trim()"
      CMinLen k w -> ".min(" <> tshow k <> ")" <> why w
      CMaxLen k w -> ".max(" <> tshow k <> ")" <> why w
      CRange (Just lo) Nothing w -> "." <> ident t "at_least" <> "(" <> tshow lo <> ")" <> why w
      CRange Nothing (Just hi) w -> "." <> ident t "at_most" <> "(" <> tshow hi <> ")" <> why w
      CRange lo hi w -> ".range(" <> maybe "" tshow lo <> ", " <> maybe "" tshow hi <> ")" <> why w
      CNonEmpty w -> "." <> ident t "non_empty" <> "()" <> why w
      CExists w -> ".exists()" <> why w
      CRefine e w ->
        let inner = case ty of TOption x -> x; x -> x
            pn = paramBase (Just inner)
            cx = (newCx m fn) {cxArg = Just (n, SName pn), cxScope = Set.insert pn (cxScope (newCx m fn))}
         in case expr cx e of
              Right s -> ".refine(" <> lambda t [pn] (B [IDo s]) <> ")" <> why w
              Left err -> ".refine(/* " <> err <> " */)"
    why = maybe "" (\w -> ".why(" <> str t w <> ")")

refineCall :: Target -> Module -> Function -> Expr -> Maybe Text -> Text
refineCall t m fn e w = case expr ((newCx m fn) {cxScope = Set.insert "input" (cxScope (newCx m fn))}) e of
  Right s -> ".refine(" <> lambda t ["input"] (B [IDo s]) <> ")" <> maybe "" (\x -> ".why(" <> str t x <> ")") w
  Left err -> ".refine(/* " <> err <> " */)"

-- §18.5 Decompiling a body ------------------------------------------------

-- | A source expression, language-neutral. Names are the IR's snake_case;
-- the renderer spells them.
data S
  = SName Text
  | SLit Value
  | SInput Text
  | SCtx Text
  | SAuto Text Text -- ^ @ctx.now("n")@ or @ctx.new_id("n")@: the method, then the name
  | SField S Text
  | SMethod S Text [Arg]
  | SFree Text [Arg]
  | -- | A free vocabulary function with its type written: @pick::<Text>(..)@
    -- in Rust, where nothing else would say which it is.
    SFreeT Text Text [Arg]
  | -- | A call to one of the module's helpers.
    SCall Text [S]
  | -- | @none::<T>()@, with @T@ already spelled.
    SNone Text
  | -- | A value whose type is written beside it where a language needs it:
    -- @Text("")@ in Swift, the value alone elsewhere.
    SAs Text S
  | -- | A struct literal: a row's or a record's, by its type's name.
    SRow Text [(FieldName, S)]
  | SList [S]
  | SDb TableName
  | SCol TableName FieldName
  | SRel TableName Text
  deriving (Show)

data Arg
  = A S
  | -- | A key or an @on@ list: a tuple in Rust whatever its length,
    -- positional arguments elsewhere.
    Tup [S]
  | -- | An order: a tuple in Rust when there are several, positional
    -- arguments elsewhere.
    Seq [S]
  | L [Text] B
  | -- | A closure some of whose parameters are written with their type:
    -- a fold's accumulator in Rust.
    LTyped [(Text, Maybe Text)] B
  deriving (Show)

newtype B = B [Item]
  deriving (Show)

data Item
  = ILet Text S
  | IDo S
  deriving (Show)

data Cx = Cx
  { cxMod :: Module
  , cxFn :: Function
  , cxSyms :: Map Sym S
  , cxTys :: Map Sym Ty
  , cxScope :: Set.Set Text
  , cxUses :: Map Sym Int
  , cxOrRefuse :: Set.Set Sym
  , -- | Inside a field's refinement: that field, and what it is called.
    cxArg :: Maybe (Text, S)
  , -- | What every struct type is called: the rows, then the records.
    cxNames :: [(Ty, Text)]
  }

newCx :: Module -> Function -> Cx
newCx m fn =
  Cx
    { cxMod = m
    , cxFn = fn
    , cxSyms = M.empty
    , cxTys = M.empty
    , cxScope = Set.fromList (["ctx", "db", "input"] ++ provided)
    , cxUses = symUses fn
    , cxOrRefuse = Set.empty
    , cxArg = Nothing
    , cxNames = structNames m
    }
  where
    -- A helper's parameters are in scope under their own names.
    provided = [providedName m fn u | u <- fnUses fn] ++ [n | fnKind fn == Helper, (n, _) <- fnInput fn]

fresh :: Cx -> Text -> (Text, Cx)
fresh cx base = (n, cx {cxScope = Set.insert n (cxScope cx)})
  where
    n = head [c | c <- base : [base <> "_" <> tshow i | i <- [2 :: Int ..]], not (Set.member c (cxScope cx))]

-- How many times each symbol is read.
symUses :: Function -> Map Sym Int
symUses fn = M.fromListWith (+) [(s, 1) | s <- concatMap stmtVars (fnBody fn)]
  where
    stmtVars = \case
      SLet _ e -> exprVars e
      SIf c a b -> exprVars c ++ concatMap stmtVars a ++ concatMap stmtVars b
      SFor _ xs b -> exprVars xs ++ concatMap stmtVars b
      SInsert _ e _ -> exprVars e
      SUpsert _ e _ -> exprVars e
      SUpdate _ ks _ e -> concatMap exprVars ks ++ exprVars e
      SDelete _ ks -> concatMap exprVars ks
      SRefuse e -> exprVars e
      SReturn me -> maybe [] exprVars me

exprVars :: Expr -> [Sym]
exprVars = \case
  EVar x -> [x]
  EField e _ -> exprVars e
  EStruct fs -> concatMap exprVars (M.elems fs)
  EList es -> concatMap exprVars es
  ESome e -> exprVars e
  EMatch e _ a b -> concatMap exprVars [e, a, b]
  EIf c a b -> concatMap exprVars [c, a, b]
  EOp _ es -> concatMap exprVars es
  ECmp _ a b -> exprVars a ++ exprVars b
  ECall _ es -> concatMap exprVars es
  EStd _ es -> concatMap exprVars es
  EMap xs _ b -> exprVars xs ++ exprVars b
  EFilter xs _ b -> exprVars xs ++ exprVars b
  EAny xs _ b -> exprVars xs ++ exprVars b
  EAll xs _ b -> exprVars xs ++ exprVars b
  ESortBy xs _ b -> exprVars xs ++ exprVars b
  EFold xs z _ _ b -> concatMap exprVars [xs, z, b]
  ESelect p -> planVars p
  EGet _ ks -> concatMap exprVars ks
  EExists _ ks -> concatMap exprVars ks
  _ -> []
  where
    planVars p = maybe [] predVars (pFilter p) ++ concatMap (planVars . rPlan) (pRelated p)
    predVars = \case
      PCmp _ _ e -> exprVars e
      PIn _ es -> concatMap exprVars es
      PAll ps -> concatMap predVars ps
      PAny ps -> concatMap predVars ps
      PNot q -> predVars q

-- The symbol at the head of an expression's chain: the receiver every
-- method is called on, all the way down.
headSym :: Expr -> Maybe Sym
headSym = \case
  EVar x -> Just x
  EMatch e _ _ _ -> headSym e
  EField e _ -> headSym e
  EStd f (a : _) | f `notElem` [Concat, IdOfText, NilId] -> headSym a
  EOp _ (a : _) -> headSym a
  ECmp _ a _ -> headSym a
  EMap xs _ _ -> headSym xs
  EFilter xs _ _ -> headSym xs
  EAny xs _ _ -> headSym xs
  EAll xs _ _ -> headSym xs
  ESortBy xs _ _ -> headSym xs
  EFold xs _ _ _ _ -> headSym xs
  _ -> Nothing

-- Whether a symbol heads the statement that follows.
headsNext :: Sym -> [Stmt] -> Bool
headsNext s = \case
  (SReturn (Just e) : _) -> headSym e == Just s
  (SLet _ e : _) -> headSym e == Just s
  _ -> False

-- | The statements of a block, as items. A statement that the lowerings
-- produce in a pair or a triple is read back as the one spelling it was.
block :: Cx -> [Stmt] -> Either Text ([Item], Cx)
block cx = \case
  [] -> Right ([], cx)
  (SLet s (ESelect p) : SLet s' (EStd First [EVar s0]) : rest)
    | s0 == s && pLimit p == Just 1 && uses s == 1 -> do
        ch <- selectChain cx p "first"
        bindValue cx s' (TOption <$> planRow p) (pTable p) ch rest
  (SLet s (ESelect p) : rest) -> do
    ch <- selectChain cx p "all"
    bindValue cx s (TList <$> planRow p) (pTable p) ch rest
  (SLet s (EGet tb ks) : rest) -> do
    ks' <- mapM (expr cx) ks
    bindValue cx s (TOption . rowTy <$> table tb) tb (SMethod (SDb tb) "get" [Tup ks']) rest
  (SLet s (EExists tb ks) : rest) -> do
    ks' <- mapM (expr cx) ks
    bindValue cx s (Just TBool) tb (SMethod (SDb tb) "exists" [Tup ks']) rest
  (SLet s e : SIf (EStd IsSome [EVar s0]) [] [SRefuse (ELit (VText msg))] : rest)
    | s0 == s -> do
        e' <- expr cx e
        let inner = case tyOf cx e of
              Just (TOption x) -> Just x
              _ -> Nothing
            base = fromMaybe "value" (inner >>= tableOfTy (modSchema (cxMod cx)))
        bindValue cx {cxOrRefuse = Set.insert s (cxOrRefuse cx)} s inner base (SMethod e' "or_refuse" [A (SLit (VText msg))]) rest
  (SLet s e : rest) -> do
    e' <- expr cx e
    bindValue cx s (tyOf cx e) ("v" <> tshow s) e' rest
  (SIf c a [] : rest) -> control rest $ \c' -> do
    a' <- body cx a
    pure (SFree "when" [A c', L [] a'])
    where
      control = ctl c
  (SIf c [] b : rest) -> ctl c rest $ \c' -> do
    b' <- body cx b
    pure (SFree "unless" [A c', L [] b'])
  (SIf c a b : rest) -> ctl c rest $ \c' -> do
    a' <- body cx a
    b' <- body cx b
    pure (SFree "if_else" [A c', L [] a', L [] b'])
  (SFor x xs b : rest) -> do
    xs' <- expr cx xs
    let el = case tyOf cx xs of
          Just (TList e) -> Just e
          _ -> Nothing
        (xn, cxIn) = fresh cx (paramBase el)
    b' <- body cxIn {cxSyms = M.insert x (SName xn) (cxSyms cx), cxTys = maybe id (M.insert x) el (cxTys cx)} b
    item (IDo (SFree "for_each" [A xs', L [xn] b'])) rest
  (SInsert tb e on : rest) -> do
    row <- rowLit cx tb e
    let ins = SMethod (SDb tb) "insert" [A row]
    item (IDo (if null on then ins else SMethod ins "on" [Tup (map (SCol tb) on)])) rest
  (SUpsert tb e on : rest) -> do
    row <- rowLit cx tb e
    let ups = SMethod (SDb tb) "upsert" [A row]
    item (IDo (if null on then ups else SMethod ups "on" [Tup (map (SCol tb) on)])) rest
  (SUpdate tb ks x e : rest) -> do
    ks' <- mapM (expr cx) ks
    let (xn, cxIn) = fresh cx "row"
    row <- rowLit cxIn {cxSyms = M.insert x (SName xn) (cxSyms cx), cxTys = maybe id (M.insert x) (rowTy <$> table tb) (cxTys cx)} tb e
    item (IDo (SMethod (SDb tb) "update" [Tup ks', L [xn] (B [IDo row])])) rest
  (SDelete tb ks : rest) -> do
    ks' <- mapM (expr cx) ks
    item (IDo (SMethod (SDb tb) "delete" [Tup ks'])) rest
  (SRefuse e : rest) -> do
    e' <- expr cx e
    item (IDo (SFree "refuse" [A e'])) rest
  (SReturn (Just e) : rest) -> do
    e' <- expr cx e
    item (IDo e') rest
  (SReturn Nothing : rest) -> block cx rest
  where
    uses s = M.findWithDefault 0 s (cxUses cx)
    table tb = lookupTable (modSchema (cxMod cx)) tb
    planRow p = rowTy <$> table (pTable p)
    item i rest = do
      (is, cx') <- block cx rest
      pure (i : is, cx')
    ctl c rest k = do
      c' <- expr cx c
      s <- k c'
      item (IDo s) rest

-- A closure's body: a block of its own, whose names do not escape.
body :: Cx -> [Stmt] -> Either Text B
body cx stmts = B . fst <$> block cx stmts

-- Bind a symbol to what it holds: inline, when its one use heads the next
-- statement; otherwise a @let@ named after what it holds.
bindValue :: Cx -> Sym -> Maybe Ty -> Text -> S -> [Stmt] -> Either Text ([Item], Cx)
bindValue cx s ty base v rest
  | readsLeft == 1 && headsNext s rest = block (withSym v cx) rest
  | otherwise = do
      let (n, cx') = fresh cx base
      (items, cx'') <- block (withSym (SName n) cx') rest
      pure (ILet n v : items, cx'')
  where
    -- An or_refuse's own check reads the symbol once more than the source did.
    readsLeft = M.findWithDefault 0 s (cxUses cx) - (if Set.member s (cxOrRefuse cx) then 1 else 0)
    withSym x c = c {cxSyms = M.insert s x (cxSyms c), cxTys = maybe id (M.insert s) ty (cxTys c)}

rowLit :: Cx -> TableName -> Expr -> Either Text S
rowLit cx tb = \case
  EStruct fs -> do
    tbl <- maybe (Left ("no table " <> tb)) Right (lookupTable (modSchema (cxMod cx)) tb)
    let cols = map colName (tColumns tbl)
    case [k | k <- M.keys fs, k `notElem` cols] of
      [] -> Right ()
      ks -> Left ("not columns of " <> tb <> ": " <> T.intercalate ", " ks)
    fields <- mapM (\c -> (,) c <$> expr cx (fs M.! c)) [c | c <- cols, M.member c fs]
    pure (SRow (pascal tb) fields)
  other -> Left ("a write of something other than a row literal: " <> T.pack (take 80 (show other)))

selectChain :: Cx -> Plan -> Text -> Either Text S
selectChain cx p terminal = do
  tbl <- maybe (Left ("no table " <> pTable p)) Right (lookupTable (modSchema (cxMod cx)) (pTable p))
  filt <- traverse (pred' cx (pTable p)) (pFilter p)
  let base = SDb (pTable p)
      withFilter = maybe base (\f -> SMethod base "filter" [A f]) filt
      order = shortestOrder (tKey tbl) (pOrder p)
      withOrder = if null order then withFilter else SMethod withFilter "order_by" [Seq [SMethod (SCol (pTable p) c) (if d == Asc then "asc" else "desc") [] | (c, d) <- order]]
      withLimit = case (terminal, pLimit p) of
        ("first", _) -> withOrder
        (_, Just n) -> SMethod withOrder "limit" [A (SLit (VInt (fromIntegral n)))]
        _ -> withOrder
  withRelated <- foldl (\acc r -> acc >>= \a -> related a r) (Right withLimit) (pRelated p)
  pure (SMethod withRelated terminal [])
  where
    related acc r
      | defaultChild r = Right (SMethod acc "with" [A (SRel (relParent (rRelation r)) (rName r))])
      | otherwise = Left ("a related plan with its own filter, order or limit has no spelling: " <> rName r)
    defaultChild r =
      let cp = rPlan r
          keyOf' = maybe [] tKey (lookupTable (modSchema (cxMod cx)) (pTable cp))
       in isNothing' (pFilter cp) && null (shortestOrder keyOf' (pOrder cp)) && pLimit cp == Nothing && null (pRelated cp)
    isNothing' = not . isJust

-- | The shortest prefix of an order whose completion by the key columns,
-- ascending, is the order: what the author wrote.
shortestOrder :: [FieldName] -> [(FieldName, Dir)] -> [(FieldName, Dir)]
shortestOrder key o = head ([pre | n <- [0 .. length o], let pre = take n o, complete pre == o] ++ [o])
  where
    complete pre = pre ++ [(k, Asc) | k <- key, k `notElem` map fst pre]

pred' :: Cx -> TableName -> Pred -> Either Text S
pred' cx tb = \case
  PCmp c op e -> do
    e' <- expr cx e
    pure (SMethod (SCol tb c) (cmpName op) [A e'])
  PIn c es -> do
    es' <- mapM (expr cx) es
    pure (SMethod (SCol tb c) "in_" [A (SList es')])
  PAll (p : ps) -> chainOf "and" p ps
  PAny (p : ps) -> chainOf "or" p ps
  PNot q -> (\q' -> SMethod q' "not" []) <$> pred' cx tb q
  other -> Left ("an empty conjunction has no spelling: " <> T.pack (show other))
  where
    chainOf m p ps = do
      p' <- pred' cx tb p
      ps' <- mapM (pred' cx tb) ps
      pure (foldl (\a b -> SMethod a m [A b]) p' ps')

cmpName :: CmpOp -> Text
cmpName = \case
  Eq -> "eq"
  Ne -> "ne"
  Lt -> "lt"
  Le -> "le"
  Gt -> "gt"
  Ge -> "ge"

paramBase :: Maybe Ty -> Text
paramBase = \case
  Just (TStruct _) -> "row"
  _ -> "x"

expr :: Cx -> Expr -> Either Text S
expr cx = \case
  ELit v -> Right (SLit v)
  EArg a -> case cxArg cx of
    Just (f, s) | f == a -> Right s
    _
      | fnKind (cxFn cx) == Helper -> Right (SName a)
      | otherwise -> Right (SInput a)
  EAuto a -> case lookup a (fnAutos (cxFn cx)) of
    Just (NewId _) -> Right (SAuto "new_id" a)
    Just Now -> Right (SAuto "now" a)
    Nothing -> Left ("no auto " <> a)
  EVar s -> maybe (Left ("unbound symbol " <> tshow s)) Right (M.lookup s (cxSyms cx))
  ECtxUser -> Right (SCtx "user")
  ECtxSession -> Right (SCtx "session")
  EProvided n -> Right (SName (providedName (cxMod cx) (cxFn cx) n))
  EField e f -> (`SField` f) <$> expr cx e
  EStruct fs -> case find (\tb -> M.keysSet (case rowTy tb of TStruct x -> x; _ -> M.empty) == M.keysSet fs) allTables of
    Just tb -> rowLit cx (tName tb) (EStruct fs)
    Nothing -> case [n | (TStruct x, n) <- cxNames cx, M.keysSet x == M.keysSet fs] of
      (n : _) -> SRow n <$> mapM (\(f, e) -> (,) f <$> expr cx e) (M.toList fs)
      [] -> Left ("a struct no function returns has no name: " <> T.intercalate ", " (M.keys fs))
  EList es -> SList <$> mapM (expr cx) es
  ESome e -> (\e' -> SFree "some" [A e']) <$> pickArg e
  ENone ty -> Right (SNone (tyName (cxNames cx) ty))
  EMatch o x a b -> do
    o' <- expr cx o
    let inner = case tyOf cx o of
          Just (TOption t) -> Just t
          _ -> Nothing
        (xn, cxIn) = fresh cx (paramBase inner)
        cxA = cxIn {cxSyms = M.insert x (SName xn) (cxSyms cx), cxTys = maybe id (M.insert x) inner (cxTys cx)}
    case (a, b) of
      (EIf p (ESome (EVar x')) (ENone _), ENone _) | x' == x -> do
        p' <- expr cxA p
        pure (SMethod o' "filter" [L [xn] (B [IDo p'])])
      (ESome v, ENone _) -> do
        v' <- expr cxA v
        pure (SMethod o' "map" [L [xn] (B [IDo v'])])
      _ -> do
        a' <- expr cxA a
        b' <- expr cx b
        pure (SMethod o' "map_or" [A b', L [xn] (B [IDo a'])])
  EIf c a b -> do
    c' <- expr cx c
    a' <- pickArg a
    b' <- pickArg b
    pure (SFree "pick" [A c', A a', A b'])
  EOp op es -> case (op, es) of
    (Neg, [a]) -> unary "neg" a
    (Not, [a]) -> unary "not" a
    (_, [a, b]) -> do
      a' <- expr cx a
      b' <- expr cx b
      pure (SMethod a' (opName op) [A b'])
    _ -> Left ("an operator at an arity it has no spelling for: " <> T.pack (show op))
  ECmp op a b -> do
    a' <- expr cx a
    b' <- expr cx b
    pure (SMethod a' (cmpName op) [A b'])
  ECall n es -> SCall n <$> mapM (expr cx) es
  EStd f es -> do
    es' <- mapM (expr cx) es
    case (f, es, es') of
      (Unwrap, [EVar s], _) | Set.member s (cxOrRefuse cx) -> maybe (Left "unbound") Right (M.lookup s (cxSyms cx))
      (Concat, _, _) -> pure (SFree "concat" (map A es'))
      (IdOfText, _, _) -> pure (SFree "id_of_text" (map A es'))
      (NilId, _, _) -> pure (SFree "nil_id" [])
      (_, _, recv : args) -> pure (SMethod recv (stdName f) (map A args))
      _ -> Left ("a standard function with no receiver: " <> T.pack (show f))
  EMap xs x b -> lam1 "map" xs x b
  EFilter xs x b -> lam1 "filter" xs x b
  EAny xs x b -> lam1 "any" xs x b
  EAll xs x b -> lam1 "all" xs x b
  ESortBy xs x b -> lam1 "sort_by" xs x b
  EFold xs z acc x b -> do
    xs' <- expr cx xs
    z' <- expr cx z
    let el = elemTy xs
        (an, cx1) = fresh cx "acc"
        (xn, cx2) = fresh cx1 (paramBase el)
        cxB = cx2 {cxSyms = M.insert acc (SName an) (M.insert x (SName xn) (cxSyms cx)), cxTys = maybe id (M.insert x) el (maybe id (M.insert acc) (tyOf cx z) (cxTys cx))}
    b' <- expr cxB b
    -- A literal start says its type in Swift (@Text("")@), which a
    -- literal alone cannot.
    let start = case (z', tyOf cx z) of
          (SLit _, Just ty) -> SAs (tyName (cxNames cx) ty) z'
          _ -> z'
    pure (SMethod xs' "fold" [A start, LTyped [(an, tyName (cxNames cx) <$> tyOf cx z), (xn, Nothing)] (B [IDo b'])])
  ESelect _ -> Left "a read outside a let"
  EGet _ _ -> Left "a read outside a let"
  EExists _ _ -> Left "a read outside a let"
  where
    allTables = schTables (modSchema (cxMod cx))
    -- A pick passed straight to @pick@ or @some@, which take any value
    -- that converts, says its type: nothing else would.
    pickArg = \case
      e@(EIf c a b) -> do
        ty <- maybe (Left "a pick of a type the printer cannot tell") Right (tyOf cx e)
        c' <- expr cx c
        a' <- pickArg a
        b' <- pickArg b
        pure (SFreeT "pick" (tyName (cxNames cx) ty) [A c', A a', A b'])
      e -> expr cx e
    unary m a = (\a' -> SMethod a' m []) <$> expr cx a
    elemTy xs = case tyOf cx xs of
      Just (TList e) -> Just e
      _ -> Nothing
    lam1 m xs x b = do
      xs' <- expr cx xs
      let el = elemTy xs
          (xn, cxIn) = fresh cx (paramBase el)
      b' <- expr cxIn {cxSyms = M.insert x (SName xn) (cxSyms cx), cxTys = maybe id (M.insert x) el (cxTys cx)} b
      pure (SMethod xs' m [L [xn] (B [IDo b'])])

opName :: Op -> Text
opName = \case
  Add -> "add"
  Sub -> "sub"
  Mul -> "mul"
  Div -> "div"
  Mod -> "rem"
  Neg -> "neg"
  And -> "and"
  Or -> "or"
  Not -> "not"

stdName :: StdFn -> Text
stdName = \case
  Trim -> "trim"
  IsEmpty -> "is_empty"
  Concat -> "concat"
  Lower -> "lower"
  IsAlnum -> "is_alnum"
  Chars -> "chars"
  TextLen -> "len"
  StartsWith -> "starts_with"
  SplitOnce -> "split_once"
  TextOfInt -> "to_text"
  Hex -> "hex"
  Min -> "min"
  Max -> "max"
  Clamp -> "clamp"
  Abs -> "abs"
  Fnv1a64 -> "fnv1a64"
  Sha256 -> "sha256"
  IdOfText -> "id_of_text"
  TextOfId -> "to_text"
  NilId -> "nil_id"
  Utf8 -> "utf8"
  First -> "first"
  Last -> "last"
  Len -> "len"
  Contains -> "contains"
  Reverse -> "reverse"
  IsSome -> "is_some"
  UnwrapOr -> "unwrap_or"
  Unwrap -> "unwrap"

-- The type of an expression, as far as naming needs it.
tyOf :: Cx -> Expr -> Maybe Ty
tyOf cx = \case
  ELit v -> case v of
    VInt _ -> Just TInt
    VText _ -> Just TText
    VBool _ -> Just TBool
    VBytes _ -> Just TBytes
    _ -> Nothing
  EArg a -> lookup a (fnArgs (cxFn cx))
  EAuto a -> case lookup a (fnAutos (cxFn cx)) of
    Just (NewId t) -> Just (TId t)
    Just Now -> Just TInt
    Nothing -> Nothing
  EVar s -> M.lookup s (cxTys cx)
  ECtxUser -> Just TText
  ECtxSession -> Just TText
  EProvided n -> lookupFunction (cxMod cx) n >>= fnRet
  EField e f -> case tyOf cx e of
    Just (TStruct fs) -> M.lookup f fs
    _ -> Nothing
  EStruct fs -> listToMaybe [ty | (ty@(TStruct x), _) <- cxNames cx, M.keysSet x == M.keysSet fs]
  EList (e : _) -> TList <$> tyOf cx e
  EList [] -> Nothing
  ESome e -> TOption <$> tyOf cx e
  ENone t -> Just (TOption t)
  EMatch o x a b -> case tyOf cx b of
    Just t -> Just t
    Nothing -> case tyOf cx o of
      Just (TOption i) -> tyOf (bind x i) a
      _ -> Nothing
  EIf _ a b -> maybe (tyOf cx b) Just (tyOf cx a)
  EOp op _ -> Just (if op `elem` [And, Or, Not] then TBool else TInt)
  ECmp {} -> Just TBool
  ECall n _ -> lookupFunction (cxMod cx) n >>= fnRet
  EStd f es -> case (f, es) of
    (First, [a]) -> listToOpt a
    (Last, [a]) -> listToOpt a
    (Unwrap, [a]) -> unOpt a
    (UnwrapOr, [a, _]) -> unOpt a
    (Reverse, [a]) -> tyOf cx a
    (Chars, _) -> Just (TList TText)
    (Sha256, _) -> Just TBytes
    (Utf8, _) -> Just TBytes
    (IdOfText, _) -> Nothing
    (NilId, _) -> Nothing
    (SplitOnce, _) -> Nothing
    _
      | f `elem` [Trim, Concat, Lower, TextOfInt, Hex, TextOfId] -> Just TText
      | f `elem` [IsEmpty, IsAlnum, StartsWith, Contains, IsSome] -> Just TBool
      | f `elem` [TextLen, Len, Min, Max, Clamp, Abs, Fnv1a64] -> Just TInt
      | otherwise -> Nothing
  EMap xs x b -> case tyOf cx xs of
    Just (TList e) -> TList <$> tyOf (bind x e) b
    _ -> Nothing
  EFilter xs _ _ -> tyOf cx xs
  ESortBy xs _ _ -> tyOf cx xs
  EAny {} -> Just TBool
  EAll {} -> Just TBool
  EFold _ z _ _ _ -> tyOf cx z
  ESelect p -> TList . rowTy <$> lookupTable (modSchema (cxMod cx)) (pTable p)
  EGet tb _ -> TOption . rowTy <$> lookupTable (modSchema (cxMod cx)) tb
  EExists _ _ -> Just TBool
  where
    bind x t = cx {cxTys = M.insert x t (cxTys cx)}
    listToOpt a = case tyOf cx a of
      Just (TList t) -> Just (TOption t)
      _ -> Nothing
    unOpt a = case tyOf cx a of
      Just (TOption t) -> Just t
      _ -> Nothing

-- What a function's body reads of its surroundings.
usesCtx, usesDb, usesInput :: Function -> Bool
usesCtx fn = any isCtx (allExprs fn) || not (null (fnAutos fn))
  where
    isCtx = \case
      ECtxUser -> True
      ECtxSession -> True
      EAuto _ -> True
      _ -> False
usesDb fn = any isRead (allExprs fn) || any isWrite (allStmts (fnBody fn))
  where
    isRead = \case
      ESelect _ -> True
      EGet _ _ -> True
      EExists _ _ -> True
      _ -> False
    isWrite = \case
      SInsert {} -> True
      SUpsert {} -> True
      SUpdate {} -> True
      SDelete {} -> True
      _ -> False
usesInput fn = any (\case EArg _ -> True; _ -> False) (allExprs fn)

usesProvided :: Function -> Text -> Bool
usesProvided fn u = any (\case EProvided n -> n == u; _ -> False) (allExprs fn)

allStmts :: [Stmt] -> [Stmt]
allStmts = concatMap $ \s -> s : case s of
  SIf _ a b -> allStmts a ++ allStmts b
  SFor _ _ b -> allStmts b
  _ -> []

-- Every expression in a body, and every expression inside those.
allExprs :: Function -> [Expr]
allExprs fn = concatMap sub (concatMap top (allStmts (fnBody fn)))
  where
    top = \case
      SLet _ e -> [e]
      SIf c _ _ -> [c]
      SFor _ xs _ -> [xs]
      SInsert _ e _ -> [e]
      SUpsert _ e _ -> [e]
      SUpdate _ ks _ e -> e : ks
      SDelete _ ks -> ks
      SRefuse e -> [e]
      SReturn me -> maybe [] pure me
    sub e = e : case e of
      EField a _ -> sub a
      EStruct fs -> concatMap sub (M.elems fs)
      EList es -> concatMap sub es
      ESome a -> sub a
      EMatch a _ b c -> concatMap sub [a, b, c]
      EIf a b c -> concatMap sub [a, b, c]
      EOp _ es -> concatMap sub es
      ECmp _ a b -> sub a ++ sub b
      ECall _ es -> concatMap sub es
      EStd _ es -> concatMap sub es
      EMap a _ b -> sub a ++ sub b
      EFilter a _ b -> sub a ++ sub b
      EAny a _ b -> sub a ++ sub b
      EAll a _ b -> sub a ++ sub b
      ESortBy a _ b -> sub a ++ sub b
      EFold a z _ _ b -> concatMap sub [a, z, b]
      ESelect p -> concatMap sub (planExprs p)
      EGet _ ks -> concatMap sub ks
      EExists _ ks -> concatMap sub ks
      _ -> []
    planExprs p = maybe [] predExprs (pFilter p) ++ concatMap (planExprs . rPlan) (pRelated p)
    predExprs = \case
      PCmp _ _ e -> [e]
      PIn _ es -> es
      PAll ps -> concatMap predExprs ps
      PAny ps -> concatMap predExprs ps
      PNot q -> predExprs q

-- §18.6 Spelling ----------------------------------------------------------

renderBody :: Target -> B -> Text
renderBody t (B items) = case items of
  [IDo s] -> renderTop t s
  _ -> case t of
    Rust -> "{ " <> T.intercalate " " (zipWith (rustItem (length items)) [1 ..] items) <> " }"
    Swift -> "\n" <> T.intercalate "\n" (zipWith (swiftItem (length items)) [1 ..] items) <> "\n"
    Kotlin -> "\n" <> T.intercalate "\n" (map kotlinItem items) <> "\n"
  where
    rustItem n i = \case
      ILet v s -> "let " <> ident t v <> " = " <> renderTop t s <> ";"
      IDo s -> renderTop t s <> (if i == (n :: Int) then "" else ";")
    swiftItem n i = \case
      ILet v s -> "let " <> ident t v <> " = " <> renderTop t s
      IDo s -> (if i == (n :: Int) then "return " else "") <> renderTop t s
    kotlinItem = \case
      ILet v s -> "val " <> ident t v <> " = " <> renderTop t s
      IDo s -> renderTop t s

-- | A statement's own expression. Swift's formatter keeps the breaks it is
-- given, so a long chain is broken here, before each call, when it has
-- more than one call and would run past a hundred columns; rustfmt and
-- ktfmt decide this for themselves.
renderTop :: Target -> S -> Text
renderTop t s = case t of
  Swift | length calls >= 2 && T.length flat > 100 -> render t base <> T.concat ["\n." <> methodName t m <> callArgs t as | (m, as) <- calls]
  _ -> flat
  where
    flat = render t s
    (base, calls) = parts s
    parts = \case
      SMethod r m as -> let (b, cs) = parts r in (b, cs ++ [(m, as)])
      other -> (other, [])

lambda :: Target -> [Text] -> B -> Text
lambda t ps b = case t of
  Rust -> "|" <> commas (map (ident t) ps) <> "| " <> renderBody t b
  Swift -> "{ " <> (if null ps then "" else commas (map (ident t) ps) <> " in ") <> renderBody t b <> " }"
  Kotlin -> "{ " <> (if null ps then "" else commas (map (ident t) ps) <> " -> ") <> renderBody t b <> " }"

render :: Target -> S -> Text
render t = \case
  SName n -> ident t n
  SLit v -> literal t v
  SAs ty v -> case (t, v) of
    (Swift, SLit _) -> ty <> "(" <> render t v <> ")"
    _ -> render t v
  SInput f -> "input." <> ident t f
  SCtx f -> "ctx." <> f
  SAuto m n -> "ctx." <> ident t m <> "(" <> str t n <> ")"
  SField s f -> render t s <> "." <> ident t f
  -- @x.is_some().not()@ is the one lowering of @x.is_none()@.
  SMethod (SMethod s "is_some" []) "not" [] | t == Rust -> render t s <> ".is_none()"
  SMethod s m args -> render t s <> "." <> methodName t m <> callArgs t args
  SFree f args -> freeName t f <> freeArgs t f args
  SFreeT f ty args -> case t of
    Rust -> f <> "::<" <> ty <> ">" <> callArgs t args
    _ -> freeName t f <> freeArgs t f args
  SCall f args -> freeName t f <> "(" <> commas (map (lifted t) args) <> ")"
  SNone ty -> case t of
    Rust -> "none::<" <> ty <> ">()"
    Swift -> "none(" <> ty <> ".self)"
    Kotlin -> "none<" <> ty <> ">()"
  SRow n fs -> case t of
    Rust -> n <> " { " <> commas [f <> ": " <> lifted t v | (f, v) <- fs] <> " }"
    Swift -> n <> "(" <> commas [ident t f <> ": " <> render t v | (f, v) <- fs] <> ")"
    Kotlin -> n <> "(" <> commas [ident t f <> " = " <> render t v | (f, v) <- fs] <> ")"
  SList es -> case t of
    Kotlin -> "list(" <> commas (map (render t) es) <> ")"
    _ -> "list([" <> commas (map (lifted t) es) <> "])"
  SDb tb -> "db." <> ident t tb
  SCol tb c -> pascal tb <> sep <> ident t c
  SRel tb r -> pascal tb <> sep <> ident t r
  where
    sep = case t of
      Rust -> "::"
      _ -> "."

-- | A value where the vocabulary takes exactly its type rather than
-- anything that converts to it — a struct's field, a list's element, a
-- helper's argument: a literal there is lifted with @.into()@ in Rust.
lifted :: Target -> S -> Text
lifted t = \case
  SLit v | t == Rust -> case v of
    VInt n | n < 0 -> "(" <> tshow n <> ").into()"
    _ -> literal t v <> ".into()"
  s -> render t s

methodName :: Target -> Text -> Text
methodName t m = case (t, m) of
  (Rust, _) -> m
  (_, "in_") -> "isIn"
  _ -> camel m

freeName :: Target -> Text -> Text
freeName t f = case (t, f) of
  (Kotlin, "when") -> "`when`"
  (Rust, _) -> f
  _ -> camel f

-- Arguments of a method call: in Swift and Kotlin a closure in the last
-- place trails the call.
callArgs :: Target -> [Arg] -> Text
callArgs t args = case t of
  Rust -> "(" <> commas (concatMap (arg t) args) <> ")"
  _ -> case reverse args of
    (L ps b : before) -> trailing before ps b
    (LTyped ps b : before) -> trailing before (map fst ps) b
    _ -> "(" <> commas (concatMap (arg t) args) <> ")"
  where
    trailing before ps b =
      let rest = concatMap (arg t) (reverse before)
       in (if null rest then " " else "(" <> commas rest <> ") ") <> lambda t ps b

freeArgs :: Target -> Text -> [Arg] -> Text
freeArgs t f args = case (t, f, args) of
  (Swift, "if_else", [A c, L [] a, L [] b]) -> "(" <> render t c <> ", then: " <> lambda t [] a <> ", else: " <> lambda t [] b <> ")"
  (Kotlin, "if_else", [A c, L [] a, L [] b]) -> "(" <> render t c <> ", " <> lambda t [] a <> ", " <> lambda t [] b <> ")"
  _ -> callArgs t args

arg :: Target -> Arg -> [Text]
arg t = \case
  A s -> [render t s]
  Tup ss -> case t of
    Rust -> [rustTuple (map (render t) ss)]
    _ -> map (render t) ss
  Seq ss -> case (t, ss) of
    (Rust, [s]) -> [render t s]
    (Rust, _) -> ["(" <> commas (map (render t) ss) <> ")"]
    _ -> map (render t) ss
  L ps b -> [lambda t ps b]
  LTyped ps b -> case t of
    Rust -> ["|" <> commas [ident t p <> maybe "" (": " <>) ty | (p, ty) <- ps] <> "| " <> renderBody t b]
    _ -> [lambda t (map fst ps) b]

literal :: Target -> Value -> Text
literal t = \case
  VInt n -> tshow n
  VText s -> str t s
  VBool b -> if b then "true" else "false"
  VNull -> "none"
  other -> "/* " <> T.pack (show other) <> " */"

langTy :: Target -> Ty -> Text
langTy t = \case
  TBool -> "Bool"
  TInt -> "Int"
  TText -> "Text"
  TBytes -> "Bytes"
  TId x -> "Id<" <> pascal x <> ">"
  TEnum _ -> "Text"
  TOption x -> "Opt<" <> langTy t x <> ">"
  TList x -> "List<" <> langTy t x <> ">"
  TStruct _ -> "Struct"

-- | A string literal. Rust and Swift share their escapes for everything
-- a domain writes; Kotlin also escapes @$@, which would start a template.
str :: Target -> Text -> Text
str t s = "\"" <> T.concatMap esc s <> "\""
  where
    esc c = case c of
      '"' -> "\\\""
      '\\' -> "\\\\"
      '\n' -> "\\n"
      '\r' -> "\\r"
      '\t' -> "\\t"
      '$' | t == Kotlin -> "\\$"
      _ -> T.singleton c

-- An identifier: snake_case in Rust, lowerCamel in Swift and Kotlin.
ident :: Target -> Text -> Text
ident t n = case t of
  Rust -> n
  _ -> camel n

camel :: Text -> Text
camel n = case T.splitOn "_" n of
  [] -> n
  (w : ws) -> T.concat (w : map cap ws)

pascal :: Text -> Text
pascal = T.concat . map cap . T.splitOn "_"

cap :: Text -> Text
cap w = case T.uncons w of
  Just (c, rest) -> T.cons (toUpper c) rest
  Nothing -> w

indent :: Int -> Text -> Text
indent d = (T.replicate (4 * d) " " <>)

commas :: [Text] -> Text
commas = T.intercalate ", "

tshow :: Show a => a -> Text
tshow = T.pack . show

-- Silence unused-import warnings for helpers kept for later targets.
_unused :: ()
_unused = const () (isAlphaNum, nub :: [Int] -> [Int], mapMaybe :: (Int -> Maybe Int) -> [Int] -> [Int], relName)
