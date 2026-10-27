//! Captions read into timed lines: WebVTT, which the transcriber writes
//! (`transcribe.rs`), and SubRip, which reads the same way.

use crate::domain::CueAt;

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
            said(&captions(srt)),
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
            said(&captions(vtt)),
            [
                (1_000, 4_500, "Shahar", "Hello & welcome."),
                (5_000, 7_000, "Jacek Galowicz", "Hi!")
            ]
        );
    }
}
