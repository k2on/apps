//! A catalogue imported (`HARKEN_CATALOGUE`, `src/catalogue.rs`), against
//! a little HTTP server standing in for the catalogue's host and the
//! recordings': what it names is downloaded once and served under
//! `/media/.catalogue/`, its library is in the log — the track, its work and
//! its composer with their descriptions, the licence, the credit — and a
//! track whose file cannot be had is not. A rescan, which syncs it again,
//! downloads nothing and authors nothing.

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use common::*;
use harken_server::catalogue::name_of;
use harken_server::domain::{Credit, Media, Person, Recording, Work};

type Shared<T> = Arc<Mutex<T>>;
/// What the host serves, by path.
type Files = Shared<BTreeMap<String, Vec<u8>>>;
/// How many times each path was asked.
type Asked = Shared<BTreeMap<String, usize>>;

/// A file server on an ephemeral port: its address, the files it serves by
/// path (404 for the rest), and how many times each path was asked.
fn host() -> (String, Files, Asked) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (files, asked): (Files, Asked) = (Shared::default(), Shared::default());
    let (served, counts) = (Arc::clone(&files), Arc::clone(&asked));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            *counts.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
            let body = served.lock().unwrap().get(&path).cloned();
            let _ = match body {
                Some(body) => write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .and_then(|_| stream.write_all(&body)),
                None => write!(
                    stream,
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ),
            };
        }
    });
    (base, files, asked)
}

/// The library's rows of a table that `f` picks out.
fn row<T: ark::format::Encode + Send + 'static>(
    server: &harken_server::Server,
    table: &'static str,
    f: impl Fn(&T) -> bool,
) -> Option<T> {
    hub_rows::<T>(server, table).into_iter().find(|r| f(r))
}

/// Falsified by authoring a track whose file did not download: "Gavotte"
/// was a song; by naming a download without its extension: its file had
/// none; by a sync downloading what it already has: the rescan asked for
/// `/air.mp3` again; and by describes that say nothing new still writing:
/// the rescan authored.
#[test]
fn a_catalogue_is_downloaded_once_and_is_the_library() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    std::fs::create_dir_all(media.join("music")).unwrap();

    let (base, files, asked) = host();
    let audio = b"ID3 the air".to_vec();
    let portrait = b"\xFF\xD8\xFF\xE0 Bach".to_vec();
    let catalogue = format!(
        r#"{{
          "version": 1,
          "people": [{{ "name": "Johann Sebastian Bach", "sort_name": "Bach, Johann Sebastian", "born": 1685,
                        "died": 1750, "kinds": ["composer"], "art": "{base}/bach.jpg", "description": "A cantor." }}],
          "works": [{{ "composer": "Johann Sebastian Bach", "catalogue": "BWV 1068", "title": "Orchestral Suite No. 3",
                       "form": "Suite", "key_sig": "D Major", "composed": 1730, "description": "With the Air." }}],
          "credits": [{{ "performer": "An Orchestra", "name": "An Orchestra", "role": "orchestra", "pos": 1 }}],
          "albums": [{{ "name": "Suites", "tracks": [
            {{ "title": "Air", "composer": "Johann Sebastian Bach", "track": 2, "catalogue": "BWV 1068",
               "performer": "An Orchestra", "licence": "CC BY 3.0", "ms": 300000, "file": "{base}/air.mp3" }},
            {{ "title": "Gavotte", "composer": "Johann Sebastian Bach", "track": 3, "catalogue": "BWV 1068",
               "performer": "An Orchestra", "licence": "CC BY 3.0", "ms": 200000, "file": "{base}/gone.mp3" }}
          ] }}]
        }}"#
    );
    {
        let mut f = files.lock().unwrap();
        f.insert("/catalogue.json".into(), catalogue.into_bytes());
        f.insert("/air.mp3".into(), audio.clone());
        f.insert("/bach.jpg".into(), portrait.clone());
    }

    let server = serve(&rt, &data, |c| {
        c.media = Some(media.clone());
        c.catalogue = Some(format!("{base}/catalogue.json"));
    });
    // The credit is the last of the import's mutations (`Catalogue::calls`):
    // once it is in, everything before it is.
    eventually(20, || {
        row::<Credit>(&server, "credit", |c| {
            c.person_name == "An Orchestra" && c.role == "orchestra"
        })
    })
    .expect("the catalogue imported, to its last credit");
    let work = row::<Work>(&server, "work", |w| w.catalogue == "BWV 1068").expect("the work");
    assert_eq!(
        (work.form.as_str(), work.key_sig.as_str(), work.composed),
        ("Suite", "D Major", 1730)
    );
    assert_eq!(work.description, "With the Air.");

    let air = format!(".catalogue/{}", name_of(&format!("{base}/air.mp3")));
    assert!(air.ends_with(".mp3"), "{air}: its type is its extension");
    let songs: Vec<Media> = hub_rows(&server, "media");
    let titles: Vec<(&str, &str)> = songs
        .iter()
        .map(|m| (m.title.as_str(), m.file.as_str()))
        .collect();
    assert_eq!(
        titles,
        [("Air", air.as_str())],
        "the track whose file is here, and not the one whose file is not"
    );

    let bach = row::<Person>(&server, "person", |p| p.name == "Johann Sebastian Bach").unwrap();
    assert_eq!(
        (bach.description.as_str(), bach.born, bach.died),
        ("A cantor.", 1685, 1750)
    );
    assert_eq!(
        bach.art,
        format!(".catalogue/{}", name_of(&format!("{base}/bach.jpg")))
    );
    let take = row::<Recording>(&server, "recording", |r| r.licence == "CC BY 3.0");
    assert!(take.is_some(), "the recording carries its licence");
    let credit = row::<Credit>(&server, "credit", |c| {
        c.person_name == "An Orchestra" && c.role == "orchestra"
    });
    assert!(credit.is_some(), "and its credit");

    // Served where the log says, as a track's file is.
    for (file, bytes) in [(&air, &audio), (&bach.art, &portrait)] {
        let url = harken_server::domain::host::url(&server.url(), file);
        let mut body = vec![];
        std::io::Read::read_to_end(
            &mut ureq::get(&url).call().unwrap().into_reader(),
            &mut body,
        )
        .unwrap();
        assert_eq!(&body, bytes, "{file}");
    }

    // A rescan syncs again: the catalogue asked for, nothing downloaded, and
    // nothing new authored.
    let entries = |s: &harken_server::Server| {
        let all = s.running.hub.with(|h| h.log.log().entries()).unwrap();
        all.iter()
            .filter(|l| l.entry.func.starts_with("library.") && l.entry.func != "library.look")
            .count()
    };
    let before = entries(&server);
    let catalogue_asked = |a: &Asked| {
        a.lock()
            .unwrap()
            .get("/catalogue.json")
            .copied()
            .unwrap_or(0)
    };
    let was = catalogue_asked(&asked);
    server.scanner.as_ref().unwrap().rescan();
    eventually(20, || (catalogue_asked(&asked) > was).then_some(())).expect("the rescan synced");
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    let asked = asked.lock().unwrap().clone();
    assert_eq!(
        asked.get("/air.mp3"),
        Some(&1),
        "downloaded once: {asked:?}"
    );
    assert_eq!(
        asked.get("/bach.jpg"),
        Some(&1),
        "downloaded once: {asked:?}"
    );
    assert_eq!(
        entries(&server),
        before,
        "a sync of nothing new authors nothing"
    );
    rt.block_on(server.stop());
}
