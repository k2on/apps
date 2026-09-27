{-# LANGUAGE OverloadedStrings #-}
-- | §15 The simulation.
--
-- A seeded fleet: one server, any number of clients, and a network that
-- reorders, duplicates and drops, with no socket, no thread and no clock.
-- A three-week partition is a few 'step's. It exists because an app has
-- exactly the engine's question — does my domain converge when two peers
-- go dark and come back — and because the @rebase/@ vectors are its
-- transcripts: the same script, run by any conformant runtime, must reach
-- the same hashes.
--
-- What it holds, after 'settle': every client's confirmed state hashes
-- equal to the server's for every scope, and nothing pending anywhere.
-- Petros's test suite found three vacuous tests by falsifying this kind of
-- claim; the emitter that writes these vectors asserts it before writing.
module Ark.Sim
  ( Sim (..)
  , newSim
  , simMutate
  , partition
  , heal
  , step
  , settle
  , clientHashes
  , serverHashes
  , quiet
  , lcg
  ) where

import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Word (Word64)

import Data.Text (Text)
import qualified Data.Text as T

import Ark.Eval (Args, Ctx (..))
import Ark.Hash (Closure, FnHash, stateHash)
import qualified Ark.Live as L
import Ark.Log (Seq, headSeq)
import Ark.Peer
import Ark.Protocol
import Ark.Schema (Schema, ScopeName)
import qualified Ark.Store as S
import Ark.Value

data Sim = Sim
  { simServer :: Server ()
  , simClients :: Map Int Client
  , -- | The connection each client currently has, if linked.
    simConn :: Map Int L.ConnId
  , simNextConn :: L.ConnId
  , -- | Frames in flight, oldest first.
    simToServer :: Map Int [ClientMsg]
  , simToClient :: Map Int [ServerMsg]
  , simSeed :: Word64
  }

-- | A machine that does nothing: the simulation is about the log.
silent :: L.Machine ()
silent = L.Machine (\_ _ s -> (s, L.Post [] False)) (\_ _ _ s -> (s, L.Post [] False)) (\_ _ s -> (s, L.Post [] False)) (const Nothing) (const ())

-- | A fleet: a trusting server hosting the given scopes, and @n@ clients
-- each holding every scope whole, all connected.
newSim :: Schema -> Map FnHash Closure -> [ScopeName] -> Int -> Word64 -> Sim
newSim sch bodies scopes n seed = foldl heal sim0 [0 .. n - 1]
  where
    server = foldl (\sv s -> host sv (authority sch s bodies)) (openServer trusting (\_ _ -> True) silent) scopes
    client i =
      foldl
        (\c s -> subscribe c Whole (open sch s bodies (S.empty sch) 0 []))
        (openClient sch (Just (name i)))
        scopes
    sim0 =
      Sim
        { simServer = server
        , simClients = M.fromList [(i, client i) | i <- [0 .. n - 1]]
        , simConn = M.empty
        , simNextConn = 1
        , simToServer = M.empty
        , simToClient = M.empty
        , simSeed = seed
        }

name :: Int -> Text
name i = T.pack ("peer-" ++ show i)

-- | A client authors an intent. A refusal by its own view is dropped, as
-- it would be in an app.
simMutate :: Sim -> Int -> ScopeName -> IdBytes -> FnHash -> Args -> Args -> Sim
simMutate sim i s eid fh autos args = case M.lookup i (simClients sim) of
  Nothing -> sim
  Just c -> case clientMutate c s eid (ctxOf i) fh autos args of
    Left _ -> sim
    Right (c', _) -> flushClient i (sim {simClients = M.insert i c' (simClients sim)})

-- Under dev auth the server names every login "dev" ('trusting'), and an
-- entry is held to the login that pushed it, so this is what a client
-- authors under.
ctxOf :: Int -> Ctx
ctxOf i = Ctx (name i) "dev"

-- | A client goes dark: its connection closes, frames in flight are lost.
partition :: Sim -> Int -> Sim
partition sim i = case M.lookup i (simConn sim) of
  Nothing -> sim
  Just conn ->
    sim
      { simServer = disconnect (simServer sim) conn
      , simClients = M.adjust disconnected i (simClients sim)
      , simConn = M.delete i (simConn sim)
      , simToServer = M.delete i (simToServer sim)
      , simToClient = M.delete i (simToClient sim)
      }
      `flushServer'` ()

-- | A client comes back on a fresh connection and says hello.
heal :: Sim -> Int -> Sim
heal sim i
  | M.member i (simConn sim) = sim
  | otherwise =
      let conn = simNextConn sim
          sim' = sim {simConn = M.insert i conn (simConn sim), simNextConn = conn + 1, simClients = M.adjust connected i (simClients sim)}
       in flushClient i sim'

-- Move what a client queued onto the wire.
flushClient :: Int -> Sim -> Sim
flushClient i sim = case M.lookup i (simClients sim) of
  Nothing -> sim
  Just c ->
    let (out, c') = takeOutgoing c
     in sim {simClients = M.insert i c' (simClients sim), simToServer = M.insertWith (flip (++)) i out (simToServer sim)}

-- Move what the server queued onto the wire, to whichever client each
-- connection belongs to; a frame for a connection nobody has is lost.
flushServer' :: Sim -> () -> Sim
flushServer' sim () =
  let (out, sv') = takeServerOutgoing (simServer sim)
      byClient = M.fromList [(conn, i) | (i, conn) <- M.toList (simConn sim)]
      place s (conn, m) = case M.lookup conn byClient of
        Just i -> s {simToClient = M.insertWith (flip (++)) i [m] (simToClient s)}
        Nothing -> s
   in foldl place sim {simServer = sv'} out

-- | §15.1 One delivery: a random frame in flight, from a random direction,
-- with a one-in-eight chance of arriving twice and one in sixteen of not
-- arriving at all.
step :: Sim -> Sim
step sim0 =
  let (r, sim) = roll sim0
      candidates = [Left i | (i, ms) <- M.toList (simToServer sim), not (null ms)] ++ [Right i | (i, ms) <- M.toList (simToClient sim), not (null ms)]
   in if null candidates
        then sim
        else
          let pick = candidates !! fromIntegral (r `mod` fromIntegral (length candidates))
              (r2, sim2) = roll sim
              fate = r2 `mod` 16 -- 0: drop; 1,2: duplicate; else once
              times = if fate == 0 then 0 else if fate <= 2 then 2 else 1 :: Int
           in case pick of
                Left i -> case M.findWithDefault [] i (simToServer sim2) of
                  m : rest -> iterateN times (deliverToServer i m) sim2 {simToServer = M.insert i rest (simToServer sim2)}
                  [] -> sim2
                Right i -> case M.findWithDefault [] i (simToClient sim2) of
                  m : rest -> iterateN times (deliverToClient i m) sim2 {simToClient = M.insert i rest (simToClient sim2)}
                  [] -> sim2

iterateN :: Int -> (a -> a) -> a -> a
iterateN 0 _ x = x
iterateN n f x = iterateN (n - 1) f (f x)

deliverToServer :: Int -> ClientMsg -> Sim -> Sim
deliverToServer i m sim = case M.lookup i (simConn sim) of
  Nothing -> sim
  Just conn -> flushServer' sim {simServer = serverRecv (simServer sim) conn m} ()

deliverToClient :: Int -> ServerMsg -> Sim -> Sim
deliverToClient i m sim = case M.lookup i (simClients sim) of
  Nothing -> sim
  Just c -> flushClient i sim {simClients = M.insert i (clientRecv c m) (simClients sim)}

-- | §15.2 Reconnect everyone — every client, on a fresh connection, so
-- that a push or a page lost on the old one is said again — and deliver
-- everything, perfectly, until nothing is in flight and nothing is
-- pending. Bounded, so a fleet that cannot converge is a failure rather
-- than a hang.
settle :: Sim -> Sim
settle sim0 = go (10000 :: Int) (foldl heal (foldl partition sim0 peers) peers)
  where
    peers = M.keys (simClients sim0)
    go 0 _ = error "settle: the fleet did not converge in 10000 rounds"
    go n sim
      | quiet sim = sim
      | otherwise = go (n - 1) (drain sim)
    drain sim =
      let s1 = foldl (\s (i, ms) -> foldl (\s' m -> deliverToServer i m s') s {simToServer = M.insert i [] (simToServer s)} ms) sim (M.toList (simToServer sim))
       in foldl (\s (i, ms) -> foldl (\s' m -> deliverToClient i m s') s {simToClient = M.insert i [] (simToClient s)} ms) s1 (M.toList (simToClient s1))

-- | Nothing in flight and nothing pending.
quiet :: Sim -> Bool
quiet sim =
  all null (M.elems (simToServer sim))
    && all null (M.elems (simToClient sim))
    && and [null (rPending r) | c <- M.elems (simClients sim), (r, _) <- M.elems (clScopes c)]

-- | Each client's confirmed hash per scope.
clientHashes :: Sim -> [(Int, ScopeName, Seq, B.ByteString)]
clientHashes sim = [(i, s, fst (verifyAt r), snd (verifyAt r)) | (i, c) <- M.toList (simClients sim), (s, (r, _)) <- M.toList (clScopes c)]

-- | The server's hash per scope, at the head.
serverHashes :: Sim -> [(ScopeName, Seq, B.ByteString)]
serverHashes sim = [(s, headOf a, stateHash (aStore a)) | (s, a) <- M.toList (svScopes (simServer sim))]
  where
    headOf a = headSeq (aLog a)

roll :: Sim -> (Word64, Sim)
roll sim = let s' = lcg (simSeed sim) in (s' `shiftR` 11, sim {simSeed = s'})

-- | Knuth's MMIX constants. The whole of the simulation's randomness, so
-- that a seed is a run.
lcg :: Word64 -> Word64
lcg s = s * 6364136223846793005 + 1442695040888963407
