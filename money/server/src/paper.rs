//! A paper receipt's items, read from what OCR saw of its photo — no
//! model reads it; rules do, and the receipt's own totals check them.
//!
//! The OCR (`receipt-ocr`, RapidOCR on PaddleOCR's PP-OCRv5 models) gives
//! the photo's boxes of text, each with where it is and how sure it was, at
//! three sizes of its text detector: a [`Reading`] each. Each is read here:
//!
//! - **Lines**: boxes near in height that never overlap side to side, the
//!   page's tilt taken off (the OCR's `y`, `rx`, `rr`) — a curled receipt's
//!   lines stay apart.
//! - **Prices**: the boxes of the right-hand column that read as a price
//!   (`7.52 FA`, `11.44 A *`, `-0.99`, `$18.00`), a blurred `$` as `*`, `%`
//!   or `§`, and the digits OCR mistakes for others (`O` 0, `l` 1, `S` 5).
//! - **Blocks**: an item's name line and the quantity or weight lines under
//!   it (`2 @ 1.19`, `1 @ 2 FOR 7.18`, `0.84 lb @ 0.99/lb`); department
//!   headings and notes are none. A restaurant's count before the name
//!   (`2 Shepherd's Pie`) is the quantity.
//! - **Pairing**: prices to blocks in order down the receipt, by the
//!   alignment with the least gap in height, a block's price beside its
//!   first line (ALDI) or its last (Food Lion).
//! - **Discounts**: a "You saved" section's lines, and any price below zero.
//! - **Totals**: the subtotal, the tax, and what was due. Items and
//!   discounts add up to the subtotal, or, with the tax, to what was due.
//!
//! The readings vote on each item's price ([`read`]). What they mostly say
//! adds up, and it is **sure**; or the readings' own other answers make it
//! add up, one or two items changed, and it is **settled**; or it does not,
//! and a person **checks** it. A digit is never guessed to make it add up
//! (a guess can make a wrong receipt look right), but for one line whose
//! twin — the same SKU — has the price the sum asks for.

use serde::Deserialize;

/// One box of text the OCR found, its place with the page's tilt taken off.
#[derive(Clone, Debug, Deserialize)]
pub struct Word {
    pub text: String,
    pub score: f64,
    pub left: f64,
    pub right: f64,
    pub y: f64,
    pub h: f64,
    #[serde(default)]
    pub rx: Option<f64>,
    #[serde(default)]
    pub rr: Option<f64>,
}

/// One reading of the photo: the OCR's boxes at one detector size.
pub type Reading = Vec<Word>;

/// How far to trust what was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// It adds up as the readings mostly said.
    Sure,
    /// It adds up after the readings' own other answers.
    Settled,
    /// It does not add up: a person looks.
    Check,
}

/// One line of the receipt: an item, or a discount (below zero).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub name: String,
    pub sku: String,
    pub cents: i64,
    pub qty: Option<u32>,
    /// The quantity or weight lines under it, as read.
    pub detail: String,
    /// What the readings gave, each price and how many.
    pub alts: Vec<(i64, u32)>,
    score: u32,
}

/// The receipt, as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paper {
    pub store: String,
    pub date: Option<String>,
    pub items: Vec<Line>,
    pub discounts: Vec<Line>,
    pub subtotal: Option<i64>,
    pub tax: i64,
    pub due: Option<i64>,
    /// What the items and discounts must add up to.
    pub target: Option<i64>,
    pub sum: i64,
    pub state: State,
    /// What the readings' other answers changed, said.
    pub changed: Vec<String>,
}

// * Characters

const DIGITISH: &str = "0123456789OoDQlI|iV!SsBgqZz";

fn digitish(c: char) -> bool {
    DIGITISH.contains(c)
}

/// A character OCR read in place of a digit, as the digit.
fn digit(c: char) -> char {
    match c {
        'O' | 'o' | 'D' | 'Q' => '0',
        'l' | 'I' | '|' | 'i' | 'V' | '!' => '1',
        'S' | 's' => '5',
        'B' => '8',
        'g' | 'q' => '9',
        'Z' | 'z' => '2',
        c => c,
    }
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `w` is in `s` with no letter or digit on either side.
fn has_word(s: &str, w: &str) -> bool {
    let mut from = 0;
    while let Some(at) = s[from..].find(w) {
        let at = from + at;
        let before = s[..at].chars().next_back();
        let after = s[at + w.len()..].chars().next();
        if !before.is_some_and(word_char) && !after.is_some_and(word_char) {
            return true;
        }
        from = at + w.len().max(1);
    }
    false
}

fn letters(s: &str) -> usize {
    s.chars().filter(|c| c.is_ascii_alphabetic()).count()
}

// * Prices

/// A price box's text: its cents and whether it is below zero; `$18.00`,
/// `7.52 FA`, `11.44 A *`, `-0.99`, `2.29 A`.
pub fn price(text: &str) -> Option<(i64, bool)> {
    let cs: Vec<char> = text.trim().chars().collect();
    let mut i = 0;
    let mut neg = false;
    let skip = |i: &mut usize, f: &dyn Fn(char) -> bool| {
        while *i < cs.len() && f(cs[*i]) {
            *i += 1;
        }
    };
    let money_sign = |c: char| "$*%§".contains(c) || c.is_whitespace();
    skip(&mut i, &money_sign);
    if i < cs.len() && cs[i] == '-' {
        neg = true;
        i += 1;
    }
    skip(&mut i, &money_sign);
    let start = i;
    skip(&mut i, &digitish);
    let whole: Vec<char> = cs[start..i].iter().map(|c| digit(*c)).collect();
    if whole.is_empty() || whole.len() > 3 {
        return None;
    }
    skip(&mut i, &|c| c.is_whitespace());
    if i >= cs.len() || !".,'`:".contains(cs[i]) {
        return None;
    }
    i += 1;
    skip(&mut i, &|c| c.is_whitespace());
    if i + 2 > cs.len() || !digitish(cs[i]) || !digitish(cs[i + 1]) {
        return None;
    }
    let cents: String = cs[i..i + 2].iter().map(|c| digit(*c)).collect();
    i += 2;
    skip(&mut i, &|c| c.is_whitespace());
    if i < cs.len() && cs[i] == '-' {
        neg = true;
        i += 1;
    }
    skip(&mut i, &|c| c.is_whitespace());
    // A tax code (FA, A, B, N, T, X) and the mark after it.
    if i < cs.len() && (cs[i].is_ascii_uppercase() || cs[i] == '¥') {
        i += 1;
        if i < cs.len() && cs[i].is_ascii_alphabetic() {
            i += 1;
        }
    }
    if cs[i..].iter().any(|c| word_char(*c)) {
        return None;
    }
    let whole: String = whole.into_iter().collect();
    let value = whole.parse::<i64>().ok()? * 100 + cents.parse::<i64>().ok()?;
    Some((value, neg))
}

/// The amounts on a line (`37.36`, `$ 99.05`, `0,00`), in cents.
fn amounts(text: &str) -> Vec<i64> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < cs.len() {
        if cs[i].is_ascii_digit() && (i == 0 || !cs[i - 1].is_ascii_digit()) {
            let mut j = i;
            while j < cs.len() && cs[j].is_ascii_digit() {
                j += 1;
            }
            if j + 2 < cs.len() && ".,:".contains(cs[j]) && cs[j + 1].is_ascii_digit() && cs[j + 2].is_ascii_digit() {
                let whole: String = cs[i.max(j.saturating_sub(5))..j].iter().collect();
                let cents: String = cs[j + 1..j + 3].iter().collect();
                if let (Ok(w), Ok(c)) = (whole.parse::<i64>(), cents.parse::<i64>()) {
                    out.push(w * 100 + c);
                }
                i = j + 3;
                continue;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

// * Lines

#[derive(Clone, Debug)]
struct Row {
    words: Vec<Word>,
    y: f64,
}

impl Row {
    fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Boxes to lines: near in height, never overlapping side to side.
fn rows(words: &[Word]) -> Vec<Row> {
    let mut sorted: Vec<&Word> = words.iter().collect();
    sorted.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
    let mut out: Vec<Row> = vec![];
    for w in sorted {
        let n = out.len();
        let mut home = None;
        for k in (n.saturating_sub(4)..n).rev() {
            let l = &out[k];
            let near = (w.y - l.y).abs() < 0.5 * median(l.words.iter().map(|x| x.h).collect());
            let apart = l.words.iter().all(|x| w.right <= x.left + 2.0 || w.left >= x.right - 2.0);
            if near && apart {
                home = Some(k);
                break;
            }
        }
        match home {
            None => out.push(Row {
                words: vec![w.clone()],
                y: w.y,
            }),
            Some(k) => {
                let l = &mut out[k];
                l.words.push(w.clone());
                l.y = l.words.iter().map(|x| x.y).sum::<f64>() / l.words.len() as f64;
            }
        }
    }
    for l in &mut out {
        l.words.sort_by(|a, b| a.left.partial_cmp(&b.left).unwrap());
    }
    out
}

// * Kinds of line

fn upper_trimmed(t: &str) -> String {
    t.trim_start_matches(|c: char| !word_char(c)).to_uppercase()
}

/// A totals or payment line, where the items end.
fn stops(t: &str) -> bool {
    let u = t.to_uppercase();
    // An item line with a SKU first is an item, whatever it says.
    let first: String = t
        .trim_start_matches(|c: char| !word_char(c))
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if first.len() >= 4 && t.trim_start_matches(|c: char| !word_char(c))[first.len()..].starts_with(' ') {
        return false;
    }
    const ANYWHERE: [&str; 20] = [
        "SUBTOTAL",
        "SUB TOTAL",
        "AMOUNT DUE",
        "BALANCE",
        "TAX",
        "VISA",
        "MASTERCARD",
        "MASTER CARD",
        "DEBIT",
        "CREDIT",
        "AMEX",
        "DISCOVER",
        "CHANGE",
        "TEND",
        "PAYMENT",
        "REF/SEQ",
        "REFSEQ",
        "TRACE",
        "AUTH",
        "APPROVED",
    ];
    const WORDS: [&str; 10] = ["TOTAL", "CASH", "AID", "TVR", "IAD", "TSI", "MID", "RRN", "PIN", "ITEMS"];
    ANYWHERE.iter().any(|w| u.contains(w)) || WORDS.iter().any(|w| has_word(&u, w)) || u.contains("T O T A L")
}

/// A department heading, or a note that is no item.
fn heading(t: &str) -> bool {
    const HEADINGS: [&str; 25] = [
        "FROZEN",
        "DAIRY",
        "GROCERY",
        "MEAT",
        "PRODUCE",
        "DELI",
        "BAKERY",
        "SEAFOOD",
        "HBC",
        "HEALTH",
        "BEAUTY",
        "GENERAL",
        "BEER",
        "WINE",
        "LIQUOR",
        "FLORAL",
        "PHARMACY",
        "DEPT",
        "DEPARTMENT",
        "STORE PRICE REDUCTION",
        "ORIGINAL PRICE",
        "PRICE REDUCTION",
        "REGULAR PRICE",
        "YOU SAVED",
        "SAVINGS",
    ];
    let u = upper_trimmed(t);
    HEADINGS
        .iter()
        .any(|h| u.starts_with(h) && !u[h.len()..].starts_with(|c: char| c.is_ascii_alphabetic()))
}

/// Where the discounts begin.
fn savings(t: &str) -> bool {
    let u = upper_trimmed(t);
    ["YOU SAVED", "SAVINGS", "COUPON", "DISCOUNT"].iter().any(|h| u.starts_with(h))
}

/// A quantity or weight line under an item.
fn quantity(t: &str) -> bool {
    let l = t.trim_start_matches(|c: char| !word_char(c)).to_lowercase();
    let lead: String = l.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = l[lead.len()..].trim_start();
    let lb = |s: &str| ["lb", "1b", "ib"].iter().any(|w| has_word(s, w));
    if !lead.is_empty() && (rest.starts_with('@') || rest.starts_with("for ") || rest == "for") {
        return true;
    }
    let numeric: String = l.chars().take_while(|c| c.is_ascii_digit() || ".,_ ".contains(*c)).collect();
    if !numeric.trim().is_empty() && numeric.chars().any(|c| c.is_ascii_digit()) && lb(&l[numeric.len()..l.len().min(numeric.len() + 3)]) {
        return true;
    }
    // `… 0.99/lb`, and `… 2 FOR 7.18`.
    let per = l
        .find('/')
        .is_some_and(|at| l[..at].trim_end().ends_with(|c: char| c.is_ascii_digit()) && lb(&l[at + 1..]));
    let two_for = l
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(3)
        .any(|w| w[0].chars().all(|c| c.is_ascii_digit()) && w[1] == "for" && w[2].chars().all(|c| c.is_ascii_digit() || c == '.'));
    per || two_for
}

/// A name as it reads, without the marks around it.
fn clean_name(s: &str) -> String {
    let kept: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "%/&'., -".contains(c) { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = kept.split_whitespace().collect();
    while words.last().is_some_and(|w| w.chars().all(|c| "-.,~xX".contains(c))) {
        words.pop();
    }
    words.join(" ").trim_matches(|c: char| " -.,~".contains(c)).to_string()
}

// * One reading

struct Block {
    discounts: bool,
    rows: Vec<Row>,
    price: Option<Word>,
}

#[derive(Clone, Debug)]
struct Read {
    items: Vec<Line>,
    discounts: Vec<Line>,
    subtotal: Option<i64>,
    tax: i64,
    due: Option<i64>,
    target: Option<i64>,
    store: String,
    date: Option<String>,
}

impl Read {
    fn sum(&self) -> i64 {
        self.items.iter().chain(&self.discounts).map(|l| l.cents).sum()
    }
}

fn read_one(words: &[Word]) -> Read {
    let lx = |w: &Word| w.rx.unwrap_or(w.left);
    let left0 = words.iter().map(lx).fold(f64::INFINITY, f64::min);
    let right0 = words.iter().map(|w| w.rr.unwrap_or(w.right)).fold(f64::NEG_INFINITY, f64::max);
    let column = left0 + 0.55 * (right0 - left0);
    let is_price = |w: &Word| lx(w) > column && price(&w.text).is_some();
    let prices: Vec<Word> = words.iter().filter(|w| is_price(w)).cloned().collect();
    let text_words: Vec<Word> = words.iter().filter(|w| !is_price(w)).cloned().collect();
    let lines = rows(&text_words);
    let h = median(words.iter().map(|w| w.h).collect());

    let mut blocks: Vec<Block> = vec![];
    let mut section = "items";
    let mut stop_y = None;
    for l in &lines {
        let t = l.text();
        if savings(&t) {
            section = "discounts";
            continue;
        }
        if stops(&t) {
            stop_y.get_or_insert(l.y);
            section = "totals";
            continue;
        }
        if section == "totals" || heading(&t) {
            continue;
        }
        let discounts = section == "discounts";
        if quantity(&t) {
            if let Some(b) = blocks.last_mut().filter(|b| b.discounts == discounts) {
                b.rows.push(l.clone());
                continue;
            }
        }
        if letters(&t) < 2 {
            continue;
        }
        blocks.push(Block {
            discounts,
            rows: vec![l.clone()],
            price: None,
        });
    }

    // Prices to blocks in order: the alignment with the least gap in height.
    let mut ps: Vec<Word> = prices.into_iter().filter(|p| stop_y.is_none_or(|s| p.y < s - 0.3 * p.h)).collect();
    ps.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
    let gap = 1.2 * h;
    let near = |b: &Block, p: &Word| (b.rows[0].y - p.y).abs().min((b.rows[b.rows.len() - 1].y - p.y).abs());
    let (n, m) = (blocks.len(), ps.len());
    let mut cost = vec![vec![0.0f64; m + 1]; n + 1];
    let mut step = vec![vec![0u8; m + 1]; n + 1];
    for i in 0..=n {
        for j in 0..=m {
            if i == 0 && j == 0 {
                continue;
            }
            let mut best = (f64::INFINITY, 0u8);
            if i > 0 && j > 0 {
                best = (cost[i - 1][j - 1] + near(&blocks[i - 1], &ps[j - 1]), 1);
            }
            if i > 0 && cost[i - 1][j] + gap < best.0 {
                best = (cost[i - 1][j] + gap, 2);
            }
            if j > 0 && cost[i][j - 1] + gap < best.0 {
                best = (cost[i][j - 1] + gap, 3);
            }
            (cost[i][j], step[i][j]) = best;
        }
    }
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        match step[i][j] {
            1 => {
                blocks[i - 1].price = Some(ps[j - 1].clone());
                i -= 1;
                j -= 1;
            }
            2 => i -= 1,
            _ => j -= 1,
        }
    }

    let mut items = vec![];
    let mut discounts = vec![];
    for b in &blocks {
        let Some(p) = &b.price else { continue };
        let Some((cents, neg)) = price(&p.text) else { continue };
        let first = b.rows[0].text();
        let trimmed = first.trim_start_matches(|c: char| !word_char(c));
        let sku_len = trimmed.chars().take_while(|c| "0123456789OoIl|S".contains(*c)).count();
        let has_sku = sku_len >= 5 && trimmed[sku_len..].starts_with(' ');
        let sku: String = if has_sku {
            trimmed[..sku_len].chars().map(digit).collect()
        } else {
            String::new()
        };
        let mut name = clean_name(if has_sku { &trimmed[sku_len..] } else { &first });
        // Food Lion's trailing flags (M, W, WM).
        if let Some((head, tail)) = name.rsplit_once(' ') {
            if (1..=2).contains(&tail.len()) && tail.chars().all(|c| c.is_ascii_uppercase()) {
                name = head.to_string();
            }
        }
        // A count before the name, after any stray mark from the paper's edge.
        let mut qty = None;
        if !has_sku {
            let parts: Vec<&str> = name.splitn(3, ' ').collect();
            let (count, rest) = match parts.as_slice() {
                [a, b, rest] if a.len() <= 2 && b.chars().all(|c| c.is_ascii_digit()) && b.len() <= 2 && !a.chars().all(|c| c.is_ascii_digit()) => {
                    (Some(*b), *rest)
                }
                [a, rest @ ..] if a.len() <= 2 && a.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() => (Some(*a), &name[a.len() + 1..]),
                _ => (None, name.as_str()),
            };
            if let Some(c) = count.filter(|_| rest.starts_with(|c: char| c.is_ascii_alphabetic())) {
                qty = c.parse().ok();
                name = rest.to_string();
            }
        }
        let line = Line {
            name,
            sku,
            cents: if neg || b.discounts { -cents } else { cents },
            qty,
            detail: b.rows[1..].iter().map(|r| r.text()).collect::<Vec<_>>().join(" / "),
            alts: vec![],
            score: (p.score * 1000.0) as u32,
        };
        if neg || b.discounts {
            discounts.push(line);
        } else {
            items.push(line);
        }
    }

    // Totals, from whole lines: the words and the price column together.
    let every = rows(words);
    let (mut subtotal, mut total_tax, mut tax, mut due) = (None, None, None::<i64>, None);
    for l in &every {
        let t = l.text();
        let Some(&last) = amounts(&t).last() else { continue };
        let u = t.to_uppercase();
        if u.contains("SUBTOTAL") || u.contains("SUB TOTAL") {
            subtotal = Some(last);
        } else if u.contains("TOTAL TAX") {
            total_tax = Some(last);
        } else if u.contains("TAX") {
            tax = Some(tax.unwrap_or(0) + last);
        } else if (u.contains("BALANCE DUE") || u.contains("AMOUNT DUE") || has_word(&u, "TOTAL") || u.contains("T O T A L")) && due.is_none() {
            due = Some(last);
        }
    }
    let tax = total_tax.or(tax).unwrap_or(0);
    let target = subtotal.or(due.map(|d| d - tax));
    Read {
        items,
        discounts,
        subtotal,
        tax,
        due,
        target,
        store: store(&every),
        date: date(&every),
    }
}

/// The store: the first line that is mostly letters.
fn store(lines: &[Row]) -> String {
    lines
        .iter()
        .take(4)
        .map(|l| l.text())
        .find(|t| letters(t) >= 3 && letters(t) >= t.chars().filter(|c| c.is_ascii_digit()).count())
        .map(|t| clean_name(&t))
        .unwrap_or_default()
}

/// The day, from the first `m/d/yy` or `m/d/yyyy` on the receipt.
fn date(lines: &[Row]) -> Option<String> {
    for l in lines {
        for token in l.text().split_whitespace() {
            let parts: Vec<&str> = token.trim_matches(|c: char| !c.is_ascii_digit()).split('/').collect();
            if let [m, d, y] = parts.as_slice() {
                let (Ok(m), Ok(d), Ok(y)) = (m.parse::<u32>(), d.parse::<u32>(), y.parse::<u32>()) else {
                    continue;
                };
                let y = if y < 100 { 2000 + y } else { y };
                if (1..=12).contains(&m) && (1..=31).contains(&d) && (2000..2100).contains(&y) {
                    return Some(format!("{y:04}-{m:02}-{d:02}"));
                }
            }
        }
    }
    None
}

// * The twin repair

/// The one confusable digit whose change, borne out by a twin line of the
/// same SKU at that price, makes the reading add up.
fn twin_repair(r: &mut Read) {
    let Some(target) = r.target else { return };
    let off = target - r.sum();
    if off == 0 {
        return;
    }
    const CONFUSE: [(char, &str); 9] = [
        ('6', "8"),
        ('8', "63"),
        ('3', "8"),
        ('5', "6"),
        ('1', "7"),
        ('7', "1"),
        ('0', "8"),
        ('9', "8"),
        ('2', "7"),
    ];
    let mut fixes = vec![];
    for (n, it) in r.items.iter().enumerate() {
        let s = format!("{:03}", it.cents.abs());
        for (k, c) in s.char_indices() {
            for (from, to) in CONFUSE {
                if c != from {
                    continue;
                }
                for alt in to.chars() {
                    let q: i64 = format!("{}{}{}", &s[..k], alt, &s[k + 1..]).parse().unwrap();
                    let twin = r
                        .items
                        .iter()
                        .enumerate()
                        .any(|(o, x)| o != n && !x.sku.is_empty() && x.sku == it.sku && x.cents == q);
                    if q - it.cents == off && twin {
                        fixes.push((n, q));
                    }
                }
            }
        }
    }
    if let [(n, q)] = fixes.as_slice() {
        r.items[*n].cents = *q;
    }
}

// * The vote

/// Likeness of two names, 0 to 1, letters alone (an edit distance).
fn likeness(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().filter(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
    let b: Vec<char> = b.chars().filter(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    1.0 - prev[b.len()] as f64 / a.len().max(b.len()) as f64
}

/// For each line of `base`, the line of `other` in its place, if any.
fn align<'a>(base: &[Line], other: &'a [Line]) -> Vec<Option<&'a Line>> {
    let (n, m) = (base.len(), other.len());
    let sim: Vec<Vec<f64>> = base.iter().map(|a| other.iter().map(|b| likeness(&a.name, &b.name)).collect()).collect();
    let mut best = vec![vec![0.0f64; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            let s = sim[i - 1][j - 1];
            let pair = best[i - 1][j - 1] + if s >= 0.5 { s } else { -1.0 };
            best[i][j] = pair.max(best[i - 1][j]).max(best[i][j - 1]);
        }
    }
    let mut out = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let s = sim[i - 1][j - 1];
        if s >= 0.5 && (best[i][j] - (best[i - 1][j - 1] + s)).abs() < 1e-9 {
            out[i - 1] = Some(&other[j - 1]);
            i -= 1;
            j -= 1;
        } else if best[i][j] == best[i - 1][j] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    out
}

/// The receipt from its readings.
pub fn read(readings: &[Reading]) -> Paper {
    let reads: Vec<Read> = readings
        .iter()
        .map(|w| {
            let mut r = read_one(w);
            twin_repair(&mut r);
            r
        })
        .collect();
    let Some(base) = reads.iter().max_by_key(|r| r.items.len()).cloned() else {
        return Paper {
            store: String::new(),
            date: None,
            items: vec![],
            discounts: vec![],
            subtotal: None,
            tax: 0,
            due: None,
            target: None,
            sum: 0,
            state: State::Check,
            changed: vec![],
        };
    };
    // The target most readings found.
    let mut targets: Vec<(i64, usize)> = vec![];
    for t in reads.iter().filter_map(|r| r.target) {
        match targets.iter_mut().find(|(x, _)| *x == t) {
            Some(c) => c.1 += 1,
            None => targets.push((t, 1)),
        }
    }
    let target = targets.iter().max_by_key(|(_, c)| *c).map(|(t, _)| *t);
    let mut items = base.items.clone();
    for (k, it) in items.iter_mut().enumerate() {
        let mut votes: Vec<(i64, u32, u32)> = vec![]; // price, count, best score
        for r in &reads {
            if let Some(o) = align(&base.items, &r.items)[k] {
                match votes.iter_mut().find(|v| v.0 == o.cents) {
                    Some(v) => {
                        v.1 += 1;
                        v.2 = v.2.max(o.score);
                    }
                    None => votes.push((o.cents, 1, o.score)),
                }
            }
        }
        votes.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
        if let Some(top) = votes.first() {
            it.cents = top.0;
        }
        it.alts = votes.iter().map(|v| (v.0, v.1)).collect();
    }
    let discounts = base.discounts.clone();
    let sum = |items: &[Line]| items.iter().chain(&discounts).map(|l| l.cents).sum::<i64>();
    let mut state = State::Check;
    let mut changed = vec![];
    if let Some(t) = target {
        if sum(&items) == t {
            state = State::Sure;
        } else {
            // The readings' other answers: one item, then two.
            let options: Vec<(usize, i64, u32)> = items
                .iter()
                .enumerate()
                .flat_map(|(k, it)| it.alts.iter().skip(1).map(move |(p, c)| (k, *p, *c)))
                .collect();
            let off = t - sum(&items);
            let mut found: Vec<(u32, Vec<(usize, i64)>)> = vec![];
            for &(k, p, c) in &options {
                if p - items[k].cents == off {
                    found.push((c, vec![(k, p)]));
                }
            }
            if found.is_empty() {
                for (a, &(k1, p1, c1)) in options.iter().enumerate() {
                    for &(k2, p2, c2) in &options[a + 1..] {
                        if k1 != k2 && (p1 - items[k1].cents) + (p2 - items[k2].cents) == off {
                            found.push((c1 + c2, vec![(k1, p1), (k2, p2)]));
                        }
                    }
                }
            }
            found.sort_by_key(|f| std::cmp::Reverse(f.0));
            if found.len() == 1 || (found.len() > 1 && found[0].0 > found[1].0) {
                for &(k, p) in &found[0].1 {
                    changed.push(format!("{}: {} read as {}", items[k].name, dollars(items[k].cents), dollars(p)));
                    items[k].cents = p;
                }
                state = State::Settled;
            }
        }
    }
    let total = sum(&items);
    Paper {
        store: base.store,
        date: base.date,
        items,
        discounts,
        subtotal: base.subtotal,
        tax: base.tax,
        due: base.due,
        target,
        sum: total,
        state,
        changed,
    }
}

fn dollars(cents: i64) -> String {
    format!("{}{}.{:02}", if cents < 0 { "-" } else { "" }, cents.abs() / 100, cents.abs() % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_read_as_receipts_print_them() {
        assert_eq!(price("7.52 FA"), Some((752, false)));
        assert_eq!(price("11.44 A *"), Some((1144, false)));
        assert_eq!(price("2.29 A "), Some((229, false)));
        assert_eq!(price("-0.99"), Some((99, true)));
        assert_eq!(price("$18,00"), Some((1800, false)));
        assert_eq!(price("*16.00"), Some((1600, false)), "a blurred $");
        assert_eq!(price("%34.00"), Some((3400, false)));
        assert_eq!(price("1O.25 FA"), Some((1025, false)), "an O for a 0");
        assert_eq!(price("1:399A"), None, "two prices run together");
        assert_eq!(price("Large Eggs"), None);
        assert_eq!(price("1406.28"), None, "no price has four digits before its point");
    }

    #[test]
    fn lines_of_a_receipt_are_told_apart() {
        assert!(stops("SUBTOTAL 98.80") && stops("B-Taxable @8.525% 0.25") && stops("T O T A L $ 99.05"));
        assert!(stops("13 BALANCE DUE 37.36") && stops("VISA 99.05"));
        assert!(!stops("382745 FP Chicken Breasts"), "an item with its SKU first");
        assert!(heading("GROCERY") && heading("FROZEN/DAIRY") && heading("ORIGINAL PRICE 12.71"));
        assert!(!heading("FL HAMBURGER BUNS"));
        assert!(quantity("2@ 1.19") && quantity("1 @ 2 FOR 7.18") && quantity("0.84_1b@ 0.99/1b") && quantity("1.30 lb x 2.09/lb"));
        assert!(!quantity("1 Black & Tan Burger"));
        assert!(savings("You saved:") && savings("Savings"));
        assert_eq!(amounts("0.00% Tax 1 37.36 0.00"), [0, 3736, 0]);
        assert_eq!(clean_name("Mild Italian Links . ~"), "Mild Italian Links");
        assert_eq!(clean_name("Black & Tan Burger"), "Black & Tan Burger");
    }

    #[test]
    fn the_day_is_the_first_date_printed() {
        let row = |t: &str| Row {
            words: vec![Word {
                text: t.into(),
                score: 1.0,
                left: 0.0,
                right: 1.0,
                y: 0.0,
                h: 1.0,
                rx: None,
                rr: None,
            }],
            y: 0.0,
        };
        assert_eq!(date(&[row("Ordered: 6/5/26 1:09 PM")]).as_deref(), Some("2026-06-05"));
        assert_eq!(date(&[row("Trace # 1"), row("05/27/2025 18:54:11")]).as_deref(), Some("2025-05-27"));
        assert_eq!(date(&[row("11/02/26 16:36 Ref/Seq # 1")]).as_deref(), Some("2026-11-02"));
        assert_eq!(date(&[row("1/2 Gal")]), None);
    }

    #[test]
    fn names_are_alike_by_their_letters() {
        assert!(likeness("Cooked Meatballs", "Cooked Meathalls") > 0.9);
        assert!(likeness("EVOO", "Raspberries") < 0.5);
    }
}
