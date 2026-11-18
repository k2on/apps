//! A space's rows on the watch: the kernel's [`Memory`], with the tables
//! it has held noted, so that the disk can walk every row in order
//! ([`Rows::each`]) to page it out.

use alloc::vec::Vec;

use ark_kernel::store::Range;
use ark_kernel::{Memory, Read, Store, TableId};

/// Called with each row's table, key and value.
pub type Visit<'a> = dyn FnMut(TableId, &[u8], &[u8]) + 'a;

#[derive(Default)]
pub struct Rows {
    mem: Memory,
    /// Every table a row was put in, ascending.
    tables: Vec<TableId>,
}

impl Rows {
    pub fn new() -> Rows {
        Rows::default()
    }

    /// Calls `f` with each row, in table and key order.
    pub fn each(&self, f: &mut Visit<'_>) {
        for &t in &self.tables {
            self.mem.scan(t, Range::all(), &mut |k, v| {
                f(t, k, v);
                true
            });
        }
    }

    /// How many rows it holds.
    pub fn len(&self) -> usize {
        self.tables.iter().map(|&t| self.mem.len(t)).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Read for Rows {
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        self.mem.get(table, key)
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        self.mem.scan(table, range, f)
    }

    fn tables(&self) -> Vec<TableId> {
        self.tables.clone()
    }
}

impl Store for Rows {
    fn put(&mut self, table: TableId, key: &[u8], value: &[u8]) {
        if let Err(i) = self.tables.binary_search(&table) {
            self.tables.insert(i, table);
        }
        self.mem.put(table, key, value);
    }

    fn delete(&mut self, table: TableId, key: &[u8]) {
        self.mem.delete(table, key);
    }
}
