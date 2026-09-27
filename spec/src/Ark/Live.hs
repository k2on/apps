{-# LANGUAGE OverloadedStrings #-}
-- | §14 Live rooms: what is true now.
--
-- The second channel on the one socket. The log is permanent and totally
-- ordered; a room is one current value, held in the authority's memory,
-- overwritten rather than appended; a frame is now. Nothing replays a
-- frame, so none of the log's compatibility rules bind one, and nothing
-- about a replica's view is touched by one — a position report a second
-- must never re-hydrate a maintained view, which is why a peer takes
-- @Heard@ before any rebase logic runs.
--
-- The engine's part is small and this module is all of it: which
-- connections are in which room, what the app's machine is told when one
-- arrives, speaks or leaves, and the one row a room may ask to keep when
-- it empties. What a frame /means/ is the app's ('Machine'), and the
-- engine never looks inside one.
--
-- A room is one account: every connection the authority verified as the
-- same user. A second @Hello@ from a connection that is already exactly
-- this peer is the log paging, not a departure and an arrival — the
-- machine hears nothing ('arrive' is idempotent per connection). A
-- @Hello@ as somebody else on the same connection is a change of rooms.
module Ark.Live
  ( Room
  , ConnId
  , Peer (..)
  , Machine (..)
  , Post (..)
  , Rooms (..)
  , emptyRooms
  , arrive
  , speak
  , depart
  , roomOf
  ) where

import qualified Data.ByteString as B
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as M
import Data.Text (Text)

type Room = Text

-- | A connection, as the transport numbers them. Never reused within one
-- authority's life.
type ConnId = Int

-- | One connection's standing in a room: which connection, which room,
-- and who it is within the room (the login's session id — "one login on
-- one device", or whatever else stably names one participant).
data Peer = Peer
  { pConn :: ConnId
  , pRoom :: Room
  , pWho :: Text
  }
  deriving (Eq, Show)

-- | What an app's machine hands back after hearing something: frames to
-- deliver, each to one connection, and whether the room's state is worth
-- a row when it empties. Asked for rather than automatic, because a
-- position report is worth nothing a second later and "the sound is on
-- the kitchen speaker at this point in this queue" is worth a disk write.
data Post = Post
  { postOut :: [(ConnId, B.ByteString)]
  , postKeep :: Bool
  }
  deriving (Eq, Show)

-- | The app's machine for one room, over opaque frames. Pure: it takes
-- what happened and says what follows. @s@ is its state.
data Machine s = Machine
  { mJoin :: [Peer] -> Peer -> s -> (s, Post)
  , mSay :: [Peer] -> Peer -> B.ByteString -> s -> (s, Post)
  , mPart :: [Peer] -> Peer -> s -> (s, Post)
  , -- | What to write when the room empties; 'Nothing' for nothing.
    mSnapshot :: s -> Maybe B.ByteString
  , -- | A fresh room, or one woken from what was kept.
    mWake :: Maybe B.ByteString -> s
  }

-- | Every room open on an authority, and the kept snapshot of the ones
-- that are not.
data Rooms s = Rooms
  { rOpen :: Map Room (s, [Peer])
  , rKept :: Map Room B.ByteString
  , rByConn :: Map ConnId Peer
  }

emptyRooms :: Rooms s
emptyRooms = Rooms M.empty M.empty M.empty

roomOf :: Rooms s -> ConnId -> Maybe Peer
roomOf rs c = M.lookup c (rByConn rs)

-- | §14.1 A connection joins a room, as this participant. Opening a room
-- wakes the machine from whatever was kept for it. A connection already
-- standing as exactly this peer is left where it is and the machine hears
-- nothing; one standing elsewhere departs there first.
arrive :: Machine s -> Rooms s -> Peer -> (Rooms s, Post)
arrive m rs p = case M.lookup (pConn p) (rByConn rs) of
  Just here | here == p -> (rs, Post [] False)
  Just _ -> let (rs', _) = depart m rs (pConn p) in join rs'
  Nothing -> join rs
  where
    join rs0 =
      let (s0, peers) = case M.lookup (pRoom p) (rOpen rs0) of
            Just open -> open
            Nothing -> (mWake m (M.lookup (pRoom p) (rKept rs0)), [])
          peers' = peers ++ [p]
          (s1, post) = mJoin m peers' p s0
       in ( rs0
              { rOpen = M.insert (pRoom p) (s1, peers') (rOpen rs0)
              , rByConn = M.insert (pConn p) p (rByConn rs0)
              , rKept = M.delete (pRoom p) (rKept rs0)
              }
          , post
          )

-- | §14.2 A connection speaks. A connection in no room is ignored: a
-- frame is dropped, never queued, because "pause" is not worth delivering
-- on Friday.
speak :: Machine s -> Rooms s -> ConnId -> B.ByteString -> (Rooms s, Post)
speak m rs c frame = case M.lookup c (rByConn rs) of
  Nothing -> (rs, Post [] False)
  Just p -> case M.lookup (pRoom p) (rOpen rs) of
    Nothing -> (rs, Post [] False)
    Just (s, peers) ->
      let (s', post) = mSay m peers p frame s
       in (rs {rOpen = M.insert (pRoom p) (s', peers) (rOpen rs)}, post)

-- | §14.3 A connection is gone. The machine is told; if it was the last
-- one in the room, the room's snapshot is kept (if the machine offers one)
-- and the room is dropped from memory, which is what bounds an authority
-- with many accounts.
depart :: Machine s -> Rooms s -> ConnId -> (Rooms s, Post)
depart m rs c = case M.lookup c (rByConn rs) of
  Nothing -> (rs, Post [] False)
  Just p -> case M.lookup (pRoom p) (rOpen rs) of
    Nothing -> (rs {rByConn = M.delete c (rByConn rs)}, Post [] False)
    Just (s, peers) ->
      let peers' = filter ((/= c) . pConn) peers
          (s', post) = mPart m peers' p s
          rs' = rs {rByConn = M.delete c (rByConn rs)}
       in if null peers'
            then
              ( rs'
                  { rOpen = M.delete (pRoom p) (rOpen rs')
                  , rKept = maybe (M.delete (pRoom p) (rKept rs')) (\b -> M.insert (pRoom p) b (rKept rs')) (mSnapshot m s')
                  }
              , post
              )
            else (rs' {rOpen = M.insert (pRoom p) (s', peers') (rOpen rs')}, post)
