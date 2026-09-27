//! Declaring the schema: a scope holds tables, a table holds columns, a
//! key, indexes and references. An id column comes in three spellings,
//! because the schema rules tell them apart: [`TableBuilder::id`] is a
//! table's own key, [`TableBuilder::id_ref`] a checked reference within the
//! scope, [`TableBuilder::id_of`] an id naming a table in another scope —
//! which the store cannot check and the schema therefore does not promise.

use ark::schema::{Column, Index, Ref, Scope, Table};

use crate::ty::Ty;

/// One scope under construction.
pub struct ScopeBuilder {
    pub(crate) scope: Scope,
}

/// One table under construction.
pub struct TableBuilder {
    pub(crate) table: Table,
}

impl ScopeBuilder {
    pub(crate) fn new(name: &str) -> ScopeBuilder {
        ScopeBuilder {
            scope: Scope {
                name: name.to_string(),
                tables: vec![],
            },
        }
    }

    /// Declare a table.
    pub fn table(&mut self, name: &str, build: impl FnOnce(&mut TableBuilder)) {
        let mut t = TableBuilder {
            table: Table {
                name: name.to_string(),
                columns: vec![],
                key: vec![],
                indexes: vec![],
                refs: vec![],
            },
        };
        build(&mut t);
        self.scope.tables.push(t.table);
    }
}

fn names(cols: &[&str]) -> Vec<String> {
    cols.iter().map(|c| c.to_string()).collect()
}

impl TableBuilder {
    /// A column of any scalar type.
    pub fn column(&mut self, name: &str, ty: Ty, nullable: bool) {
        self.table.columns.push(Column {
            name: name.to_string(),
            ty: ty.to_ir(),
            nullable,
        });
    }

    /// An id naming a row of this table — its own key, typically.
    pub fn id(&mut self, name: &str) {
        let own = self.table.name.clone();
        self.column(name, Ty::Id(own), false);
    }

    /// An id naming a row of `parent`, a table in this scope, with the
    /// reference that has the store hold it to one.
    pub fn id_ref(&mut self, name: &str, parent: &str) {
        self.column(name, Ty::id(parent), false);
        self.table.refs.push(Ref {
            column: name.to_string(),
            table: parent.to_string(),
        });
    }

    /// An id naming a row of `table` with no reference: the unchecked
    /// spelling, for a table in another scope.
    pub fn id_of(&mut self, name: &str, table: &str) {
        self.column(name, Ty::id(table), false);
    }

    pub fn text(&mut self, name: &str) {
        self.column(name, Ty::Text, false);
    }

    pub fn text_opt(&mut self, name: &str) {
        self.column(name, Ty::Text, true);
    }

    pub fn int(&mut self, name: &str) {
        self.column(name, Ty::Int, false);
    }

    pub fn int_opt(&mut self, name: &str) {
        self.column(name, Ty::Int, true);
    }

    pub fn bool(&mut self, name: &str) {
        self.column(name, Ty::Bool, false);
    }

    pub fn bool_opt(&mut self, name: &str) {
        self.column(name, Ty::Bool, true);
    }

    pub fn bytes(&mut self, name: &str) {
        self.column(name, Ty::Bytes, false);
    }

    pub fn bytes_opt(&mut self, name: &str) {
        self.column(name, Ty::Bytes, true);
    }

    /// A column holding one of the named variants.
    pub fn enumeration(&mut self, name: &str, variants: &[&str]) {
        self.column(name, Ty::enumeration(variants.iter().copied()), false);
    }

    /// The key: the columns that identify a row.
    pub fn key(&mut self, cols: &[&str]) {
        self.table.key = names(cols);
    }

    /// A uniqueness constraint.
    pub fn unique(&mut self, cols: &[&str]) {
        self.table.indexes.push(Index {
            columns: names(cols),
            unique: true,
        });
    }

    /// An index: a statement about performance, not a constraint.
    pub fn index(&mut self, cols: &[&str]) {
        self.table.indexes.push(Index {
            columns: names(cols),
            unique: false,
        });
    }
}
