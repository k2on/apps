//! `ark-fuzz [sessions] [first seed] [traces [DIR]]`: that many sessions
//! (`ark_dev::fuzz::session`), seeds counted up from the first, each
//! checked, every client's trace replayed when `traces` is given or, with
//! `DIR`, written there instead as `seed-<seed>-client-<i>.json` (a trace
//! corpus, `spec/RUNNERS.md` § trace, which the flake's `traces` check
//! replays); each failure printed with its seed, which reproduces it.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: u64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(10_000);
    let first: u64 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1);
    let traces = args.get(3).is_some_and(|a| a == "traces");
    let out = args.get(4).filter(|_| traces);
    if let Some(d) = out {
        std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{d}: {e}"));
    }
    let (mut failed, mut entries, mut refused, mut ops) = (0, 0, 0, 0);
    for seed in first..first + n {
        match std::panic::catch_unwind(|| ark_dev::fuzz::session(seed, traces && out.is_none())) {
            Ok(Ok((net, f))) => {
                (entries, refused, ops) = (entries + f.entries, refused + f.refused, ops + f.ops);
                for (i, c) in net.clients.iter().filter(|_| out.is_some()) {
                    let path = format!("{}/seed-{seed}-client-{i}.json", out.expect("a directory"));
                    let t = ark_dev::trace::record(&format!("seed {seed} client {i}"), c);
                    std::fs::write(&path, t).unwrap_or_else(|e| panic!("{path}: {e}"));
                }
            }
            Ok(Err(e)) => {
                failed += 1;
                println!("seed {seed}: {e}");
            }
            Err(_) => {
                failed += 1;
                println!("seed {seed}: panicked");
            }
        }
    }
    println!("{n} sessions from seed {first}: {failed} failed; {ops} ops, {entries} entries logged, {refused} refusals told");
    std::process::exit((failed > 0) as i32);
}
