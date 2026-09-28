{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §9 Verification.
--
-- What a module must satisfy before anything runs it, prints it or hashes
-- it. A builder makes most of these hard to write; the verifier refuses
-- them anyway, because a module may arrive from anywhere. Every runtime
-- executes only modules that verify, and a function's hash
-- ('Ark.Hash.functionHash') is taken of the form 'verify' returns — orders
-- completed, symbols renumbered — so that the hash is of the program as it
-- will be run.
--
-- The rules, each a constructor of 'VerifyError' or 'Complaint':
--
-- * the schema is well-formed ('Ark.Schema.checkSchema'), and the module's
--   spec version is this one;
-- * function and router names are unique; a router uses only guards and
--   providers;
-- * a mutator or query is on a router and runs a subsequence
--   of that router's middleware, each of which reads only input fields the
--   procedure has, at the same types; a mutator returns nothing and a
--   query returns on every path a value of its declared type;
-- * middleware is on no router, runs no middleware, has
--   no autos and no checks; a guard returns nothing, a provider returns
--   its declared type on every path;
-- * a helper names no router, no middleware, no autos, no
--   checks, and returns on every path;
-- * every check suits its field's type, an @exists@ check names a table
--   that exists, and a refinement is a boolean over the input
--   alone;
-- * the body is well-typed under the rules of 'infer', with no option of
--   an option anywhere, and reads a provided value only from a provider
--   the procedure runs;
-- * a read ('ESelect', 'EGet', 'EExists') appears only as the whole
--   right-hand side of a 'SLet', never in a helper;
-- * a write appears only in a mutator, a refusal never in a helper, and
--   every table a function touches exists;
-- * an @insert@ or @upsert@ that names columns names a declared unique
--   index of its table;
-- * a helper is called only by functions declared after it, so that the
--   call graph is acyclic and every function terminates;
-- * every plan's order is made total by appending the table's key columns
--   ascending where the author left them out.
module Ark.Verify
  ( VerifyError (..)
  , Complaint (..)
  , verify
  , verifyFunction
  , completeOrders
  , providedTypes
  ) where

import Control.Monad (foldM, forM_, unless, when, zipWithM_)
import Data.List (isSubsequenceOf, nub, (\\))
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing)
import Data.Text (Text)
import qualified Data.Text as T

import Ark.Encode (normalize)
import Ark.IR
import Ark.Schema
import Ark.Value

data VerifyError
  = BadSpecVersion SpecVersion
  | BadSchema SchemaError
  | DuplicateFunction Text
  | DuplicateRouter Text
  | -- | Router, then what is wrong with it.
    BadRouter Text Complaint
  | -- | Function, then the complaint.
    In Text Complaint
  deriving (Eq, Show)

data Complaint
  = AutosOnNonMutator
  | ReturnTypeOnMutator
  | NoReturnType
  | DuplicateName Text
  | UnknownAutoTable TableName
  | NestedOption
  | -- | A procedure not on a router, or a router named by something that
    -- is not a procedure.
    NoRouter
  | RouterOnNonProcedure
  | UnknownRouter Text
  | -- | Middleware named that the router does not declare, or out of the
    -- router's order.
    UsesNotOnRouter [Text]
  | UsesOnNonProcedure
  | -- | A name in @uses@ that is not a guard or provider.
    NotMiddleware Text
  | -- | Middleware, field: the procedure lacks the field, or has it at
    -- another type.
    MiddlewareInputMismatch Text Text
  | ChecksOutsideProcedure
  | -- | Check, field type: a check that does not suit the type it is on.
    BadCheck Text Ty
  | UnknownProvided Text
  | -- | A provided value read where none exists yet: in a check or a
    -- refinement.
    ProvidedInCheck
  | -- | Table, columns: an @insert@ or @upsert@ @on@ columns that are not a
    -- declared unique index.
    OnNotUnique TableName [FieldName]
  | TypeMismatch Text Ty Ty -- ^ where, expected, actual
  | NotAStruct Text
  | NoSuchField FieldName
  | UnknownArg Text
  | UnknownAuto Text
  | UnboundSymbol Sym
  | ReadNotBound
  | ReadInHelper
  | WriteOutsideMutator
  | RefuseInHelper
  | UnknownTable TableName
  | UnknownColumn TableName FieldName
  | UnknownHelper Text
  | HelperNotYetDeclared Text
  | NotAHelper Text
  | Arity Text Int Int
  | BadOp Text
  | MayNotReturn
  | NeedsAnnotation Text
  | BadRelation TableName TableName
  | KeyArity TableName Int Int
  | StdMisuse Text
  deriving (Eq, Show)

-- | Verify a module. On success, the module as it is to be hashed and
-- printed: orders completed and every function normalised.
verify :: Module -> Either [VerifyError] Module
verify m0 = do
  let m = completeOrders m0
      sch = modSchema m
  when (modSpec m /= specVersion) (Left [BadSpecVersion (modSpec m)])
  case checkSchema sch of
    [] -> Right ()
    es -> Left (map BadSchema es)
  let names = map fnName (modFunctions m)
  case names \\ nub names of
    [] -> Right ()
    ds -> Left (map DuplicateFunction (nub ds))
  let rnames = map rtName (modRouters m)
  case rnames \\ nub rnames of
    [] -> Right ()
    ds -> Left (map DuplicateRouter (nub ds))
  case concatMap (router m) (modRouters m) of
    [] -> Right ()
    es -> Left es
  let checked = zipWith (\i fn -> verifyFunction m i fn) [0 ..] (modFunctions m)
      placed = [In (fnName fn) (UnknownRouter r) | fn <- modFunctions m, Just r <- [fnRouter fn], isNothing (lookupRouter m r)]
  case concat [es | Left es <- checked] ++ placed of
    [] -> Right m {modFunctions = map normalize (modFunctions m)}
    es -> Left es
  where
    router m r =
      concat
          [ case lookupFunction m u of
              Nothing -> [BadRouter (rtName r) (NotMiddleware u)]
              Just f
                | not (isMiddleware f) -> [BadRouter (rtName r) (NotMiddleware u)]
                | otherwise -> []
          | u <- rtUses r
          ]
        ++ [BadRouter (rtName r) (DuplicateName u) | u <- rtUses r \\ nub (rtUses r)]

-- | Verify the @i@th function of a module (its index decides which
-- helpers it may call). A procedure's router is held to it only when the
-- module has that router: 'Ark.Compat.checkRetained' verifies a closure
-- against a later module whose routers may have been renamed, and a
-- router is a grouping, not a meaning.
verifyFunction :: Module -> Int -> Function -> Either [VerifyError] ()
verifyFunction m i fn = either (Left . map (In (fnName fn))) Right $ do
  let sch = modSchema m
  case fnKind fn of
    k | k == Mutator || k == Query -> do
      r <- maybe (Left [NoRouter]) Right (fnRouter fn)
      forM_ (lookupRouter m r) $ \rt ->
        unless (fnUses fn `isSubsequenceOf` rtUses rt) (Left [UsesNotOnRouter (fnUses fn)])
      case fnUses fn \\ nub (fnUses fn) of
        [] -> Right ()
        ds -> Left (map DuplicateName ds)
      forM_ (fnUses fn) $ \u -> case lookupFunction m u of
        Nothing -> Left [NotMiddleware u]
        Just mw -> do
          unless (isMiddleware mw) (Left [NotMiddleware u])
          forM_ (fnArgs mw) $ \(a, t) -> unless (lookup a (fnArgs fn) == Just t) (Left [MiddlewareInputMismatch u a])
      if k == Mutator
        then when (isJust (fnRet fn)) (Left [ReturnTypeOnMutator])
        else do
          unless (null (fnAutos fn)) (Left [AutosOnNonMutator])
          unless (isJust (fnRet fn)) (Left [NoReturnType])
    Helper -> do
      plain
      unless (isJust (fnRet fn)) (Left [NoReturnType])
    Guard -> do
      plain
      when (isJust (fnRet fn)) (Left [ReturnTypeOnMutator])
    Provide -> do
      plain
      unless (isJust (fnRet fn)) (Left [NoReturnType])
  let argNames = map fst (fnInput fn) ++ map fst (fnAutos fn)
  case argNames \\ nub argNames of
    [] -> Right ()
    ds -> Left (map DuplicateName ds)
  mapM_ (\(_, t) -> noNestedOption t) (fnArgs fn)
  mapM_ (\(_, a) -> case a of NewId t | not (isJust (lookupTable sch t)) -> Left [UnknownAutoTable t]; _ -> Right ()) (fnAutos fn)
  maybe (Right ()) noNestedOption (fnRet fn)
  let g =
        G
          { gMod = m
          , gIndex = i
          , gFn = fn
          , gLocals = M.empty
          , gProvided = providedTypes m fn
          , gInChecks = True
          }
  mapM_ (\(name, f) -> mapM_ (checkOk g name (fTy f)) (fChecks f)) (fnInput fn)
  mapM_ (\(e, _) -> expect g "refine" TBool e) (fnRefine fn)
  _ <- block g {gInChecks = False} (fnBody fn)
  when (fnKind fn `elem` [Query, Helper, Provide] && not (returns (fnBody fn))) (Left [MayNotReturn])
  where
    -- Neither middleware nor a helper is on a router, runs middleware,
    -- draws autos or checks its input.
    plain = do
      when (isJust (fnRouter fn)) (Left [RouterOnNonProcedure])
      unless (null (fnUses fn)) (Left [UsesOnNonProcedure])
      unless (null (fnAutos fn)) (Left [AutosOnNonMutator])
      unless (all (null . fChecks . snd) (fnInput fn) && null (fnRefine fn)) (Left [ChecksOutsideProcedure])

-- | The values a procedure's providers hand its body, by middleware name.
providedTypes :: Module -> Function -> Map Text Ty
providedTypes m fn = M.fromList [(u, t) | u <- fnUses fn, Just mw <- [lookupFunction m u], fnKind mw == Provide, Just t <- [fnRet mw]]

-- The typing environment.
data G = G
  { gMod :: Module
  , gIndex :: Int
  , gFn :: Function
  , gLocals :: Map Sym Ty
  , gProvided :: Map Text Ty
  , -- | Inside a check or a refinement, where nothing has been provided
    -- yet.
    gInChecks :: Bool
  }

type Check' a = Either [Complaint] a

err :: Complaint -> Check' a
err c = Left [c]

schema :: G -> Schema
schema = modSchema . gMod

kind :: G -> FnKind
kind = fnKind . gFn

noNestedOption :: Ty -> Check' ()
noNestedOption = \case
  TOption (TOption _) -> err NestedOption
  TOption t -> noNestedOption t
  TList t -> noNestedOption t
  TStruct fs -> mapM_ noNestedOption (M.elems fs)
  _ -> Right ()

-- | §9.0 A check suits the type it is on: the option, if any, is looked
-- through, since a check on an optional field runs when the value is
-- present.
checkOk :: G -> Text -> Ty -> Check -> Check' ()
checkOk g _ ty c = case (c, base) of
  (CTrim, TText) -> Right ()
  (CMinLen _ _, TText) -> Right ()
  (CMaxLen _ _, TText) -> Right ()
  (CRange _ _ _, TInt) -> Right ()
  (CNonEmpty _, TList _) -> Right ()
  (CExists _, TId t) -> () <$ table g t
  (CRefine e _, _) -> expect g "refine" TBool e
  _ -> err (BadCheck (name c) ty)
  where
    base = case ty of TOption t -> t; t -> t
    name = \case
      CTrim -> "trim"
      CMinLen {} -> "min_len"
      CMaxLen {} -> "max_len"
      CRange {} -> "range"
      CNonEmpty {} -> "non_empty"
      CExists {} -> "exists"
      CRefine {} -> "refine"

-- A block definitely returns when its last statement does, or is an
-- @if@ both of whose branches do.
returns :: Block -> Bool
returns [] = False
returns stmts = case last stmts of
  SReturn _ -> True
  SRefuse _ -> True
  SIf _ a b -> returns a && returns b
  _ -> False

bindL :: Sym -> Ty -> G -> G
bindL x t g = g {gLocals = M.insert x t (gLocals g)}

block :: G -> Block -> Check' G
block = foldM stmt

stmt :: G -> Stmt -> Check' G
stmt g = \case
  SLet x e -> do
    t <- case e of
      ESelect p -> readOk >> planTy g p
      EGet tbl ks -> readOk >> keyed g tbl ks >> (TOption . rowTy <$> table g tbl)
      EExists tbl ks -> readOk >> keyed g tbl ks >> pure TBool
      _ -> infer g Nothing e
    pure (bindL x t g)
  SIf c a b -> do
    expect g "if condition" TBool c
    _ <- block g a
    _ <- block g b
    pure g
  SFor x xs body -> do
    t <- infer g Nothing xs >>= elemOf "for"
    _ <- block (bindL x t g) body
    pure g
  SInsert tbl e on -> write "insert" tbl e on g
  SUpsert tbl e on -> write "upsert" tbl e on g
  SUpdate tbl ks x e -> do
    mutating
    keyed g tbl ks
    t <- table g tbl
    rowOk "update" tbl (bindL x (rowTy t) g) e
    pure g
  SDelete tbl ks -> do
    mutating
    keyed g tbl ks
    pure g
  SRefuse e -> do
    when (kind g == Helper) (err RefuseInHelper)
    expect g "refuse" TText e
    pure g
  SReturn me -> do
    case (fnRet (gFn g), me) of
      (Nothing, Nothing) -> Right ()
      (Nothing, Just _) -> err ReturnTypeOnMutator
      (Just _, Nothing) -> err NoReturnType
      (Just want, Just e) -> expect g "return" want e
    pure g
  where
    mutating = unless (kind g == Mutator) (err WriteOutsideMutator)
    readOk = when (kind g == Helper) (err ReadInHelper)
    write site tbl e on g' = do
      mutating
      t <- table g' tbl
      unless (null on || Index on True `elem` tIndexes t) (err (OnNotUnique tbl on))
      rowOk site tbl g' e
      pure g'

-- A write may leave nullable columns out (they are written as None), so
-- the struct is checked field by field against the row type: every field
-- it has must be a column of the right type, and every non-nullable
-- column must be there. This is what lets a table grow a nullable column
-- after the mutators writing it were hashed.
rowOk :: Text -> TableName -> G -> Expr -> Check' ()
rowOk site tbl g e = do
  t <- table g tbl
  got <- infer g (Just (rowTy t)) e
  case (rowTy t, got) of
    (TStruct want, TStruct have) -> do
      mapM_
        ( \(k, ty) -> case M.lookup k want of
            Nothing -> err (UnknownColumn tbl k)
            Just w -> unless (w == ty) (err (TypeMismatch (site <> " " <> tbl <> "." <> k) w ty))
        )
        (M.toList have)
      mapM_ (\c -> unless (colNullable c || M.member (colName c) have) (err (TypeMismatch (site <> " " <> tbl) (rowTy t) got))) (tColumns t)
    _ -> err (TypeMismatch (site <> " " <> tbl) (rowTy t) got)

table :: G -> TableName -> Check' Table
table g tbl = maybe (err (UnknownTable tbl)) Right (lookupTable (schema g) tbl)

-- A key expression list matches the table's key columns in number and type.
keyed :: G -> TableName -> [Expr] -> Check' ()
keyed g tbl ks = do
  t <- table g tbl
  let want = keyTy t
  unless (length want == length ks) (err (KeyArity tbl (length want) (length ks)))
  zipWithM_ (expect g ("key of " <> tbl)) want ks

-- | Check an expression against an expected type.
expect :: G -> Text -> Ty -> Expr -> Check' ()
expect g site want e = do
  got <- infer g (Just want) e
  unless (got == want) (err (TypeMismatch site want got))

elemOf :: Text -> Ty -> Check' Ty
elemOf _ (TList t) = Right t
elemOf site t = err (TypeMismatch site (TList t) t)

-- | §9.1 Typing of expressions. The expected type, when known, is used
-- only where an expression cannot be typed on its own: an empty list, a
-- @None@ without its annotation, a nil id, a struct field of one of those.
infer :: G -> Maybe Ty -> Expr -> Check' Ty
infer g want = \case
  ELit v -> lit v
  EArg a -> maybe (err (UnknownArg a)) Right (lookup a (fnArgs (gFn g)))
  EAuto a -> case lookup a (fnAutos (gFn g)) of
    Just (NewId t) -> Right (TId t)
    Just Now -> Right TInt
    Nothing -> err (UnknownAuto a)
  EVar x -> maybe (err (UnboundSymbol x)) Right (M.lookup x (gLocals g))
  ECtxUser -> Right TText
  ECtxSession -> Right TText
  EProvided n
    | gInChecks g -> err ProvidedInCheck
    | otherwise -> maybe (err (UnknownProvided n)) Right (M.lookup n (gProvided g))
  EField e f -> do
    t <- infer g Nothing e
    case t of
      TStruct fs -> maybe (err (NoSuchField f)) Right (M.lookup f fs)
      _ -> err (NotAStruct f)
  EStruct fs -> do
    let wantF = case want of Just (TStruct ws) -> ws; _ -> M.empty
    ts <- M.traverseWithKey (\k e -> infer g (M.lookup k wantF) e) fs
    pure (TStruct ts)
  EList es -> do
    let wantE = case want of Just (TList t) -> Just t; _ -> Nothing
    ts <- mapM (infer g wantE) es
    case (ts, wantE) of
      ([], Just t) -> Right (TList t)
      ([], Nothing) -> err (NeedsAnnotation "empty list")
      (t : rest, _) -> do
        mapM_ (\t' -> unless (t' == t) (err (TypeMismatch "list element" t t'))) rest
        pure (TList t)
  ESome e -> do
    t <- infer g (case want of Just (TOption t) -> Just t; _ -> Nothing) e
    case t of
      TOption _ -> err NestedOption
      _ -> Right (TOption t)
  ENone t -> noNestedOption (TOption t) >> Right (TOption t)
  EMatch e x a b -> do
    te <- infer g Nothing e
    inner <- case te of
      TOption t -> Right t
      _ -> err (TypeMismatch "match" (TOption te) te)
    ta <- infer (bindL x inner g) want a
    tb <- infer g (Just ta) b
    unless (ta == tb) (err (TypeMismatch "match arms" ta tb))
    pure ta
  EIf c a b -> do
    expect g "if" TBool c
    ta <- infer g want a
    tb <- infer g (Just ta) b
    unless (ta == tb) (err (TypeMismatch "if arms" ta tb))
    pure ta
  EOp op es -> case (op, es) of
    (And, [a, b]) -> bools [a, b]
    (Or, [a, b]) -> bools [a, b]
    (Not, [a]) -> bools [a]
    (Neg, [a]) -> ints [a]
    (o, [a, b]) | o `elem` [Add, Sub, Mul, Div, Mod] -> ints [a, b]
    (o, _) -> err (BadOp (T.pack (show o)))
  ECmp _ a b -> do
    ta <- infer g Nothing a
    expect g "comparison" ta b
    pure TBool
  ECall name es -> do
    let fns = modFunctions (gMod g)
    case [(j, f) | (j, f) <- zip [0 :: Int ..] fns, fnName f == name] of
      [] -> err (UnknownHelper name)
      ((j, f) : _) -> do
        unless (fnKind f == Helper) (err (NotAHelper name))
        unless (j < gIndex g) (err (HelperNotYetDeclared name))
        unless (length es == length (fnArgs f)) (err (Arity name (length (fnArgs f)) (length es)))
        zipWithM_ (\(_, t) e -> expect g ("argument of " <> name) t e) (fnArgs f) es
        maybe (err (NotAHelper name)) Right (fnRet f)
  EStd f es -> do
    ts <- mapM (infer g Nothing) es
    stdTy f ts want
  EMap xs x b -> do
    t <- infer g Nothing xs >>= elemOf "map"
    u <- infer (bindL x t g) (case want of Just (TList u) -> Just u; _ -> Nothing) b
    pure (TList u)
  EFilter xs x b -> do
    t <- infer g Nothing xs >>= elemOf "filter"
    expect (bindL x t g) "filter body" TBool b
    pure (TList t)
  EAny xs x b -> do
    t <- infer g Nothing xs >>= elemOf "any"
    expect (bindL x t g) "any body" TBool b
    pure TBool
  EAll xs x b -> do
    t <- infer g Nothing xs >>= elemOf "all"
    expect (bindL x t g) "all body" TBool b
    pure TBool
  ESortBy xs x k -> do
    t <- infer g Nothing xs >>= elemOf "sort_by"
    _ <- infer (bindL x t g) Nothing k
    pure (TList t)
  EFold xs z acc x b -> do
    t <- infer g Nothing xs >>= elemOf "fold"
    a <- infer g want z
    expect (bindL acc a (bindL x t g)) "fold body" a b
    pure a
  ESelect _ -> err ReadNotBound
  EGet _ _ -> err ReadNotBound
  EExists _ _ -> err ReadNotBound
  where
    bools es = mapM_ (expect g "boolean operator" TBool) es >> pure TBool
    ints es = mapM_ (expect g "arithmetic" TInt) es >> pure TInt
    lit = \case
      VNull -> err (NeedsAnnotation "null literal; use None")
      VBool _ -> Right TBool
      VInt _ -> Right TInt
      VText _ -> case want of
        Just t@(TEnum _) -> Right t
        _ -> Right TText
      VBytes _ -> Right TBytes
      VId _ -> case want of
        Just t@(TId _) -> Right t
        _ -> err (NeedsAnnotation "id literal")
      VList _ -> err (NeedsAnnotation "list literal; use a list expression")
      VStruct _ -> err (NeedsAnnotation "struct literal; use a struct expression")

-- | §9.2 The type of a plan's rows: the table's columns, plus a list field
-- per relationship read beneath. Filter columns must exist and their
-- right-hand sides must have the column's type; order columns must exist;
-- a relationship must be one the schema declares between the two tables.
planTy :: G -> Plan -> Check' Ty
planTy g p = do
  t <- table g (pTable p)
  maybe (Right ()) (predOk t) (pFilter p)
  mapM_ (\(c, _) -> col t c) (pOrder p)
  rels <- mapM related (pRelated p)
  base <- case rowTy t of
    TStruct fs -> Right fs
    other -> err (TypeMismatch "row" (TStruct M.empty) other)
  pure (TList (TStruct (M.union (M.fromList rels) base)))
  where
    col t c = maybe (err (UnknownColumn (tName t) c)) Right (column t c)
    predOk t = \case
      PCmp c _ e -> col t c >>= \cl -> expect g ("filter on " <> c) (columnTy cl) e
      PIn c es -> col t c >>= \cl -> mapM_ (expect g ("filter on " <> c) (columnTy cl)) es
      PAll ps -> mapM_ (predOk t) ps
      PAny ps -> mapM_ (predOk t) ps
      PNot q -> predOk t q
    related r = do
      let rel = rRelation r
      unless (relParent rel == pTable p && rel `elem` childrenOf (schema g) (pTable p)) (err (BadRelation (pTable p) (relChild rel)))
      unless (pTable (rPlan r) == relChild rel) (err (BadRelation (pTable p) (pTable (rPlan r))))
      ct <- planTy g (rPlan r)
      pure (rName r, ct)

-- | §9.3 Signatures of the standard library.
stdTy :: StdFn -> [Ty] -> Maybe Ty -> Check' Ty
stdTy f ts want = case (f, ts) of
  (Trim, [TText]) -> Right TText
  (IsEmpty, [TText]) -> Right TBool
  (Concat, [TList TText]) -> Right TText
  (Lower, [TText]) -> Right TText
  (IsAlnum, [TText]) -> Right TBool
  (Chars, [TText]) -> Right (TList TText)
  (TextLen, [TText]) -> Right TInt
  (StartsWith, [TText, TText]) -> Right TBool
  (SplitOnce, [TText, TText]) -> Right (TOption (TStruct (M.fromList [("before", TText), ("after", TText)])))
  (TextOfInt, [TInt]) -> Right TText
  (Hex, [TBytes]) -> Right TText
  (Min, [TInt, TInt]) -> Right TInt
  (Max, [TInt, TInt]) -> Right TInt
  (Clamp, [TInt, TInt, TInt]) -> Right TInt
  (Abs, [TInt]) -> Right TInt
  (Fnv1a64, [TText]) -> Right TInt
  (Sha256, [TBytes]) -> Right TBytes
  (IdOfText, [TText]) -> case want of
    Just t@(TOption (TId _)) -> Right t
    _ -> err (NeedsAnnotation "id_of_text needs its table from context")
  (TextOfId, [TId _]) -> Right TText
  (NilId, []) -> case want of
    Just t@(TId _) -> Right t
    _ -> err (NeedsAnnotation "nil id needs its table from context")
  (Utf8, [TText]) -> Right TBytes
  (First, [TList t]) -> option t
  (Last, [TList t]) -> option t
  (Len, [TList _]) -> Right TInt
  (Contains, [TList t, t']) | t == t' -> Right TBool
  (Reverse, [TList t]) -> Right (TList t)
  (IsSome, [TOption _]) -> Right TBool
  (UnwrapOr, [TOption t, t']) | t == t' -> Right t
  (Unwrap, [TOption t]) -> Right t
  _ -> err (StdMisuse (T.pack (show f ++ " applied to " ++ show ts)))
  where
    option (TOption _) = err NestedOption
    option t = Right (TOption t)

-- | §9.4 Make every order total: append the table's key columns, ascending,
-- after whatever the author ordered by, omitting any already present. A
-- @limit 1@ over a partial order is exactly the kind of thing two backends
-- answer differently, so no plan is left with one. Applied to every plan
-- in every function, and to every related plan.
completeOrders :: Module -> Module
completeOrders m = m {modFunctions = map fn (modFunctions m)}
  where
    sch = modSchema m
    fn f = f {fnBody = map stmt' (fnBody f)}
    stmt' = \case
      SLet x e -> SLet x (ex e)
      SIf c a b -> SIf (ex c) (map stmt' a) (map stmt' b)
      SFor x xs b -> SFor x (ex xs) (map stmt' b)
      SInsert t e on -> SInsert t (ex e) on
      SUpsert t e on -> SUpsert t (ex e) on
      SUpdate t ks x e -> SUpdate t (map ex ks) x (ex e)
      SDelete t ks -> SDelete t (map ex ks)
      SRefuse e -> SRefuse (ex e)
      SReturn me -> SReturn (fmap ex me)
    ex = \case
      ESelect p -> ESelect (plan p)
      EField e f -> EField (ex e) f
      EStruct fs -> EStruct (M.map ex fs)
      EList es -> EList (map ex es)
      ESome e -> ESome (ex e)
      EMatch e x a b -> EMatch (ex e) x (ex a) (ex b)
      EIf c a b -> EIf (ex c) (ex a) (ex b)
      EOp o es -> EOp o (map ex es)
      ECmp o a b -> ECmp o (ex a) (ex b)
      ECall n es -> ECall n (map ex es)
      EStd s es -> EStd s (map ex es)
      EMap xs x b -> EMap (ex xs) x (ex b)
      EFilter xs x b -> EFilter (ex xs) x (ex b)
      EAny xs x b -> EAny (ex xs) x (ex b)
      EAll xs x b -> EAll (ex xs) x (ex b)
      ESortBy xs x k -> ESortBy (ex xs) x (ex k)
      EFold xs z acc x b -> EFold (ex xs) (ex z) acc x (ex b)
      EGet t ks -> EGet t (map ex ks)
      EExists t ks -> EExists t (map ex ks)
      e -> e
    plan p =
      let keyCols = maybe [] tKey (lookupTable sch (pTable p))
          present = map fst (pOrder p)
          extra = [(c, Asc) | c <- keyCols, c `notElem` present]
       in p {pOrder = pOrder p ++ extra, pRelated = map (\r -> r {rPlan = plan (rPlan r)}) (pRelated p)}
