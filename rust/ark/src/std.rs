//! §1.4 The standard library: what a function may compute that is not an
//! operator or a read, by v4's names (`spec/README.md` §5).
//!
//! [`sig`] is each function's signature, and says which refinements it
//! keeps: a function of text returns `Text` — UTF-8 kept, every other
//! refinement of its argument (a `Name`'s length) dropped; `Reverse` and
//! `Move` keep every refinement on a list's length (`NonEmpty`, `len`),
//! since they keep its length; `Insert` and `ListConcat` keep `NonEmpty`
//! of their first argument, since they only add; every other list
//! function returns the bare `List<T>`. [`apply`] is the meaning. `Sum`,
//! appended for views, adds in an `i128` and judges the total once
//! ([`total`]).
//!
//! `Trim`, `Lower` and `IsAlnum` consult the pinned tables in
//! [`crate::unicode_tables`] and nothing of the platform. An index out of
//! range is a refusal in exact words, never clamped: `<f>: index <i> out of
//! range for a list of <n>`, and `slice: <start>..<end> …`.

use crate::check::Types;
use crate::hash::{hex, sha256};
use crate::interp::V;
use crate::ir::{Rule, Std};
use crate::ty::Ty;
use crate::unicode_tables::{is_alphanumeric, is_white_space, to_lower_simple};
use crate::Refusal;

/// The type `f` returns for arguments of these types, or why they are not
/// its arguments.
pub fn sig(cx: &Types, f: Std, args: &[Ty]) -> Result<Ty, String> {
    use Std::*;
    let text = Types::text();
    let (int, bytes, bool) = (Ty::Int, Ty::Bytes, Ty::bool());
    let elem = |i: usize| match cx.structure(&args[i]) {
        Ty::List(e) => Ok(*e),
        t => Err(format!("{f:?}: argument {i} is {t:?}, not a list")),
    };
    let opt = |i: usize| match cx.structure(&args[i]) {
        t @ Ty::Sum(_) if t.is_option() => Ok(match t {
            Ty::Sum(mut vs) => vs.remove(1).remove(0),
            _ => unreachable!(),
        }),
        t => Err(format!("{f:?}: argument {i} is {t:?}, not an option")),
    };
    // The first argument's own type when its refinements are all `keep`.
    let keeps = |keep: &dyn Fn(&Rule) -> bool| -> Result<Ty, String> {
        let e = elem(0)?;
        Ok(match &args[0] {
            Ty::Named(n) if matches!(cx.decl(n), Some(d) if matches!(d.ty, Ty::List(_)) && d.refine.iter().all(|r| keep(&r.rule))) => args[0].clone(),
            _ => Ty::list(e),
        })
    };
    let length = |r: &Rule| matches!(r, Rule::NonEmpty | Rule::Len(_) | Rule::LenRange(..));
    let (want, ret): (Vec<Ty>, Ty) = match f {
        Trim | Lower => (vec![text.clone()], text.clone()),
        IsEmpty | IsAlnum => (vec![text.clone()], bool),
        Concat => (vec![Ty::list(text.clone())], text.clone()),
        Chars => (vec![text.clone()], Ty::list(text.clone())),
        TextLen | Fnv1a64 => (vec![text.clone()], int),
        StartsWith => (vec![text.clone(), text.clone()], bool),
        SplitOnce => (vec![text.clone(), text.clone()], Ty::option(Ty::Tuple(vec![text.clone(), text.clone()]))),
        TextOfInt => (vec![int], text.clone()),
        Hex | TextOfId => (vec![bytes], text.clone()),
        Min | Max => (vec![int.clone(), int.clone()], int),
        Clamp => (vec![int.clone(), int.clone(), int.clone()], int),
        Abs => (vec![int.clone()], int),
        Sha256 => (vec![bytes.clone()], bytes),
        IdOfText => (vec![text.clone()], Ty::option(bytes)),
        NilId => (vec![], bytes),
        Utf8 => (vec![text.clone()], bytes),
        First | Last => (vec![Ty::list(elem(0)?)], Ty::option(elem(0)?)),
        Len => (vec![Ty::list(elem(0)?)], int),
        Contains => (vec![Ty::list(elem(0)?), elem(0)?], bool),
        Reverse => (vec![Ty::list(elem(0)?)], keeps(&length)?),
        Move => (vec![Ty::list(elem(0)?), int.clone(), int], keeps(&length)?),
        ListConcat => (vec![Ty::list(elem(0)?), Ty::list(elem(0)?)], keeps(&|r| *r == Rule::NonEmpty)?),
        Insert => (vec![Ty::list(elem(0)?), int, elem(0)?], keeps(&|r| *r == Rule::NonEmpty)?),
        RemoveAt => (vec![Ty::list(elem(0)?), int], Ty::list(elem(0)?)),
        Slice => (vec![Ty::list(elem(0)?), int.clone(), int], Ty::list(elem(0)?)),
        IsSome => (vec![Ty::option(opt(0)?)], bool),
        UnwrapOr => (vec![Ty::option(opt(0)?), opt(0)?], opt(0)?),
        Unwrap => (vec![Ty::option(opt(0)?)], opt(0)?),
        Sum => (vec![Ty::list(int.clone())], int),
    };
    if want.len() != args.len() {
        return Err(format!("{f:?} takes {} arguments, not {}", want.len(), args.len()));
    }
    for (i, (a, w)) in args.iter().zip(&want).enumerate() {
        if !cx.sub(a, w) {
            return Err(format!("{f:?}: argument {i} is {a:?}, not {w:?}"));
        }
    }
    Ok(ret)
}

fn place(f: &str, i: i64, n: usize, end: bool) -> Result<usize, Refusal> {
    if i >= 0 && (i as u64) < n as u64 + u64::from(end) {
        Ok(i as usize)
    } else {
        Err(Refusal(format!("{f}: index {i} out of range for a list of {n}")))
    }
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf29ce484222325u64, |h, b| (h ^ (*b as u64)).wrapping_mul(0x00000100000001b3))
}

/// White space off both ends, by the pinned table.
pub fn trim(t: &str) -> &str {
    t.trim_matches(is_white_space)
}

fn id_of_text(t: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = t.split('-').collect();
    let ok = parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()));
    let h = parts.concat();
    ok.then(|| (0..32).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect())
}

/// v4's §6.5 rule for a sum: added in an `i128`, so an overflow on the way,
/// which depends on the members' order, is none, and only the total must
/// fit. A view keeps a `Group`'s sum as such a number, moved by members in
/// no particular order (`crate::view`), and the two answers agree.
pub fn total(n: i128) -> Result<V, Refusal> {
    i64::try_from(n).map(V::Int).map_err(|_| Refusal::new("integer overflow"))
}

/// `f` applied to evaluated arguments the checker typed by [`sig`].
pub fn apply(f: Std, args: Vec<V>) -> Result<V, Refusal> {
    use Std::*;
    use V::*;
    let s = |v: &V| String::from_utf8(v.bytes().to_vec()).expect("checked text");
    let t = |x: &str| Bytes(x.as_bytes().to_vec());
    let (int, list) = (|v: &V| v.int(), |v: &V| v.list().to_vec());
    let a = &args;
    Ok(match f {
        Trim => t(trim(&s(&a[0]))),
        IsEmpty => V::bool(a[0].bytes().is_empty()),
        Concat => Bytes(list(&a[0]).iter().flat_map(|x| x.bytes().to_vec()).collect()),
        Lower => t(&s(&a[0]).chars().map(to_lower_simple).collect::<String>()),
        IsAlnum => V::bool(!a[0].bytes().is_empty() && s(&a[0]).chars().all(is_alphanumeric)),
        Chars => List(s(&a[0]).chars().map(|c| t(c.encode_utf8(&mut [0; 4]))).collect()),
        TextLen => Int(s(&a[0]).chars().count() as i64),
        StartsWith => V::bool(a[0].bytes().starts_with(a[1].bytes())),
        SplitOnce => match s(&a[1]).is_empty() {
            true => V::none(),
            false => s(&a[0]).split_once(&s(&a[1])).map_or(V::none(), |(b, c)| V::some(Tup(vec![t(b), t(c)]))),
        },
        TextOfInt => t(&int(&a[0]).to_string()),
        Hex => t(&hex(a[0].bytes())),
        Min => Int(int(&a[0]).min(int(&a[1]))),
        Max => Int(int(&a[0]).max(int(&a[1]))),
        Clamp if int(&a[1]) > int(&a[2]) => return Err(Refusal::new("clamp: lower bound above upper bound")),
        Clamp => Int(int(&a[0]).clamp(int(&a[1]), int(&a[2]))),
        Abs => Int(int(&a[0]).checked_abs().ok_or_else(|| Refusal::new("integer overflow"))?),
        Fnv1a64 => Int(fnv1a64(a[0].bytes()) as i64),
        Sha256 => Bytes(sha256(&[a[0].bytes()]).to_vec()),
        IdOfText => id_of_text(&s(&a[0])).map_or(V::none(), |b| V::some(Bytes(b))),
        TextOfId => {
            let h = hex(a[0].bytes());
            match h.len() {
                32 => t(&format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])),
                _ => return Err(Refusal::new("text_of_id: not an id")),
            }
        }
        NilId => Bytes(vec![0; 16]),
        Utf8 => a[0].clone(),
        First => list(&a[0]).first().cloned().map_or(V::none(), V::some),
        Last => list(&a[0]).last().cloned().map_or(V::none(), V::some),
        Len => Int(a[0].list().len() as i64),
        Contains => V::bool(a[0].list().contains(&a[1])),
        Reverse => List(list(&a[0]).into_iter().rev().collect()),
        ListConcat => List([list(&a[0]), list(&a[1])].concat()),
        Insert => {
            let mut xs = list(&a[0]);
            xs.insert(place("insert", int(&a[1]), xs.len(), true)?, a[2].clone());
            List(xs)
        }
        RemoveAt => {
            let mut xs = list(&a[0]);
            xs.remove(place("remove_at", int(&a[1]), xs.len(), false)?);
            List(xs)
        }
        Move => {
            let mut xs = list(&a[0]);
            let from = place("move", int(&a[1]), xs.len(), false)?;
            let to = place("move", int(&a[2]), xs.len(), false)?;
            let x = xs.remove(from);
            xs.insert(to, x);
            List(xs)
        }
        Slice => {
            let (xs, i, j) = (list(&a[0]), int(&a[1]), int(&a[2]));
            if i < 0 || i > j || j > xs.len() as i64 {
                return Err(Refusal(format!("slice: {i}..{j} out of range for a list of {}", xs.len())));
            }
            List(xs[i as usize..j as usize].to_vec())
        }
        IsSome => V::bool(a[0].variant().0 == 1),
        UnwrapOr | Unwrap => match a[0].variant() {
            (1, fs) => fs[0].clone(),
            _ if f == UnwrapOr => a[1].clone(),
            _ => return Err(Refusal::new("unwrapped none")),
        },
        Std::Sum => total(a[0].list().iter().map(|x| x.int() as i128).sum())?,
    })
}
