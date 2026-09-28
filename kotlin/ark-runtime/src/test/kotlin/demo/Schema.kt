package demo

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

class Demo(
    val playlist: Table<Playlist>,
    val item: Table<Item>,
) : Tables

class Playlist(
    val id: Id<Playlist>,
    val name: Text,
    val userId: Text,
) : Row<Key1<Id<Playlist>>> {
    companion object : Row.Of<Playlist> {
        override val NAME = "playlist"
        override fun columns(): Columns<Playlist> = columns<Playlist>()
            .id(id)
            .text(name)
            .text(userId)
            .key(id)
            .unique(userId, name)
        val id = col<Playlist, Id<Playlist>>("id")
        val name = col<Playlist, Text>("name")
        val userId = col<Playlist, Text>("user_id")
        val item = rel<Playlist, Item>("item")
    }
}

class Item(
    val playlistId: Id<Playlist>,
    val trackId: Text,
    val pos: Int,
) : Row<Key2<Id<Playlist>, Text>> {
    companion object : Row.Of<Item> {
        override val NAME = "item"
        override fun columns(): Columns<Item> = columns<Item>()
            .id(playlistId)
            .refs<Playlist>()
            .text(trackId)
            .int(pos)
            .key(playlistId, trackId)
            .unique(playlistId, pos)
        val playlistId = col<Item, Id<Playlist>>("playlist_id")
        val trackId = col<Item, Text>("track_id")
        val pos = col<Item, Int>("pos")
    }
}
