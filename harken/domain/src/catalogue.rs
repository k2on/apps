//! The classical catalogue (github.com/k2on/classical): recordings free to
//! download, with their works, people, credits and pictures, as one JSON
//! document. What the demo's library is, and what a server imports.
//!
//! [`Catalogue::calls`] is the whole of what a catalogue means to the
//! library: the mutations, in the order they run — people first (a work's
//! composer and a recording's credits need a `person` row, and
//! `describe_person` is the verb that makes one), then the tracks, then
//! what a track cannot carry: a work's key, form, period, year and
//! description, a recording's licence, and its credits. The keys are
//! `host`'s, the ones `add_song` computes, so a describe never names a
//! work or a recording `add_song` did not make.

use std::collections::{BTreeMap, BTreeSet};

use ark::format::Encode;
use serde::Deserialize;

use crate::host::{recording_key, work_key};
use crate::types::{AddSong, CreditRecording, DescribePerson, DescribeRecording, DescribeWork};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Catalogue {
    pub version: i64,
    pub people: Vec<Person>,
    pub works: Vec<Work>,
    pub credits: Vec<Credit>,
    pub albums: Vec<Album>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Person {
    pub name: String,
    pub sort_name: String,
    pub born: i64,
    pub died: i64,
    /// `composer`, `pianist`, `orchestra`, …: kept, and not yet a column.
    pub kinds: Vec<String>,
    pub art: String,
    pub banner: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Work {
    pub composer: String,
    /// Unique across the catalogue: what a track names its work by.
    pub catalogue: String,
    pub title: String,
    pub form: String,
    pub period: String,
    pub key_sig: String,
    pub composed: i64,
    pub description: String,
}

/// One person on the recordings whose tracks name `performer`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Credit {
    pub performer: String,
    pub name: String,
    pub role: String,
    pub instrument: String,
    pub pos: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Album {
    pub name: String,
    pub art: String,
    /// The year it came out, or its recordings were made; 0 for not known.
    pub year: i64,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Track {
    pub title: String,
    pub composer: String,
    pub part: String,
    pub track: i64,
    pub catalogue: String,
    pub performer: String,
    /// Empty for public domain; otherwise the licence it asks a credit under.
    pub licence: String,
    pub bpm: i64,
    pub ms: i64,
    /// Where to download it.
    pub file: String,
}

/// One mutation: its name, what it is of (for a refusal to say), and its
/// input's bytes.
#[derive(Clone, Debug)]
pub struct Call {
    pub name: &'static str,
    pub what: String,
    pub input: Vec<u8>,
}

impl Call {
    fn of(name: &'static str, what: &str, input: &impl Encode) -> Call {
        Call {
            name,
            what: what.to_string(),
            input: input.to_bytes(),
        }
    }
}

impl Catalogue {
    pub fn parse(json: &str) -> Result<Catalogue, String> {
        serde_json::from_str(json).map_err(|e| format!("the catalogue: {e}"))
    }

    /// Every file and picture it names: what a server downloads.
    pub fn urls(&self) -> BTreeSet<&str> {
        let tracks = self.albums.iter().flat_map(|a| a.tracks.iter().map(|t| t.file.as_str()));
        let albums = self.albums.iter().map(|a| a.art.as_str());
        let people = self.people.iter().flat_map(|p| [p.art.as_str(), p.banner.as_str()]);
        tracks.chain(albums).chain(people).filter(|u| !u.is_empty()).collect()
    }

    /// The mutations that make the library it describes, in order. `local`
    /// says what a row holds for a URL — the URL itself, or where a server
    /// keeps its copy — and `None` for one it has not got: a track without
    /// its file is left out, and a picture without one is no picture.
    pub fn calls(&self, local: impl Fn(&str) -> Option<String>) -> Vec<Call> {
        let picture = |url: &str| match url.is_empty() {
            true => String::new(),
            false => local(url).unwrap_or_default(),
        };
        let mut calls = Vec::new();
        for p in &self.people {
            let person = DescribePerson {
                name: p.name.clone(),
                sort_name: p.sort_name.clone(),
                born: p.born,
                died: p.died,
                art: picture(&p.art),
                banner: picture(&p.banner),
                description: p.description.clone(),
            };
            calls.push(Call::of("library.describe_person", &p.name, &person));
        }
        let work_of = |catalogue: &str| self.works.iter().find(|w| !catalogue.is_empty() && w.catalogue == catalogue);
        let art_of = |name: &str| self.people.iter().find(|p| p.name == name).map(|p| picture(&p.art)).unwrap_or_default();

        // Which recordings there are falls out of the tracks, as it does
        // for `add_song`: one work as played by one set of people.
        let mut takes: BTreeMap<String, (&str, &str)> = BTreeMap::new();
        let mut work_ids: BTreeMap<&str, String> = BTreeMap::new();
        for album in &self.albums {
            for t in &album.tracks {
                let Some(file) = local(&t.file) else { continue };
                // The work's own name where the catalogue has one, and the
                // album's where it does not.
                let work = work_of(&t.catalogue);
                let work_title = work.map_or(album.name.clone(), |w| w.title.clone());
                let song = AddSong {
                    title: t.title.clone(),
                    artist: t.composer.clone(),
                    album: album.name.clone(),
                    duration_ms: t.ms,
                    file,
                    track: t.track,
                    part: t.part.clone(),
                    catalogue: t.catalogue.clone(),
                    performer: t.performer.clone(),
                    bpm: t.bpm,
                    album_art: picture(&album.art),
                    artist_art: art_of(&t.composer),
                    disc: 1,
                    work_title: work_title.clone(),
                    movement_no: t.track,
                    artist_banner: String::new(),
                    released: album.year,
                };
                calls.push(Call::of("library.add_song", &t.title, &song));
                let work_id = work_key(&t.composer, &t.catalogue, &work_title);
                let who = match t.performer.is_empty() {
                    true => t.composer.as_str(),
                    false => t.performer.as_str(),
                };
                takes.insert(recording_key(&work_id, who), (t.performer.as_str(), t.licence.as_str()));
                work_ids.insert(t.catalogue.as_str(), work_id);
            }
        }
        for w in &self.works {
            // A work the catalogue describes and none of whose tracks are
            // here is not a work in this library: `describe_work` refuses it.
            let Some(id) = work_ids.get(w.catalogue.as_str()) else { continue };
            let work = DescribeWork {
                id: id.clone(),
                key_sig: w.key_sig.clone(),
                form: w.form.clone(),
                period: w.period.clone(),
                composed: w.composed,
                description: w.description.clone(),
                ..DescribeWork::default()
            };
            calls.push(Call::of("library.describe_work", &w.title, &work));
        }
        for (id, (performer, licence)) in &takes {
            if !licence.is_empty() {
                let recording = DescribeRecording {
                    id: id.clone(),
                    licence: licence.to_string(),
                    ..DescribeRecording::default()
                };
                calls.push(Call::of("library.describe_recording", licence, &recording));
            }
            for c in self.credits.iter().filter(|c| c.performer == *performer) {
                let credit = CreditRecording {
                    recording_id: id.clone(),
                    person_name: c.name.clone(),
                    role: c.role.clone(),
                    instrument: c.instrument.clone(),
                    pos: c.pos,
                };
                calls.push(Call::of("library.credit_recording", &c.name, &credit));
            }
        }
        calls
    }
}
