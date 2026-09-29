//! A view plan as Haskell's `show` printed it: the `plan` field of the
//! spec v3 `views/` vectors is that text, and the runner rebuilds the plan
//! from the file's name rather than parsing it. Kept only so that the v3
//! files regenerate byte for byte; a v4 plan is data and is written as a
//! value (§13).

use ark::ir::CmpOp;
use ark::schema::{Dir, Relation};
use ark::value::Value;
use ark::view::{Filter, ViewPlan};

/// `showsPrec 0` of a `ViewPlan`.
pub fn view_plan(vp: &ViewPlan) -> String {
    plan(vp, 0)
}

// Derived `Show` parenthesises a constructor applied to arguments — record
// syntax included — at precedence 11, which is an argument's.
fn paren(d: u8, s: String) -> String {
    if d >= 11 {
        format!("({s})")
    } else {
        s
    }
}

fn plan(vp: &ViewPlan, d: u8) -> String {
    let related: Vec<String> = vp
        .related
        .iter()
        .map(|(n, r, child)| format!("({},{},{})", string(n.chars()), relation(r), plan(child, 0)))
        .collect();
    paren(
        d,
        format!(
            "ViewPlan {{vpTable = {}, vpFilter = {}, vpOrder = [{}], vpLimit = {}, vpRelated = [{}]}}",
            string(vp.table.chars()),
            maybe(vp.filter.as_ref().map(|f| filter(f, 11))),
            vp.order
                .iter()
                .map(|(c, dir)| format!("({},{})", string(c.chars()), dir_name(*dir)))
                .collect::<Vec<_>>()
                .join(","),
            maybe(vp.limit.map(|n| int(n, 11))),
            related.join(",")
        ),
    )
}

fn relation(r: &Relation) -> String {
    format!(
        "Relation {{relParent = {}, relChild = {}, relColumn = {}}}",
        string(r.parent.chars()),
        string(r.child.chars()),
        string(r.column.chars())
    )
}

fn maybe(x: Option<String>) -> String {
    x.map_or("Nothing".into(), |s| format!("Just {s}"))
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::Asc => "Asc",
        Dir::Desc => "Desc",
    }
}

fn cmp_name(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "Eq",
        CmpOp::Ne => "Ne",
        CmpOp::Lt => "Lt",
        CmpOp::Le => "Le",
        CmpOp::Gt => "Gt",
        CmpOp::Ge => "Ge",
    }
}

fn filter(f: &Filter, d: u8) -> String {
    let list = |fs: &[Filter]| fs.iter().map(|f| filter(f, 0)).collect::<Vec<_>>().join(",");
    paren(
        d,
        match f {
            Filter::Cmp(c, op, v) => format!("FCmp {} {} {}", string(c.chars()), cmp_name(*op), value(v, 11)),
            Filter::In(c, vs) => format!(
                "FIn {} [{}]",
                string(c.chars()),
                vs.iter().map(|v| value(v, 0)).collect::<Vec<_>>().join(",")
            ),
            Filter::All(fs) => format!("FAll [{}]", list(fs)),
            Filter::Any(fs) => format!("FAny [{}]", list(fs)),
            Filter::Not(f) => format!("FNot {}", filter(f, 11)),
        },
    )
}

fn int(n: i64, d: u8) -> String {
    if n < 0 && d > 6 {
        format!("({n})")
    } else {
        n.to_string()
    }
}

fn value(v: &Value, d: u8) -> String {
    let bytes = |b: &[u8]| string(b.iter().map(|&x| x as char));
    match v {
        Value::Null => "VNull".into(),
        Value::Bool(b) => paren(d, format!("VBool {}", if *b { "True" } else { "False" })),
        Value::Int(n) => paren(d, format!("VInt {}", int(*n, 11))),
        Value::Text(t) => paren(d, format!("VText {}", string(t.chars()))),
        Value::Bytes(b) => paren(d, format!("VBytes {}", bytes(b))),
        Value::Id(i) => paren(d, format!("VId (IdBytes {})", bytes(i))),
        Value::List(xs) => paren(d, format!("VList [{}]", xs.iter().map(|x| value(x, 0)).collect::<Vec<_>>().join(","))),
        Value::Struct(m) => paren(
            d,
            format!(
                "VStruct (fromList [{}])",
                m.iter()
                    .map(|(k, x)| format!("({},{})", string(k.chars()), value(x, 0)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
    }
}

/// Haskell's `show` of a `String` (`showLitChar`): the ASCII control names,
/// decimal for everything from DEL up, and `\&` where the next character
/// would otherwise be read as part of the escape.
fn string(cs: impl Iterator<Item = char>) -> String {
    const ASCII: [&str; 32] = [
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "a", "b", "t", "n", "v", "f", "r", "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK",
        "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS", "RS", "US",
    ];
    let cs: Vec<char> = cs.collect();
    let mut out = String::from("\"");
    for (i, &c) in cs.iter().enumerate() {
        let next = cs.get(i + 1).copied();
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7f}' => out.push_str("\\DEL"),
            c if c > '\u{7f}' => {
                out.push_str(&format!("\\{}", c as u32));
                if next.is_some_and(|n| n.is_ascii_digit()) {
                    out.push_str("\\&");
                }
            }
            c if c < ' ' => {
                out.push('\\');
                out.push_str(ASCII[c as usize]);
                if c == '\u{0e}' && next == Some('H') {
                    out.push_str("\\&");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The escapes that are easy to get wrong. Falsified by dropping the
    /// `\&` after a decimal escape, which makes `"\200\&1"` read back as
    /// U+07D1.
    #[test]
    fn a_string_as_haskell_shows_it() {
        assert_eq!(
            string("\u{0}\u{1}\n\u{e}H\u{7f}\u{c8}1\"\\".chars()),
            r#""\NUL\SOH\n\SO\&H\DEL\200\&1\"\\""#
        );
    }
}
