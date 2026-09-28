import ArkAuthoring

public struct Harken {
    public var media: Table<Media>
    public var album: Table<Album>
    public var person: Table<Person>
    public var work: Table<Work>
    public var movement: Table<Movement>
    public var recording: Table<Recording>
    public var credit: Table<Credit>
    public var song: Table<Song>
    public var playlist: Table<Playlist>
    public var playlistItem: Table<PlaylistItem>
}
extension Harken: Tables {
    public static func open() -> Self {
        Harken(
            media: table(), album: table(), person: table(), work: table(), movement: table(), recording: table(),
            credit: table(), song: table(), playlist: table(), playlistItem: table())
    }
}

public struct Media {
    public var id: Id<Media>
    public var kind: Text
    public var title: Text
    public var creator: Text
    public var durationMs: Int
    public var file: Text
    public var pos: Int
    public var addedMs: Int
    public var userId: Text
}
extension Media: Row {
    public static let NAME = "media"
    public typealias Key = Id<Media>
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.id)
            .text(Self.kind)
            .text(Self.title)
            .text(Self.creator)
            .int(Self.durationMs)
            .text(Self.file)
            .int(Self.pos)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.id)
    }
}
extension Media {
    public static let id = col<Media, Id<Media>>("id")
    public static let kind = col<Media, Text>("kind")
    public static let title = col<Media, Text>("title")
    public static let creator = col<Media, Text>("creator")
    public static let durationMs = col<Media, Int>("duration_ms")
    public static let file = col<Media, Text>("file")
    public static let pos = col<Media, Int>("pos")
    public static let addedMs = col<Media, Int>("added_ms")
    public static let userId = col<Media, Text>("user_id")
    public static let song = rel<Media, Song>("song")
    public static let playlistItem = rel<Media, PlaylistItem>("playlist_item")
}

public struct Album {
    public var name: Text
    public var label: Text
    public var released: Int
    public var art: Text
    public var addedMs: Int
    public var userId: Text
}
extension Album: Row {
    public static let NAME = "album"
    public typealias Key = Text
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .text(Self.name)
            .text(Self.label)
            .int(Self.released)
            .text(Self.art)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.name)
    }
}
extension Album {
    public static let name = col<Album, Text>("name")
    public static let label = col<Album, Text>("label")
    public static let released = col<Album, Int>("released")
    public static let art = col<Album, Text>("art")
    public static let addedMs = col<Album, Int>("added_ms")
    public static let userId = col<Album, Text>("user_id")
    public static let song = rel<Album, Song>("song")
}

public struct Person {
    public var name: Text
    public var sortName: Text
    public var born: Int
    public var died: Int
    public var art: Text
    public var addedMs: Int
    public var userId: Text
}
extension Person: Row {
    public static let NAME = "person"
    public typealias Key = Text
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .text(Self.name)
            .text(Self.sortName)
            .int(Self.born)
            .int(Self.died)
            .text(Self.art)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.name)
    }
}
extension Person {
    public static let name = col<Person, Text>("name")
    public static let sortName = col<Person, Text>("sort_name")
    public static let born = col<Person, Int>("born")
    public static let died = col<Person, Int>("died")
    public static let art = col<Person, Text>("art")
    public static let addedMs = col<Person, Int>("added_ms")
    public static let userId = col<Person, Text>("user_id")
    public static let work = rel<Person, Work>("work")
    public static let credit = rel<Person, Credit>("credit")
}

public struct Work {
    public var id: Text
    public var composer: Text
    public var title: Text
    public var catalogue: Text
    public var opus: Text
    public var keySig: Text
    public var form: Text
    public var period: Text
    public var composed: Int
    public var art: Text
    public var addedMs: Int
    public var userId: Text
}
extension Work: Row {
    public static let NAME = "work"
    public typealias Key = Text
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .text(Self.id)
            .text(Self.composer)
            .refs(Person.self)
            .text(Self.title)
            .text(Self.catalogue)
            .text(Self.opus)
            .text(Self.keySig)
            .text(Self.form)
            .text(Self.period)
            .int(Self.composed)
            .text(Self.art)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.id)
    }
}
extension Work {
    public static let id = col<Work, Text>("id")
    public static let composer = col<Work, Text>("composer")
    public static let title = col<Work, Text>("title")
    public static let catalogue = col<Work, Text>("catalogue")
    public static let opus = col<Work, Text>("opus")
    public static let keySig = col<Work, Text>("key_sig")
    public static let form = col<Work, Text>("form")
    public static let period = col<Work, Text>("period")
    public static let composed = col<Work, Int>("composed")
    public static let art = col<Work, Text>("art")
    public static let addedMs = col<Work, Int>("added_ms")
    public static let userId = col<Work, Text>("user_id")
    public static let movement = rel<Work, Movement>("movement")
    public static let recording = rel<Work, Recording>("recording")
}

public struct Movement {
    public var id: Text
    public var workId: Text
    public var no: Int
    public var title: Text
    public var part: Text
    public var addedMs: Int
    public var userId: Text
}
extension Movement: Row {
    public static let NAME = "movement"
    public typealias Key = Text
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .text(Self.id)
            .text(Self.workId)
            .refs(Work.self)
            .int(Self.no)
            .text(Self.title)
            .text(Self.part)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.id)
    }
}
extension Movement {
    public static let id = col<Movement, Text>("id")
    public static let workId = col<Movement, Text>("work_id")
    public static let no = col<Movement, Int>("no")
    public static let title = col<Movement, Text>("title")
    public static let part = col<Movement, Text>("part")
    public static let addedMs = col<Movement, Int>("added_ms")
    public static let userId = col<Movement, Text>("user_id")
    public static let song = rel<Movement, Song>("song")
}

public struct Recording {
    public var id: Text
    public var workId: Opt<Text>
    public var recorded: Int
    public var venue: Text
    public var label: Text
    public var licence: Text
    public var art: Text
    public var addedMs: Int
    public var userId: Text
}
extension Recording: Row {
    public static let NAME = "recording"
    public typealias Key = Text
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .text(Self.id)
            .text(Self.workId)
            .nullable()
            .refs(Work.self)
            .int(Self.recorded)
            .text(Self.venue)
            .text(Self.label)
            .text(Self.licence)
            .text(Self.art)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.id)
    }
}
extension Recording {
    public static let id = col<Recording, Text>("id")
    public static let workId = col<Recording, Opt<Text>>("work_id")
    public static let recorded = col<Recording, Int>("recorded")
    public static let venue = col<Recording, Text>("venue")
    public static let label = col<Recording, Text>("label")
    public static let licence = col<Recording, Text>("licence")
    public static let art = col<Recording, Text>("art")
    public static let addedMs = col<Recording, Int>("added_ms")
    public static let userId = col<Recording, Text>("user_id")
    public static let credit = rel<Recording, Credit>("credit")
    public static let song = rel<Recording, Song>("song")
}

public struct Credit {
    public var recordingId: Text
    public var personName: Text
    public var role: Text
    public var instrument: Text
    public var pos: Int
    public var addedMs: Int
    public var userId: Text
}
extension Credit: Row {
    public static let NAME = "credit"
    public typealias Key = (Text, Text, Text)
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .text(Self.recordingId)
            .refs(Recording.self)
            .text(Self.personName)
            .refs(Person.self)
            .text(Self.role)
            .text(Self.instrument)
            .int(Self.pos)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.recordingId, Self.personName, Self.role)
    }
}
extension Credit {
    public static let recordingId = col<Credit, Text>("recording_id")
    public static let personName = col<Credit, Text>("person_name")
    public static let role = col<Credit, Text>("role")
    public static let instrument = col<Credit, Text>("instrument")
    public static let pos = col<Credit, Int>("pos")
    public static let addedMs = col<Credit, Int>("added_ms")
    public static let userId = col<Credit, Text>("user_id")
}

public struct Song {
    public var mediaId: Id<Media>
    public var albumName: Opt<Text>
    public var disc: Int
    public var track: Int
    public var recordingId: Text
    public var movementId: Opt<Text>
    public var bpm: Int
}
extension Song: Row {
    public static let NAME = "song"
    public typealias Key = Id<Media>
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.mediaId)
            .refs(Media.self)
            .text(Self.albumName)
            .nullable()
            .refs(Album.self)
            .int(Self.disc)
            .int(Self.track)
            .text(Self.recordingId)
            .refs(Recording.self)
            .text(Self.movementId)
            .nullable()
            .refs(Movement.self)
            .int(Self.bpm)
            .key(Self.mediaId)
    }
}
extension Song {
    public static let mediaId = col<Song, Id<Media>>("media_id")
    public static let albumName = col<Song, Opt<Text>>("album_name")
    public static let disc = col<Song, Int>("disc")
    public static let track = col<Song, Int>("track")
    public static let recordingId = col<Song, Text>("recording_id")
    public static let movementId = col<Song, Opt<Text>>("movement_id")
    public static let bpm = col<Song, Int>("bpm")
}

public struct Playlist {
    public var id: Id<Playlist>
    public var name: Text
    public var pos: Int
    public var createdMs: Int
    public var userId: Text
}
extension Playlist: Row {
    public static let NAME = "playlist"
    public typealias Key = Id<Playlist>
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.id)
            .text(Self.name)
            .int(Self.pos)
            .int(Self.createdMs)
            .text(Self.userId)
            .key(Self.id)
            .unique(Self.userId, Self.name)
    }
}
extension Playlist {
    public static let id = col<Playlist, Id<Playlist>>("id")
    public static let name = col<Playlist, Text>("name")
    public static let pos = col<Playlist, Int>("pos")
    public static let createdMs = col<Playlist, Int>("created_ms")
    public static let userId = col<Playlist, Text>("user_id")
    public static let playlistItem = rel<Playlist, PlaylistItem>("playlist_item")
}

public struct PlaylistItem {
    public var playlistId: Id<Playlist>
    public var mediaId: Id<Media>
    public var pos: Int
    public var addedMs: Int
    public var userId: Text
}
extension PlaylistItem: Row {
    public static let NAME = "playlist_item"
    public typealias Key = (Id<Playlist>, Id<Media>)
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.playlistId)
            .refs(Playlist.self)
            .id(Self.mediaId)
            .refs(Media.self)
            .int(Self.pos)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.playlistId, Self.mediaId)
    }
}
extension PlaylistItem {
    public static let playlistId = col<PlaylistItem, Id<Playlist>>("playlist_id")
    public static let mediaId = col<PlaylistItem, Id<Media>>("media_id")
    public static let pos = col<PlaylistItem, Int>("pos")
    public static let addedMs = col<PlaylistItem, Int>("added_ms")
    public static let userId = col<PlaylistItem, Text>("user_id")
}
