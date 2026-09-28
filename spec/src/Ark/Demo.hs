{-# LANGUAGE OverloadedStrings #-}
-- | The demo domain every vector and every runtime's first test is built
-- on: one scope, a playlist and its items, and the three procedures
-- @spec/AUTHORING.md@ Appendix B writes out in the vocabulary. Small enough
-- to read in a minute, and shaped like the real thing: a trimmed and
-- checked input, a fresh id, an existence check, an insert that a second
-- device's duplicate lands on as a no-op, a @MAX(pos) + 1@ read, and a
-- query with an order.
--
-- What is here is the IR those three procedures emit, written as the
-- lowerings of §6 say; the runtimes author the same three in their own
-- languages and hold @emit@ to these bytes.
module Ark.Demo
  ( demoSchema
  , demoRouter
  , createPlaylist
  , addToPlaylist
  , items
  , demoModule
  ) where

import qualified Data.Map.Strict as M

import Ark.IR
import Ark.Schema
import Ark.Value

-- | One scope, @demo@: @playlist(id, name, user_id)@ unique on
-- @(user_id, name)@, and @item(playlist_id -> playlist, track_id, pos)@
-- keyed by @(playlist_id, track_id)@ and unique on @(playlist_id, pos)@.
demoSchema :: Schema
demoSchema =
  Schema
    [ Scope
        "demo"
        [ Table
            "playlist"
            [Column "id" (TId "playlist") False, Column "name" TText False, Column "user_id" TText False]
            ["id"]
            [Index ["user_id", "name"] True]
            []
        , Table
            "item"
            [ Column "playlist_id" (TId "playlist") False
            , Column "track_id" TText False
            , Column "pos" TInt False
            ]
            ["playlist_id", "track_id"]
            [Index ["playlist_id", "pos"] True]
            [Ref "playlist_id" "playlist"]
        ]
    ]

demoRouter :: Router
demoRouter = Router "demo" "demo" []

-- | @create_playlist@: a trimmed, non-empty name; a fresh id; an insert
-- that lands once per @(user_id, name)@.
--
-- > demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
-- >     db.playlist
-- >         .insert(Playlist { id: ctx.new_id("id"), name: input.name, user_id: ctx.user })
-- >         .on((Playlist::user_id, Playlist::name))
-- > })
createPlaylist :: Function
createPlaylist =
  Function
    { fnName = "create_playlist"
    , fnKind = Mutator
    , fnScope = Just "demo"
    , fnRouter = Just "demo"
    , fnUses = []
    , fnAutos = [("id", NewId "playlist")]
    , fnInput = [("name", Field TText [CTrim, CMinLen 1 (Just "a playlist needs a name")])]
    , fnRefine = []
    , fnRet = Nothing
    , fnNames = M.empty
    , fnBody =
        [ SInsert
            "playlist"
            (EStruct (M.fromList [("id", EAuto "id"), ("name", EArg "name"), ("user_id", ECtxUser)]))
            ["user_id", "name"]
        ]
    }

-- | @add_to_playlist@: after everything already on the playlist, which is
-- what makes the rebase visible.
--
-- > demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
-- >     let item = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
-- >     db.item.insert(Item {
-- >         playlist_id: input.playlist_id,
-- >         track_id: input.track_id,
-- >         pos: item.map_or(0, |row| row.pos).add(1),
-- >     })
-- > })
addToPlaylist :: Function
addToPlaylist =
  Function
    { fnName = "add_to_playlist"
    , fnKind = Mutator
    , fnScope = Just "demo"
    , fnRouter = Just "demo"
    , fnUses = []
    , fnAutos = []
    , fnInput =
        [ ("playlist_id", Field (TId "playlist") [CExists Nothing])
        , ("track_id", Field TText [CMinLen 1 Nothing])
        ]
    , fnRefine = []
    , fnRet = Nothing
    , fnNames = M.fromList [(1, "item"), (2, "row")]
    , fnBody =
        [ SLet
            0
            ( ESelect
                Plan
                  { pTable = "item"
                  , pFilter = Just (PCmp "playlist_id" Eq (EArg "playlist_id"))
                  , pOrder = [("pos", Desc)]
                  , pLimit = Just 1
                  , pRelated = []
                  }
            )
        , SLet 1 (EStd First [EVar 0])
        , SInsert
            "item"
            ( EStruct
                ( M.fromList
                    [ ("playlist_id", EArg "playlist_id")
                    , ("track_id", EArg "track_id")
                    , ("pos", EOp Add [EMatch (EVar 1) 2 (EField (EVar 2) "pos") (ELit (VInt 0)), ELit (VInt 1)])
                    ]
                )
            )
            []
        ]
    }

-- | @items@: a playlist's items in position order.
--
-- > demo.input::<Items>().query("items", |_ctx, db, input| {
-- >     db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc()).all()
-- > })
items :: Function
items =
  Function
    { fnName = "items"
    , fnKind = Query
    , fnScope = Just "demo"
    , fnRouter = Just "demo"
    , fnUses = []
    , fnAutos = []
    , fnInput = [("playlist_id", Field (TId "playlist") [])]
    , fnRefine = []
    , fnRet = Just (TList (TStruct (M.fromList [("playlist_id", TId "playlist"), ("track_id", TText), ("pos", TInt)])))
    , fnNames = M.empty
    , fnBody =
        [ SLet
            0
            ( ESelect
                Plan
                  { pTable = "item"
                  , pFilter = Just (PCmp "playlist_id" Eq (EArg "playlist_id"))
                  , pOrder = [("pos", Asc)]
                  , pLimit = Nothing
                  , pRelated = []
                  }
            )
        , SReturn (Just (EVar 0))
        ]
    }

demoModule :: Module
demoModule = Module specVersion demoSchema [createPlaylist, addToPlaylist, items] [demoRouter] []
