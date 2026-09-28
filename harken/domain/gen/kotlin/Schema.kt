package harken.gen

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int

class Library(
    val track: Table<Track>,
) : Scope {
    companion object : Scope.Of {
        override val NAME = "library"
    }
}

class Playlists(
    val playlist: Table<Playlist>,
    val playlistItem: Table<PlaylistItem>,
) : Scope {
    companion object : Scope.Of {
        override val NAME = "playlists"
    }
}

class Track(
    val id: Id<Track>,
    val title: Text,
    val artist: Text,
    val album: Opt<Text>,
    val durationMs: Int,
    val file: Text,
    val addedMs: Int,
    val userId: Text,
) : Row<Key1<Id<Track>>> {
    companion object : Row.Of<Track> {
        override val NAME = "track"

        override fun columns(): Columns<Track> =
            columns<Track>()
                .id(id)
                .text(title)
                .text(artist)
                .text(album)
                .nullable()
                .int(durationMs)
                .text(file)
                .int(addedMs)
                .text(userId)
                .key(id)
                .unique(file)

        val id = col<Track, Id<Track>>("id")
        val title = col<Track, Text>("title")
        val artist = col<Track, Text>("artist")
        val album = col<Track, Opt<Text>>("album")
        val durationMs = col<Track, Int>("duration_ms")
        val file = col<Track, Text>("file")
        val addedMs = col<Track, Int>("added_ms")
        val userId = col<Track, Text>("user_id")
    }
}

class Playlist(
    val id: Id<Playlist>,
    val name: Text,
    val userId: Text,
    val createdMs: Int,
) : Row<Key1<Id<Playlist>>> {
    companion object : Row.Of<Playlist> {
        override val NAME = "playlist"

        override fun columns(): Columns<Playlist> =
            columns<Playlist>()
                .id(id)
                .text(name)
                .text(userId)
                .int(createdMs)
                .key(id)
                .unique(userId, name)

        val id = col<Playlist, Id<Playlist>>("id")
        val name = col<Playlist, Text>("name")
        val userId = col<Playlist, Text>("user_id")
        val createdMs = col<Playlist, Int>("created_ms")
        val playlistItem = rel<Playlist, PlaylistItem>("playlist_item")
    }
}

class PlaylistItem(
    val playlistId: Id<Playlist>,
    val trackId: Id<Track>,
    val pos: Int,
    val addedMs: Int,
    val userId: Text,
) : Row<Key2<Id<Playlist>, Id<Track>>> {
    companion object : Row.Of<PlaylistItem> {
        override val NAME = "playlist_item"

        override fun columns(): Columns<PlaylistItem> =
            columns<PlaylistItem>()
                .id(playlistId)
                .refs<Playlist>()
                .id(trackId)
                .int(pos)
                .int(addedMs)
                .text(userId)
                .key(playlistId, trackId)

        val playlistId = col<PlaylistItem, Id<Playlist>>("playlist_id")
        val trackId = col<PlaylistItem, Id<Track>>("track_id")
        val pos = col<PlaylistItem, Int>("pos")
        val addedMs = col<PlaylistItem, Int>("added_ms")
        val userId = col<PlaylistItem, Text>("user_id")
    }
}
