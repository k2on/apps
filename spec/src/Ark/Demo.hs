{-# LANGUAGE OverloadedStrings #-}
-- | The demo domain every vector and every runtime's first test is built
-- on: one scope, a playlist and its items, keyed as harken's are, and the
-- two mutators the design document works through. Small enough to read in
-- a minute, and shaped like the real thing: a fresh id, a trimmed name, a
-- refusal, an existence check, a @MAX(pos) + 1@ read, a put with a
-- reference.
module Ark.Demo
  ( demoSchema
  , createPlaylist
  , addToPlaylist
  , demoModule
  ) where

import qualified Data.Map.Strict as M

import Ark.IR
import Ark.Schema
import Ark.Value

-- The demo schema: one scope, a playlist and its items, keyed as harken's.
demoSchema :: Schema
demoSchema =
  Schema
    [ Scope
        "playlists"
        [ Table
            "playlist"
            [Column "id" (TId "playlist") False, Column "name" TText False, Column "user_id" TText False]
            ["id"]
            []
            []
        , Table
            "playlist_item"
            [ Column "playlist_id" (TId "playlist") False
            , Column "media_id" TBytes False
            , Column "pos" TInt False
            , Column "added_ms" TInt False
            , Column "user_id" TText False
            ]
            ["playlist_id", "media_id"]
            []
            [Ref "playlist_id" "playlist"]
        ]
    ]

-- | @add_to_playlist@, as a builder would emit it: the worked example of
-- docs/arkdb.md §3.4, in IR. Symbols are written as the builder numbered
-- them; the verifier renumbers.
addToPlaylist :: Function
addToPlaylist =
  Function
    { fnName = "add_to_playlist"
    , fnKind = Mutator
    , fnScope = Just "playlists"
    , fnAutos = [("added_ms", Now)]
    , fnArgs = [("playlist_id", TId "playlist"), ("media_id", TBytes)]
    , fnRet = Nothing
    , fnNames = M.fromList [(0, "rows"), (1, "r"), (2, "last")]
    , fnBody =
        [ SLet 10 (EExists "playlist" [EArg "playlist_id"])
        , SIf (EOp Not [EVar 10]) [SReturn Nothing] []
        , SLet 11 (EExists "playlist_item" [EArg "playlist_id", EArg "media_id"])
        , SIf (EVar 11) [SReturn Nothing] []
        , SLet
            0
            ( ESelect
                Plan
                  { pTable = "playlist_item"
                  , pFilter = Just (PCmp "playlist_id" Eq (EArg "playlist_id"))
                  , pOrder = [("pos", Desc)]
                  , pLimit = Just 1
                  , pRelated = []
                  }
            )
        , SLet 2 (EStd UnwrapOr [EMatch (EStd First [EVar 0]) 1 (ESome (EField (EVar 1) "pos")) (ENone TInt), ELit (VInt 0)])
        , SPut
            "playlist_item"
            ( EStruct
                ( M.fromList
                    [ ("playlist_id", EArg "playlist_id")
                    , ("media_id", EArg "media_id")
                    , ("pos", EOp Add [EVar 2, ELit (VInt 1)])
                    , ("added_ms", EAuto "added_ms")
                    , ("user_id", ECtxUser)
                    ]
                )
            )
        ]
    }

-- | @create_playlist@: the mutator whose auto is a fresh id.
createPlaylist :: Function
createPlaylist =
  Function
    { fnName = "create_playlist"
    , fnKind = Mutator
    , fnScope = Just "playlists"
    , fnAutos = [("id", NewId "playlist")]
    , fnArgs = [("name", TText)]
    , fnRet = Nothing
    , fnNames = M.empty
    , fnBody =
        [ SIf (EStd IsEmpty [EStd Trim [EArg "name"]]) [SRefuse (ELit (VText "a playlist needs a name"))] []
        , SLet 0 (EExists "playlist" [EAuto "id"])
        , SIf (EVar 0) [SReturn Nothing] []
        , SPut "playlist" (EStruct (M.fromList [("id", EAuto "id"), ("name", EStd Trim [EArg "name"]), ("user_id", ECtxUser)]))
        ]
    }

demoModule :: Module
demoModule = Module specVersion demoSchema [createPlaylist, addToPlaylist] []

