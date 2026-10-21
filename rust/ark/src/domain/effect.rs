//! Effects as rows (§1.16). A mutation is replayed, so it may only record
//! facts; what must act on the world outside — a server reading files, a
//! device downloading — is a **job**: a row whose `state` is a [`Job`],
//! which a handler on the one process that registered it (`ark-peer`'s
//! runner) claims, does, and finishes, each step a mutation the log
//! orders.
//!
//! ```text
//! Pending{due} ──claim (now ≥ due)──► Claimed{by, at} ──finish Ok──► Done(T)
//!     │                                 │      ▲    └──finish Err──► Failed{why, retry}
//!     └─cancel─► Cancelled              │      └─claim (now ≥ at + lease)   │
//!                                       └─────────◄──claim (now ≥ retry)───┘
//! ```
//!
//! `#[derive(Effect)]` on a row with a `state: Job<T>` field writes
//! [`Effect`]; [`Effect::register`] adds `<table>.claim {id, by}`,
//! `<table>.finish {id, by, outcome}` and `<table>.cancel {id}` to a
//! domain, and the query `<table>.due`: the rows a claim would take now,
//! and when the next is due (what a handler outside Rust asks). Each is judged at the authority in log order: of a claim and a
//! cancel of one pending row, whichever comes first wins and the other is
//! refused; a finish is refused unless the row is claimed by its `by`. A
//! claim is judged at its entry's `now`. A `Done` carries its outcome:
//! there is no done without one.

use serde::{Deserialize, Serialize};

use crate::db::{Db, Durable, Tables};
use crate::domain::Domain;
use crate::peer::Ctx;
use crate::row::{Describe, Input, Row, Type, Value};
use crate::Refusal;

/// Where a job stands.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub enum Job<T> {
    Pending { due_ms: i64 },
    Claimed { by: String, at_ms: i64 },
    Done(#[serde(with = "value")] T),
    Failed { why: String, retry_ms: i64 },
    Cancelled,
}

impl<T: Describe> Describe for Job<T> {
    fn describe() -> Type {
        let f = |fs: &[(&str, Type)]| fs.iter().map(|(n, t)| (n.to_string(), t.clone())).collect::<Vec<_>>();
        let vs = [
            ("Pending", f(&[("due_ms", Type::Int)])),
            ("Claimed", f(&[("by", Type::Text), ("at_ms", Type::Int)])),
            ("Done", f(&[("value", T::describe())])),
            ("Failed", f(&[("why", Type::Text), ("retry_ms", Type::Int)])),
            ("Cancelled", vec![]),
        ];
        let name = format!("Job{}", T::describe().decl().replace(|c: char| !c.is_alphanumeric(), ""));
        Type::Sum(name, vs.into_iter().map(|(n, fs)| (n.to_string(), fs)).collect())
    }
}

impl<T> Job<T> {
    /// When it may be claimed, while it may be: a pending job at its due, a
    /// failed one at its retry, a claimed one when its lease runs out.
    pub fn due(&self, lease_ms: i64) -> Option<i64> {
        match self {
            Job::Pending { due_ms } => Some(*due_ms),
            Job::Failed { retry_ms, .. } => Some(*retry_ms),
            Job::Claimed { at_ms, .. } => Some(at_ms + lease_ms),
            Job::Done(_) | Job::Cancelled => None,
        }
    }
}

/// A row that is a job (`#[derive(Effect)]`).
pub trait Effect: Row<Space = Durable> {
    /// What it comes to when it is done.
    type Outcome: Value + Describe;
    /// How long a claim holds before another may claim it again.
    const LEASE_MS: i64;
    /// How long after a failure it is due again.
    const RETRY_MS: i64;
    /// The role a claim and a finish need, when one does.
    const ROLE: Option<&'static str>;
    fn job(&self) -> &Job<Self::Outcome>;
    fn job_mut(&mut self) -> &mut Job<Self::Outcome>;
    /// Whose it is: who may cancel it (`#[effect(owner = field)]`); with
    /// none, whoever holds its role.
    fn owner(&self) -> Option<&str> {
        None
    }

    /// `<table>.claim`, `.finish` and `.cancel`, and `.due`, over the
    /// schema `S`.
    fn register<S: Tables>(d: Domain) -> Domain {
        let name = |f: &str| format!("{}.{f}", Self::TABLE);
        let d = d.mutator(&name("claim"), claim::<S, Self>).query(&name("due"), due::<S, Self>);
        d.mutator(&name("finish"), finish::<S, Self>).mutator(&name("cancel"), cancel::<S, Self>)
    }

    /// Its one key column's type (a job is keyed by one column), and the
    /// key it makes.
    type Id: Value + Describe;
    fn key_of(id: Self::Id) -> Self::Key;
    fn id(&self) -> Self::Id;
}

/// `claim`'s input: the job, and who claims it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Claim<R: Effect> {
    pub id: R::Id,
    pub by: String,
}

/// `finish`'s input: the job, who claimed it, and what it came to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Finish<R: Effect> {
    pub id: R::Id,
    pub by: String,
    #[serde(with = "outcome")]
    pub outcome: Result<R::Outcome, String>,
}

// A variant carrying one value is written as a record of it, `{value}`
// (`{why}` for a failure): the same bytes in the binary format, and in
// JSON a shape the phones' generated types read.
mod value {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct V<T> {
        value: T,
    }

    pub fn serialize<S: Serializer, T: Serialize>(t: &T, s: S) -> Result<S::Ok, S::Error> {
        V { value: t }.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<T, D::Error> {
        V::<T>::deserialize(d).map(|v| v.value)
    }
}

mod outcome {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    enum O<T, W> {
        Ok { value: T },
        Err { why: W },
    }

    pub fn serialize<S: Serializer, T: Serialize>(r: &Result<T, String>, s: S) -> Result<S::Ok, S::Error> {
        match r {
            Ok(value) => O::<&T, &String>::Ok { value }.serialize(s),
            Err(why) => O::<&T, &String>::Err { why }.serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Result<T, String>, D::Error> {
        Ok(match O::<T, String>::deserialize(d)? {
            O::Ok { value } => Ok(value),
            O::Err { why } => Err(why),
        })
    }
}

/// `cancel`'s input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Cancel<R: Effect> {
    pub id: R::Id,
}

/// `due`'s answer: the jobs a claim would take now, and when the next falls
/// due.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Due<R: Effect> {
    pub rows: Vec<R>,
    pub next_ms: Option<i64>,
}

/// A type named for its job's table — `LookClaim` — so that each kind's
/// are types of their own in the generated Kotlin and Swift.
fn named<R: Effect>(what: &str, fs: Vec<(&str, Type)>) -> Type {
    let table: String = R::TABLE
        .split('_')
        .flat_map(|w| w[..1].to_uppercase().chars().chain(w[1..].chars()).collect::<Vec<_>>())
        .collect();
    Type::Record(format!("{table}{what}"), fs.into_iter().map(|(n, t)| (n.into(), t)).collect())
}

impl<R: Effect> Describe for Claim<R> {
    fn describe() -> Type {
        named::<R>("Claim", vec![("id", R::Id::describe()), ("by", Type::Text)])
    }
}

impl<R: Effect> Describe for Finish<R> {
    fn describe() -> Type {
        let Type::Record(name, _) = named::<R>("Outcome", vec![]) else {
            unreachable!()
        };
        let (ok, err) = (vec![("value".into(), R::Outcome::describe())], vec![("why".into(), Type::Text)]);
        let outcome = Type::Sum(name, vec![("Ok".into(), ok), ("Err".into(), err)]);
        named::<R>("Finish", vec![("id", R::Id::describe()), ("by", Type::Text), ("outcome", outcome)])
    }
}

impl<R: Effect> Describe for Cancel<R> {
    fn describe() -> Type {
        named::<R>("Cancel", vec![("id", R::Id::describe())])
    }
}

impl<R: Effect> Describe for Due<R> {
    fn describe() -> Type {
        named::<R>("Due", vec![("rows", Vec::<R>::describe()), ("next_ms", Option::<i64>::describe())])
    }
}

macro_rules! unrefined {
    ($($t:ident),*) => {$(
        impl<R: Effect> Input for $t<R> {
            fn check(self) -> Result<Self, Vec<(String, String)>> {
                Ok(self)
            }
        }
    )*};
}
unrefined!(Claim, Finish, Cancel);

fn job<S: Tables, R: Effect>(db: &Db<S>, id: R::Id) -> Result<R, Refusal> {
    db.tab::<R>()
        .at(&R::key_of(id))
        .ok_or_else(|| Refusal::new(format!("{}: no such job", R::TABLE)))
}

fn role<R: Effect>(ctx: &Ctx) -> Result<(), Refusal> {
    match R::ROLE {
        Some(r) if !ctx.has_role(r) => Err(Refusal::new(format!("{}: only the {r} role runs it", R::TABLE))),
        _ => Ok(()),
    }
}

fn claim<S: Tables, R: Effect>(ctx: &Ctx, db: &mut Db<S>, i: Claim<R>) -> Result<(), Refusal> {
    role::<R>(ctx)?;
    let mut r = job::<S, R>(db, i.id)?;
    match r.job().due(R::LEASE_MS) {
        Some(due) if ctx.now >= due => *r.job_mut() = Job::Claimed { by: i.by, at_ms: ctx.now },
        Some(_) => return Err(Refusal::new(format!("{}: not due", R::TABLE))),
        None => return Err(Refusal::new(format!("{}: over", R::TABLE))),
    }
    db.tab::<R>().update(r)
}

fn finish<S: Tables, R: Effect>(ctx: &Ctx, db: &mut Db<S>, i: Finish<R>) -> Result<(), Refusal> {
    role::<R>(ctx)?;
    let mut r = job::<S, R>(db, i.id)?;
    if !matches!(r.job(), Job::Claimed { by, .. } if *by == i.by) {
        return Err(Refusal::new(format!("{}: not claimed by {}", R::TABLE, i.by)));
    }
    *r.job_mut() = match i.outcome {
        Ok(t) => Job::Done(t),
        Err(why) => Job::Failed {
            why,
            retry_ms: ctx.now + R::RETRY_MS,
        },
    };
    db.tab::<R>().update(r)
}

fn cancel<S: Tables, R: Effect>(ctx: &Ctx, db: &mut Db<S>, i: Cancel<R>) -> Result<(), Refusal> {
    let mut r = job::<S, R>(db, i.id)?;
    match r.owner() {
        Some(o) if o != ctx.user() => return Err(Refusal::new(format!("{}: not yours", R::TABLE))),
        None => role::<R>(ctx)?,
        Some(_) => {}
    }
    if !matches!(r.job(), Job::Pending { .. }) {
        return Err(Refusal::new(format!("{}: not pending", R::TABLE)));
    }
    *r.job_mut() = Job::Cancelled;
    db.tab::<R>().update(r)
}

fn due<S: Tables, R: Effect>(ctx: &Ctx, db: &Db<S>, _: ()) -> Result<Due<R>, Refusal> {
    let open = db.tab::<R>().scan().into_iter().filter_map(|r| Some((r.job().due(R::LEASE_MS)?, r)));
    let (now, later): (Vec<_>, Vec<_>) = open.partition(|(d, _)| *d <= ctx.now);
    let next_ms = later.iter().map(|(d, _)| *d).min();
    Ok(Due {
        rows: now.into_iter().map(|(_, r)| r).collect(),
        next_ms,
    })
}
