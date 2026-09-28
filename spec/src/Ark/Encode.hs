{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §7 The module as a value, and the normal form a hash is taken of.
--
-- A module travels and is hashed as a 'Value' — every node a struct with a
-- @"t"@ tag — so that 'Ark.Canon.encode' is the only encoder there is and
-- a module's bytes are canonical by the same rule as a row's. This is the
-- form an authority stores each accepted function in, the form a builder
-- emits, and the form 'Ark.Hash.functionHash' names an entry's function
-- by.
--
-- __Normalisation__ ('normalize') renumbers a function's symbols in
-- binding order, so that two authors' local names — or two builders'
-- numbering — cannot make one function into two. The author's names live
-- in 'fnNames', which this encoding does not carry; the printable form
-- reads them back, the hash never sees them.
--
-- Decoding a module from a value (the other direction, which a peer needs
-- to take a function it was sent) is 'fromValue', and 'toValue' followed
-- by 'fromValue' is the identity on normalised modules — a property the
-- vectors hold.
module Ark.Encode
  ( toValue
  , functionValue
  , schemaValue
  , tyValue
  , normalize
  , normalizeModule
  , calls
  ) where

import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text as T

import Ark.IR
import Ark.Schema
import Ark.Value

-- Small constructors for the tagged-struct convention ------------------

node :: Text -> [(FieldName, Value)] -> Value
node t fs = VStruct (M.fromList (("t", VText t) : fs))

txt :: Text -> Value
txt = VText

int :: Int -> Value
int = VInt . fromIntegral

list :: (a -> Value) -> [a] -> Value
list f = VList . map f

optText :: Maybe Text -> Value
optText = maybe VNull txt

-- | The whole module. Functions are carried without their dependency
-- hashes here, because the module carries the helpers themselves.
toValue :: Module -> Value
toValue m =
  node
    "module"
    [ ("spec", int (modSpec m))
    , ("schema", schemaValue (modSchema m))
    , ("functions", list (functionValue M.empty) (modFunctions m))
    , ("routers", list router (modRouters m))
    , ("live", list (\(n, t) -> node "frame" [("name", txt n), ("ty", tyValue t)]) (modLive m))
    ]
  where
    router r = node "router" [("name", txt (rtName r)), ("uses", list txt (rtUses r))]

schemaValue :: Schema -> Value
schemaValue (Schema tables) = list table tables
  where
    table t =
      node
        "table"
        [ ("name", txt (tName t))
        , ("columns", list col (tColumns t))
        , ("key", list txt (tKey t))
        , ("indexes", list ix (tIndexes t))
        , ("refs", list ref (tRefs t))
        ]
    col c = node "column" [("name", txt (colName c)), ("ty", tyValue (colTy c)), ("nullable", VBool (colNullable c))]
    ix i = node "index" [("columns", list txt (ixColumns i)), ("unique", VBool (ixUnique i))]
    ref r = node "ref" [("column", txt (refColumn r)), ("table", txt (refTable r))]

tyValue :: Ty -> Value
tyValue = \case
  TBool -> node "bool" []
  TInt -> node "int" []
  TText -> node "text" []
  TBytes -> node "bytes" []
  TId t -> node "id" [("table", txt t)]
  TEnum vs -> node "enum" [("variants", list txt vs)]
  TOption t -> node "option" [("of", tyValue t)]
  TList t -> node "list" [("of", tyValue t)]
  TStruct fs -> node "struct" [("fields", VStruct (M.map tyValue fs))]

-- | One function, as normalised, together with the hashes of the helpers
-- and middleware it depends on — so that a function's hash moves when a
-- helper it calls or a guard it runs changes, and an entry that names a
-- hash names a whole meaning and not only a body. The caller supplies
-- each dependency's hash ('Ark.Hash.closure' computes them recursively).
-- Names are not carried.
functionValue :: Map Text Value -> Function -> Value
functionValue deps fn0 =
  node
    "fn"
    [ ("name", txt (fnName fn))
    , ("deps", VStruct deps)
    , ("kind", txt (kind (fnKind fn)))
    , ("router", maybe VNull txt (fnRouter fn))
    , ("uses", list txt (fnUses fn))
    , ("autos", list auto (fnAutos fn))
    , ("input", list field (fnInput fn))
    , ("refine", list refine (fnRefine fn))
    , ("ret", maybe VNull tyValue (fnRet fn))
    , ("body", list stmt (fnBody fn))
    ]
  where
    fn = normalize fn0
    kind Mutator = "mutator"
    kind Query = "query"
    kind Helper = "helper"
    kind Guard = "guard"
    kind Provide = "provide"
    auto (n, NewId t) = node "new_id" [("name", txt n), ("table", txt t)]
    auto (n, Now) = node "now" [("name", txt n)]
    field (n, f) = node "field" [("name", txt n), ("ty", tyValue (fTy f)), ("checks", list check (fChecks f))]
    refine (e, why) = node "refine" [("e", expr e), ("why", optText why)]

check :: Check -> Value
check = \case
  CTrim -> node "trim" []
  CMinLen n why -> node "min_len" [("n", int n), ("why", optText why)]
  CMaxLen n why -> node "max_len" [("n", int n), ("why", optText why)]
  CRange lo hi why -> node "range" [("lo", maybe VNull int lo), ("hi", maybe VNull int hi), ("why", optText why)]
  CNonEmpty why -> node "non_empty" [("why", optText why)]
  CExists why -> node "exists" [("why", optText why)]
  CRefine e why -> node "refine" [("e", expr e), ("why", optText why)]

stmt :: Stmt -> Value
stmt = \case
  SLet x e -> node "let" [("sym", int x), ("e", expr e)]
  SIf c a b -> node "if" [("c", expr c), ("then", list stmt a), ("else", list stmt b)]
  SFor x xs b -> node "for" [("sym", int x), ("in", expr xs), ("body", list stmt b)]
  SInsert t e on -> node "insert" [("table", txt t), ("row", expr e), ("on", list txt on)]
  SUpsert t e on -> node "upsert" [("table", txt t), ("row", expr e), ("on", list txt on)]
  SUpdate t ks x e -> node "update" [("table", txt t), ("key", list expr ks), ("sym", int x), ("row", expr e)]
  SDelete t ks -> node "delete" [("table", txt t), ("key", list expr ks)]
  SRefuse e -> node "refuse" [("e", expr e)]
  SReturn me -> node "return" [("e", maybe VNull expr me)]

expr :: Expr -> Value
expr = \case
  ELit v -> node "lit" [("v", v)]
  EArg a -> node "arg" [("name", txt a)]
  EAuto a -> node "auto" [("name", txt a)]
  EVar x -> node "var" [("sym", int x)]
  ECtxUser -> node "ctx_user" []
  ECtxSession -> node "ctx_session" []
  EProvided n -> node "provided" [("fn", txt n)]
  EField e f -> node "field" [("e", expr e), ("name", txt f)]
  EStruct fs -> node "struct" [("fields", VStruct (M.map expr fs))]
  EList es -> node "list" [("items", list expr es)]
  ESome e -> node "some" [("e", expr e)]
  ENone t -> node "none" [("ty", tyValue t)]
  EMatch e x a b -> node "match" [("e", expr e), ("sym", int x), ("some", expr a), ("none", expr b)]
  EIf c a b -> node "ife" [("c", expr c), ("then", expr a), ("else", expr b)]
  EOp op es -> node "op" [("op", txt (T.toLower (T.pack (show op)))), ("args", list expr es)]
  ECmp op a b -> node "cmp" [("op", txt (T.toLower (T.pack (show op)))), ("l", expr a), ("r", expr b)]
  ECall n es -> node "call" [("fn", txt n), ("args", list expr es)]
  EStd f es -> node "std" [("fn", txt (T.pack (show f))), ("args", list expr es)]
  EMap xs x b -> node "map" [("in", expr xs), ("sym", int x), ("body", expr b)]
  EFilter xs x b -> node "filter" [("in", expr xs), ("sym", int x), ("body", expr b)]
  EAny xs x b -> node "any" [("in", expr xs), ("sym", int x), ("body", expr b)]
  EAll xs x b -> node "all" [("in", expr xs), ("sym", int x), ("body", expr b)]
  ESortBy xs x k -> node "sort_by" [("in", expr xs), ("sym", int x), ("key", expr k)]
  EFold xs z acc x b -> node "fold" [("in", expr xs), ("init", expr z), ("acc", int acc), ("sym", int x), ("body", expr b)]
  ESelect p -> node "select" [("plan", plan p)]
  EGet t ks -> node "get" [("table", txt t), ("key", list expr ks)]
  EExists t ks -> node "exists" [("table", txt t), ("key", list expr ks)]

plan :: Plan -> Value
plan p =
  node
    "plan"
    [ ("table", txt (pTable p))
    , ("filter", maybe VNull predV (pFilter p))
    , ("order", list (\(c, d) -> node "by" [("column", txt c), ("dir", txt (if d == Asc then "asc" else "desc"))]) (pOrder p))
    , ("limit", maybe VNull int (pLimit p))
    , ("related", list related (pRelated p))
    ]
  where
    related r =
      node
        "related"
        [ ("name", txt (rName r))
        , ("parent", txt (relParent (rRelation r)))
        , ("child", txt (relChild (rRelation r)))
        , ("column", txt (relColumn (rRelation r)))
        , ("plan", plan (rPlan r))
        ]

predV :: Pred -> Value
predV = \case
  PCmp c op e -> node "pcmp" [("column", txt c), ("op", txt (T.toLower (T.pack (show op)))), ("e", expr e)]
  PIn c es -> node "pin" [("column", txt c), ("items", list expr es)]
  PAll ps -> node "pall" [("items", list predV ps)]
  PAny ps -> node "pany" [("items", list predV ps)]
  PNot q -> node "pnot" [("e", predV q)]

-- | §7.1 Alpha-normalisation.
--
-- Symbols are renumbered 0, 1, 2… in the order their binders are met
-- walking the function top to bottom, left to right, binders before the
-- scopes they open: first each input field's checks in field order, then
-- the whole-input refinements, then the body. The walk is the evaluation
-- order of 'Ark.Eval', so the @n@th binding executed in a straight-line
-- body is symbol @n@. Free symbols (a bug the verifier reports) are left
-- as they are.
normalize :: Function -> Function
normalize fn = fn {fnInput = input', fnRefine = refine', fnBody = body, fnNames = names}
  where
    (input', n1) = renumberInput (fnInput fn) 0
    (refine', n2) = renumberRefine (fnRefine fn) n1
    (body, mapping) = renumberBlock M.empty n2 (fnBody fn)
    names = M.fromList [(new, n) | (old, new) <- M.toList mapping, Just n <- [M.lookup old (fnNames fn)]]
    renumberInput [] n = ([], n)
    renumberInput ((name, Field t cs) : rest) n =
      let (cs', n') = renumberChecks cs n
          (rest', n'') = renumberInput rest n'
       in ((name, Field t cs') : rest', n'')
    renumberChecks [] n = ([], n)
    renumberChecks (c : cs) n =
      let (c', n') = case c of
            CRefine e why -> let (e', k) = renumberExpr M.empty n e in (CRefine e' why, k)
            other -> (other, n)
          (cs', n'') = renumberChecks cs n'
       in (c' : cs', n'')
    renumberRefine [] n = ([], n)
    renumberRefine ((e, why) : rest) n =
      let (e', n') = renumberExpr M.empty n e
          (rest', n'') = renumberRefine rest n'
       in ((e', why) : rest', n'')

normalizeModule :: Module -> Module
normalizeModule m = m {modFunctions = map normalize (modFunctions m)}

type Ren = Map Sym Sym

renumberBlock :: Ren -> Int -> Block -> (Block, Ren)
renumberBlock ren _ [] = ([], ren)
renumberBlock ren next (s : rest) =
  let (s', ren', next') = renumberStmt ren next s
      (rest', ren'') = renumberBlock ren' next' rest
   in (s' : rest', ren'')

-- Returns the statement, the renaming extended by any binder it
-- introduced for the statements after it, and the next free number.
renumberStmt :: Ren -> Int -> Stmt -> (Stmt, Ren, Int)
renumberStmt ren next = \case
  SLet x e ->
    let (e', next') = renumberExpr ren next e
        ren' = M.insert x next' ren
     in (SLet next' e', ren', next' + 1)
  SIf c a b ->
    let (c', n1) = renumberExpr ren next c
        (a', n2) = inner ren n1 a
        (b', n3) = inner ren n2 b
     in (SIf c' a' b', ren, n3)
  SFor x xs b ->
    let (xs', n1) = renumberExpr ren next xs
        ren' = M.insert x n1 ren
        (b', n2) = inner ren' (n1 + 1) b
     in (SFor n1 xs' b', ren, n2)
  SInsert t e on -> let (e', n1) = renumberExpr ren next e in (SInsert t e' on, ren, n1)
  SUpsert t e on -> let (e', n1) = renumberExpr ren next e in (SUpsert t e' on, ren, n1)
  SUpdate t ks x e ->
    let (ks', n1) = renumberMany ren next ks
        ren' = M.insert x n1 ren
        (e', n2) = renumberExpr ren' (n1 + 1) e
     in (SUpdate t ks' n1 e', ren, n2)
  SDelete t ks -> let (ks', n1) = renumberMany ren next ks in (SDelete t ks', ren, n1)
  SRefuse e -> let (e', n1) = renumberExpr ren next e in (SRefuse e', ren, n1)
  SReturn me -> case me of
    Nothing -> (SReturn Nothing, ren, next)
    Just e -> let (e', n1) = renumberExpr ren next e in (SReturn (Just e'), ren, n1)
  where
    -- A nested block's bindings do not escape it, but their numbers are
    -- still consumed, so that numbering is a function of the whole body.
    inner r n blk = let (blk', _) = renumberBlock r n blk in (blk', countBinders blk' n)

-- The next free number after a renumbered block: one past the largest
-- binder in it, or the given floor.
countBinders :: Block -> Int -> Int
countBinders blk n = maximum (n : map (+ 1) (concatMap binders blk))
  where
    binders = \case
      SLet x e -> x : exprBinders e
      SIf c a b -> exprBinders c ++ concatMap binders a ++ concatMap binders b
      SFor x xs b -> x : exprBinders xs ++ concatMap binders b
      SInsert _ e _ -> exprBinders e
      SUpsert _ e _ -> exprBinders e
      SUpdate _ ks x e -> x : concatMap exprBinders ks ++ exprBinders e
      SDelete _ ks -> concatMap exprBinders ks
      SRefuse e -> exprBinders e
      SReturn me -> maybe [] exprBinders me

exprBinders :: Expr -> [Sym]
exprBinders = \case
  EMatch e x a b -> x : concatMap exprBinders [e, a, b]
  EMap xs x b -> x : concatMap exprBinders [xs, b]
  EFilter xs x b -> x : concatMap exprBinders [xs, b]
  EAny xs x b -> x : concatMap exprBinders [xs, b]
  EAll xs x b -> x : concatMap exprBinders [xs, b]
  ESortBy xs x k -> x : concatMap exprBinders [xs, k]
  EFold xs z acc x b -> acc : x : concatMap exprBinders [xs, z, b]
  EField e _ -> exprBinders e
  EStruct fs -> concatMap exprBinders (M.elems fs)
  EList es -> concatMap exprBinders es
  ESome e -> exprBinders e
  EIf c a b -> concatMap exprBinders [c, a, b]
  EOp _ es -> concatMap exprBinders es
  ECmp _ a b -> exprBinders a ++ exprBinders b
  ECall _ es -> concatMap exprBinders es
  EStd _ es -> concatMap exprBinders es
  ESelect p -> planBinders p
  EGet _ ks -> concatMap exprBinders ks
  EExists _ ks -> concatMap exprBinders ks
  _ -> []

planBinders :: Plan -> [Sym]
planBinders p = maybe [] predBinders (pFilter p) ++ concatMap (planBinders . rPlan) (pRelated p)

predBinders :: Pred -> [Sym]
predBinders = \case
  PCmp _ _ e -> exprBinders e
  PIn _ es -> concatMap exprBinders es
  PAll ps -> concatMap predBinders ps
  PAny ps -> concatMap predBinders ps
  PNot q -> predBinders q

renumberMany :: Ren -> Int -> [Expr] -> ([Expr], Int)
renumberMany _ next [] = ([], next)
renumberMany ren next (e : es) =
  let (e', n1) = renumberExpr ren next e
      (es', n2) = renumberMany ren n1 es
   in (e' : es', n2)

renumberExpr :: Ren -> Int -> Expr -> (Expr, Int)
renumberExpr ren next = \case
  EVar x -> (EVar (M.findWithDefault x x ren), next)
  EField e f -> let (e', n) = renumberExpr ren next e in (EField e' f, n)
  EStruct fs ->
    let (es, n) = renumberMany ren next (M.elems fs)
     in (EStruct (M.fromList (zip (M.keys fs) es)), n)
  EList es -> let (es', n) = renumberMany ren next es in (EList es', n)
  ESome e -> let (e', n) = renumberExpr ren next e in (ESome e', n)
  EMatch e x a b ->
    let (e', n1) = renumberExpr ren next e
        ren' = M.insert x n1 ren
        (a', n2) = renumberExpr ren' (n1 + 1) a
        (b', n3) = renumberExpr ren n2 b
     in (EMatch e' n1 a' b', n3)
  EIf c a b ->
    let (c', n1) = renumberExpr ren next c
        (a', n2) = renumberExpr ren n1 a
        (b', n3) = renumberExpr ren n2 b
     in (EIf c' a' b', n3)
  EOp op es -> let (es', n) = renumberMany ren next es in (EOp op es', n)
  ECmp op a b ->
    let (a', n1) = renumberExpr ren next a
        (b', n2) = renumberExpr ren n1 b
     in (ECmp op a' b', n2)
  ECall f es -> let (es', n) = renumberMany ren next es in (ECall f es', n)
  EStd f es -> let (es', n) = renumberMany ren next es in (EStd f es', n)
  EMap xs x b -> binder1 EMap xs x b
  EFilter xs x b -> binder1 EFilter xs x b
  EAny xs x b -> binder1 EAny xs x b
  EAll xs x b -> binder1 EAll xs x b
  ESortBy xs x k -> binder1 ESortBy xs x k
  EFold xs z acc x b ->
    let (xs', n0) = renumberExpr ren next xs
        (z', n1) = renumberExpr ren n0 z
        ren' = M.insert x (n1 + 1) (M.insert acc n1 ren)
        (b', n2) = renumberExpr ren' (n1 + 2) b
     in (EFold xs' z' n1 (n1 + 1) b', n2)
  ESelect p -> let (p', n) = renumberPlan ren next p in (ESelect p', n)
  EGet t ks -> let (ks', n) = renumberMany ren next ks in (EGet t ks', n)
  EExists t ks -> let (ks', n) = renumberMany ren next ks in (EExists t ks', n)
  e -> (e, next)
  where
    binder1 mk xs x b =
      let (xs', n1) = renumberExpr ren next xs
          ren' = M.insert x n1 ren
          (b', n2) = renumberExpr ren' (n1 + 1) b
       in (mk xs' n1 b', n2)

renumberPlan :: Ren -> Int -> Plan -> (Plan, Int)
renumberPlan ren next p =
  let (f', n1) = case pFilter p of
        Nothing -> (Nothing, next)
        Just f -> let (f'', n) = renumberPred ren next f in (Just f'', n)
      (rels, n2) = go n1 (pRelated p)
   in (p {pFilter = f', pRelated = rels}, n2)
  where
    go n [] = ([], n)
    go n (r : rs) =
      let (rp, n') = renumberPlan ren n (rPlan r)
          (rs', n'') = go n' rs
       in (r {rPlan = rp} : rs', n'')

renumberPred :: Ren -> Int -> Pred -> (Pred, Int)
renumberPred ren next = \case
  PCmp c op e -> let (e', n) = renumberExpr ren next e in (PCmp c op e', n)
  PIn c es -> let (es', n) = renumberMany ren next es in (PIn c es', n)
  PAll ps -> let (ps', n) = many ps in (PAll ps', n)
  PAny ps -> let (ps', n) = many ps in (PAny ps', n)
  PNot q -> let (q', n) = renumberPred ren next q in (PNot q', n)
  where
    many [] = ([], next)
    many (q : qs) =
      let (q', n1) = renumberPred ren next q
          (qs', n2) = go n1 qs
       in (q' : qs', n2)
    go n [] = ([], n)
    go n (q : qs) = let (q', n1) = renumberPred ren n q; (qs', n2) = go n1 qs in (q' : qs', n2)

-- | The names of the helpers a function calls, directly: in its checks,
-- its refinements and its body. Middleware a procedure runs is 'fnUses',
-- not a call.
calls :: Function -> [Text]
calls fn = nubOrd (concatMap checkCalls (concatMap (fChecks . snd) (fnInput fn)) ++ concatMap (exprCalls . fst) (fnRefine fn) ++ concatMap stmtCalls (fnBody fn))
  where
    nubOrd = M.keys . M.fromList . map (\x -> (x, ()))
    checkCalls = \case
      CRefine e _ -> exprCalls e
      _ -> []
    stmtCalls = \case
      SLet _ e -> exprCalls e
      SIf c a b -> exprCalls c ++ concatMap stmtCalls a ++ concatMap stmtCalls b
      SFor _ xs b -> exprCalls xs ++ concatMap stmtCalls b
      SInsert _ e _ -> exprCalls e
      SUpsert _ e _ -> exprCalls e
      SUpdate _ ks _ e -> concatMap exprCalls ks ++ exprCalls e
      SDelete _ ks -> concatMap exprCalls ks
      SRefuse e -> exprCalls e
      SReturn me -> maybe [] exprCalls me
    exprCalls = \case
      ECall n es -> n : concatMap exprCalls es
      EField e _ -> exprCalls e
      EStruct fs -> concatMap exprCalls (M.elems fs)
      EList es -> concatMap exprCalls es
      ESome e -> exprCalls e
      EMatch e _ a b -> concatMap exprCalls [e, a, b]
      EIf c a b -> concatMap exprCalls [c, a, b]
      EOp _ es -> concatMap exprCalls es
      ECmp _ a b -> exprCalls a ++ exprCalls b
      EStd _ es -> concatMap exprCalls es
      EMap xs _ b -> exprCalls xs ++ exprCalls b
      EFilter xs _ b -> exprCalls xs ++ exprCalls b
      EAny xs _ b -> exprCalls xs ++ exprCalls b
      EAll xs _ b -> exprCalls xs ++ exprCalls b
      ESortBy xs _ k -> exprCalls xs ++ exprCalls k
      EFold xs z _ _ b -> concatMap exprCalls [xs, z, b]
      ESelect p -> planCalls p
      EGet _ ks -> concatMap exprCalls ks
      EExists _ ks -> concatMap exprCalls ks
      _ -> []
    planCalls p = maybe [] predCalls (pFilter p) ++ concatMap (planCalls . rPlan) (pRelated p)
    predCalls = \case
      PCmp _ _ e -> exprCalls e
      PIn _ es -> concatMap exprCalls es
      PAll ps -> concatMap predCalls ps
      PAny ps -> concatMap predCalls ps
      PNot q -> predCalls q
