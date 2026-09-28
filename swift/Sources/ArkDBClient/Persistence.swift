import Foundation
import ArkDB

/// What is durable about the log on this device: the confirmed store at
/// its cursor and the intents still pending, as one canonical-CBOR record
/// written whole (temp file, then rename). Nothing optimistic is written:
/// the view is `replay(confirmed) then replay(pending)` on open.
///
///     { t: "replica", mode: "server" | "alone", cursor, confirmed: { table: [row…] }, pending: [entry…] }
public enum ReplicaFile {
    public struct Contents {
        /// `"server"` for a replica of an authority elsewhere, `"alone"` for
        /// one that is its own authority. A directory keeps the mode it was
        /// opened with: the sequences mean different things in the two.
        public var mode: String
        public var cursor: Seq
        public var confirmed: MemoryStore
        public var pending: [Entry]
    }

    /// The one file under a session's directory.
    public static let fileName = "log.replica"

    public static func encode(_ c: Contents) -> [UInt8] {
        return Canon.encode(.record([
            "t": .text("replica"),
            "mode": .text(c.mode),
            "cursor": .int(c.cursor),
            "confirmed": c.confirmed.asValue(),
            "pending": .list(c.pending.map(Wire.entryValue)),
        ]))
    }

    public static func decode(_ bytes: [UInt8], schema: Schema) throws -> Contents {
        let v = try Canon.decode(bytes)
        guard case .record(let m) = v, m["t"] == .text("replica") else { throw SessionError.corrupt("not a replica file") }
        guard case .text(let mode)? = m["mode"], case .int(let cursor)? = m["cursor"],
              case .record(let tables)? = m["confirmed"], case .list(let pend)? = m["pending"] else {
            throw SessionError.corrupt("replica file is missing a field")
        }
        let st = MemoryStore(schema: schema)
        for (t, rows) in tables {
            guard case .list(let rs) = rows else { throw SessionError.corrupt("rows of \(t) are not a list") }
            for r in rs {
                guard case .record(let row) = r else { throw SessionError.corrupt("a row of \(t) is not a struct") }
                st.applyChange(.add(t, row))
            }
        }
        let entries = try pend.map { try Wire.entryFromValue($0) }
        return Contents(mode: mode, cursor: cursor, confirmed: st, pending: entries)
    }

    /// Write whole: to a temporary file beside the target, then rename over
    /// it, so a crash leaves either the old file or the new one.
    public static func write(_ bytes: [UInt8], to url: URL) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let tmp = url.deletingLastPathComponent().appendingPathComponent("." + url.lastPathComponent + ".tmp")
        try Data(bytes).write(to: tmp)
        // rename(2) replaces the target atomically on every platform this
        // runs on; FileManager's move refuses an existing destination.
        if rename(tmp.path, url.path) != 0 {
            let err = errno
            try? fm.removeItem(at: tmp)
            throw SessionError.io("rename \(url.lastPathComponent): errno \(err)")
        }
    }

    public static func read(_ url: URL) throws -> [UInt8]? {
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        return [UInt8](try Data(contentsOf: url))
    }
}
