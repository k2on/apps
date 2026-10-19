//! Generated types and values for the codec's properties: a value tree whose
//! derived order is §1.1's order of values (an `Int` numerically, `Bytes`
//! and a list lexicographically with a prefix first, a tuple field by
//! field, a sum by variant and then payload), written with the crate's
//! writers and read back with its reader. Seeded by Knuth's MMIX
//! generator, as the simulator is.

#![allow(dead_code)]

use ark5::codec::{put_bytes, put_int, put_variant, Reader};
use ark5::ty::Ty;
use ark5::Refusal;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Val {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Val>),
    Tuple(Vec<Val>),
    Sum(u8, Vec<Val>),
}

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

pub fn ty(r: &mut Rng, depth: u32) -> Ty {
    let k = if depth == 0 { r.below(2) } else { r.below(5) };
    match k {
        0 => Ty::Int,
        1 => Ty::Bytes,
        2 => Ty::list(ty(r, depth - 1)),
        3 => Ty::Tuple((0..r.below(4)).map(|_| ty(r, depth - 1)).collect()),
        _ => Ty::Sum((0..1 + r.below(4)).map(|_| (0..r.below(3)).map(|_| ty(r, depth - 1)).collect()).collect()),
    }
}

/// A value of `t`, small and full of the bytes the encoding treats
/// specially: `00`, `01` and `FF`, and the extremes of an `Int`.
pub fn val(r: &mut Rng, t: &Ty) -> Val {
    match t {
        Ty::Int => Val::Int(match r.below(6) {
            0 => i64::MIN,
            1 => i64::MAX,
            2 => -1,
            3 => 0,
            4 => r.below(5) as i64 - 2,
            _ => r.next() as i64 - (1 << 52),
        }),
        Ty::Bytes => Val::Bytes((0..r.below(5)).map(|_| [0u8, 1, 0xFF, b'a', 0xFE][r.below(5) as usize]).collect()),
        Ty::List(e) => Val::List((0..r.below(4)).map(|_| val(r, e)).collect()),
        Ty::Tuple(ts) => Val::Tuple(ts.iter().map(|t| val(r, t)).collect()),
        Ty::Sum(vs) => {
            let i = r.below(vs.len() as u64) as usize;
            Val::Sum(i as u8, vs[i].iter().map(|t| val(r, t)).collect())
        }
    }
}

pub fn enc(v: &Val, out: &mut Vec<u8>) {
    match v {
        Val::Int(n) => put_int(out, *n),
        Val::Bytes(b) => put_bytes(out, b),
        Val::List(xs) => ark5::codec::put_list(out, xs, |o, x| enc(x, o)),
        Val::Tuple(xs) => xs.iter().for_each(|x| enc(x, out)),
        Val::Sum(i, xs) => {
            put_variant(out, *i);
            xs.iter().for_each(|x| enc(x, out));
        }
    }
}

pub fn bytes(v: &Val) -> Vec<u8> {
    let mut out = vec![];
    enc(v, &mut out);
    out
}

pub fn dec(t: &Ty, r: &mut Reader) -> Result<Val, Refusal> {
    Ok(match t {
        Ty::Int => Val::Int(r.int()?),
        Ty::Bytes => Val::Bytes(r.bytes()?),
        Ty::List(e) => Val::List(r.list(|r| dec(e, r))?),
        Ty::Tuple(ts) => Val::Tuple(ts.iter().map(|t| dec(t, r)).collect::<Result<_, _>>()?),
        Ty::Sum(vs) => {
            let i = r.variant(vs.len())?;
            Val::Sum(i, vs[i as usize].iter().map(|t| dec(t, r)).collect::<Result<_, _>>()?)
        }
    })
}
