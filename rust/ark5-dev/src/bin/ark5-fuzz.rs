//! `ark5-fuzz [sessions] [first seed] [traces]`: that many sessions
//! (`ark5_dev::fuzz::session`), seeds counted up from the first, each
//! checked, every client's trace replayed when `traces` is given; each
//! failure printed with its seed, which reproduces it.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: u64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(10_000);
    let first: u64 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1);
    let traces = args.get(3).is_some_and(|a| a == "traces");
    let (mut failed, mut entries, mut refused, mut ops) = (0, 0, 0, 0);
    for seed in first..first + n {
        match std::panic::catch_unwind(|| ark5_dev::fuzz::session(seed, traces)) {
            Ok(Ok((_, f))) => (entries, refused, ops) = (entries + f.entries, refused + f.refused, ops + f.ops),
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
