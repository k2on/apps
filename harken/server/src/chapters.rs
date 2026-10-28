//! Chapters for an episode its feed gave none, made here from what was
//! heard in it (`transcribe.rs`) — by counting words, with no model.
//!
//! A [`Chapterer`] is one way of making them, and which are tried is the
//! server's configuration (`HARKEN_CHAPTERS`, [`chain`]): each in turn, the
//! first that has any wins. Two are here:
//!
//! - [`Outline`] — the episode's description lists what it talks about
//!   (Full Time Nix's do, as a list), in order, and the transcript says
//!   where: each topic starts where its words are said most, the topics
//!   kept in the description's order.
//! - [`Tiling`] — TextTiling (Hearst, 1997): where the words said in the
//!   minutes before a moment and the minutes after it have least in
//!   common, the subject changed. Each part is titled by the words most its
//!   own.
//!
//! Timed to the transcript, they are timed to the server's copy, as the
//! transcript is.

use std::collections::{HashMap, HashSet};

use crate::domain::{ChapterAt, CueAt};

/// What an episode is, beside what was heard in it.
pub struct About<'a> {
    pub title: &'a str,
    pub description: &'a str,
}

pub trait Chapterer: Send {
    /// What `HARKEN_CHAPTERS` calls it.
    fn name(&self) -> &'static str;

    /// Its chapters, in order and the first at 0; none for an episode it
    /// cannot say anything of.
    fn chapters(&self, about: &About, cues: &[CueAt]) -> Vec<ChapterAt>;
}

/// The chapterers `spec` names, comma-separated and in the order to try
/// them; an unknown name is said and left out, and `none` (or nothing) is
/// none at all.
pub fn chain(spec: &str) -> Vec<Box<dyn Chapterer>> {
    spec.split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty() && *n != "none")
        .filter_map(|n| -> Option<Box<dyn Chapterer>> {
            match n {
                "outline" => Some(Box::new(Outline)),
                "tiling" => Some(Box::new(Tiling::default())),
                other => {
                    eprintln!("harken-server: chapters: no chapterer called {other}");
                    None
                }
            }
        })
        .collect()
}

/// What the first of `chain` to have any says.
pub fn first(chain: &[Box<dyn Chapterer>], about: &About, cues: &[CueAt]) -> Vec<ChapterAt> {
    chain
        .iter()
        .map(|c| c.chapters(about, cues))
        .find(|c| !c.is_empty())
        .unwrap_or_default()
}

fn chapter(start_ms: i64, title: String) -> ChapterAt {
    ChapterAt {
        start_ms,
        title,
        ..ChapterAt::default()
    }
}

/// The words of a line that say what it is about: lower case, three
/// letters or more, and not one every sentence has.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_lowercase)
        .filter(|w| !STOP.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit()))
        // A plural said as its singular, where taking the `s` off leaves
        // a word: `repositories` and `class` stay.
        .map(
            |w| match ["ies", "ss", "us", "is"].iter().any(|e| w.ends_with(e)) {
                true => w,
                false => w
                    .strip_suffix('s')
                    .filter(|s| s.len() >= 3)
                    .map_or(w.clone(), str::to_string),
            },
        )
        .collect()
}

/// What English says in every sentence, and what talk says between them.
const STOP: &[&str] = &[
    "the",
    "and",
    "for",
    "are",
    "but",
    "not",
    "you",
    "all",
    "any",
    "can",
    "had",
    "her",
    "was",
    "one",
    "our",
    "out",
    "has",
    "him",
    "his",
    "how",
    "its",
    "may",
    "new",
    "now",
    "old",
    "see",
    "two",
    "way",
    "who",
    "did",
    "get",
    "got",
    "let",
    "say",
    "she",
    "too",
    "use",
    "that",
    "this",
    "with",
    "have",
    "from",
    "they",
    "will",
    "would",
    "there",
    "their",
    "what",
    "about",
    "which",
    "when",
    "make",
    "like",
    "time",
    "just",
    "know",
    "take",
    "into",
    "year",
    "your",
    "some",
    "could",
    "them",
    "than",
    "then",
    "look",
    "only",
    "come",
    "over",
    "think",
    "also",
    "back",
    "after",
    "work",
    "first",
    "well",
    "even",
    "want",
    "because",
    "these",
    "give",
    "most",
    "very",
    "yeah",
    "okay",
    "really",
    "right",
    "thing",
    "things",
    "going",
    "gonna",
    "kind",
    "sort",
    "mean",
    "actually",
    "something",
    "people",
    "maybe",
    "much",
    "more",
    "were",
    "been",
    "being",
    "here",
    "where",
    "does",
    "doing",
    "done",
    "said",
    "should",
    "other",
    "those",
    "through",
    "while",
    "still",
    "pretty",
    "lot",
    "yes",
    "sure",
    "good",
    "great",
    "stuff",
    "probably",
    "basically",
    "anyway",
    "need",
    "where",
    "every",
    "many",
    "such",
    "same",
    "own",
    "why",
    "let's",
    "it's",
    "that's",
    "don",
    "didn",
    "doesn",
    "isn",
    "wasn",
    "aren",
    "can't",
    "won",
    "able",
    "uh",
    "um",
    "hmm",
    "mhm",
    "yep",
    "nope",
    "blah",
    "guess",
    "talk",
    "talking",
    "said",
    "say",
    "saying",
    "tell",
    "feel",
    "bit",
];

/// Cues gathered into blocks of `span_ms`, each block its words: the
/// unit both chapterers count in.
struct Blocks {
    starts: Vec<i64>,
    words: Vec<Vec<String>>,
}

impl Blocks {
    fn of(cues: &[CueAt], span_ms: i64) -> Blocks {
        let (mut starts, mut words): (Vec<i64>, Vec<Vec<String>>) = (vec![], vec![]);
        for c in cues {
            let at = c.start_ms / span_ms;
            while starts.len() as i64 <= at {
                starts.push(starts.len() as i64 * span_ms);
                words.push(vec![]);
            }
            words[at as usize].extend(self::words(&c.text));
        }
        Blocks { starts, words }
    }

    fn len(&self) -> usize {
        self.starts.len()
    }
}

/// The first cue at or after `ms` that says one of `of`, if one does
/// before `until`; else the first at or after `ms`: where a chapter
/// found by its block starts to the line.
fn snap(cues: &[CueAt], ms: i64, until: i64, of: &HashSet<String>) -> i64 {
    let from = cues
        .iter()
        .filter(|c| c.start_ms >= ms && c.start_ms < until);
    from.clone()
        .find(|c| words(&c.text).iter().any(|w| of.contains(w)))
        .or_else(|| from.clone().next())
        .map_or(ms, |c| c.start_ms)
}

/// The topics a description lists — its `• ` lines, as `feeds::plain`
/// writes a list — and each one's words.
fn topics(description: &str) -> Vec<(String, Vec<String>)> {
    description
        .lines()
        .filter_map(|l| l.trim().strip_prefix("• "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .map(|t| {
            let w = words(&t);
            (t, w)
        })
        .collect()
}

/// Where each of a description's topics is said, in its order.
pub struct Outline;

impl Outline {
    /// The span a topic is looked for in from its start: a topic is
    /// talked about for a while, and its words come back.
    const SPAN_MS: i64 = 30_000;
    const LOOK: usize = 4;
}

impl Chapterer for Outline {
    fn name(&self) -> &'static str {
        "outline"
    }

    fn chapters(&self, about: &About, cues: &[CueAt]) -> Vec<ChapterAt> {
        let topics: Vec<(String, Vec<String>)> = topics(about.description)
            .into_iter()
            .filter(|(_, w)| !w.is_empty())
            .collect();
        let blocks = Blocks::of(cues, Self::SPAN_MS);
        let n = blocks.len();
        if topics.len() < 2 || n < topics.len() {
            return vec![];
        }
        // How rare each word is in the episode: a topic is found by the
        // words of it said in few places, not by the show's own name.
        let mut seen_in: HashMap<&str, usize> = HashMap::new();
        for b in &blocks.words {
            for w in b.iter().map(String::as_str).collect::<HashSet<_>>() {
                *seen_in.entry(w).or_insert(0) += 1;
            }
        }
        let idf = |w: &str| {
            seen_in
                .get(w)
                .map_or(0.0, |d| ((n as f64 + 1.0) / *d as f64).ln())
        };
        // How much of each topic is said from each block on.
        let score: Vec<Vec<f64>> = topics
            .iter()
            .map(|(_, of)| {
                let of: HashSet<&str> = of.iter().map(String::as_str).collect();
                (0..n)
                    .map(|b| {
                        let mut said: HashMap<&str, f64> = HashMap::new();
                        for w in blocks.words[b..(b + Self::LOOK).min(n)].iter().flatten() {
                            if of.contains(w.as_str()) {
                                *said.entry(w.as_str()).or_insert(0.0) += 1.0;
                            }
                        }
                        said.iter().map(|(w, k)| k.min(3.0) * idf(w)).sum()
                    })
                    .collect()
            })
            .collect();
        // The starts, in the description's order, that say the most of
        // their topics: best[t][b] is the most topics 0..=t can say with
        // topic t starting at block b.
        let t_n = topics.len();
        let mut best = vec![vec![f64::NEG_INFINITY; n]; t_n];
        let mut from = vec![vec![0usize; n]; t_n];
        best[0] = score[0].clone();
        for t in 1..t_n {
            let (mut top, mut at) = (f64::NEG_INFINITY, 0);
            for b in 1..n {
                if best[t - 1][b - 1] > top {
                    (top, at) = (best[t - 1][b - 1], b - 1);
                }
                best[t][b] = top + score[t][b];
                from[t][b] = at;
            }
        }
        // The earliest of equals: a topic starts where it is first said.
        let mut b = (0..n).fold(0, |m, x| {
            if best[t_n - 1][x] > best[t_n - 1][m] {
                x
            } else {
                m
            }
        });
        let mut picked = vec![0usize; t_n];
        for t in (0..t_n).rev() {
            picked[t] = b;
            b = from[t][b];
        }
        // Said nowhere, a topic is a guess: most must be found.
        let found = (0..t_n).filter(|&t| score[t][picked[t]] > 0.0).count();
        if found * 2 < t_n {
            return vec![];
        }
        let mut out: Vec<ChapterAt> = vec![];
        for (t, (title, of)) in topics.iter().enumerate() {
            let s = blocks.starts[picked[t]];
            let of: HashSet<String> = of.iter().cloned().collect();
            let at = snap(cues, s, s + Self::LOOK as i64 * Self::SPAN_MS, &of);
            if out.last().is_none_or(|c| c.start_ms < at) {
                out.push(chapter(at, title.clone()));
            }
        }
        // What comes before the first topic: the episode's opening.
        if out.first().is_some_and(|c| c.start_ms > 0) {
            out.insert(0, chapter(0, "Introduction".into()));
        }
        out
    }
}

/// TextTiling over blocks of the transcript.
pub struct Tiling {
    /// The blocks, and how many either side of a gap are compared.
    pub span_ms: i64,
    pub window: usize,
    /// The shortest chapter, and the most an hour has.
    pub shortest_ms: i64,
    pub per_hour: usize,
}

impl Default for Tiling {
    fn default() -> Self {
        Tiling {
            span_ms: 20_000,
            window: 6,
            shortest_ms: 4 * 60_000,
            per_hour: 10,
        }
    }
}

impl Chapterer for Tiling {
    fn name(&self) -> &'static str {
        "tiling"
    }

    fn chapters(&self, _: &About, cues: &[CueAt]) -> Vec<ChapterAt> {
        let blocks = Blocks::of(cues, self.span_ms);
        let n = blocks.len();
        let end = cues.last().map_or(0, |c| c.end_ms.max(c.start_ms));
        if end < 2 * self.shortest_ms || n < 2 * self.window {
            return vec![];
        }
        // How alike the words either side of each gap are.
        let count = |range: std::ops::Range<usize>| -> HashMap<&str, f64> {
            let mut m = HashMap::new();
            for b in range {
                for w in &blocks.words[b] {
                    *m.entry(w.as_str()).or_insert(0.0) += 1.0;
                }
            }
            m
        };
        let cosine = |a: &HashMap<&str, f64>, b: &HashMap<&str, f64>| {
            let dot: f64 = a.iter().filter_map(|(w, x)| b.get(w).map(|y| x * y)).sum();
            let norm = |m: &HashMap<&str, f64>| m.values().map(|x| x * x).sum::<f64>().sqrt();
            match norm(a) * norm(b) {
                d if d > 0.0 => dot / d,
                _ => 0.0,
            }
        };
        let alike: Vec<f64> = (1..n)
            .map(|g| {
                let before = count(g.saturating_sub(self.window)..g);
                let after = count(g..(g + self.window).min(n));
                cosine(&before, &after)
            })
            .collect();
        // How deep each gap's valley is: up to the highest either side
        // before it falls again.
        let depth: Vec<f64> = (0..alike.len())
            .map(|i| {
                let mut left = alike[i];
                for j in (0..i).rev() {
                    if alike[j] < left {
                        break;
                    }
                    left = alike[j];
                }
                let mut right = alike[i];
                for &a in &alike[i + 1..] {
                    if a < right {
                        break;
                    }
                    right = a;
                }
                (left - alike[i]) + (right - alike[i])
            })
            .collect();
        let mean = depth.iter().sum::<f64>() / depth.len() as f64;
        let sd =
            (depth.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / depth.len() as f64).sqrt();
        // The deepest first, each far enough from those taken and from
        // either end, as many as the length allows.
        let mut gaps: Vec<usize> = (0..depth.len())
            .filter(|&i| depth[i] > mean + sd / 2.0)
            .collect();
        gaps.sort_by(|a, b| depth[*b].total_cmp(&depth[*a]));
        let most = ((end as f64 / 3_600_000.0) * self.per_hour as f64).ceil() as usize;
        let mut cuts: Vec<i64> = vec![];
        for g in gaps {
            let at = blocks.starts[g + 1];
            let clear = at >= self.shortest_ms
                && end - at >= self.shortest_ms
                && cuts.iter().all(|c| (c - at).abs() >= self.shortest_ms);
            if clear && cuts.len() < most {
                cuts.push(at);
            }
        }
        if cuts.is_empty() {
            return vec![];
        }
        cuts.sort();
        let starts: Vec<i64> = std::iter::once(0).chain(cuts).collect();
        // Each part titled by its words that are most its own.
        let parts: Vec<Vec<String>> = starts
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let until = starts.get(i + 1).copied().unwrap_or(i64::MAX);
                cues.iter()
                    .filter(|c| c.start_ms >= *s && c.start_ms < until)
                    .flat_map(|c| words(&c.text))
                    .collect()
            })
            .collect();
        let titles = distinctive(&parts);
        starts
            .iter()
            .zip(titles)
            .enumerate()
            .map(|(i, (s, (title, of)))| {
                let until = starts.get(i + 1).copied().unwrap_or(i64::MAX);
                let at = if i == 0 {
                    0
                } else {
                    snap(cues, *s, until, &of)
                };
                chapter(at, title)
            })
            .collect()
    }
}

/// Each part's three words most its own (said most in it, in fewest of
/// the others), as a title, and those words.
fn distinctive(parts: &[Vec<String>]) -> Vec<(String, HashSet<String>)> {
    let mut seen_in: HashMap<&str, usize> = HashMap::new();
    for p in parts {
        for w in p.iter().map(String::as_str).collect::<HashSet<_>>() {
            *seen_in.entry(w).or_insert(0) += 1;
        }
    }
    parts
        .iter()
        .map(|p| {
            let mut tf: HashMap<&str, f64> = HashMap::new();
            for w in p {
                *tf.entry(w.as_str()).or_insert(0.0) += 1.0;
            }
            let idf = |w: &str| ((parts.len() as f64 + 1.0) / (seen_in[w] as f64)).ln();
            let mut ranked: Vec<(&str, f64)> = tf.iter().map(|(w, n)| (*w, n * idf(w))).collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
            let top: Vec<&str> = ranked.iter().take(3).map(|(w, _)| *w).collect();
            let title = top
                .iter()
                .map(|w| capital(w))
                .collect::<Vec<_>>()
                .join(", ");
            (title, top.into_iter().map(str::to_string).collect())
        })
        .collect()
}

fn capital(w: &str) -> String {
    let mut c = w.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line every ten seconds from `from` for `minutes`, each saying
    /// some of `words` and the talk between them.
    fn said(from_ms: i64, minutes: i64, words: &[&str]) -> Vec<CueAt> {
        (0..minutes * 6)
            .map(|i| {
                let w = |k: i64| words[((i + k) as usize) % words.len()];
                CueAt {
                    start_ms: from_ms + i * 10_000,
                    end_ms: from_ms + i * 10_000 + 9_000,
                    text: format!("yeah so the {} and the {}, you know, really", w(0), w(1)),
                    ..CueAt::default()
                }
            })
            .collect()
    }

    fn at(chapters: &[ChapterAt]) -> Vec<(i64, &str)> {
        chapters
            .iter()
            .map(|c| (c.start_ms / 1000, c.title.as_str()))
            .collect()
    }

    const NOTHING: About = About {
        title: "",
        description: "",
    };

    /// Two subjects one after the other are two chapters, cut where the
    /// one gives way to the other, each titled by its own words.
    /// Falsified by cutting at the gap of most alike words.
    #[test]
    fn tiling_cuts_where_the_words_change() {
        let mut cues = said(0, 15, &["kernel", "driver", "module", "compile"]);
        cues.extend(said(
            15 * 60_000,
            15,
            &["garden", "tomato", "soil", "water"],
        ));
        let made = Tiling::default().chapters(&NOTHING, &cues);
        assert_eq!(made.len(), 2, "{:?}", at(&made));
        assert_eq!(made[0].start_ms, 0);
        assert!(
            (made[1].start_ms - 15 * 60_000).abs() <= 20_000,
            "{:?}",
            at(&made)
        );
        assert!(
            made[0].title.contains("Kernel") || made[0].title.contains("Driver"),
            "{}",
            made[0].title
        );
        assert!(
            made[1].title.contains("Garden") || made[1].title.contains("Tomato"),
            "{}",
            made[1].title
        );
        // An episode of one subject, or a short one, has none.
        assert!(Tiling::default()
            .chapters(&NOTHING, &said(0, 30, &["kernel", "driver"]))
            .is_empty());
        assert!(Tiling::default()
            .chapters(&NOTHING, &said(0, 5, &["a"]))
            .is_empty());
    }

    /// A description's list is its chapters, each where its words are
    /// said, in the list's order, after the episode's opening.
    /// Falsified by letting a topic start before the one listed above it.
    #[test]
    fn outline_finds_where_each_listed_topic_is_said() {
        let mut cues = said(0, 1, &["welcome", "episode"]);
        cues.extend(said(60_000, 4, &["flake", "lockfile", "input"]));
        cues.extend(said(5 * 60_000, 5, &["tomato", "garden", "soil"]));
        cues.extend(said(10 * 60_000, 5, &["kernel", "driver", "module"]));
        let about = About {
            title: "Nix Freaks 51",
            description: "Pre-roll transcript:\nNix Freaks\n• Flakes and their lockfiles\n• A tomato garden\n• Kernel drivers",
        };
        let made = Outline.chapters(&about, &cues);
        assert_eq!(
            at(&made),
            [
                (0, "Introduction"),
                (60, "Flakes and their lockfiles"),
                (300, "A tomato garden"),
                (600, "Kernel drivers")
            ]
        );
        // A list said nowhere in it is no chapters, and the next is tried.
        let elsewhere = About {
            description: "• Rust traits\n• Haskell monads",
            ..about
        };
        assert!(Outline.chapters(&elsewhere, &cues).is_empty());
        assert!(Outline.chapters(&NOTHING, &cues).is_empty());
    }

    #[test]
    fn the_chain_is_named_in_order() {
        let names = |spec: &str| chain(spec).iter().map(|c| c.name()).collect::<Vec<_>>();
        assert_eq!(names("outline,tiling"), ["outline", "tiling"]);
        assert_eq!(names(" tiling , nonsense "), ["tiling"]);
        assert!(names("none").is_empty() && names("").is_empty());
    }
}
