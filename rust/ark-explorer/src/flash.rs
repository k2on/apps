//! What changed, lit as it lands (`docs/plan-guards.md` D4): when the host's
//! store moves while the explorer shows a table — a sync landing, a mutation
//! made in the app beside it — a cell whose value changed is lit in the
//! host's accent and fades over [`FADE_MS`]; a new row is lit the same way;
//! a removed row stays drawn, as removed, for as long, and then goes. A
//! query's answer on its form is held to the same rule when its rows move.
//!
//! **The diff is by the row's key and the column, from what was last
//! drawn** — never by place. A row inserted above, or a sort, moves every
//! row below it down one place; by place that is every cell changed, and by
//! key it is one row added. The host says when its store moved (a
//! generation it bumps), so nothing is diffed on a tick where nothing did.
//!
//! No clock here: the host hands the time in, as everything else is handed
//! in, so a browser's and a desktop's clocks are the host's business and a
//! test's is a number.

use std::collections::BTreeMap;

use ark::value::Value;

/// How long a change stays lit, fading all the while.
pub const FADE_MS: u64 = 2000;

/// A row's key: the table's key columns, or for a query's answer the
/// column that names its rows.
pub type Key = Vec<Value>;

/// A row that was drawn and is gone: where it was, what it held, and when
/// it went.
#[derive(Clone, Debug, PartialEq)]
pub struct Gone {
    pub since: u64,
    pub at: usize,
    pub key: Key,
    pub values: Vec<Value>,
}

/// What was last drawn of one table or one answer, and what has lit since.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracker {
    /// What is tracked — a table, an answer — so that showing something
    /// else starts again rather than lighting everything as new.
    what: Option<String>,
    columns: Vec<String>,
    rows: BTreeMap<Key, Vec<Value>>,
    order: Vec<Key>,
    /// A whole row added (`None`) or one of its cells changed (`Some`), and
    /// since when.
    lit: BTreeMap<(Key, Option<usize>), u64>,
    pub gone: Vec<Gone>,
}

impl Tracker {
    /// Whether this tracks `what`, under these columns.
    pub fn tracks(&self, what: &str, columns: &[String]) -> bool {
        self.what.as_deref() == Some(what) && self.columns == columns
    }

    /// Take `rows` — each its key and its values, in the order drawn — as
    /// what is drawn now. Against the same thing drawn before, every row
    /// whose key is new is lit, every cell whose value moved is lit, and
    /// every row whose key is gone is kept as gone; anything else, it is
    /// only remembered.
    pub fn observe(&mut self, what: &str, columns: &[String], rows: Vec<(Key, Vec<Value>)>, now: u64) {
        let fresh = !self.tracks(what, columns);
        if fresh {
            self.lit.clear();
            self.gone.clear();
        } else {
            for (key, values) in &rows {
                match self.rows.get(key) {
                    None => {
                        self.lit.insert((key.clone(), None), now);
                    }
                    Some(before) => {
                        for (c, (a, b)) in before.iter().zip(values).enumerate() {
                            if a != b {
                                self.lit.insert((key.clone(), Some(c)), now);
                            }
                        }
                    }
                }
            }
            let now_keys: std::collections::BTreeSet<&Key> = rows.iter().map(|(k, _)| k).collect();
            for (at, key) in self.order.iter().enumerate() {
                if !now_keys.contains(key) {
                    if let Some(values) = self.rows.get(key) {
                        self.gone.push(Gone {
                            since: now,
                            at,
                            key: key.clone(),
                            values: values.clone(),
                        });
                    }
                }
            }
            // A key that comes back is not gone.
            self.gone.retain(|g| !now_keys.contains(&g.key));
        }
        self.what = Some(what.to_string());
        self.columns = columns.to_vec();
        self.order = rows.iter().map(|(k, _)| k.clone()).collect();
        self.rows = rows.into_iter().collect();
    }

    /// How lit the cell of row `key` in column `c` is, from 1 as it lands to
    /// 0 after [`FADE_MS`]: its own change, or its row's arrival.
    pub fn strength(&self, key: &[Value], c: usize, now: u64) -> f32 {
        let at = |k: Option<usize>| self.lit.get(&(key.to_vec(), k)).map_or(0.0, |since| fade(*since, now));
        at(Some(c)).max(at(None))
    }

    /// How far a gone row has faded.
    pub fn gone_strength(g: &Gone, now: u64) -> f32 {
        fade(g.since, now)
    }

    /// Forget what has faded out.
    pub fn prune(&mut self, now: u64) {
        self.lit.retain(|_, since| fade(*since, now) > 0.0);
        self.gone.retain(|g| fade(g.since, now) > 0.0);
    }

    /// Whether anything is still lit: a host that redraws only when asked
    /// keeps a clock running while this says so.
    pub fn fading(&self, now: u64) -> bool {
        self.lit.values().any(|s| fade(*s, now) > 0.0) || self.gone.iter().any(|g| fade(g.since, now) > 0.0)
    }
}

/// From 1 at `since` to 0 at `since + FADE_MS`, eased so most of the fade is
/// late: a change is seen at full strength before it starts to go.
pub fn fade(since: u64, now: u64) -> f32 {
    let t = now.saturating_sub(since) as f32 / FADE_MS as f32;
    if t >= 1.0 {
        0.0
    } else {
        1.0 - t * t
    }
}

/// Which column names a query's rows, for the diff: the first whose values
/// are all ids and all different, else the first whose values are all
/// different and none of them null; `None` when none does, and then a row
/// is named by all of its values.
pub fn key_column(rows: &[Vec<Value>], columns: usize) -> Option<usize> {
    let distinct = |c: usize, ids: bool| {
        let mut seen: Vec<&Value> = Vec::with_capacity(rows.len());
        for r in rows {
            let v = &r[c];
            if v.is_null() || (ids && !matches!(v, Value::Id(_))) || seen.contains(&v) {
                return false;
            }
            seen.push(v);
        }
        !rows.is_empty()
    };
    (0..columns)
        .find(|&c| distinct(c, true))
        .or_else(|| (0..columns).find(|&c| distinct(c, false)))
}

/// How one drawn row is lit: which row of the grid it is (none for a row
/// that is gone), how lit each of its cells is, and how lit it is as gone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowLight {
    pub origin: Option<usize>,
    pub cells: Vec<f32>,
    pub gone: f32,
}

/// `grid`'s rows lit from `tracker`, each by its key, with the rows that
/// went put back where they were for as long as they fade. `from` is the
/// place of the grid's first row among all the rows tracked (a table draws
/// a page), and `last` says the grid ends where they do, so a row that was
/// last is put back after it.
pub fn light(grid: &crate::cells::Grid, keys: &[Key], from: usize, last: bool, tracker: &Tracker, now: u64) -> (crate::cells::Grid, Vec<RowLight>) {
    let mut out = crate::cells::Grid {
        columns: grid.columns.clone(),
        headings: grid.headings.clone(),
        rows: Vec::with_capacity(grid.rows.len()),
        widths: grid.widths.clone(),
    };
    let mut lights = Vec::with_capacity(grid.rows.len());
    let to = from + grid.rows.len();
    let mut gone: Vec<&Gone> = tracker
        .gone
        .iter()
        .filter(|g| (g.at >= from && g.at < to) || (last && g.at >= to))
        .filter(|g| fade(g.since, now) > 0.0)
        .collect();
    gone.sort_by_key(|g| g.at);
    let mut gone = gone.into_iter().peekable();
    let put_gone = |out: &mut crate::cells::Grid, lights: &mut Vec<RowLight>, g: &Gone| {
        out.rows.push(g.values.clone());
        lights.push(RowLight {
            origin: None,
            cells: vec![0.0; g.values.len()],
            gone: fade(g.since, now),
        });
    };
    for (i, row) in grid.rows.iter().enumerate() {
        while let Some(g) = gone.next_if(|g| g.at <= from + i) {
            put_gone(&mut out, &mut lights, g);
        }
        out.rows.push(row.clone());
        lights.push(RowLight {
            origin: Some(i),
            cells: (0..row.len()).map(|c| keys.get(i).map_or(0.0, |k| tracker.strength(k, c, now))).collect(),
            gone: 0.0,
        });
    }
    for g in gone {
        put_gone(&mut out, &mut lights, g);
    }
    (out, lights)
}
