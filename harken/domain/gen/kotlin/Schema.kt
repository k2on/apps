package harken.gen

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int

class Harken(
    val media: Table<Media>,
    val album: Table<Album>,
    val person: Table<Person>,
    val work: Table<Work>,
    val movement: Table<Movement>,
    val recording: Table<Recording>,
    val credit: Table<Credit>,
    val song: Table<Song>,
    val playlist: Table<Playlist>,
    val playlistItem: Table<PlaylistItem>,
) : Tables

class Media(
    val id: Id<Media>,
    val kind: Text,
    val title: Text,
    val creator: Text,
    val durationMs: Int,
    val file: Text,
    val pos: Int,
    val addedMs: Int,
    val userId: Text,
) : Row<Key1<Id<Media>>> {
    companion object : Row.Of<Media> {
        override val NAME = "media"

        override fun columns(): Columns<Media> =
            columns<Media>()
                .id(id)
                .text(kind)
                .text(title)
                .text(creator)
                .int(durationMs)
                .text(file)
                .int(pos)
                .int(addedMs)
                .text(userId)
                .key(id)

        val id = col<Media, Id<Media>>("id")
        val kind = col<Media, Text>("kind")
        val title = col<Media, Text>("title")
        val creator = col<Media, Text>("creator")
        val durationMs = col<Media, Int>("duration_ms")
        val file = col<Media, Text>("file")
        val pos = col<Media, Int>("pos")
        val addedMs = col<Media, Int>("added_ms")
        val userId = col<Media, Text>("user_id")
        val song = rel<Media, Song>("song")
        val playlistItem = rel<Media, PlaylistItem>("playlist_item")
    }
}

class Album(
    val name: Text,
    val label: Text,
    val released: Int,
    val art: Text,
    val addedMs: Int,
    val userId: Text,
) : Row<Key1<Text>> {
    companion object : Row.Of<Album> {
        override val NAME = "album"

        override fun columns(): Columns<Album> =
            columns<Album>()
                .text(name)
                .text(label)
                .int(released)
                .text(art)
                .int(addedMs)
                .text(userId)
                .key(name)

        val name = col<Album, Text>("name")
        val label = col<Album, Text>("label")
        val released = col<Album, Int>("released")
        val art = col<Album, Text>("art")
        val addedMs = col<Album, Int>("added_ms")
        val userId = col<Album, Text>("user_id")
        val song = rel<Album, Song>("song")
    }
}

class Person(
    val name: Text,
    val sortName: Text,
    val born: Int,
    val died: Int,
    val art: Text,
    val addedMs: Int,
    val userId: Text,
) : Row<Key1<Text>> {
    companion object : Row.Of<Person> {
        override val NAME = "person"

        override fun columns(): Columns<Person> =
            columns<Person>()
                .text(name)
                .text(sortName)
                .int(born)
                .int(died)
                .text(art)
                .int(addedMs)
                .text(userId)
                .key(name)

        val name = col<Person, Text>("name")
        val sortName = col<Person, Text>("sort_name")
        val born = col<Person, Int>("born")
        val died = col<Person, Int>("died")
        val art = col<Person, Text>("art")
        val addedMs = col<Person, Int>("added_ms")
        val userId = col<Person, Text>("user_id")
        val work = rel<Person, Work>("work")
        val credit = rel<Person, Credit>("credit")
    }
}

class Work(
    val id: Text,
    val composer: Text,
    val title: Text,
    val catalogue: Text,
    val opus: Text,
    val keySig: Text,
    val form: Text,
    val period: Text,
    val composed: Int,
    val art: Text,
    val addedMs: Int,
    val userId: Text,
) : Row<Key1<Text>> {
    companion object : Row.Of<Work> {
        override val NAME = "work"

        override fun columns(): Columns<Work> =
            columns<Work>()
                .text(id)
                .text(composer)
                .refs<Person>()
                .text(title)
                .text(catalogue)
                .text(opus)
                .text(keySig)
                .text(form)
                .text(period)
                .int(composed)
                .text(art)
                .int(addedMs)
                .text(userId)
                .key(id)

        val id = col<Work, Text>("id")
        val composer = col<Work, Text>("composer")
        val title = col<Work, Text>("title")
        val catalogue = col<Work, Text>("catalogue")
        val opus = col<Work, Text>("opus")
        val keySig = col<Work, Text>("key_sig")
        val form = col<Work, Text>("form")
        val period = col<Work, Text>("period")
        val composed = col<Work, Int>("composed")
        val art = col<Work, Text>("art")
        val addedMs = col<Work, Int>("added_ms")
        val userId = col<Work, Text>("user_id")
        val movement = rel<Work, Movement>("movement")
        val recording = rel<Work, Recording>("recording")
    }
}

class Movement(
    val id: Text,
    val workId: Text,
    val no: Int,
    val title: Text,
    val part: Text,
    val addedMs: Int,
    val userId: Text,
) : Row<Key1<Text>> {
    companion object : Row.Of<Movement> {
        override val NAME = "movement"

        override fun columns(): Columns<Movement> =
            columns<Movement>()
                .text(id)
                .text(workId)
                .refs<Work>()
                .int(no)
                .text(title)
                .text(part)
                .int(addedMs)
                .text(userId)
                .key(id)

        val id = col<Movement, Text>("id")
        val workId = col<Movement, Text>("work_id")
        val no = col<Movement, Int>("no")
        val title = col<Movement, Text>("title")
        val part = col<Movement, Text>("part")
        val addedMs = col<Movement, Int>("added_ms")
        val userId = col<Movement, Text>("user_id")
        val song = rel<Movement, Song>("song")
    }
}

class Recording(
    val id: Text,
    val workId: Opt<Text>,
    val recorded: Int,
    val venue: Text,
    val label: Text,
    val licence: Text,
    val art: Text,
    val addedMs: Int,
    val userId: Text,
) : Row<Key1<Text>> {
    companion object : Row.Of<Recording> {
        override val NAME = "recording"

        override fun columns(): Columns<Recording> =
            columns<Recording>()
                .text(id)
                .text(workId)
                .nullable()
                .refs<Work>()
                .int(recorded)
                .text(venue)
                .text(label)
                .text(licence)
                .text(art)
                .int(addedMs)
                .text(userId)
                .key(id)

        val id = col<Recording, Text>("id")
        val workId = col<Recording, Opt<Text>>("work_id")
        val recorded = col<Recording, Int>("recorded")
        val venue = col<Recording, Text>("venue")
        val label = col<Recording, Text>("label")
        val licence = col<Recording, Text>("licence")
        val art = col<Recording, Text>("art")
        val addedMs = col<Recording, Int>("added_ms")
        val userId = col<Recording, Text>("user_id")
        val credit = rel<Recording, Credit>("credit")
        val song = rel<Recording, Song>("song")
    }
}

class Credit(
    val recordingId: Text,
    val personName: Text,
    val role: Text,
    val instrument: Text,
    val pos: Int,
    val addedMs: Int,
    val userId: Text,
) : Row<Key3<Text, Text, Text>> {
    companion object : Row.Of<Credit> {
        override val NAME = "credit"

        override fun columns(): Columns<Credit> =
            columns<Credit>()
                .text(recordingId)
                .refs<Recording>()
                .text(personName)
                .refs<Person>()
                .text(role)
                .text(instrument)
                .int(pos)
                .int(addedMs)
                .text(userId)
                .key(recordingId, personName, role)

        val recordingId = col<Credit, Text>("recording_id")
        val personName = col<Credit, Text>("person_name")
        val role = col<Credit, Text>("role")
        val instrument = col<Credit, Text>("instrument")
        val pos = col<Credit, Int>("pos")
        val addedMs = col<Credit, Int>("added_ms")
        val userId = col<Credit, Text>("user_id")
    }
}

class Song(
    val mediaId: Id<Media>,
    val albumName: Opt<Text>,
    val disc: Int,
    val track: Int,
    val recordingId: Text,
    val movementId: Opt<Text>,
    val bpm: Int,
) : Row<Key1<Id<Media>>> {
    companion object : Row.Of<Song> {
        override val NAME = "song"

        override fun columns(): Columns<Song> =
            columns<Song>()
                .id(mediaId)
                .refs<Media>()
                .text(albumName)
                .nullable()
                .refs<Album>()
                .int(disc)
                .int(track)
                .text(recordingId)
                .refs<Recording>()
                .text(movementId)
                .nullable()
                .refs<Movement>()
                .int(bpm)
                .key(mediaId)

        val mediaId = col<Song, Id<Media>>("media_id")
        val albumName = col<Song, Opt<Text>>("album_name")
        val disc = col<Song, Int>("disc")
        val track = col<Song, Int>("track")
        val recordingId = col<Song, Text>("recording_id")
        val movementId = col<Song, Opt<Text>>("movement_id")
        val bpm = col<Song, Int>("bpm")
    }
}

class Playlist(
    val id: Id<Playlist>,
    val name: Text,
    val pos: Int,
    val createdMs: Int,
    val userId: Text,
) : Row<Key1<Id<Playlist>>> {
    companion object : Row.Of<Playlist> {
        override val NAME = "playlist"

        override fun columns(): Columns<Playlist> =
            columns<Playlist>()
                .id(id)
                .text(name)
                .int(pos)
                .int(createdMs)
                .text(userId)
                .key(id)
                .unique(userId, name)

        val id = col<Playlist, Id<Playlist>>("id")
        val name = col<Playlist, Text>("name")
        val pos = col<Playlist, Int>("pos")
        val createdMs = col<Playlist, Int>("created_ms")
        val userId = col<Playlist, Text>("user_id")
        val playlistItem = rel<Playlist, PlaylistItem>("playlist_item")
    }
}

class PlaylistItem(
    val playlistId: Id<Playlist>,
    val mediaId: Id<Media>,
    val pos: Int,
    val addedMs: Int,
    val userId: Text,
) : Row<Key2<Id<Playlist>, Id<Media>>> {
    companion object : Row.Of<PlaylistItem> {
        override val NAME = "playlist_item"

        override fun columns(): Columns<PlaylistItem> =
            columns<PlaylistItem>()
                .id(playlistId)
                .refs<Playlist>()
                .id(mediaId)
                .refs<Media>()
                .int(pos)
                .int(addedMs)
                .text(userId)
                .key(playlistId, mediaId)

        val playlistId = col<PlaylistItem, Id<Playlist>>("playlist_id")
        val mediaId = col<PlaylistItem, Id<Media>>("media_id")
        val pos = col<PlaylistItem, Int>("pos")
        val addedMs = col<PlaylistItem, Int>("added_ms")
        val userId = col<PlaylistItem, Text>("user_id")
    }
}
