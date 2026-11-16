use ark_app::Runtime;

use super::*;
use crate::testing::Demo;

fn own() -> Playing {
    Playing {
        key: "00".into(),
        title: "Aria".into(),
        under: "Bach".into(),
        at_ms: 5_000,
        since_ms: 1,
        playing: true,
        length_ms: 180_000,
        can_forward: true,
        ..Playing::default()
    }
}

/// Alone, the player draws what this device plays on its own, as the host
/// says it; nothing, no bar. Its presses are the host's.
#[test]
fn alone_the_player_draws_its_own_sound_and_asks_the_host() {
    let mut rt = Runtime::<Player>::new();
    let mut d = Demo::<Player>::new();
    assert!(!d.page(&mut rt).has, "nothing plays: no bar");
    d.run(
        &mut rt,
        [PlayerMsg::Own {
            own: Some(own()),
            queue: vec![],
        }],
    );
    let p = d.page(&mut rt);
    assert!(p.has && !p.joined);
    assert_eq!(
        (p.now.title.as_str(), p.play_glyph.as_str(), p.output.as_str()),
        ("Aria", "pause", "This Device")
    );
    d.run(&mut rt, [PlayerMsg::Toggle, PlayerMsg::Next, PlayerMsg::Seek { at_ms: 9_000 }]);
    assert_eq!(
        d.requests,
        [HarkenRequest::Toggle, HarkenRequest::Skip { by: 1 }, HarkenRequest::Seek { at_ms: 9_000 }]
    );
    d.run(&mut rt, [PlayerMsg::Devices]);
    assert!(d.page(&mut rt).devices_open);
    d.run(&mut rt, [PlayerMsg::CloseDevices]);
    assert!(!d.page(&mut rt).devices_open);
}

/// Alone, the volume is this device's own, as its sound says it; let go,
/// a level is asked of the host, and the loudspeaker asks the other way
/// from what is drawn. Falsified by Mute asking for what already is.
#[test]
fn the_volume_is_asked_of_the_host() {
    let mut rt = Runtime::<Player>::new();
    let mut d = Demo::<Player>::new();
    assert!(!d.page(&mut rt).has_volume, "nothing plays: no volume");
    let quiet = Playing {
        volume: 40,
        muted: true,
        ..own()
    };
    d.run(
        &mut rt,
        [PlayerMsg::Own {
            own: Some(quiet),
            queue: vec![],
        }],
    );
    let p = d.page(&mut rt);
    assert!(p.has_volume);
    assert_eq!((p.now.volume, p.now.muted), (40, true));
    d.run(&mut rt, [PlayerMsg::Volume { level: 70 }, PlayerMsg::Mute]);
    assert_eq!(d.requests, [HarkenRequest::Volume { level: 70 }, HarkenRequest::Mute { muted: false }]);
}

fn row(i: i64, title: &str, at: i64) -> QueueRow {
    QueueRow {
        index: i,
        at: i.to_string(),
        key: format!("{i:02}"),
        title: title.into(),
        now: i == at,
        up: i > at + 1,
        down: i > at && i < 2,
        ..QueueRow::default()
    }
}

/// Alone, the queue is this device's own, as the host says it: what plays,
/// then what comes after, counted; a row played, moved and taken out are
/// the host's presses. Falsified by a move sent the wrong way.
#[test]
fn the_queue_lists_what_plays_and_what_follows() {
    let mut rt = Runtime::<Player>::new();
    let mut d = Demo::<Player>::new();
    let queue = vec![row(0, "Aria", 0), row(1, "Variatio 1", 0), row(2, "Variatio 2", 0)];
    d.run(&mut rt, [PlayerMsg::Own { own: Some(own()), queue }, PlayerMsg::Queue]);
    let p = d.page(&mut rt);
    assert!(p.queue_open && !p.devices_open);
    assert_eq!(p.queue.len(), 3);
    assert!(p.queue[0].now && !p.queue[1].now);
    assert_eq!(p.next, "2 Tracks");
    d.run(
        &mut rt,
        [
            PlayerMsg::Jump { at: "2".into() },
            PlayerMsg::MoveUp { at: "2".into() },
            PlayerMsg::MoveDown { at: "1".into() },
            PlayerMsg::Remove { at: "1".into() },
            PlayerMsg::Jump { at: "x".into() },
        ],
    );
    assert_eq!(
        d.requests,
        [
            HarkenRequest::Jump { index: 2 },
            HarkenRequest::Move { from: 2, to: 1 },
            HarkenRequest::Move { from: 1, to: 2 },
            HarkenRequest::Remove { index: 1 },
        ]
    );
    d.run(&mut rt, [PlayerMsg::CloseQueue]);
    assert!(!d.page(&mut rt).queue_open);
}

/// An episode with a transcript offers it, and its lines say who speaks
/// over the first of theirs in a row, the one said now in the accent;
/// a song offers none. Falsified by every line lit, or none.
#[test]
fn an_episode_offers_its_transcript_with_the_line_said_now_lit() {
    let mut rt = Runtime::<Player>::new();
    let mut d = Demo::<Player>::new();
    // The demo's one episode with a transcript, found by its file among
    // its show's episodes.
    let file = harken_seed::shelf().besides().1[0].file.clone();
    d.hold(vec![harken_domain::HarkenView::Shows]);
    let shows: Vec<harken_domain::ShowsEntry> = crate::words::rows_of(&d.store, &harken_domain::HarkenView::Shows);
    let with = shows
        .iter()
        .find_map(|s| {
            let k = harken_domain::HarkenView::Show {
                playlist: [0; 16],
                id: s.id.clone(),
            };
            d.hold(vec![k.clone()]);
            crate::words::rows_of::<harken_domain::LibraryEntry>(&d.store, &k)
                .into_iter()
                .find(|e| e.file == file)
        })
        .expect("the demo has an episode with a transcript");
    let cues_of = harken_domain::HarkenView::Transcript { media: with.id.0 };
    d.hold(vec![cues_of.clone()]);
    let cues: Vec<harken_domain::TranscriptEntry> = crate::words::rows_of(&d.store, &cues_of);
    let second = cues.get(1).expect("two lines at least").start_ms;
    let playing = Playing {
        key: with.id.hex(),
        title: with.title.clone(),
        at_ms: second,
        playing: false,
        ..own()
    };
    d.run(
        &mut rt,
        [
            PlayerMsg::Own {
                own: Some(playing),
                queue: vec![],
            },
            PlayerMsg::Now { key: with.id.hex() },
            PlayerMsg::Transcript,
        ],
    );
    let p = d.page(&mut rt);
    assert!(p.has_transcript && p.transcript_open, "no transcript");
    assert!(!p.lines.is_empty());
    assert_eq!(p.lines.iter().filter(|l| l.tint == "accent").count(), 1, "one line said now");
    assert_eq!(
        p.lines.iter().position(|l| l.tint == "accent"),
        Some(1),
        "the second line is said at its start"
    );
    assert!(!p.lines[0].speaker.is_empty() || p.lines.iter().all(|l| l.speaker.is_empty()));
    // A song has none.
    let song = d.first_song();
    d.run(
        &mut rt,
        [
            PlayerMsg::Own {
                own: Some(Playing { key: song.clone(), ..own() }),
                queue: vec![],
            },
            PlayerMsg::Now { key: song },
        ],
    );
    let p = d.page(&mut rt);
    assert!(!p.has_transcript && !p.transcript_open);
}
