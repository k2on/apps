//! A finding as a vector, in the suite's own format, so that a found bug
//! becomes a permanent case by copying one file into `spec/vectors/<dir>/`
//! (`spec/README.md`, "The vectors"): a session is a `rebase/fleet-fuzz-*`
//! script, a view a `views/` case, a frame a `protocol/` frame, a verdict
//! an `eval/` case. Each is what was observed, with the expectation the
//! suite holds every runtime to — convergence, a fresh hydrate, the frame's
//! own bytes, the interpreter's verdict — and not what the broken engine
//! answered: a vector of a bug fails until the bug is fixed.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use ark::canon::encode;
use ark::eval::{self, Args, EvalFault};
use ark::hash::closure;
use ark::ir::{module_value, Module};
use ark::protocol::change_value;
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::{hex, Value};
use ark::view::{self, Env};

use super::json::{array, json, obj, quoted};
use super::session::Finding;

fn ctx_value(c: &ark::eval::Ctx) -> Value {
    Value::record(vec![("user", Value::text(c.user.clone())), ("session", Value::text(c.session.clone()))])
}

// File names say what they are, in the characters a file name may have.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

/// Write `f` under `out`, and a line for it in `out/README`. The path
/// written, relative to `out`.
pub fn write(out: &Path, seed: u64, m: &Module, f: &Finding) -> std::io::Result<String> {
    let (dir, name, text) = vector(seed, m, f);
    fs::create_dir_all(out.join(dir))?;
    let rel = format!("{dir}/{name}");
    fs::write(out.join(&rel), text)?;
    let mut readme = fs::OpenOptions::new().create(true).append(true).open(out.join("README"))?;
    let why: String = f.why().lines().next().unwrap_or("").chars().take(160).collect();
    writeln!(
        readme,
        "- `{rel}` (seed {seed}, {}): copy into `spec/vectors/{dir}/` when it is a plain bug, `rust/ark/tests/fuzz-findings/` when it would change what the spec says. {why}",
        f.check()
    )?;
    Ok(rel)
}

/// The directory, the file name and the text of a finding's vector.
pub fn vector(seed: u64, m: &Module, f: &Finding) -> (&'static str, String, String) {
    let mv = module_value(m);
    let fuzz = |check: &str, why: &str| obj(&[("seed", json(&Value::Int(seed as i64))), ("check", quoted(check)), ("why", quoted(why))]);
    match f {
        Finding::Session {
            check,
            why,
            clients,
            sim_seed,
            script,
        } => (
            "rebase",
            format!("fleet-fuzz-{seed}-{}.json", slug(check)),
            obj(&[
                ("module", json(&mv)),
                ("clients", json(&Value::Int(*clients))),
                ("seed", json(&Value::Int(*sim_seed as i64))),
                ("script", json(&Value::List(script.iter().map(|o| o.value()).collect()))),
                ("fuzz", fuzz(check, why)),
            ]),
        ),
        Finding::View {
            why,
            query,
            ctx,
            args,
            base,
            batches,
        } => {
            let plan = mv
                .field("functions")
                .as_list()
                .into_iter()
                .find(|g| g.field("name") == Value::text(query.clone()))
                .map(|g| g.field("plan"))
                .unwrap_or(Value::Null);
            let (before, steps) = answers(m, query, ctx, args, base, batches);
            (
                "views",
                format!("fuzz-{seed}-{}.json", slug(query)),
                obj(&[
                    ("module", json(&mv)),
                    ("query", quoted(query)),
                    ("plan", json(&plan)),
                    ("ctx", json(&ctx_value(ctx))),
                    ("args", json(&Value::from(args.clone()))),
                    ("store_before", json(&base.store_value())),
                    ("rows_before", json(&Value::from(before))),
                    (
                        "batches",
                        json(&Value::List(
                            batches.iter().map(|b| Value::List(b.iter().map(change_value).collect())).collect(),
                        )),
                    ),
                    // The answer a fresh read gives after each batch; no
                    // patches, since what a correct engine patches is not
                    // what the broken one did, and the splice is checked
                    // against the answer anyway.
                    ("steps", array(steps.into_iter().map(|rows| obj(&[("rows", json(&Value::from(rows)))])))),
                    ("fuzz", fuzz("view", why)),
                ]),
            )
        }
        Finding::Frame { client, value, why } => (
            "protocol",
            format!("{}-fuzz-{seed}.json", if *client { "client" } else { "server" }),
            obj(&[
                ("frame", json(value)),
                ("bytes", quoted(&hex(&encode(value)))),
                ("fuzz", fuzz("frame", why)),
            ]),
        ),
        Finding::Verdict {
            check,
            why,
            function,
            ctx,
            autos,
            args,
            store,
        } => {
            // The interpreter's verdict is the expectation: it is what the
            // spec means.
            let refused = match m.lookup_function(function) {
                Some(func) => {
                    let mut st = store.clone();
                    match eval::apply_closure(&m.schema, &closure(m, func), ctx, autos, args, &mut st) {
                        Ok(Err(Refusal::Refused(t))) => quoted(&t),
                        Ok(Err(r)) => quoted(&format!("{r}")),
                        _ => "null".into(),
                    }
                }
                None => "null".into(),
            };
            (
                "eval",
                format!("fuzz-{seed}-{}.json", slug(check)),
                obj(&[
                    ("module", json(&mv)),
                    ("store_before", json(&store.store_value())),
                    ("ctx", json(&ctx_value(ctx))),
                    (
                        "cases",
                        array([obj(&[
                            ("name", quoted(&format!("fuzz-{seed}"))),
                            ("function", quoted(function)),
                            ("autos", json(&Value::from(autos.clone()))),
                            ("args", json(&Value::from(args.clone()))),
                            ("refused", refused),
                        ])]),
                    ),
                    ("fuzz", fuzz(check, why)),
                ]),
            )
        }
    }
}

// The fresh answer over the base store, then after each batch applied.
fn answers(
    m: &Module,
    query: &str,
    ctx: &ark::eval::Ctx,
    args: &Args,
    base: &MemoryStore,
    batches: &[Vec<ark::store::Change>],
) -> (Vec<Value>, Vec<Vec<Value>>) {
    let Some(f) = m.lookup_function(query) else { return (vec![], vec![]) };
    let Some(plan) = f.plan.clone() else { return (vec![], vec![]) };
    let c = closure(m, f);
    // The middleware once, over the store the view was hydrated on: a
    // view's scope is fixed at hydrate, as the runner's is.
    let env = match eval::middleware(&m.schema, &c, ctx, args, base) {
        Ok((a, provided)) => Env {
            helpers: c.helpers.clone(),
            ctx: ctx.clone(),
            args: a,
            provided,
        },
        Err(_) => return (vec![], vec![]),
    };
    let read = |st: &MemoryStore| -> Vec<Value> {
        view::read(&m.schema, &plan, &env.scope(&m.schema), st).unwrap_or_else(|e: EvalFault| vec![Value::text(format!("{e:?}"))])
    };
    let mut st = base.clone();
    let before = read(&st);
    let mut steps = vec![];
    for b in batches {
        st.apply_changes(b);
        steps.push(read(&st));
    }
    (before, steps)
}
