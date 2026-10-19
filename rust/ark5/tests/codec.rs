//! §1.1 held over generated types and values: the encoding is canonical,
//! prefix-free and order-preserving, and `parse` takes canonical bytes of
//! its type and nothing else.

mod support {
    pub mod gen;
}

use ark5::codec::{parse, Reader};
use ark5::ty::Ty;
use support::gen::{bytes, dec, ty, val, Rng, Val};

/// For each of 400 generated types, 24 values: `parse` accepts each
/// encoding and reads back the value; every pair of distinct values
/// compares as their bytes do and neither's bytes are a prefix of the
/// other's. Falsified by writing an `Int` without flipping its sign bit
/// (negatives ordered after positives), and by ending `Bytes` with a
/// single `00`: each failed here.
#[test]
fn canonical_prefix_free_order_preserving() {
    let mut r = Rng(5);
    let mut pairs = 0;
    for _ in 0..400 {
        let t = ty(&mut r, 3);
        let vs: Vec<Val> = (0..24).map(|_| val(&mut r, &t)).collect();
        for v in &vs {
            let b = bytes(v);
            parse(&t, &b).unwrap_or_else(|e| panic!("{t:?} {v:?}: {e}"));
            let mut rd = Reader::new(&b);
            assert_eq!(&dec(&t, &mut rd).unwrap(), v, "{t:?}");
        }
        for a in &vs {
            for b in &vs {
                let (x, y) = (bytes(a), bytes(b));
                assert_eq!(a.cmp(b), x.cmp(&y), "{t:?}: {a:?} vs {b:?}");
                if a != b {
                    assert!(!y.starts_with(&x), "{t:?}: {a:?} is a prefix of {b:?}");
                    pairs += 1;
                }
            }
        }
    }
    assert!(pairs > 50_000, "too few distinct pairs: {pairs}");
}

/// Every byte string `parse` accepts is the encoding of the value it reads:
/// so it refuses every non-canonical one. Mutations of valid encodings
/// (a byte changed, inserted, removed; the end cut or extended), 200,000 of
/// them. Falsified by letting `escaped` take `00` followed by any byte as an
/// escape (`00 01` read as `00`): a mutation was accepted and re-encoded
/// differently.
#[test]
fn parse_accepts_only_canonical_bytes() {
    let mut r = Rng(11);
    let (mut refused, mut accepted) = (0, 0);
    for _ in 0..2_000 {
        let t = ty(&mut r, 3);
        let v = val(&mut r, &t);
        let good = bytes(&v);
        for _ in 0..100 {
            let mut b = good.clone();
            let at = r.below(b.len() as u64 + 1) as usize;
            let x = [0u8, 1, 2, 0xFF, 0x80][r.below(5) as usize];
            match r.below(5) {
                0 if at < b.len() => b[at] = x,
                1 => b.insert(at, x),
                2 if at < b.len() => drop(b.remove(at)),
                3 => b.truncate(at),
                _ => b.push(x),
            }
            match parse(&t, &b) {
                Err(_) => refused += 1,
                Ok(_) => {
                    let back = dec(&t, &mut Reader::new(&b)).unwrap();
                    assert_eq!(bytes(&back), b, "{t:?}: accepted bytes that are not canonical");
                    accepted += 1;
                }
            }
        }
    }
    assert!(refused > 100_000 && accepted > 1_000, "refused {refused}, accepted {accepted}");
}

/// The four ways bytes are not canonical, each refused by name. Falsified
/// by dropping `parse`'s `end()`: the trailing byte was accepted.
#[test]
fn parse_refuses_by_name() {
    let refuses = |t: &Ty, b: &[u8], why: &str| {
        let e = parse(t, b).expect_err(why);
        assert!(e.0.contains(why), "{:?}: {} does not say {why}", b, e.0);
    };
    refuses(&Ty::Bytes, &[b'a', 0, b'b', 0, 0], "an unescaped 00");
    refuses(&Ty::Bytes, &[b'a', 0, 0xFF], "terminator");
    refuses(&Ty::Bytes, &[b'a'], "terminator");
    refuses(&Ty::bool(), &[2], "variant 2 of a sum of 2");
    refuses(&Ty::Int, &[0x80, 0, 0, 0, 0, 0, 0, 0, 1], "trailing bytes");
    refuses(&Ty::Int, &[0x80, 0, 0], "eight bytes");
    refuses(&Ty::list(Ty::Int), &[2], "list marker");
    refuses(&Ty::list(Ty::Int), &[1, 0x80, 0, 0, 0, 0, 0, 0, 0], "a list");
    assert!(parse(&Ty::Tuple(vec![]), &[]).is_ok(), "the unit is no bytes");
}

/// The declarations order as §1.1 says: `None < Some`, an `Int` by its
/// sign, text by its bytes. Falsified by declaring `Ty::option` as `Sum
/// [(T), ()]`: `None`'s bytes no longer parsed as one.
#[test]
fn declarations_order_as_declared() {
    let none = bytes(&Val::Sum(0, vec![]));
    let some = bytes(&Val::Sum(1, vec![Val::Int(i64::MIN)]));
    assert!(parse(&Ty::option(Ty::Int), &none).is_ok() && parse(&Ty::option(Ty::Int), &some).is_ok());
    assert!(none < some);
    assert!(bytes(&Val::Int(-1)) < bytes(&Val::Int(0)));
    assert!(bytes(&Val::Bytes(b"t11".to_vec())) < bytes(&Val::Bytes(b"t7".to_vec())));
    assert_ne!(Ty::option(Ty::Int).hash(), Ty::list(Ty::Int).hash(), "a type's hash is its structure's");
}
