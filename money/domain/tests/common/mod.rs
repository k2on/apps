//! One budget, one store, every mutation run over it as the machine runs
//! it, every query asked of it.

#![allow(dead_code)]

use std::sync::OnceLock;

use ark::domain::Domain;
use ark::format::Encode;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::store::{Change, Store};
use money_domain::*;

/// The domain, assembled once.
pub fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(money_domain::domain)
}

/// A function's binding by its last name: `assign` is `budget.assign`.
pub fn full(name: &str) -> String {
    let hit: Vec<&String> = domain().fns.keys().filter(|b| b.rsplit('.').next() == Some(name)).collect();
    match hit[..] {
        [b] => b.clone(),
        _ => panic!("{name}: {hit:?}"),
    }
}

pub struct Book {
    pub store: Store,
    /// What the last mutation changed.
    pub last: Vec<Change>,
    local: i64,
    now: i64,
}

impl Default for Book {
    fn default() -> Self {
        Book::new()
    }
}

impl Book {
    pub fn new() -> Book {
        Book {
            store: Store::new(domain().schema.clone(), ark::schema::Space::Durable),
            last: vec![],
            local: 0,
            // 2026-10-26.
            now: 1_793_000_000_000,
        }
    }

    pub fn ctx(&mut self, user: &str, roles: &[&str]) -> Ctx {
        self.local += 1;
        self.now += 1;
        Ctx {
            id: EntryId::of("s", self.local),
            actor: user.into(),
            session: "s".into(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            now: self.now,
            ..Default::default()
        }
    }

    /// Author as `user`: the number of changes, or the refusal's text.
    pub fn as_(&mut self, user: &str, name: &str, input: &impl Encode) -> Result<usize, String> {
        let ctx = self.ctx(user, &[]);
        self.apply(&ctx, name, input)
    }

    /// Author as alice.
    pub fn run(&mut self, name: &str, input: &impl Encode) -> Result<usize, String> {
        self.as_("alice", name, input)
    }

    /// Author as the bank importer.
    pub fn import(&mut self, input: &Import) -> Result<usize, String> {
        let ctx = self.ctx("importer", &[BANK]);
        self.apply(&ctx, "import", input)
    }

    pub fn apply(&mut self, ctx: &Ctx, name: &str, input: &impl Encode) -> Result<usize, String> {
        let f = domain()
            .get(&full(name))
            .and_then(|f| f.proc.clone())
            .unwrap_or_else(|| panic!("{name} writes nothing"));
        self.last = f.apply(&mut self.store, ctx, &input.to_bytes()).map_err(|r| r.0)?;
        Ok(self.last.len())
    }

    /// A query that must answer.
    pub fn list<T: Encode>(&self, name: &str, input: &impl Encode) -> Vec<T> {
        let ctx = Ctx {
            id: EntryId::of("q", 0),
            actor: "bob".into(),
            session: "q".into(),
            now: self.now,
            ..Default::default()
        };
        let b = domain()
            .ask(&self.store, &ctx, &full(name), &input.to_bytes())
            .unwrap_or_else(|r| panic!("{name}: {r}"));
        Vec::<T>::from_bytes(&b).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    pub fn rows<T: Encode>(&self, table: &str) -> Vec<T> {
        self.store.rows(table).map(|r| T::from_bytes(r).unwrap()).collect()
    }

    // * Shorthand

    pub fn group(&mut self, name: &str) -> Id<Group> {
        self.run("add_group", &AddGroup { name: name.into() }).unwrap();
        self.rows::<Group>("group").into_iter().find(|g| g.name == name).unwrap().id
    }

    pub fn category(&mut self, group: Id<Group>, name: &str) -> Id<Category> {
        self.run(
            "add_category",
            &AddCategory {
                group_id: group,
                name: name.into(),
            },
        )
        .unwrap();
        self.rows::<Category>("category").into_iter().find(|c| c.name == name).unwrap().id
    }

    pub fn account(&mut self, name: &str, kind: &str, balance: i64) -> Id<Account> {
        let open = OpenAccount {
            name: name.into(),
            kind: kind.into(),
            on_budget: money_domain::accounts::on_budget_by_default(kind),
            balance,
            date: "2026-09-01".into(),
        };
        self.run("open_account", &open).unwrap();
        self.rows::<Account>("account").into_iter().find(|a| a.name == name).unwrap().id
    }

    pub fn spend(&mut self, account: Id<Account>, date: &str, payee: &str, category: Option<Id<Category>>, amount: i64) {
        let t = AddTransaction {
            account_id: account,
            date: date.into(),
            payee: payee.into(),
            category_id: category,
            amount,
            ..AddTransaction::default()
        };
        self.run("add_transaction", &t).unwrap();
    }

    pub fn assign(&mut self, month: &str, category: Id<Category>, amount: i64) {
        let a = Assign {
            month: month.into(),
            category_id: category,
            amount,
        };
        self.run("assign", &a).unwrap();
    }

    pub fn summary(&self, month: &str) -> Summary {
        self.list::<Summary>("summary", &Month { month: month.into() }).pop().expect("a summary")
    }

    pub fn line(&self, month: &str, category: Id<Category>) -> BudgetLine {
        let lines: Vec<BudgetLine> = self.list("month", &Month { month: month.into() });
        lines.into_iter().find(|l| l.category_id == category).expect("the category's line")
    }

    pub fn register(&self, account: Option<Id<Account>>) -> Vec<TxnLine> {
        self.list("register", &Register { account_id: account })
    }

    pub fn accounts(&self) -> Vec<AccountLine> {
        self.list("accounts", &())
    }
}
