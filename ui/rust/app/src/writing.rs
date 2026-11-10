//! Long-form writing, once for every toolkit: headings, paragraphs with
//! links, emphasis and code inline, lists, highlighted code, callouts,
//! quotes, tables and pictures — a help page, a release note, the docs
//! site — as data ([`Block`], [`Inline`]) that each toolkit draws in its
//! own words (`arkui::writing` in iced, `arkui_web::writing` in the DOM).
//!
//! It is written as Markdown ([`parse`]): CommonMark's everyday subset,
//! GitHub's callouts (`> [!NOTE]`) and pipe tables. What it does not read
//! (HTML, reference links, setext headings) is left as the words it is.
//! A view says it with [`writing`] (`Part::Writing`), the Markdown itself
//! the value, so the phones' emitters hand the same text to SwiftUI and
//! Compose.
//!
//! Code is highlighted by [`highlight`]: a small lexer per language
//! (Rust, Swift, Kotlin, the shell, TOML, JSON, Nix, JavaScript, CSS),
//! each token a [`Token`] that a toolkit gives a colour of the theme's.

use crate::view::{Part, Val, View};

/// A block of writing, in the order it is read.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// `level` 1 to 6; `id` is its anchor ([`slug`] of its words).
    Heading {
        level: u8,
        id: String,
        text: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    /// Each item is blocks of its own: a paragraph, then any list in it.
    List {
        ordered: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    },
    /// `lang` as the fence says (`rust`, `sh`), empty when it says none.
    Code {
        lang: String,
        text: String,
    },
    Callout {
        kind: Callout,
        blocks: Vec<Block>,
    },
    Quote(Vec<Block>),
    Table {
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// A picture on a line of its own.
    Image {
        alt: String,
        src: String,
        title: String,
    },
    Rule,
}

/// Words inside a block.
#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Code(String),
    Link {
        href: String,
        text: Vec<Inline>,
    },
    /// A line ended on purpose (two spaces, or `\`, before it).
    Break,
}

/// What a callout says it is (GitHub's five).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callout {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl Callout {
    /// Its title, as the reader sees it.
    pub fn title(self) -> &'static str {
        match self {
            Callout::Note => "Note",
            Callout::Tip => "Tip",
            Callout::Important => "Important",
            Callout::Warning => "Warning",
            Callout::Caution => "Caution",
        }
    }

    /// Its glyph, of ArkUI's shared names.
    pub fn glyph(self) -> &'static str {
        match self {
            Callout::Note => "pencil",
            Callout::Tip => "wand-sparkles",
            Callout::Important => "badge-check",
            Callout::Warning => "triangle-alert",
            Callout::Caution => "zap",
        }
    }

    /// Its word in CSS and in Markdown, lowercase.
    pub fn word(self) -> &'static str {
        match self {
            Callout::Note => "note",
            Callout::Tip => "tip",
            Callout::Important => "important",
            Callout::Warning => "warning",
            Callout::Caution => "caution",
        }
    }

    fn of(word: &str) -> Option<Callout> {
        [Callout::Note, Callout::Tip, Callout::Important, Callout::Warning, Callout::Caution]
            .into_iter()
            .find(|c| c.word().eq_ignore_ascii_case(word))
    }
}

/// Markdown as a view's part: drawn by each toolkit's writing.
pub fn writing<M>(markdown: impl Into<Val<String>>) -> View<M> {
    View::Part(Part::Writing { markdown: markdown.into() })
}

// * Blocks

/// `markdown` read as blocks.
pub fn parse(markdown: &str) -> Vec<Block> {
    let lines: Vec<&str> = markdown.lines().collect();
    blocks(&lines)
}

fn indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn fence(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    ["```", "~~~"].into_iter().find(|f| t.starts_with(*f)).map(|f| (f, t[3..].trim()))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    let rest = &t[n..];
    ((1..=6).contains(&n) && (rest.is_empty() || rest.starts_with(' '))).then(|| (n as u8, rest.trim().trim_end_matches('#').trim_end()))
}

fn rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && ["-", "*", "_"].iter().any(|c| t.chars().all(|x| x.to_string() == *c))
}

/// A list item's marker: whether it is numbered, its number, and how far
/// in its words start.
fn marker(line: &str) -> Option<(bool, u32, usize)> {
    let lead = indent(line);
    let t = &line[lead..];
    let mut chars = t.chars();
    match chars.next()? {
        '-' | '*' | '+' => (t.len() == 1 || t[1..].starts_with(' ')).then(|| (false, 1, lead + 1 + indent(&t[1..]).clamp(1, 4))),
        c if c.is_ascii_digit() => {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            let after = &t[digits..];
            ((after.starts_with(". ") || after.starts_with(") ")) && digits < 10).then(|| {
                (
                    true,
                    t[..digits].parse().unwrap_or(1),
                    lead + digits + 1 + indent(&after[1..]).clamp(1, 4),
                )
            })
        }
        _ => None,
    }
}

fn cells(line: &str) -> Vec<&str> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(str::trim).collect()
}

fn separator(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn starts_block(lines: &[&str], i: usize) -> bool {
    let l = lines[i];
    fence(l).is_some()
        || heading(l).is_some()
        || l.trim_start().starts_with('>')
        || marker(l).is_some()
        || rule(l)
        || (l.trim_start().starts_with('|') && lines.get(i + 1).is_some_and(|n| separator(n)))
}

fn blocks(lines: &[&str]) -> Vec<Block> {
    let mut out = vec![];
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        if let Some((f, lang)) = fence(line) {
            let lead = indent(line);
            let mut text = vec![];
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(f) {
                let l = lines[i];
                text.push(&l[indent(l).min(lead)..]);
                i += 1;
            }
            i += 1;
            out.push(Block::Code {
                lang: lang.split_whitespace().next().unwrap_or("").to_lowercase(),
                text: text.join("\n"),
            });
            continue;
        }
        if let Some((level, words)) = heading(line) {
            let text = inline(words);
            out.push(Block::Heading {
                level,
                id: slug(&plain(&text)),
                text,
            });
            i += 1;
            continue;
        }
        if rule(line) {
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        if line.trim_start().starts_with('>') {
            let mut inner = vec![];
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let t = &lines[i].trim_start()[1..];
                inner.push(t.strip_prefix(' ').unwrap_or(t));
                i += 1;
            }
            let kind = inner
                .first()
                .and_then(|f| f.trim().strip_prefix("[!")?.strip_suffix(']'))
                .and_then(Callout::of);
            out.push(match kind {
                Some(kind) => Block::Callout {
                    kind,
                    blocks: blocks(&inner[1..]),
                },
                None => Block::Quote(blocks(&inner)),
            });
            continue;
        }
        if let Some((ordered, start, _)) = marker(line) {
            let lead = indent(line);
            let mut items = vec![];
            while i < lines.len() {
                let Some((o, _, at)) = marker(lines[i]).filter(|_| indent(lines[i]) == lead) else {
                    break;
                };
                if o != ordered {
                    break;
                }
                let mut item = vec![&lines[i][at.min(lines[i].len())..]];
                i += 1;
                while i < lines.len() {
                    let l = lines[i];
                    if blank(l) {
                        // A blank line, then more of this item only if it is indented under it.
                        if lines.get(i + 1).is_some_and(|n| !blank(n) && indent(n) > lead) {
                            item.push("");
                            i += 1;
                            continue;
                        }
                        break;
                    }
                    if indent(l) > lead {
                        item.push(&l[indent(l).min(at)..]);
                    } else if marker(l).is_none() && !starts_block(lines, i) {
                        item.push(l.trim_start());
                    } else {
                        break;
                    }
                    i += 1;
                }
                items.push(blocks(&item));
                // A blank line between items keeps the list.
                if i < lines.len()
                    && blank(lines[i])
                    && lines
                        .get(i + 1)
                        .is_some_and(|n| marker(n).is_some_and(|m| m.0 == ordered) && indent(n) == lead)
                {
                    i += 1;
                }
            }
            out.push(Block::List { ordered, start, items });
            continue;
        }
        if line.trim_start().starts_with('|') && lines.get(i + 1).is_some_and(|n| separator(n)) {
            let head = cells(line).into_iter().map(inline).collect();
            i += 2;
            let mut rows = vec![];
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                rows.push(cells(lines[i]).into_iter().map(inline).collect());
                i += 1;
            }
            out.push(Block::Table { head, rows });
            continue;
        }
        let mut para = vec![line.trim()];
        let mut hard = vec![line.ends_with("  ") || line.ends_with('\\')];
        i += 1;
        while i < lines.len() && !blank(lines[i]) && !starts_block(lines, i) {
            para.push(lines[i].trim());
            hard.push(lines[i].ends_with("  ") || lines[i].ends_with('\\'));
            i += 1;
        }
        if para.len() == 1 {
            if let Some(img) = image(para[0]) {
                out.push(img);
                continue;
            }
        }
        let mut words = vec![];
        for (n, (l, h)) in para.iter().zip(&hard).enumerate() {
            if n > 0 {
                words.push(if hard[n - 1] { Inline::Break } else { Inline::Text(" ".into()) });
            }
            let l = if *h { l.trim_end_matches('\\').trim_end() } else { l };
            words.extend(inline(l));
        }
        out.push(Block::Paragraph(merge(words)));
    }
    out
}

/// `![alt](src "title")` and nothing else.
fn image(line: &str) -> Option<Block> {
    let rest = line.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let target = rest.strip_suffix(')')?;
    let (src, title) = match target.split_once(" \"") {
        Some((s, t)) => (s, t.trim_end_matches('"')),
        None => (target, ""),
    };
    Some(Block::Image {
        alt: alt.into(),
        src: src.trim().into(),
        title: title.into(),
    })
}

// * Inline

/// One line's words (or a paragraph's, joined) read for emphasis, code
/// and links.
pub fn inline(s: &str) -> Vec<Inline> {
    let chars: Vec<char> = s.chars().collect();
    merge(spans(&chars))
}

fn merge(words: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = vec![];
    for w in words {
        match (out.last_mut(), w) {
            (Some(Inline::Text(a)), Inline::Text(b)) => a.push_str(&b),
            (_, w) => out.push(w),
        }
    }
    out
}

fn word_char(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric())
}

fn spans(c: &[char]) -> Vec<Inline> {
    let mut out = vec![];
    let mut text = String::new();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' && c.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) {
            text.push(c[i + 1]);
            i += 2;
            continue;
        }
        if ch == '`' {
            let run = c[i..].iter().take_while(|x| **x == '`').count();
            let close = (i + run..c.len()).find(|&j| c[j..].iter().take_while(|x| **x == '`').count() == run && (j == 0 || c[j - 1] != '`'));
            if let Some(j) = close {
                let code: String = c[i + run..j].iter().collect();
                let code = match code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() {
                    true => code[1..code.len() - 1].to_string(),
                    false => code,
                };
                flush(&mut text, &mut out);
                out.push(Inline::Code(code));
                i = j + run;
                continue;
            }
            text.extend(&c[i..i + run]);
            i += run;
            continue;
        }
        if ch == '[' {
            if let Some((label, href, end)) = link(c, i) {
                flush(&mut text, &mut out);
                out.push(Inline::Link {
                    href,
                    text: merge(spans(&label)),
                });
                i = end;
                continue;
            }
        }
        if ch == '<' {
            if let Some(end) = c[i..].iter().position(|x| *x == '>') {
                let inner: String = c[i + 1..i + end].iter().collect();
                if (inner.starts_with("https://") || inner.starts_with("http://") || inner.starts_with("mailto:")) && !inner.contains(' ') {
                    flush(&mut text, &mut out);
                    out.push(Inline::Link {
                        text: vec![Inline::Text(inner.trim_start_matches("mailto:").into())],
                        href: inner,
                    });
                    i += end + 1;
                    continue;
                }
            }
        }
        if ch == '*' || ch == '_' {
            let double = c.get(i + 1) == Some(&ch);
            let n = if double { 2 } else { 1 };
            let opens = c.get(i + n).is_some_and(|x| !x.is_whitespace()) && (ch == '*' || !word_char(i.checked_sub(1).and_then(|p| c.get(p))));
            if opens {
                let close = (i + n + 1..=c.len().saturating_sub(n)).find(|&j| {
                    c[j..j + n].iter().all(|x| *x == ch)
                        && !c[j - 1].is_whitespace()
                        && (n == 2 || c.get(j + 1) != Some(&ch))
                        && (ch == '*' || !word_char(c.get(j + n)))
                });
                if let Some(j) = close {
                    flush(&mut text, &mut out);
                    let inner = merge(spans(&c[i + n..j]));
                    out.push(if double { Inline::Strong(inner) } else { Inline::Emphasis(inner) });
                    i = j + n;
                    continue;
                }
            }
        }
        text.push(ch);
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

/// `[label](href)` at `i`: its label, where it goes, and where it ends.
fn link(c: &[char], i: usize) -> Option<(Vec<char>, String, usize)> {
    let mut depth = 0;
    let mut j = i;
    let close = loop {
        match c.get(j)? {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break j;
                }
            }
            '`' => {
                // A bracket inside code is the code's.
                let k = c[j + 1..].iter().position(|x| *x == '`')?;
                j += k + 1;
            }
            _ => {}
        }
        j += 1;
    };
    if c.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = close + 2 + c[close + 2..].iter().position(|x| *x == ')')?;
    let target: String = c[close + 2..end].iter().collect();
    let href = target.split(" \"").next().unwrap_or("").trim().to_string();
    Some((c[i + 1..close].to_vec(), href, end + 1))
}

/// The words alone, for an anchor, a search or a title.
pub fn plain(words: &[Inline]) -> String {
    let mut out = String::new();
    for w in words {
        match w {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Emphasis(i) | Inline::Strong(i) | Inline::Link { text: i, .. } => out.push_str(&plain(i)),
            Inline::Break => out.push(' '),
        }
    }
    out
}

/// Every word of `blocks`, for a search.
pub fn text_of(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        let part = match b {
            Block::Heading { text, .. } | Block::Paragraph(text) => plain(text),
            Block::List { items, .. } => items.iter().map(|i| text_of(i)).collect::<Vec<_>>().join(" "),
            Block::Code { text, .. } => text.clone(),
            Block::Callout { blocks, .. } | Block::Quote(blocks) => text_of(blocks),
            Block::Table { head, rows } => head.iter().chain(rows.iter().flatten()).map(|c| plain(c)).collect::<Vec<_>>().join(" "),
            Block::Image { alt, .. } => alt.clone(),
            Block::Rule => String::new(),
        };
        if !part.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&part);
        }
    }
    out
}

/// `words` as an anchor: lowercase, its words joined by `-`.
pub fn slug(words: &str) -> String {
    let mut out = String::new();
    for c in words.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if (c == ' ' || c == '-' || c == '_') && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

// * Code

/// What a piece of code is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Plain,
    Keyword,
    /// A name that starts with a capital: a type, a case.
    Type,
    /// A name called: `f(`, a macro's `name!`.
    Function,
    String,
    Number,
    Comment,
    /// `#[derive]`, `@State`.
    Attribute,
}

impl Token {
    /// Its word in CSS (`ark-tok-<word>`).
    pub fn word(self) -> &'static str {
        match self {
            Token::Plain => "plain",
            Token::Keyword => "keyword",
            Token::Type => "type",
            Token::Function => "function",
            Token::String => "string",
            Token::Number => "number",
            Token::Comment => "comment",
            Token::Attribute => "attribute",
        }
    }
}

struct Lang {
    keywords: &'static [&'static str],
    /// Comments to the line's end.
    line: &'static [&'static str],
    block: bool,
    /// `'` quotes a string (else Rust's char, or nothing).
    single: bool,
    /// `#[…]` (Rust) or `@Name` (Swift, Kotlin) is an attribute.
    attribute: Option<char>,
}

fn lang(name: &str) -> Option<Lang> {
    const RUST: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
        "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
        "unsafe", "use", "where", "while",
    ];
    const SWIFT: &[&str] = &[
        "actor",
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "continue",
        "default",
        "defer",
        "do",
        "else",
        "enum",
        "extension",
        "false",
        "for",
        "func",
        "guard",
        "if",
        "import",
        "in",
        "init",
        "inout",
        "let",
        "nil",
        "private",
        "protocol",
        "public",
        "return",
        "self",
        "Self",
        "some",
        "static",
        "struct",
        "switch",
        "throw",
        "throws",
        "true",
        "try",
        "var",
        "where",
        "while",
        "any",
    ];
    const KOTLIN: &[&str] = &[
        "as",
        "break",
        "class",
        "companion",
        "continue",
        "data",
        "do",
        "else",
        "enum",
        "false",
        "for",
        "fun",
        "if",
        "import",
        "in",
        "interface",
        "internal",
        "is",
        "null",
        "object",
        "override",
        "package",
        "private",
        "return",
        "sealed",
        "super",
        "this",
        "true",
        "val",
        "var",
        "when",
        "while",
        "suspend",
        "by",
    ];
    const SH: &[&str] = &[
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "case", "esac", "while", "function", "export", "local", "return", "cd",
        "echo", "nix", "cargo", "git",
    ];
    const NIX: &[&str] = &[
        "let", "in", "with", "inherit", "rec", "if", "then", "else", "import", "true", "false", "null",
    ];
    const JS: &[&str] = &[
        "const",
        "let",
        "var",
        "function",
        "return",
        "if",
        "else",
        "for",
        "of",
        "in",
        "while",
        "new",
        "class",
        "import",
        "from",
        "export",
        "async",
        "await",
        "true",
        "false",
        "null",
        "undefined",
        "this",
        "try",
        "catch",
    ];
    const NONE: &[&str] = &["true", "false", "null"];
    let l = |keywords, line, block, single, attribute| Lang {
        keywords,
        line,
        block,
        single,
        attribute,
    };
    Some(match name {
        "rust" | "rs" => l(RUST, &["//"], true, false, Some('#')),
        "swift" => l(SWIFT, &["//"], true, false, Some('@')),
        "kotlin" | "kt" | "kts" => l(KOTLIN, &["//"], true, false, Some('@')),
        "sh" | "bash" | "shell" | "console" | "zsh" => l(SH, &["#"], false, true, None),
        "toml" => l(NONE, &["#"], false, true, None),
        "json" => l(NONE, &[], false, false, None),
        "nix" => l(NIX, &["#"], true, true, None),
        "js" | "javascript" | "ts" | "typescript" => l(JS, &["//"], true, true, None),
        "css" => l(&[], &[], true, true, None),
        _ => return None,
    })
}

/// `code` in `lang` as its pieces, in order, each with what it is; every
/// character of `code` is in exactly one piece. A language it does not
/// know is one plain piece.
pub fn highlight(lang_name: &str, code: &str) -> Vec<(Token, String)> {
    let Some(lang) = lang(lang_name) else {
        return vec![(Token::Plain, code.to_string())];
    };
    let c: Vec<char> = code.chars().collect();
    let mut out: Vec<(Token, String)> = vec![];
    let push = |out: &mut Vec<(Token, String)>, t: Token, s: &[char]| match out.last_mut() {
        Some((last, text)) if *last == t => text.extend(s),
        _ => out.push((t, s.iter().collect())),
    };
    let mut i = 0;
    while i < c.len() {
        let at = |s: &str| s.chars().enumerate().all(|(k, x)| c.get(i + k) == Some(&x));
        // A comment starts a word: `.#docs` is no shell comment.
        let starts = i == 0 || c[i - 1].is_whitespace();
        if lang.line.iter().any(|l| at(l) && (*l != "#" || starts)) {
            let end = c[i..].iter().position(|x| *x == '\n').map_or(c.len(), |p| i + p);
            push(&mut out, Token::Comment, &c[i..end]);
            i = end;
            continue;
        }
        if lang.block && at("/*") {
            let end = (i + 2..c.len().saturating_sub(1))
                .find(|&j| c[j] == '*' && c[j + 1] == '/')
                .map_or(c.len(), |j| j + 2);
            push(&mut out, Token::Comment, &c[i..end]);
            i = end;
            continue;
        }
        let ch = c[i];
        if ch == '"' || (ch == '\'' && lang.single) {
            let mut j = i + 1;
            while j < c.len() && c[j] != ch && c[j] != '\n' {
                j += if c[j] == '\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(c.len());
            push(&mut out, Token::String, &c[i..end]);
            i = end;
            continue;
        }
        // Rust's char: 'a' or '\n', never a lifetime.
        if ch == '\'' && lang.attribute == Some('#') {
            let end = if c.get(i + 1) == Some(&'\\') { i + 4 } else { i + 3 };
            if c.get(end - 1) == Some(&'\'') {
                push(&mut out, Token::String, &c[i..end]);
                i = end;
                continue;
            }
        }
        if lang.attribute == Some('#') && at("#[") || lang.attribute == Some('#') && at("#![") {
            let end = c[i..].iter().position(|x| *x == ']').map_or(c.len(), |p| i + p + 1);
            push(&mut out, Token::Attribute, &c[i..end]);
            i = end;
            continue;
        }
        if lang.attribute == Some('@') && ch == '@' && c.get(i + 1).is_some_and(|x| x.is_alphabetic()) {
            let end = i + 1 + c[i + 1..].iter().take_while(|x| x.is_alphanumeric() || **x == '_').count();
            push(&mut out, Token::Attribute, &c[i..end]);
            i = end;
            continue;
        }
        let before_word = i.checked_sub(1).and_then(|p| c.get(p)).is_some_and(|x| x.is_alphanumeric() || *x == '_');
        if ch.is_ascii_digit() && !before_word {
            let end = i + c[i..]
                .iter()
                .take_while(|x| x.is_ascii_alphanumeric() || **x == '_' || **x == '.')
                .count();
            push(&mut out, Token::Number, &c[i..end]);
            i = end;
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let end = i + c[i..]
                .iter()
                .take_while(|x| x.is_alphanumeric() || **x == '_' || (lang_name == "css" && **x == '-'))
                .count();
            let w: String = c[i..end].iter().collect();
            let next = c.get(end);
            let t = if lang.keywords.contains(&w.as_str()) {
                Token::Keyword
            } else if next == Some(&'(') || (next == Some(&'!') && lang.attribute == Some('#')) {
                Token::Function
            } else if w.chars().next().is_some_and(char::is_uppercase) {
                Token::Type
            } else {
                Token::Plain
            };
            let end = if t == Token::Function && next == Some(&'!') { end + 1 } else { end };
            push(&mut out, t, &c[i..end]);
            i = end;
            continue;
        }
        push(&mut out, Token::Plain, &c[i..=i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Inline {
        Inline::Text(s.into())
    }

    /// Falsified by reading `_` inside a word as emphasis (snake_case).
    #[test]
    fn inline_words_read_as_markdown_says() {
        assert_eq!(
            inline("a *b* **c** `d` [e](f) snake_case_name"),
            vec![
                t("a "),
                Inline::Emphasis(vec![t("b")]),
                t(" "),
                Inline::Strong(vec![t("c")]),
                t(" "),
                Inline::Code("d".into()),
                t(" "),
                Inline::Link {
                    href: "f".into(),
                    text: vec![t("e")]
                },
                t(" snake_case_name"),
            ]
        );
        assert_eq!(inline("``a ` b``"), vec![Inline::Code("a ` b".into())]);
        assert_eq!(inline(r"\*not\*"), vec![t("*not*")]);
        assert_eq!(inline("2 * 3 * 4"), vec![t("2 * 3 * 4")]);
        assert_eq!(
            inline("[`code` link](x)"),
            vec![Inline::Link {
                href: "x".into(),
                text: vec![Inline::Code("code".into()), t(" link")]
            }]
        );
    }

    /// Falsified by ending a list at an item's indented code.
    #[test]
    fn blocks_read_as_markdown_says() {
        let md = "# Title Here\n\nOne\ntwo.\n\n- a\n- b\n  - c\n\n1. x\n2. y\n\n```rust\nfn main() {}\n```\n\n> [!TIP]\n> Said.\n\n> Quoted.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n![Alt](p.webp)\n\n---\n";
        let b = parse(md);
        assert_eq!(
            b[0],
            Block::Heading {
                level: 1,
                id: "title-here".into(),
                text: vec![t("Title Here")]
            }
        );
        assert_eq!(b[1], Block::Paragraph(vec![t("One two.")]));
        let Block::List { ordered: false, items, .. } = &b[2] else {
            panic!("{:?}", b[2])
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[1][1], Block::List { .. }), "{:?}", items[1]);
        assert!(matches!(&b[3], Block::List { ordered: true, items, .. } if items.len() == 2));
        assert_eq!(
            b[4],
            Block::Code {
                lang: "rust".into(),
                text: "fn main() {}".into()
            }
        );
        assert_eq!(
            b[5],
            Block::Callout {
                kind: Callout::Tip,
                blocks: vec![Block::Paragraph(vec![t("Said.")])]
            }
        );
        assert_eq!(b[6], Block::Quote(vec![Block::Paragraph(vec![t("Quoted.")])]));
        assert!(matches!(&b[7], Block::Table { head, rows } if head.len() == 2 && rows.len() == 1));
        assert!(matches!(&b[8], Block::Image { src, .. } if src == "p.webp"));
        assert_eq!(b[9], Block::Rule);
        assert_eq!(b.len(), 10);
    }

    /// Falsified by dropping a character between tokens.
    #[test]
    fn highlighted_code_is_the_code_in_pieces() {
        let code = "#[view]\npub fn a(x: Vec<u8>) -> &'static str { // hi\n    println!(\"{}\", 'c', 1_000); }";
        let pieces = highlight("rust", code);
        assert_eq!(pieces.iter().map(|(_, s)| s.as_str()).collect::<String>(), code);
        let kind = |w: &str| pieces.iter().find(|(_, s)| s.contains(w)).map(|(t, _)| *t);
        assert_eq!(kind("#[view]"), Some(Token::Attribute));
        assert_eq!(kind("pub"), Some(Token::Keyword));
        assert_eq!(kind("Vec"), Some(Token::Type));
        assert_eq!(kind("println!"), Some(Token::Function));
        assert_eq!(kind("// hi"), Some(Token::Comment));
        assert_eq!(kind("\"{}\""), Some(Token::String));
        assert_eq!(kind("1_000"), Some(Token::Number));
        assert_eq!(highlight("klingon", "x"), vec![(Token::Plain, "x".into())]);
        let sh = highlight("sh", "nix build .#docs # the site");
        assert_eq!(sh.last().map(|p| p.0), Some(Token::Comment));
        assert!(sh.iter().all(|(t, s)| *t != Token::Comment || !s.contains(".#docs")), "{sh:?}");
    }
}
