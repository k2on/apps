//! A [`Read`] that counts what a read costs, ported from v4's: every row a
//! scan hands back and every `get`. v5's store examines only the rows it
//! returns — a filter's equalities go down as a key or index prefix — so
//! the rows returned are the rows examined.

#![allow(dead_code)]

use std::cell::Cell;

use ark5::schema::Schema;
use ark5::store::{Read, Scan};

/// What one stretch of reads cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    /// Calls to `get`.
    pub gets: usize,
    /// Rows scans returned.
    pub rows: usize,
}

pub struct Counting<'a> {
    pub inner: &'a dyn Read,
    gets: Cell<usize>,
    rows: Cell<usize>,
}

impl<'a> Counting<'a> {
    pub fn new(inner: &'a dyn Read) -> Counting<'a> {
        Counting {
            inner,
            gets: Cell::new(0),
            rows: Cell::new(0),
        }
    }

    pub fn reads(&self) -> Reads {
        Reads {
            gets: self.gets.get(),
            rows: self.rows.get(),
        }
    }
}

impl Read for Counting<'_> {
    fn schema(&self) -> &Schema {
        self.inner.schema()
    }

    fn get(&self, table: &str, key: &[u8]) -> Option<Vec<u8>> {
        self.gets.set(self.gets.get() + 1);
        self.inner.get(table, key)
    }

    fn scan(&self, table: &str, s: &Scan) -> Vec<Vec<u8>> {
        let rs = self.inner.scan(table, s);
        self.rows.set(self.rows.get() + rs.len());
        rs
    }
}
