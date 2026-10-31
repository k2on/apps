//! The demo's household: three months of a family's money, up to today —
//! what the demo window (`money/iced`) and the phones' demo and previews
//! (`money/ffi`, `money_demo`) open on, behind the `seed` feature.
//!
//! Everything goes in through the domain's own mutations, the ones every
//! peer runs — accounts opened, a budget laid out, paydays, bills, a card
//! the bank feeds through Plaid ([`Bank`]) — applied once each over one
//! store and kept as the snapshot a peer alone opens on, which is what
//! harken's demo does with its library (`harken/iced/src/seed.rs`). The
//! dates are today's, so the month on screen is this one; the amounts are
//! drawn from a fixed sequence, so every load is the same household.
//!
//! Two people keep this budget, as a household signed in through its
//! OpenID provider would: the window is Alex's, and some of what is in it
//! Sam entered.

use std::collections::BTreeMap;

use ark::format::Encode;
use ark::log::{EntryId, Record, Snapshot};
use ark::peer::Ctx;
use ark::store::Store;
use ark_peer::{Config, Disk, Memory, Peer as Replica};

use crate::dates::{add_days, add_months, day_of_ms, month_of};
use crate::plaid::{import_of, PlaidAccount, PlaidRemoved, PlaidTransaction, Pulled};
use crate::*;

fn domain() -> std::sync::Arc<ark::domain::Domain> {
    crate::shared()
}

/// Whose window the demo is.
pub const ME: &str = "alex";
/// The household's other member.
pub const PARTNER: &str = "sam";
const LOCAL: &str = "local";

/// The bank the card is at, and its account there.
pub const BANK_NAME: &str = "chase";
const INSTITUTION: &str = "Chase";
const CARD_ID: &str = "chase-sapphire";
const CARD_NAME: &str = "Chase Sapphire";

/// The fixed sequence the amounts are drawn from.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Dollars between `lo` and `hi`, to the cent.
    fn cents(&mut self, lo: i64, hi: i64) -> i64 {
        lo * 100 + (self.next() % ((hi - lo) as u64 * 100 + 1)) as i64
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

/// The budget's shape: groups, their categories and each one's monthly
/// target in dollars.
const BUDGET: &[(&str, &[(&str, i64)])] = &[
    (
        "Bills",
        &[
            ("Rent", 1_850),
            ("Electric", 120),
            ("Water", 45),
            ("Internet", 70),
            ("Phone", 85),
            ("Insurance", 160),
        ],
    ),
    (
        "Everyday",
        &[("Groceries", 650), ("Dining Out", 200), ("Gas & Transit", 180), ("Household", 80)],
    ),
    (
        "Quality of Life",
        &[("Entertainment", 60), ("Subscriptions", 45), ("Clothing", 75), ("Health", 60)],
    ),
    (
        "Savings Goals",
        &[("Emergency Fund", 300), ("Vacation", 250), ("Gifts", 50), ("Car Maintenance", 60)],
    ),
];

/// What the household spends on, where and when: a category, the payees
/// it goes to, a range in dollars, the days of the month, and whether it
/// goes on the card (which the bank tells of) or is entered by hand.
struct Habit {
    category: &'static str,
    payees: &'static [&'static str],
    lo: i64,
    hi: i64,
    days: &'static [i64],
    card: bool,
}

const HABITS: &[Habit] = &[
    Habit {
        category: "Rent",
        payees: &["Greenway Property Mgmt"],
        lo: 1_850,
        hi: 1_850,
        days: &[1],
        card: false,
    },
    Habit {
        category: "Insurance",
        payees: &["State Farm"],
        lo: 160,
        hi: 160,
        days: &[3],
        card: false,
    },
    Habit {
        category: "Internet",
        payees: &["Sonic Fiber"],
        lo: 70,
        hi: 70,
        days: &[8],
        card: false,
    },
    Habit {
        category: "Electric",
        payees: &["City Power & Light"],
        lo: 88,
        hi: 142,
        days: &[12],
        card: false,
    },
    Habit {
        category: "Water",
        payees: &["Municipal Water"],
        lo: 36,
        hi: 52,
        days: &[18],
        card: false,
    },
    Habit {
        category: "Phone",
        payees: &["Mint Mobile"],
        lo: 85,
        hi: 85,
        days: &[21],
        card: false,
    },
    Habit {
        category: "Health",
        payees: &["Walgreens"],
        lo: 12,
        hi: 38,
        days: &[18],
        card: false,
    },
    Habit {
        category: "Groceries",
        payees: &["Trader Joe's", "Whole Foods Market", "Costco"],
        lo: 42,
        hi: 165,
        days: &[2, 6, 9, 13, 16, 20, 23, 27, 30],
        card: true,
    },
    Habit {
        category: "Dining Out",
        payees: &["Chipotle", "Blue Bottle Coffee", "Pizzeria Delfina", "Thai Basil"],
        lo: 9,
        hi: 68,
        days: &[5, 11, 14, 19, 22, 25, 28],
        card: true,
    },
    Habit {
        category: "Gas & Transit",
        payees: &["Shell", "Chevron", "Clipper Card"],
        lo: 30,
        hi: 62,
        days: &[7, 17, 26],
        card: true,
    },
    Habit {
        category: "Subscriptions",
        payees: &["Netflix"],
        lo: 15,
        hi: 15,
        days: &[4],
        card: true,
    },
    Habit {
        category: "Subscriptions",
        payees: &["Spotify"],
        lo: 17,
        hi: 17,
        days: &[10],
        card: true,
    },
    Habit {
        category: "Entertainment",
        payees: &["AMC Theatres", "Steam"],
        lo: 18,
        hi: 45,
        days: &[15, 29],
        card: true,
    },
    Habit {
        category: "Clothing",
        payees: &["Uniqlo", "Target"],
        lo: 28,
        hi: 90,
        days: &[12],
        card: true,
    },
    Habit {
        category: "Household",
        payees: &["Target", "Home Depot"],
        lo: 14,
        hi: 70,
        days: &[10, 24],
        card: true,
    },
];

/// The day `d` of `month`, when it has come yet.
fn day_in(month: &str, d: i64, today: &str) -> Option<String> {
    let (y, m) = crate::dates::parse_month(month)?;
    let day = crate::dates::day(y, m, d.min(crate::dates::days_in_month(y, m)));
    (day.as_str() <= today).then_some(day)
}

/// The demo's mutations, run once each over one store.
struct Seeder {
    store: Store,
    ctx: Ctx,
    local: i64,
}

impl Seeder {
    fn new(now: i64) -> Seeder {
        Seeder {
            store: Store::new(domain().schema.clone(), ark::schema::Space::Durable),
            ctx: Ctx {
                id: EntryId::of(LOCAL, 0),
                actor: ME.into(),
                session: LOCAL.into(),
                roles: vec![BANK.into()],
                now,
                ..Default::default()
            },
            local: 0,
        }
    }

    /// One mutation by the name the domain binds it to, as `who`. A seed
    /// mutation is refused only by a mistake, so a refusal is a panic.
    fn run(&mut self, who: &str, name: &str, input: &impl Encode) {
        self.local += 1;
        self.ctx.id = EntryId::of(LOCAL, self.local);
        self.ctx.now += 1;
        self.ctx.actor = who.into();
        let d = domain();
        let b = d.binding(name).unwrap_or_else(|| panic!("the demo authors {name}"));
        let f = d.get(b).and_then(|f| f.proc.clone()).unwrap_or_else(|| panic!("{name} writes nothing"));
        f.apply(&mut self.store, &self.ctx, &input.to_bytes())
            .unwrap_or_else(|r| panic!("the demo could not {name}: {r}"));
    }

    fn ask<T: Encode>(&self, name: &str, input: &impl Encode) -> Vec<T> {
        let d = domain();
        let b = d.binding(name).unwrap_or_else(|| panic!("the demo reads {name}"));
        let bytes = d
            .ask(&self.store, &self.ctx, b, &input.to_bytes())
            .unwrap_or_else(|r| panic!("the demo reads {name}: {r}"));
        Vec::<T>::from_bytes(&bytes).unwrap_or_else(|r| panic!("{name}: {r}"))
    }

    fn rows<R: ark::row::Row>(&self) -> Vec<R> {
        ark::plan::rows::<R>(&self.store)
    }

    fn account(&self, name: &str) -> Id<Account> {
        self.rows::<Account>()
            .into_iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no account {name}"))
            .id
    }

    fn category(&self, name: &str) -> Id<Category> {
        self.rows::<Category>()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no category {name}"))
            .id
    }

    fn open(&mut self, name: &str, kind: &str, on_budget: bool, balance: i64, date: &str) {
        let a = OpenAccount {
            name: name.into(),
            kind: kind.into(),
            on_budget,
            balance,
            date: date.into(),
        };
        self.run(ME, "open_account", &a);
    }

    #[allow(clippy::too_many_arguments)]
    fn txn(&mut self, who: &str, account: &str, date: &str, payee: &str, category: Option<&str>, amount: i64, cleared: bool) {
        let t = AddTransaction {
            account_id: self.account(account),
            date: date.into(),
            payee: payee.into(),
            category_id: category.map(|c| self.category(c)),
            amount,
            cleared,
            ..AddTransaction::default()
        };
        self.run(who, "add_transaction", &t);
    }

    fn transfer(&mut self, from: &str, to: &str, date: &str, amount: i64) {
        let t = AddTransaction {
            account_id: self.account(from),
            date: date.into(),
            amount: -amount,
            cleared: true,
            transfer_to: Some(self.account(to)),
            ..AddTransaction::default()
        };
        self.run(ME, "add_transaction", &t);
    }

    fn assign(&mut self, month: &str, category: &str, cents: i64) {
        let a = Assign {
            month: month.into(),
            category_id: self.category(category),
            amount: cents,
        };
        self.run(ME, "assign", &a);
    }

    fn ready_to_assign(&self, month: &str) -> i64 {
        self.ask::<Summary>("summary", &Month { month: month.into() })
            .first()
            .map_or(0, |s| s.ready_to_assign)
    }
}

/// The card as Plaid's `/accounts/get` says it, owing `owed` (our sign).
pub fn card(owed: i64) -> PlaidAccount {
    PlaidAccount {
        account_id: CARD_ID.into(),
        name: CARD_NAME.into(),
        mask: Some("4417".into()),
        kind: "credit".into(),
        subtype: Some("credit card".into()),
        balances: crate::plaid::Balances {
            current: Some(-owed as f64 / 100.0),
        },
    }
}

/// A card transaction as Plaid's `/transactions/sync` says it: dollars,
/// money leaving positive.
fn plaid_txn(id: &str, date: &str, payee: &str, cents: i64, pending: bool, replaces: Option<&str>) -> PlaidTransaction {
    PlaidTransaction {
        transaction_id: id.into(),
        account_id: CARD_ID.into(),
        amount: -cents as f64 / 100.0,
        date: date.into(),
        name: payee.to_uppercase(),
        merchant_name: Some(payee.into()),
        original_description: Some(format!("{} {}", payee.to_uppercase(), &id[id.len().saturating_sub(4)..])),
        pending,
        pending_transaction_id: replaces.map(Into::into),
    }
}

/// The household, three months of it up to `today`.
pub fn seeded_disk(today: &str, now: i64) -> Memory {
    let this = month_of(today);
    let months = [add_months(&this, -2), add_months(&this, -1), this.clone()];
    let first = format!("{}-01", months[0]);
    let mut s = Seeder::new(now - 1_000_000);
    let mut draw = Draw(7);

    s.run(ME, "setup", &());
    s.open("Checking", "checking", true, 284_250, &first);
    s.open("Savings", "savings", true, 615_000, &first);
    s.open("Cash", "cash", true, 12_000, &first);
    s.open("Mortgage", "loan", false, -31_245_000, &first);
    s.open("Retirement", "investment", false, 6_430_000, &first);
    for (group, categories) in BUDGET {
        s.run(ME, "add_group", &AddGroup { name: group.to_string() });
        let g = s.rows::<Group>().into_iter().find(|g| g.name == *group).expect("the group just made").id;
        for (name, target) in *categories {
            s.run(
                ME,
                "add_category",
                &AddCategory {
                    group_id: g,
                    name: name.to_string(),
                },
            );
            let id = s.category(name);
            s.run(ME, "set_target", &SetTarget { id, target: target * 100 });
        }
    }

    let mut owed = 0;
    let mut card_spent: BTreeMap<String, i64> = BTreeMap::new();
    let mut n = 0;
    for (k, month) in months.iter().enumerate() {
        let current = k == 2;
        if current {
            // Checking reconciled at the end of last month, as the
            // statement said.
            let checking = s.account("Checking");
            let accounts = s.ask::<AccountLine>("accounts", &());
            let balance = accounts.iter().find(|a| a.id == checking).map_or(0, |a| a.cleared);
            let date = add_days(&format!("{month}-01"), -1);
            s.run(
                ME,
                "reconcile",
                &Reconcile {
                    account_id: checking,
                    balance,
                    date,
                },
            );
        }
        // Paydays, both of them.
        for (who, payee, cents, days) in [
            (ME, "Acme Corp", 315_000, &[1, 15][..]),
            (PARTNER, "Riverside Schools", 224_000, &[1][..]),
        ] {
            for d in days {
                if let Some(day) = day_in(month, *d, today) {
                    s.txn(who, "Checking", &day, payee, Some("Ready to Assign"), cents, true);
                }
            }
        }
        // The budget: every target, and then what is left parked in the
        // emergency fund — except this month, where some of it is left to
        // assign and a couple of categories are short.
        for (_, categories) in BUDGET {
            for (name, target) in *categories {
                let amount = match (current, *name) {
                    (true, "Vacation" | "Gifts") => 0,
                    (true, "Dining Out") => 12_000,
                    _ => target * 100,
                };
                s.assign(month, name, amount);
            }
        }
        let leave = if current { 128_450 } else { 0 };
        let extra = s.ready_to_assign(month) - leave;
        let ef = (30_000 + extra).max(0);
        s.assign(month, "Emergency Fund", ef);

        // Last month's card statement, paid from checking: the transfer is
        // entered by hand, and matched by the bank's payment when it comes.
        let mut card_txns: Vec<PlaidTransaction> = Vec::new();
        if k > 0 {
            if let (Some(day), Some(spent)) = (day_in(month, 5, today), card_spent.get(&months[k - 1])) {
                s.transfer("Checking", CARD_NAME, &day, *spent);
                n += 1;
                card_txns.push(plaid_txn(&format!("pay{n:04}"), &day, "Payment Thank You", *spent, false, None));
                owed += spent;
            }
        }
        if let Some(day) = day_in(month, 2, today) {
            s.transfer("Checking", "Savings", &day, 25_000);
        }
        if let Some(day) = day_in(month, 9, today).filter(|_| k == 1) {
            s.txn(PARTNER, "Checking", &day, "Jiffy Lube", Some("Car Maintenance"), -8_999, true);
        }
        if let Some(day) = day_in(month, 7, today) {
            s.txn(PARTNER, "Cash", &day, "Farmers Market", Some("Groceries"), -draw.cents(18, 32), true);
        }

        // What the household spends: by hand on checking, on the card as
        // the bank will tell of it.
        let mut payee_category: BTreeMap<&str, &str> = BTreeMap::new();
        for h in HABITS {
            for d in h.days {
                let Some(day) = day_in(month, *d, today) else { continue };
                let payee = draw.pick(h.payees);
                let cents = -draw.cents(h.lo, h.hi);
                payee_category.insert(payee, h.category);
                if h.card {
                    n += 1;
                    let pending = current && day == today;
                    card_txns.push(plaid_txn(&format!("txn{n:04}"), &day, payee, cents, pending, None));
                    if !pending {
                        *card_spent.entry(month.clone()).or_default() -= cents;
                        owed += cents;
                    }
                } else {
                    let who = if draw.next().is_multiple_of(3) { PARTNER } else { ME };
                    let cleared = !current || add_days(&day, 3).as_str() < today;
                    s.txn(who, "Checking", &day, payee, Some(h.category), cents, cleared);
                }
            }
        }

        // The bank's sync for the month, as the importer authors it; then
        // the household files what the card bought — all of it, but the
        // last few days of this month, which are still waiting to be looked
        // at. The first month's import opens the card here.
        let pulled = Pulled {
            added: card_txns,
            cursor: format!("cursor-{month}"),
            ..Pulled::default()
        };
        s.run(ME, "import", &import_of(BANK_NAME, INSTITUTION, &[card(owed)], &pulled));
        let card = s.account(CARD_NAME);
        let lines = s.ask::<TxnLine>("register", &Register { account_id: Some(card) });
        let recent = add_days(today, -4);
        let mut filed: BTreeMap<Id<Category>, Vec<Id<Txn>>> = BTreeMap::new();
        for l in lines.iter().filter(|l| !l.approved && !l.transfer && !(current && l.date >= recent)) {
            if let Some(c) = payee_category.get(l.payee.as_str()) {
                filed.entry(s.category(c)).or_default().push(l.id);
            }
        }
        for (c, ids) in filed {
            s.run(ME, "categorize", &Categorize { ids, category_id: Some(c) });
        }
        let approve: Vec<Id<Txn>> = lines.iter().filter(|l| !l.approved && l.transfer).map(|l| l.id).collect();
        if !approve.is_empty() {
            s.run(ME, "approve", &Txns { ids: approve });
        }
        // What the card bought beyond its categories' money is debt the
        // household covers at the month's end, from the emergency fund —
        // so the statement it pays next month is all set aside. This
        // month's last days are not filed yet: their debt waits.
        if !current {
            let pay = crate::cards::payment_category(card);
            let short = s
                .ask::<BudgetLine>("month", &Month { month: month.clone() })
                .into_iter()
                .find(|l| l.category_id == pay)
                .map_or(0, |l| l.underfunded);
            if short > 0 {
                let m = MoveMoney {
                    month: month.clone(),
                    from: Some(s.category("Emergency Fund")),
                    to: Some(pay),
                    amount: short,
                };
                s.run(ME, "move_money", &m);
            }
        }
    }

    let snapshot = Snapshot {
        seq: 0,
        hash: Default::default(),
        state: s.store.state_hash(),
        rows: s.store.snapshot_rows(),
    };
    let mut disk = Memory::default();
    disk.record(&Record::Rows(snapshot.kv_rows()).enc()).expect("memory takes it");
    ark_peer::migrate::put_schema(&mut disk, s.store.schema_arc()).expect("memory takes it");
    ark_peer::local::preset(&mut disk, |r| (r.actor, r.session) = (Some(ME.into()), LOCAL.into())).expect("memory takes it");
    disk
}

/// The demo's peer: the seeded household, its own authority, holding the
/// bank importer's role so that "Sync" can run the import here as a
/// server's importer would.
pub fn open(disk: Memory) -> Replica {
    let mut peer = Replica::open_as(Config::of(domain()), Box::new(disk), None);
    peer.set_roles([BANK]);
    peer.alone();
    peer
}

/// The demo, seeded for today.
pub fn seeded() -> Replica {
    let now = ark_peer::now_ms();
    open(seeded_disk(&day_of_ms(now), now))
}

/// The card's bank, standing in for Plaid when "Sync" is pressed: each sync
/// is one `/transactions/sync` run from the last cursor. The first brings a
/// coffee still pending and a fill-up; the second posts the coffee with
/// its tip (the pending one replaced, as Plaid does it) and brings an
/// order; after that the bank has nothing new.
#[derive(Debug, Default, Clone)]
pub struct Bank {
    pub syncs: u32,
}

impl Bank {
    /// The next run's import, owing `owed` before it.
    pub fn next(&mut self, today: &str, owed: i64) -> Import {
        self.syncs += 1;
        let k = self.syncs;
        let pulled = match k {
            1 => Pulled {
                added: vec![
                    plaid_txn("live-coffee", today, "Blue Bottle Coffee", -650, true, None),
                    plaid_txn("live-shell", today, "Shell", -4_820, false, None),
                ],
                ..Pulled::default()
            },
            2 => Pulled {
                added: vec![
                    plaid_txn("live-coffee-posted", today, "Blue Bottle Coffee", -775, false, Some("live-coffee")),
                    plaid_txn("live-amazon", today, "Amazon", -2_399, false, None),
                ],
                removed: vec![PlaidRemoved {
                    transaction_id: "live-coffee".into(),
                    account_id: CARD_ID.into(),
                }],
                ..Pulled::default()
            },
            _ => Pulled::default(),
        };
        let added: i64 = pulled.added.iter().filter(|t| !t.pending).map(|t| -crate::plaid::cents(t.amount)).sum();
        let pulled = Pulled {
            cursor: format!("live-{k}"),
            ..pulled
        };
        import_of(BANK_NAME, INSTITUTION, &[card(owed + added)], &pulled)
    }
}
