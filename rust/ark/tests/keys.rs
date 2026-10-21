//! §1.1 held: a value's JSON and key are one, whichever side made them. A
//! Rust value of every shape a column may have — ints, text, bools, bytes,
//! ids, options, lists, tuples, records, sums of every kind of variant —
//! serializes to the same JSON and the same key as its JSON read under its
//! described `Type` (`ark::json::Typed`, what the store and the explorer
//! use); keys order as the values do and none is a prefix of another; and
//! a `Desc` key orders last first.

use ark::json::{put_key, typed_json, typed_key, Json};
use ark::plan::Desc;
use ark::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Describe)]
enum Shape {
    Dot,
    Line(i64),
    Pair(String, bool),
    Box { w: i64, label: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Describe)]
struct Every {
    n: i64,
    text: String,
    flag: bool,
    id: Id<Every>,
    maybe: Option<i64>,
    list: Vec<String>,
    pair: (i64, String),
    shape: Shape,
    shapes: Vec<Shape>,
}

impl Row for Every {
    const TABLE: &'static str = "every";
    type Key = (i64,);
    fn key(&self) -> (i64,) {
        (self.n,)
    }
    fn table() -> ark::schema::Table {
        ark::schema::Table::new("every")
    }
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) % n.max(1)
    }
    fn text(&mut self) -> String {
        let alphabet = ["", "a", "b", "\0", "ü", "zz", "a\"b", "\u{10FFFF}"];
        (0..self.below(3)).map(|_| alphabet[self.below(8) as usize]).collect()
    }
    fn int(&mut self) -> i64 {
        [i64::MIN, -1, 0, 1, 7, i64::MAX][self.below(6) as usize]
    }
    fn shape(&mut self) -> Shape {
        match self.below(4) {
            0 => Shape::Dot,
            1 => Shape::Line(self.int()),
            2 => Shape::Pair(self.text(), self.below(2) == 1),
            _ => Shape::Box {
                w: self.int(),
                label: (self.below(2) == 1).then(|| self.text()),
            },
        }
    }
    fn every(&mut self) -> Every {
        let mut id = [0; 16];
        id[15] = self.below(3) as u8;
        id[0] = self.below(2) as u8 * 0xFF;
        Every {
            n: self.int(),
            text: self.text(),
            flag: self.below(2) == 1,
            id: Id::new(id),
            maybe: (self.below(2) == 1).then(|| self.int()),
            list: (0..self.below(3)).map(|_| self.text()).collect(),
            pair: (self.int(), self.text()),
            shape: self.shape(),
            shapes: (0..self.below(3)).map(|_| self.shape()).collect(),
        }
    }
}

fn key<T: Serialize>(v: &T) -> Vec<u8> {
    let mut out = vec![];
    put_key(v, &mut out);
    out
}

/// 2,000 generated values: the JSON and the key the Rust value makes are
/// the JSON and the key its JSON makes read under `Every::describe()`, and
/// the JSON reads back as the value. Falsified by `Typed` writing a record
/// variant's fields as a tuple's: no `Box` shape's JSON was the derive's.
#[test]
fn a_value_and_its_json_under_its_type_are_one() {
    let mut r = Rng(1);
    let ty = Every::describe();
    for _ in 0..2000 {
        let v = r.every();
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(typed_json(&ty, &json).unwrap(), v.to_bytes(), "{v:?}");
        let mut k = vec![];
        typed_key(&ty, &json, &mut k).unwrap();
        assert_eq!(k, key(&v), "{v:?}");
        assert_eq!(Every::from_bytes(&v.to_bytes()), Ok(v));
    }
}

/// Keys order as the values' derived order does, field by field, and no
/// key is a proper prefix of another's: so a prefix of the key's columns
/// is a prefix of its bytes; a `Desc` part orders last first. Falsified by
/// `Desc` not complementing its bytes: the descending keys ordered
/// ascending.
#[test]
fn keys_order_as_values_and_are_prefix_free() {
    let mut r = Rng(2);
    let vs: Vec<Every> = (0..400).map(|_| r.every()).collect();
    for a in &vs {
        for b in &vs {
            let (ka, kb) = (key(a), key(b));
            assert_eq!(ka.cmp(&kb), a.cmp(b), "{a:?} against {b:?}");
            assert!(ka == kb || !kb.starts_with(&ka), "{a:?} a prefix of {b:?}");
            let (da, db) = (key(&(Desc(a.n), a.text.clone())), key(&(Desc(b.n), b.text.clone())));
            assert_eq!(da.cmp(&db), b.n.cmp(&a.n).then(a.text.cmp(&b.text)), "Desc");
        }
    }
}
