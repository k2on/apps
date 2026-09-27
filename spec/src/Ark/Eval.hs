{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE LambdaCase #-}
-- | §6 Evaluation: what generated code must mean.
--
-- Nobody interprets Ark IR in production; every runtime executes native
-- source a generator wrote from it. This module is what that source is
-- held to. An @eval/@ vector is a module, a store, an entry and this
-- function's answer; generated Rust, Swift and Kotlin must give the same
-- rows, the same changes and the same refusal text.
--
-- Two kinds of failure are kept apart throughout, as Petros keeps
-- @Rejected@ and @Sqlite@ apart. A 'Refusal' is a __verdict__: a
-- deterministic fact about the entry that every replica reaches (an
-- explicit @refuse@, a constraint, an overflow). An 'EvalError' is a
-- __bug__: a module the verifier would have refused, or a generator that
-- disagrees with this file. A conformant runtime never reports the second
-- for a verified module, and a vector never expects one.
--
-- __Evaluation order is part of the meaning__, because two faults can
-- race: 'And' and 'Or' short-circuit left to right, 'EIf' and 'EMatch'
-- evaluate only the taken arm, list elements and call arguments are
-- evaluated left to right, and a struct's fields are evaluated in field
-- name order. Every target language can be made to do exactly this, and
-- none does it by accident.
module Ark.Eval
  ( Ctx (..)
  , EvalError (..)
  , Fault (..)
  , Args
  , apply
  , applyClosure
  , query
  , queryClosure
  , evalHelper
  ) where

import Control.Monad (unless)
import Control.Monad.Except (ExceptT, catchError, runExceptT, throwError)
import Control.Monad.State.Strict (State, gets, modify', runState)
import Control.Monad.Trans.Class (lift)
import Data.Int (Int64)
import Data.List (sortBy)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Text (Text)
import qualified Data.Text as T

import Ark.Hash (Closure (..), closure)
import Ark.IR
import Ark.Schema
import qualified Ark.Std as Std
import Ark.Store (Change, Refusal (..), Row, Store)
import qualified Ark.Store as S
import Ark.Value

-- | Who authored an entry: the user the authority verified for the
-- connection, and the login it was authored under. Frozen with the entry.
data Ctx = Ctx
  { ctxUser :: Text
  , ctxSession :: Text
  }
  deriving (Eq, Show)

-- | Arguments (or autos) by name.
type Args = Map Text Value

-- | A bug, never a verdict. See the module header.
data EvalError
  = UnknownFunction Text
  | WrongKind Text FnKind
  | MissingArg Text
  | MissingAuto Text
  | UnboundVar Sym
  | NoSuchField FieldName
  | TypeError Text
  | Arity Text
  | -- | A helper or query fell off the end of its body without returning.
    NoReturn Text
  | -- | A helper reached a read or a write, or a query a write.
    Impure Text
  | UnknownTable TableName
  | -- | A relationship whose parent key is not a single column.
    CompositeParentKey TableName
  deriving (Eq, Show)

-- | The two ways a computation stops short.
data Fault
  = Verdict Refusal
  | Bug EvalError
  deriving (Eq, Show)

-- Why a block stopped: a return, a verdict, or a bug.
data Stop
  = Returned (Maybe Value)
  | Halt Fault

data Env = Env
  { envSchema :: Schema
  , -- | The helpers in reach: a closure's, never a module's, so that an
    -- entry replays against the helper versions its function was hashed
    -- with.
    envHelpers :: [Function]
  , envKind :: FnKind
  , envCtx :: Ctx
  , envArgs :: Args
  , envAutos :: Args
  , envLocals :: Map Sym Value
  }

-- The store as it stands and the changes made so far, newest first.
data St = St
  { stStore :: Store
  , stChanges :: [Change]
  }

type Run = ExceptT Stop (State St)

halt :: Fault -> Run a
halt = throwError . Halt

bug :: EvalError -> Run a
bug = halt . Bug

verdict :: Refusal -> Run a
verdict = halt . Verdict

-- | §6.1 Apply a mutator to a store.
--
-- Outer 'Left' is a bug; inner 'Left' is the verdict; 'Right' is the store
-- after the mutator and the changes it made, in order. On a verdict the
-- store is unchanged — the whole entry rolls back, as one transaction.
--
-- 'apply' finds the function in a module by name and runs its current
-- closure; 'applyClosure' runs a closure directly, which is how an entry
-- replays: by the hash it recorded, whatever the module says now.
apply :: Module -> Text -> Ctx -> Args -> Args -> Store -> Either EvalError (Either Refusal (Store, [Change]))
apply m name ctx autos args st = do
  fn <- function m name
  applyClosure (modSchema m) (closure m fn) ctx autos args st

applyClosure :: Schema -> Closure -> Ctx -> Args -> Args -> Store -> Either EvalError (Either Refusal (Store, [Change]))
applyClosure sch (Closure fn helpers) ctx autos args st = do
  let name = fnName fn
  unless (fnKind fn == Mutator) (Left (WrongKind name (fnKind fn)))
  mapM_ (\(a, _) -> unless (M.member a autos) (Left (MissingAuto a))) (fnAutos fn)
  mapM_ (\(a, _) -> unless (M.member a args) (Left (MissingArg a))) (fnArgs fn)
  let env = Env sch helpers Mutator ctx args autos M.empty
  case runState (runExceptT (block env (fnBody fn))) (St st []) of
    (Right _, St st' chs) -> Right (Right (st', reverse chs))
    (Left (Returned _), St st' chs) -> Right (Right (st', reverse chs))
    (Left (Halt (Verdict r)), _) -> Right (Left r)
    (Left (Halt (Bug e)), _) -> Left e

-- | §6.2 Run a query. A query never changes the store; a 'Verdict' here is
-- a fault such as an overflow, deterministic but not a log verdict, and a
-- caller shows it as an error rather than recording anything.
query :: Module -> Text -> Args -> Store -> Either Fault Value
query m name args st = do
  fn <- either (Left . Bug) Right (function m name)
  queryClosure (modSchema m) (closure m fn) args st

queryClosure :: Schema -> Closure -> Args -> Store -> Either Fault Value
queryClosure sch (Closure fn helpers) args st = do
  let name = fnName fn
  unless (fnKind fn == Query) (Left (Bug (WrongKind name (fnKind fn))))
  mapM_ (\(a, _) -> unless (M.member a args) (Left (Bug (MissingArg a)))) (fnArgs fn)
  let env = Env sch helpers Query (Ctx "" "") args M.empty M.empty
  case fst (runState (runExceptT (block env (fnBody fn))) (St st [])) of
    Right _ -> Left (Bug (NoReturn name))
    Left (Returned (Just v)) -> Right v
    Left (Returned Nothing) -> Left (Bug (NoReturn name))
    Left (Halt f) -> Left f

-- | Run a helper on its arguments, with no store at all.
evalHelper :: Module -> Text -> [Value] -> Either Fault Value
evalHelper m name vals = do
  fn <- either (Left . Bug) Right (function m name)
  let Closure _ helpers = closure m fn
      env = Env (modSchema m) helpers Helper (Ctx "" "") M.empty M.empty M.empty
      st0 = St (S.empty (modSchema m)) []
  case fst (runState (runExceptT (call env fn vals)) st0) of
    Right v -> Right v
    Left (Returned _) -> Left (Bug (NoReturn name))
    Left (Halt f) -> Left f

function :: Module -> Text -> Either EvalError Function
function m name = maybe (Left (UnknownFunction name)) Right (lookupFunction m name)

-- §6.3 Statements -----------------------------------------------------

block :: Env -> Block -> Run Env
block env [] = pure env
block env (s : rest) = exec env s >>= \env' -> block env' rest

exec :: Env -> Stmt -> Run Env
exec env = \case
  SLet x e -> do
    v <- eval env e
    pure (bind x v env)
  SIf c yes no -> do
    b <- eval env c >>= bool
    _ <- block env (if b then yes else no)
    pure env
  SFor x xs body -> do
    vs <- eval env xs >>= list
    mapM_ (\v -> block (bind x v env) body) vs
    pure env
  SPut t e -> do
    mutating env
    row <- eval env e >>= struct
    st <- lift (gets stStore)
    case S.put st t row of
      Left r -> verdict r
      Right (st', ch) -> record st' ch >> pure env
  SDelete t ks -> do
    mutating env
    key <- mapM (eval env) ks
    st <- lift (gets stStore)
    case S.delete st t key of
      Left r -> verdict r
      Right (st', ch) -> record st' ch >> pure env
  SRefuse e -> do
    unless (envKind env == Mutator) (bug (Impure "refuse outside a mutator"))
    t <- eval env e >>= text
    verdict (Refused t)
  SReturn me -> do
    v <- traverse (eval env) me
    throwError (Returned v)
  where
    record :: Store -> Maybe Change -> Run ()
    record st' ch = lift (modify' (\s -> s {stStore = st', stChanges = maybe id (:) ch (stChanges s)}))

mutating :: Env -> Run ()
mutating env = unless (envKind env == Mutator) (bug (Impure "write outside a mutator"))

reading :: Env -> Run ()
reading env = unless (envKind env /= Helper) (bug (Impure "read inside a helper"))

bind :: Sym -> Value -> Env -> Env
bind x v env = env {envLocals = M.insert x v (envLocals env)}

-- §6.4 Expressions ----------------------------------------------------

eval :: Env -> Expr -> Run Value
eval env = \case
  ELit v -> pure v
  EArg a -> maybe (bug (MissingArg a)) pure (M.lookup a (envArgs env))
  EAuto a -> maybe (bug (MissingAuto a)) pure (M.lookup a (envAutos env))
  EVar x -> maybe (bug (UnboundVar x)) pure (M.lookup x (envLocals env))
  ECtxUser -> pure (VText (ctxUser (envCtx env)))
  ECtxSession -> pure (VText (ctxSession (envCtx env)))
  EField e f -> do
    m <- eval env e >>= struct
    maybe (bug (NoSuchField f)) pure (M.lookup f m)
  -- Fields are evaluated in field-name order, which is the map's order.
  EStruct fs -> VStruct <$> traverse (eval env) fs
  EList es -> VList <$> mapM (eval env) es
  -- An option is flat: @Some v@ is @v@ and @None@ is 'VNull'. The verifier
  -- forbids an option of an option, which is what makes this unambiguous.
  ESome e -> eval env e
  ENone _ -> pure VNull
  EMatch e x some none -> do
    v <- eval env e
    if isNull v then eval env none else eval (bind x v env) some
  EIf c a b -> do
    t <- eval env c >>= bool
    eval env (if t then a else b)
  EOp And [a, b] -> do
    x <- eval env a >>= bool
    if x then eval env b else pure (VBool False)
  EOp Or [a, b] -> do
    x <- eval env a >>= bool
    if x then pure (VBool True) else eval env b
  EOp Not [a] -> VBool . not <$> (eval env a >>= bool)
  EOp Neg [a] -> do
    n <- eval env a >>= int
    if n == minBound then overflow else pure (VInt (negate n))
  EOp op [a, b] | op `elem` [Add, Sub, Mul, Div, Mod] -> do
    x <- eval env a >>= int
    y <- eval env b >>= int
    either (verdict . Refused) (pure . VInt) (arith op x y)
  EOp op _ -> bug (Arity (T.pack (show op)))
  ECmp op a b -> do
    x <- eval env a
    y <- eval env b
    pure (VBool (cmp op x y))
  ECall name es -> do
    vals <- mapM (eval env) es
    fn <- case [h | h <- envHelpers env, fnName h == name] of
      (h : _) -> pure h
      [] -> bug (UnknownFunction name)
    unless (fnKind fn == Helper) (bug (WrongKind name (fnKind fn)))
    call env fn vals
  EStd f es -> do
    vals <- mapM (eval env) es
    case Std.std f vals of
      Right v -> pure v
      Left (Std.Fault t) -> verdict (Refused t)
      Left (Std.Arity g n) -> bug (Arity (T.pack (show g ++ "/" ++ show n)))
      Left (Std.TypeMismatch g) -> bug (TypeError (T.pack (show g)))
  EMap xs x body -> do
    vs <- eval env xs >>= list
    VList <$> mapM (\v -> eval (bind x v env) body) vs
  EFilter xs x body -> do
    vs <- eval env xs >>= list
    VList <$> filterM' (\v -> eval (bind x v env) body >>= bool) vs
  EAny xs x body -> do
    vs <- eval env xs >>= list
    VBool . or <$> mapM (\v -> eval (bind x v env) body >>= bool) vs
  EAll xs x body -> do
    vs <- eval env xs >>= list
    VBool . and <$> mapM (\v -> eval (bind x v env) body >>= bool) vs
  -- Stable, under 'compareValue' of the key. Stability is part of the
  -- meaning: two rows with equal keys keep their input order.
  ESortBy xs x key -> do
    vs <- eval env xs >>= list
    keyed <- mapM (\v -> (,) v <$> eval (bind x v env) key) vs
    pure (VList (map fst (sortBy (\(_, k) (_, k') -> compareValue k k') keyed)))
  EFold xs z acc x body -> do
    vs <- eval env xs >>= list
    z0 <- eval env z
    foldM' (\a v -> eval (bind acc a (bind x v env)) body) z0 vs
  ESelect p -> reading env >> VList <$> select env p
  EGet t ks -> do
    reading env
    key <- mapM (eval env) ks
    st <- lift (gets stStore)
    pure (maybe VNull VStruct (S.get st t key))
  EExists t ks -> do
    reading env
    key <- mapM (eval env) ks
    st <- lift (gets stStore)
    pure (VBool (S.exists st t key))
  where
    overflow = verdict (Refused "integer overflow")

-- Call a helper: bind its arguments as a fresh environment, run its body,
-- and take what it returned. Helpers never see locals, autos, arguments
-- or the context of their caller.
call :: Env -> Function -> [Value] -> Run Value
call env fn vals = do
  unless (length vals == length (fnArgs fn)) (bug (Arity (fnName fn)))
  let env' = env {envKind = Helper, envArgs = M.fromList (zip (map fst (fnArgs fn)) vals), envAutos = M.empty, envLocals = M.empty}
  (block env' (fnBody fn) >> bug (NoReturn (fnName fn)))
    `catchError` \case
      Returned (Just v) -> pure v
      Returned Nothing -> bug (NoReturn (fnName fn))
      other -> throwError other

-- | §6.5 Checked arithmetic. Every fault here is the same text in every
-- runtime, because it may become a refusal recorded against an entry.
-- Division truncates toward zero and the remainder takes the dividend's
-- sign, which is what @/@ and @%@ do in Rust, Swift and Kotlin alike;
-- @minBound / -1@ and @minBound % -1@ are overflows (Swift traps on the
-- second where Kotlin answers 0, so the spec refuses both).
arith :: Op -> Int64 -> Int64 -> Either Text Int64
arith op x y = case op of
  Add -> ranged (toInteger x + toInteger y)
  Sub -> ranged (toInteger x - toInteger y)
  Mul -> ranged (toInteger x * toInteger y)
  Div
    | y == 0 -> Left "division by zero"
    | x == minBound && y == -1 -> Left "integer overflow"
    | otherwise -> Right (x `quot` y)
  Mod
    | y == 0 -> Left "division by zero"
    | x == minBound && y == -1 -> Left "integer overflow"
    | otherwise -> Right (x `rem` y)
  _ -> Left "not an arithmetic operator"
  where
    ranged n
      | n < toInteger (minBound :: Int64) || n > toInteger (maxBound :: Int64) = Left "integer overflow"
      | otherwise = Right (fromInteger n)

-- | Comparison is the total order, so @NULL = NULL@ is true and @NULL < 0@
-- is true. This is deliberately not SQL's reading: Petros made its Rust
-- filter and its SQL agree by translating @= NULL@ to @IS NULL@, and this
-- design has no SQL to agree with, so one order answers every comparison.
cmp :: CmpOp -> Value -> Value -> Bool
cmp op a b = case op of
  Eq -> o == EQ
  Ne -> o /= EQ
  Lt -> o == LT
  Le -> o /= GT
  Gt -> o == GT
  Ge -> o /= LT
  where
    o = compareValue a b

-- §6.6 Select ----------------------------------------------------------

-- | Pull a plan: scan the table, keep the rows the filter admits, sort
-- them by the order (stably, so equal rows keep key order), take the
-- limit, and hang each relationship's rows beneath as a field of the
-- relationship's name. A child plan runs once per parent with the join
-- column pinned to the parent's key, so a child limit is per parent.
select :: Env -> Plan -> Run [Value]
select env p = do
  st <- lift (gets stStore)
  tbl <- maybe (bug (UnknownTable (pTable p))) pure (lookupTable (envSchema env) (pTable p))
  keep <- maybe (pure (const True)) (predicate env) (pFilter p)
  let admitted = filter keep (S.scan st (pTable p))
      ordered = sortBy (orderBy (pOrder p)) admitted
      taken = maybe ordered (`take` ordered) (fromIntegral <$> pLimit p)
  mapM (attach env tbl (pRelated p)) taken

attach :: Env -> Table -> [Related] -> Row -> Run Value
attach env tbl rels row = do
  pk <- case keyOf tbl row of
    [k] -> pure k
    _ | null rels -> pure VNull
    _ -> bug (CompositeParentKey (tName tbl))
  fields <- mapM (one pk) rels
  pure (VStruct (M.union (M.fromList fields) row))
  where
    one pk r = do
      let pin = PCmp (relColumn (rRelation r)) Eq (ELit pk)
          child = (rPlan r) {pFilter = Just (maybe pin (\f -> PAll [pin, f]) (pFilter (rPlan r)))}
      kids <- select env child
      pure (rName r, VList kids)

-- The right-hand sides of a filter are evaluated once, before the scan;
-- they cannot mention the row.
predicate :: Env -> Pred -> Run (Row -> Bool)
predicate env = \case
  PCmp c op e -> do
    v <- eval env e
    pure (\row -> cmp op (M.findWithDefault VNull c row) v)
  PIn c es -> do
    vs <- mapM (eval env) es
    pure (\row -> any (cmp Eq (M.findWithDefault VNull c row)) vs)
  PAll ps -> do
    fs <- mapM (predicate env) ps
    pure (\row -> all ($ row) fs)
  PAny ps -> do
    fs <- mapM (predicate env) ps
    pure (\row -> any ($ row) fs)
  PNot q -> do
    f <- predicate env q
    pure (not . f)

orderBy :: [(FieldName, Dir)] -> Row -> Row -> Ordering
orderBy cols a b = mconcat [dir d (compareValue (get' c a) (get' c b)) | (c, d) <- cols]
  where
    get' c = M.findWithDefault VNull c
    dir Asc o = o
    dir Desc o = case o of LT -> GT; GT -> LT; EQ -> EQ

-- Coercions: a bug when the verifier's type does not hold ------------

bool :: Value -> Run Bool
bool (VBool b) = pure b
bool v = bug (TypeError (T.pack ("expected Bool, got " ++ take 60 (show v))))

int :: Value -> Run Int64
int (VInt n) = pure n
int v = bug (TypeError (T.pack ("expected Int, got " ++ take 60 (show v))))

text :: Value -> Run Text
text (VText t) = pure t
text v = bug (TypeError (T.pack ("expected Text, got " ++ take 60 (show v))))

list :: Value -> Run [Value]
list (VList xs) = pure xs
list v = bug (TypeError (T.pack ("expected List, got " ++ take 60 (show v))))

struct :: Value -> Run (Map FieldName Value)
struct (VStruct m) = pure m
struct v = bug (TypeError (T.pack ("expected Struct, got " ++ take 60 (show v))))

filterM' :: (a -> Run Bool) -> [a] -> Run [a]
filterM' _ [] = pure []
filterM' f (x : xs) = do
  keep <- f x
  rest <- filterM' f xs
  pure (if keep then x : rest else rest)

foldM' :: (b -> a -> Run b) -> b -> [a] -> Run b
foldM' _ z [] = pure z
foldM' f z (x : xs) = f z x >>= \z' -> foldM' f z' xs

