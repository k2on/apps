//! A [`Store`] that counts what a read costs: every row a scan hands back
//! or a filter is asked about, and every `get`. Wrapped around a
//! [`MemoryStore`], it passes the equalities through so the store's own
//! indexes still serve — and counts each row the store *examines* (each
//! call to `keep`), not only the rows it returns, because a read through
//! the wrong index returns the right rows and examines the wrong number.

#![allow(dead_code)]

use std::cell::Cell;

use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Row, Store};
use ark::value::Value;

/// What one stretch of reads cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    /// Calls to `get`.
    pub gets: usize,
    /// Rows a scan returned, plus rows a filtered scan examined.
    pub rows: usize,
}

pub struct Counting<'a> {
    pub inner: &'a MemoryStore,
    gets: Cell<usize>,
    rows: Cell<usize>,
}

impl<'a> Counting<'a> {
    pub fn new(inner: &'a MemoryStore) -> Counting<'a> {
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

impl Store for Counting<'_> {
    fn schema(&self) -> &Schema {
        self.inner.schema()
    }

    fn get(&self, table: &str, key: &[Value]) -> Option<Row> {
        self.gets.set(self.gets.get() + 1);
        self.inner.get(table, key)
    }

    fn scan(&self, table: &str) -> Vec<Row> {
        let rs = self.inner.scan(table);
        self.rows.set(self.rows.get() + rs.len());
        rs
    }

    fn scan_where(&self, table: &str, keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        self.inner.scan_where(table, &|r| {
            self.rows.set(self.rows.get() + 1);
            keep(r)
        })
    }

    fn scan_where_eq(&self, table: &str, eq: &[(&str, &Value)], keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        self.inner.scan_where_eq(table, eq, &|r| {
            self.rows.set(self.rows.get() + 1);
            keep(r)
        })
    }

    fn apply_change(&mut self, _: &Change) {
        unreachable!("a counting store is read, never written")
    }

    fn as_store(&self) -> &dyn Store {
        self
    }
}
