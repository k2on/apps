{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §9 Verification.
--
-- What a module must satisfy before anything runs it, generates from it or
-- hashes it. A builder makes most of these hard to write; the verifier
-- refuses them anyway, because a module may arrive from anywhere. Every
-- runtime executes only modules that verify, and a function's hash
-- ('Ark.Hash.functionHash') is taken of the form 'verify' returns — orders
-- completed, symbols renumbered — so that the hash is of the program as it
-- will be run.
--
-- The rules, each a constructor of 'VerifyError':
--
-- * the schema is well-formed ('Ark.Schema.checkSchema'), and the module's
--   spec version is this one;
-- * function names are unique; a mutator names an existing scope and
--   returns nothing; a query or helper names no scope, declares no autos,
--   and returns on every path a value of its declared type;
-- * the body is well-typed under the rules of 'infer', with no option of
--   an option anywhere;
-- * a read ('ESelect', 'EGet', 'EExists') appears only as the whole
--   right-hand side of a 'SLet', never in a helper;
-- * a write or a refusal appears only in a mutator, and every table a
--   mutator touches is in the mutator's scope;
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
  ) where

import Control.Monad (foldM, unless, when, zipWithM_)
import Data.List (nub, (\\))
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
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
  | -- | Function, then the complaint.
    In Text Complaint
  deriving (Eq, Show)

data Complaint
  = MutatorWithoutScope
  | UnknownScope ScopeName
  | ScopeOnNonMutator
  | AutosOnNonMutator
  | ReturnTypeOnMutator
  | NoReturnType
  | DuplicateName Text
  | UnknownAutoTable TableName
  | NestedOption
  | TypeMismatch Text Ty Ty -- ^ where, expected, actual
  | NotAStruct Text
  | NoSuchField FieldName
  | UnknownArg Text
  | UnknownAuto Text
  | UnboundSymbol Sym
  | ReadNotBound
  | ReadInHelper
  | WriteOutsideMutator
  | RefuseOutsideMutator
  | OutOfScope TableName
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
-- generated from: orders completed and every function normalised.
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
  let checked = zipWith (\i fn -> verifyFunction m i fn) [0 ..] (modFunctions m)
  case concat [es | Left es <- checked] of
    [] -> Right m {modFunctions = map normalize (modFunctions m)}
    es -> Left es

-- | Verify the @i@th function of a module (its index decides which
-- helpers it may call).
verifyFunction :: Module -> Int -> Function -> Either [VerifyError] ()
verifyFunction m i fn = either (Left . map (In (fnName fn))) Right $ do
  let sch = modSchema m
  case fnKind fn of
    Mutator -> do
      scope <- maybe (Left [MutatorWithoutScope]) Right (fnScope fn)
      unless (isJust (scopeOf sch scope)) (Left [UnknownScope scope])
      when (isJust (fnRet fn)) (Left [ReturnTypeOnMutator])
    _ -> do
      when (isJust (fnScope fn)) (Left [ScopeOnNonMutator])
      unless (null (fnAutos fn)) (Left [AutosOnNonMutator])
      unless (isJust (fnRet fn)) (Left [NoReturnType])
  let argNames = map fst (fnArgs fn) ++ map fst (fnAutos fn)
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
          }
  _ <- block g (fnBody fn)
  when (fnKind fn /= Mutator && not (returns (fnBody fn))) (Left [MayNotReturn])

-- The typing environment.
data G = G
  { gMod :: Module
  , gIndex :: Int
  , gFn :: Function
  , gLocals :: Map Sym Ty
  }

type Check a = Either [Complaint] a

err :: Complaint -> Check a
err c = Left [c]

schema :: G -> Schema
schema = modSchema . gMod

kind :: G -> FnKind
kind = fnKind . gFn

noNestedOption :: Ty -> Check ()
noNestedOption = \case
  TOption (TOption _) -> err NestedOption
  TOption t -> noNestedOption t
  TList t -> noNestedOption t
  TStruct fs -> mapM_ noNestedOption (M.elems fs)
  _ -> Right ()

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

block :: G -> Block -> Check G
block = foldM stmt

stmt :: G -> Stmt -> Check G
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
  SPut tbl e -> do
    mutating
    inScope g tbl
    t <- table g tbl
    expect g ("put " <> tbl) (rowTy t) e
    pure g
  SDelete tbl ks -> do
    mutating
    inScope g tbl
    keyed g tbl ks
    pure g
  SRefuse e -> do
    unless (kind g == Mutator) (err RefuseOutsideMutator)
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

inScope :: G -> TableName -> Check ()
inScope g tbl =
  when (kind g == Mutator && tableScope (schema g) tbl /= fnScope (gFn g)) (err (OutOfScope tbl))

table :: G -> TableName -> Check Table
table g tbl = maybe (err (UnknownTable tbl)) Right (lookupTable (schema g) tbl)

-- A key expression list matches the table's key columns in number and type.
keyed :: G -> TableName -> [Expr] -> Check ()
keyed g tbl ks = do
  inScope g tbl
  t <- table g tbl
  let want = keyTy t
  unless (length want == length ks) (err (KeyArity tbl (length want) (length ks)))
  zipWithM_ (expect g ("key of " <> tbl)) want ks

-- | Check an expression against an expected type.
expect :: G -> Text -> Ty -> Expr -> Check ()
expect g site want e = do
  got <- infer g (Just want) e
  unless (got == want) (err (TypeMismatch site want got))

elemOf :: Text -> Ty -> Check Ty
elemOf _ (TList t) = Right t
elemOf site t = err (TypeMismatch site (TList t) t)

-- | §9.1 Typing of expressions. The expected type, when known, is used
-- only where an expression cannot be typed on its own: an empty list, a
-- @None@ without its annotation, a nil id, a struct field of one of those.
infer :: G -> Maybe Ty -> Expr -> Check Ty
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
planTy :: G -> Plan -> Check Ty
planTy g p = do
  inScope g (pTable p)
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
stdTy :: StdFn -> [Ty] -> Maybe Ty -> Check Ty
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
      SPut t e -> SPut t (ex e)
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
