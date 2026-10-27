//! An episode's transcript as its feed links it (Podcasting 2.0's
//! `<podcast:transcript url type>`), read into timed lines: SubRip and
//! WebVTT captions, and Podcast Index's JSON. A transcript with no times
//! — plain text, HTML — cannot follow the audio, and is not read.

use crate::domain::CueAt;

/// How good a transcript of this type is to read, best highest; none for
/// one with no times.
pub fn rank(kind: &str) -> Option<u8> {
    match kind.split(';').next().unwrap_or("").trim() {
        "application/json" => Some(3),
        "text/vtt" => Some(2),
        "application/x-subrip" | "application/srt" | "text/srt" => Some(1),
        _ => None,
    }
}

/// A transcript's lines, as its type says to read it.
pub fn read(kind: &str, text: &str) -> Vec<CueAt> {
    match rank(kind) {
        Some(3) => json(text),
        Some(_) => captions(text),
        None => vec![],
    }
}

/// SubRip or WebVTT: blocks apart by a blank line, each a line of
/// `start --> end` and the lines said. WebVTT's header, notes and styles
/// have no `-->`, and are passed over; its `<v Speaker>` names who says it.
pub fn captions(text: &str) -> Vec<CueAt> {
    let text = text.replace("\r\n", "\n");
    let mut out = vec![];
    for block in text.split("\n\n") {
        let mut lines = block.lines().skip_while(|l| !l.contains("-->"));
        let Some(times) = lines.next() else { continue };
        let (Some(start), Some(end)) = times.split_once("-->").map_or((None, None), |(a, b)| {
            (time(a), time(b.split_whitespace().next().unwrap_or("")))
        }) else {
            continue;
        };
        let mut speaker = String::new();
        let said: Vec<String> = lines
            .map(|l| {
                if let Some(v) = l.trim().strip_prefix("<v").and_then(|v| v.split_once('>')) {
                    speaker =
                        v.0.trim_start_matches(|c: char| c == '.' || c.is_alphanumeric())
                            .trim()
                            .to_string();
                }
                untagged(l)
            })
            .filter(|l| !l.is_empty())
            .collect();
        if said.is_empty() {
            continue;
        }
        out.push(CueAt {
            start_ms: start,
            end_ms: end,
            speaker,
            text: said.join(" "),
        });
    }
    out.sort_by_key(|c| c.start_ms);
    out
}

/// `HH:MM:SS,mmm`, `HH:MM:SS.mmm` or `MM:SS.mmm`, as milliseconds.
fn time(s: &str) -> Option<i64> {
    let s = s.trim().replace(',', ".");
    let (whole, frac) = s.split_once('.').unwrap_or((&s, "0"));
    let mut ms = 0i64;
    for part in whole.split(':') {
        ms = ms * 60 + part.parse::<i64>().ok()?;
    }
    let frac: String = frac.chars().take(3).collect();
    let frac = format!("{frac:0<3}").parse::<i64>().ok()?;
    Some(ms * 1000 + frac)
}

/// A caption line without its markup: `<i>`, `<v Ada>`, `<00:01.000>`.
fn untagged(line: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in line.chars() {
        match c {
            '<' => inside = true,
            '>' if inside => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .trim()
        .to_string()
}

/// The longest line made of a JSON transcript's segments, which are often
/// a word each: a line ends at a sentence's end, a new speaker or a pause.
const LINE: usize = 200;
const PAUSE_MS: i64 = 2_000;

/// Podcast Index's JSON: `{"segments": [{"speaker", "startTime",
/// "endTime", "body"}]}`, its segments put together into lines.
pub fn json(text: &str) -> Vec<CueAt> {
    #[derive(serde::Deserialize)]
    struct File {
        #[serde(default)]
        segments: Vec<Segment>,
    }
    #[derive(serde::Deserialize)]
    struct Segment {
        #[serde(default)]
        speaker: String,
        #[serde(rename = "startTime", default)]
        start: f64,
        #[serde(rename = "endTime", default)]
        end: f64,
        #[serde(default)]
        body: String,
    }
    let Ok(file) = serde_json::from_str::<File>(text) else {
        return vec![];
    };
    let mut out: Vec<CueAt> = vec![];
    let mut open = false;
    for s in file.segments {
        let body = s.body.trim();
        if body.is_empty() {
            continue;
        }
        let (start, end) = ((s.start * 1000.0) as i64, (s.end * 1000.0) as i64);
        let speaker = s.speaker.trim();
        match out.last_mut() {
            Some(last)
                if open
                    && last.speaker == speaker
                    && start - last.end_ms < PAUSE_MS
                    && last.text.len() < LINE =>
            {
                last.text.push(' ');
                last.text.push_str(body);
                last.end_ms = end.max(last.end_ms);
            }
            _ => out.push(CueAt {
                start_ms: start,
                end_ms: end,
                speaker: speaker.to_string(),
                text: body.to_string(),
            }),
        }
        open = !body.ends_with(['.', '?', '!']);
    }
    out.sort_by_key(|c| c.start_ms);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(cues: &[CueAt]) -> Vec<(i64, i64, &str, &str)> {
        cues.iter()
            .map(|c| (c.start_ms, c.end_ms, c.speaker.as_str(), c.text.as_str()))
            .collect()
    }

    /// Full Time Nix's, as Transistor writes it.
    #[test]
    fn subrip_is_read() {
        let srt = "1\r\n00:00:03,096 --> 00:00:06,476\r\nHello and welcome to Full Time Nix.\r\n\r\n2\r\n00:00:06,736 --> 00:00:18,520\r\nThis is an episode about release notes of\r\nNix 2.23.0 with Jacek Galowicz.\r\n\r\n3\r\n00:00:18,520 --> 01:00:19,612\r\nHi, <i>Jacek</i>.\r\n";
        assert_eq!(
            said(&read("application/x-subrip", srt)),
            [
                (3_096, 6_476, "", "Hello and welcome to Full Time Nix."),
                (
                    6_736,
                    18_520,
                    "",
                    "This is an episode about release notes of Nix 2.23.0 with Jacek Galowicz."
                ),
                (18_520, 3_619_612, "", "Hi, Jacek."),
            ]
        );
    }

    #[test]
    fn webvtt_is_read_with_its_speakers() {
        let vtt = "WEBVTT\n\nNOTE made by hand\n\nintro\n00:01.000 --> 00:04.5 align:start\n<v Shahar>Hello &amp; welcome.</v>\n\n00:00:05.000 --> 00:00:07.000\n<v.loud Jacek Galowicz>Hi!\n";
        assert_eq!(
            said(&read("text/vtt; charset=utf-8", vtt)),
            [
                (1_000, 4_500, "Shahar", "Hello & welcome."),
                (5_000, 7_000, "Jacek Galowicz", "Hi!")
            ]
        );
    }

    #[test]
    fn json_words_become_lines() {
        let json = r#"{"version":"1.0.0","segments":[
            {"speaker":"Ada","startTime":0.0,"endTime":0.4,"body":"Hello"},
            {"speaker":"Ada","startTime":0.4,"endTime":0.9,"body":"there."},
            {"speaker":"Ada","startTime":1.0,"endTime":1.5,"body":"Again"},
            {"speaker":"Bo","startTime":1.6,"endTime":2.0,"body":"Hi"},
            {"speaker":"Bo","startTime":9.0,"endTime":9.5,"body":"later"}
        ]}"#;
        assert_eq!(
            said(&read("application/json", json)),
            [
                (0, 900, "Ada", "Hello there."),
                (1_000, 1_500, "Ada", "Again"),
                (1_600, 2_000, "Bo", "Hi"),
                (9_000, 9_500, "Bo", "later")
            ]
        );
    }

    #[test]
    fn untimed_transcripts_are_not_read() {
        assert_eq!(rank("text/plain"), None);
        assert_eq!(rank("text/html"), None);
        assert!(read("text/plain", "00:00:01,000 --> 00:00:02,000\nhi").is_empty());
        assert!(
            rank("application/json") > rank("text/vtt")
                && rank("text/vtt") > rank("application/x-subrip")
        );
    }
}
