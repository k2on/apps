//! `docs/plan-auth.md` A table's rules, from the vocabulary to the wire.
//!
//! The vocabulary says a rule as a domain reads it — `visible(Everyone)`,
//! `writable(Role("library"))`, `Self::user_id.is(Me)`, the one lookup
//! through a reference — and this holds it to the IR the schema carries,
//! which is what is encoded, hashed and enforced.

#![allow(non_upper_case_globals, dead_code)]

use ark::authoring::*;
use ark::ir::{CmpOp, Expr, Pred as IrPred};
use ark::schema::{check_schema, Schema};

pub struct Owner {
    pub id: Id<Owner>,
    pub user_id: Text,
    pub name: Text,
}
impl Row for Owner {
    const NAME: &str = "owner";
    type Key = (Id<Owner>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::user_id)
            .text(Self::name)
            .key((Self::id,))
            .visible(Self::user_id.is(Me).or(exists(Member::owner_id, Member::user_id.is(Me))))
            .writable(Self::user_id.is(Me))
    }
}
impl Owner {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const name: Col<Self, Text> = col("name");
}

pub struct Member {
    pub id: Id<Member>,
    pub owner_id: Id<Owner>,
    pub user_id: Text,
}
impl Row for Member {
    const NAME: &str = "member";
    type Key = (Id<Member>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .id(Self::owner_id)
            .refs::<Owner>()
            .text(Self::user_id)
            .key((Self::id,))
            .visible(Everyone)
            .writable(Role("library"))
    }
}
impl Member {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const owner_id: Col<Self, Id<Owner>> = col("owner_id");
    pub const user_id: Col<Self, Text> = col("user_id");
}

/// The vocabulary writes the IR the design names: `Me` is the context's
/// user, a role is a leaf, the lookup names the referencing table and its
/// column with that table's own predicate, and `Everyone` is nothing at
/// all. Falsified by writing `Everyone` as an always-true predicate (an
/// empty `All`): the `member` table's `visible` was `Some`.
#[test]
fn the_vocabulary_writes_the_rule_the_schema_carries() {
    let me = |c: &str| IrPred::Cmp(c.into(), CmpOp::Eq, Expr::CtxUser);
    let owner = table_of::<Owner>();
    assert_eq!(
        owner.visible,
        Some(IrPred::Any(vec![
            me("user_id"),
            IrPred::Exists("member".into(), "owner_id".into(), Box::new(me("user_id")))
        ]))
    );
    assert_eq!(owner.writable, Some(me("user_id")));
    let member = table_of::<Member>();
    assert_eq!(member.visible, None, "Everyone is not written down");
    assert_eq!(member.writable, Some(IrPred::Role("library".into())));
    let sch = Schema { tables: vec![owner, member] };
    assert_eq!(check_schema(&sch), vec![]);
}

pub struct Db {
    pub owner: Table<Owner>,
    pub member: Table<Member>,
}
impl Tables for Db {
    fn open() -> Self {
        Db {
            owner: table(),
            member: table(),
        }
    }
}

/// A rule's leaves are a table's and are refused in a plan: a role is asked
/// of an identity, and a read has none. The same module with the filter on
/// a column instead verifies. Falsified by letting `pred_ok` accept a
/// role: the module built.
#[test]
fn a_role_in_a_plan_is_refused() {
    let routes = |role: bool| {
        let r = router::<Db>("r");
        r.routes((r.query("owners", move |_ctx, db, _input: ()| {
            if role {
                db.owner.filter(Pred::from(Role("admin")))
            } else {
                db.owner.filter(Owner::name.eq("x"))
            }
        }),))
    };
    assert!(Module::new((routes(false),)).try_build().is_ok());
    let errs = Module::new((routes(true),)).try_build().expect_err("a role in a filter is refused");
    assert!(errs.iter().any(|e| e.contains("RuleLeafInPlan")), "{errs:?}");
}

// A domain over `owner` and `member` ---------------------------------------

pub struct Make {
    pub id: Id<Owner>,
    pub name: Text,
}
impl Input for Make {
    fn schema() -> Object<Self> {
        object().field("id", id::<Owner>()).field("name", text())
    }
}

pub struct Join {
    pub owner_id: Id<Owner>,
    pub user_id: Text,
}
impl Input for Join {
    fn schema() -> Object<Self> {
        object().field("owner_id", id::<Owner>().exists()).field("user_id", text())
    }
}

pub struct Leave {
    pub id: Id<Member>,
}
impl Input for Leave {
    fn schema() -> Object<Self> {
        object().field("id", id::<Member>())
    }
}

/// `make` writes an owner row under an id the caller names, as the context's
/// user — so another user naming the same id is an edit of somebody else's
/// row; `join` adds a member (the `library` role's to write); `leave`
/// removes one.
fn domain() -> Module {
    let r = router::<Db>("r");
    Module::new((r.routes((
        r.input::<Make>().mutation("make", |ctx, db, input| {
            db.owner.upsert(Owner {
                id: input.id,
                user_id: ctx.user,
                name: input.name,
            })
        }),
        r.input::<Join>().mutation("join", |ctx, db, input| {
            db.member.insert(Member {
                id: ctx.new_id("id"),
                owner_id: input.owner_id,
                user_id: input.user_id,
            })
        }),
        r.input::<Leave>().mutation("leave", |_ctx, db, input| db.member.delete((input.id,))),
    )),))
}

mod net {
    use std::collections::BTreeMap;

    use ark::eval::{Args, Ctx};
    use ark::hash::FnHash;
    use ark::live::Silent;
    use ark::peer::{Authority, Replica};
    use ark::protocol::{open_access, trusting, Client, Mode, Server};
    use ark::schema::Schema;
    use ark::store::{MemoryStore, Refusal};
    use ark::value::{Id, Value};

    /// A trusting server and clients on a perfect network: a token is a
    /// name, `name:role,role` a name holding roles.
    pub struct Net {
        pub sv: Server<Silent>,
        pub clients: BTreeMap<i64, Client>,
        pub schema: Schema,
        pub fns: BTreeMap<String, FnHash>,
        next: u8,
    }

    impl Net {
        pub fn new(m: &ark::authoring::Module) -> Net {
            let built = m.build();
            let bodies = ark::hash::closures(built);
            let fns = m.procedures().into_iter().map(|(h, p)| (p.name().to_string(), h)).collect();
            Net {
                sv: Server::open(trusting(), open_access(), Silent, Authority::new(built.schema.clone(), bodies)),
                clients: BTreeMap::new(),
                schema: built.schema.clone(),
                fns,
                next: 0,
            }
        }

        /// A client at 0 on connection `c`, signed in with `token`.
        pub fn join(&mut self, c: i64, token: &str) {
            let bodies = self.sv.authority.bodies.clone();
            let r = Replica::open(self.schema.clone(), bodies, MemoryStore::empty(self.schema.clone()), 0, vec![]);
            let mut cl = Client::open(r, Mode::Whole, Some(token.into()));
            cl.connected();
            self.clients.insert(c, cl);
            self.pump();
        }

        /// Connection `c` drops and comes back with `token`.
        pub fn reconnect(&mut self, c: i64, token: &str) {
            self.sv.disconnect(c);
            let cl = self.clients.get_mut(&c).unwrap();
            cl.disconnected();
            cl.token = Some(token.into());
            cl.connected();
            self.pump();
        }

        pub fn id(&mut self) -> Id {
            self.next += 1;
            let mut id = [0u8; 16];
            id[15] = self.next;
            id
        }

        /// Client `c` authors `function` as `ctx`, and everything is
        /// delivered: the local verdict.
        pub fn mutate(&mut self, c: i64, ctx: &Ctx, function: &str, args: Args) -> Result<(), Refusal> {
            let fh = self.fns[function].clone();
            let (eid, auto) = (self.id(), self.id());
            let autos: Args = [("id".to_string(), Value::Id(auto))].into_iter().collect();
            let out = self.clients.get_mut(&c).unwrap().mutate(eid, ctx, &fh, &autos, &args).map(|_| ());
            self.pump();
            out
        }

        /// Deliver everything, both ways, until nothing moves.
        pub fn pump(&mut self) {
            for _ in 0..100 {
                let mut moved = false;
                for (c, cl) in self.clients.iter_mut() {
                    for m in cl.take_outgoing() {
                        moved = true;
                        self.sv.recv(*c, m);
                    }
                }
                for (to, m) in self.sv.take_outgoing() {
                    moved = true;
                    if let Some(cl) = self.clients.get_mut(&to) {
                        cl.recv(m);
                    }
                }
                for cl in self.clients.values_mut() {
                    cl.settle();
                }
                if !moved {
                    return;
                }
            }
            panic!("the network did not settle");
        }
    }
}

fn args<const N: usize>(pairs: [(&str, ark::value::Value); N]) -> ark::eval::Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// `docs/plan-auth.md` A write is held to the table's `writable` rule after
/// the run: on the device for its own login's roles, before anything is
/// pending, and at the authority for the connection's, before anything is
/// sequenced. A device that believes it holds a role it does not is
/// refused by the authority with `Forbidden` — rolled back, answered as any
/// refusal, and landing nowhere; an edit is held on its old row too, so
/// writing over somebody else's row is refused whatever the new row says.
/// Falsified by leaving the check out of `Authority::sequence_as` (the
/// believer's member row was sequenced), and out of `Replica::mutate` (bob's
/// own device recorded the forbidden intent pending).
#[test]
fn a_write_is_held_to_its_tables_rule_on_the_device_and_at_the_authority() {
    use ark::eval::Ctx;
    use ark::store::{Refusal, Store};
    use ark::value::Value;

    let m = domain();
    let mut net = net::Net::new(&m);
    net.join(1, "alice");
    net.join(2, "bob");
    net.join(3, "carol:library");
    let alice = Ctx::new("alice", "dev");
    let bob = Ctx::new("bob", "dev");
    let carol = Ctx::new("carol", "dev").with_roles(["library"]);
    let o = Value::Id(net.id());
    net.mutate(1, &alice, "make", args([("id", o.clone()), ("name", Value::text("mine"))]))
        .expect("alice writes her own row");
    // Bob, naming alice's row: refused on his own device, nothing pending.
    let forbidden = Err(Refusal::Forbidden("owner".into()));
    assert_eq!(
        net.mutate(2, &bob, "make", args([("id", o.clone()), ("name", Value::text("theirs"))])),
        forbidden
    );
    assert!(net.clients[&2].replica.pending.is_empty(), "a forbidden intent is never pending");
    // Members are the library's: carol may, bob may not.
    let join = || args([("owner_id", o.clone()), ("user_id", Value::text("bob"))]);
    net.mutate(3, &carol, "join", join()).expect("the library writes members");
    assert_eq!(net.mutate(2, &bob, "join", join()), Err(Refusal::Forbidden("member".into())));
    // A device that believes it holds the role: its preview takes it, the
    // authority does not, and it lands nowhere.
    let believer = Ctx::new("bob", "dev").with_roles(["library"]);
    net.mutate(2, &believer, "join", join()).expect("the believer's own view takes it");
    let refused = &net.clients[&2].replica.rejections;
    assert_eq!(
        refused.last().map(|(_, r)| r),
        Some(&Refusal::Refused("member: not this login's to write".into()))
    );
    assert!(net.clients[&2].replica.pending.is_empty());
    let a = &net.sv.authority;
    assert_eq!(a.log.head_seq(), 2, "two entries: alice's row and carol's member");
    assert_eq!(a.store.scan("member").len(), 1);
    assert_eq!(a.store.scan("owner")[0].get("name"), Some(&Value::text("mine")));
    for (i, c) in &net.clients {
        assert_eq!(c.replica.verify_at(), (2, ark::hash::state_hash(&a.store)), "client {i} converged");
    }
}
