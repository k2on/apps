# Files in ArkDB: bytes beside the log, synced like rows

Status: **building**.

## Why

A receipt photo, a playlist's art, a scan: an app's rows want to say "this
picture", and today each app makes its own way (harken's `data/art`, a
call carrying a photo as base64 that is gone after it is read). What is
wanted is one way, ArkDB's, that works as rows do:

- **offline first**: a photo taken, or a playlist's art set, with no
  connection is there at once on that device, its row pending like any
  intent; both reach the server when the device is next online;
- **no cap**: a file is sent in chunks, and a transfer cut off goes on
  from where it was;
- **cached as opened**, as Zero does: a device holds a file once it has
  shown it (or made it), never the whole server's;
- **seen as its rows are**: whoever may read a row that names a file may
  read the file, and nobody else;
- **the server's jobs read them**: a receipt photo arriving is OCR'd by an
  effect job on the server, and every photo is kept.

## The idea: a row names a file by its hash

```rust
#[derive(Describe, Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct FileRef { pub hash: String, pub size: i64, pub media: String }
```

`ark::file::FileRef` is an ordinary record column — the hex sha256 of the
bytes, how many there are and their media type — so every layer that
already carries a record (the schema, its hash, JSON, the Swift and Kotlin
generators, the UniFFI twins) carries it with no change. **The bytes never
enter the log**: not an entry, a fact, a snapshot or a recording. A
mutator only ever sees the reference, so replay and the trace player are
untouched; the bytes go beside the log, as `Call`/`Reply` do.

Content addressing makes everything else simple: a file is immutable, two
rows naming the same picture name one file, a device knows a cached copy
is right by its name, and a transfer resumes by offset.

## Where bytes live: `ark_peer::files::Store`

Whole files by hash, files part-sent, and the files **made here and not
yet sent**: `<hash>` whole, `<hash>.part` arriving, `<hash>.unsent`
marking one this device made. A whole one is renamed from a part once its
sha256 is checked, so it is whole or not there.

- `Store::Dir`: a directory. A device's is `files/` beside its disk
  (`Disk::files`), so a phone's is under its app directory and iced's
  under its data directory; a server's is `data/log/files`.
- `Store::Memory`: a simulated device's and, for now, the web build's (a
  browser keeps a page's files until it closes; IndexedDB is later).

A part nothing was written to for `files::STALE` (three days) is dropped
as the store is opened, and on a server once an hour as uploads begin: an
upload given up, a download let go, a put cut off.

## The API

```rust
peer.put_file(reader, "video/mp4")?          // -> FileRef
peer.file(&file_ref)?                        // -> Option<Box<dyn Bytes>>
```

`put_file` takes any `Read` — a `File`, a `&[u8]` — streams it into the
store and hashes it as it goes, once. `file` gives a reader of the bytes
(`Read + Seek`: read it in order, seek to a range, `io::copy` it to a path
or a writer) when they are here whole; `Ok(None)` when they are not, and
they are wanted; `Err(why)` when the server said why not. Natively neither
ever holds a file whole; in a browser the store is memory, behind the same
calls. Over UniFFI, `put_file(bytes)` and `put_file_at(path)`, and
`file(hash, size, media, at, n)`: at most `n` bytes from `at`.

## The frames

No machine's, like `Call`: intercepted before routing, never recorded.

```text
Offer  { hash, size }                 device: I made this; want it?
Want   { hash, at }                   send me its bytes from `at`
Chunk  { hash, at, size, bytes }      `bytes` of it from `at`, of `size`
Have   { hash }                       server: it is whole here
Absent { hash, why }                  server: not for you, or not here yet
```

Each chunk is at most `files::CHUNK` (1 MiB, well under a socket's frame
limit), and each is asked for by a `Want` — pull, like a fill's parts.
Whoever takes a file keeps `files::WINDOW` (8) chunks asked ahead of what
it holds, so a transfer goes at the link's speed rather than a chunk a
round trip, and neither end holds more than a window of a file in its
outbox. The frames are as they were: an older peer answers each `Want` it
is sent, and asks one at a time itself.

**Up** (a file this device made). `Peer::put_file(reader, media)` writes
the file to the store, marks it unsent and returns its `FileRef` for the
app's mutation, which is pending as any intent is. On every connection,
and at once when linked, each unsent file is `Offer`ed; the server answers
with a window of `Want{at}` from what it holds of it already (a cut-off
upload goes on, a file it has whole is `Have` at once); each `Chunk` is
written and the window moved on; the last is checked against its hash and
the file is `Have`. A file larger than the server's `Config::max_file`
(16 GiB unless `<APP>_MAX_FILE_MB` says) is `Absent`, and none of it kept.
`Have` takes the mark off. The rows go as they always do, beside the
upload, not behind it: a large file never holds back a device's sync.

**Down** (a file a row names). `Peer::file(&FileRef)` gives the bytes when
the store holds them; when it does not, it sends `Want{at: partial}` (once)
and gives none, and the app draws a placeholder. The first `Chunk` says
how big the file is, and the rest are asked a window at a time; the last
is checked and the views are rung (`Pumped::shown`), so the app asks again
and draws it. A file asked for while the link is down is asked for on the
next connection.

**Not here yet.** A row can reach the server before its file does (it was
made offline and the upload is still going). A `Want` for a file the
server does not hold yet is answered `Absent{why: "not here yet"}` and
remembered; when the file is whole the server sends `Have` to everyone who
wanted it, and they want it again.

**Who may read.** The server answers a `Want` when the connection may read
a row that names the hash: for a connection fed the log whole, any row of
the store; for one fed by a scope, a row its scope holds (`Peer::reads`).
Rows are searched by their type: each table's columns that are, or hold,
a `FileRef`. Anyone signed in may upload; an upload is checked against its
hash, so nobody can put other bytes under a file's name.

## The server's jobs

A job that reads a file is an effect handler (`Peer::handle`) on a worker
the server runs in process (`Handle::<Authority>::worker`), whose files
are the server's: `p.file(&photo)` is then a read of the disk. A mutator that inserts a row naming a file
inserts the job beside it (`Job::Pending{due_ms: ctx.now}`); the handler
returns `Err("not here yet")` while the upload is still going, and the
job is retried after its `RETRY_MS`.

## Receipts on it (money)

- `receipt_photo { id, file: FileRef, at, by, state: Job<Read> }` — a
  photo kept forever, its OCR a job of the server's (`receipt-ocr`).
- `receipts.photo(file, transaction?)` inserts it pending; the job's
  handler runs `receipt-ocr` on the bytes and writes the receipt it read
  (sure, settled or to check) with the photo it came from.
- The `stores.read_receipt` call goes.

## Harken's art on it

A playlist's `art: Option<FileRef>`: set from a phone or the web offline,
drawn at once from the device's store, uploaded on the next connection,
fetched by every other device the first time it shows the playlist.

## Later

- The web build's store in IndexedDB, so a page's unsent files survive a
  reload.
- Evicting a device's least-recently opened files past a size.
- An index of file hashes by table for the authorisation check, instead of
  a scan of the rows.
- The server forgetting a file no row has named for a while (never, for
  receipt photos).
