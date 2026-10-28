//! `arkdb`: ArkDB's tools, one command.
//!
//! ```text
//! arkdb gen kotlin|swift DOMAIN [NAME [PACKAGE]]   a domain's typed Kotlin or Swift, to stdout
//! arkdb fuzz [SESSIONS] [FIRST SEED] [traces [DIR]] the fuzzer
//! arkdb tail [--from DIR] [-n N] [--follow] [--peer P] [--refused] [--grep S]
//! arkdb trace ID [--from DIR]                       what a tap or an entry caused, everywhere
//! arkdb peer P [--from DIR]                         one peer's events
//! arkdb rows TABLE [COLUMN=VALUE]... [--sort [-]COLUMN] [-n N] [--from DIR]
//!                                                   a server's rows, a JSON object a line
//! ```

const USAGE: &str = "arkdb gen kotlin|swift DOMAIN [NAME [PACKAGE]]
arkdb fuzz [SESSIONS] [FIRST SEED] [traces [DIR]]
arkdb tail [--from DIR|FILE|-] [-n N] [--follow] [--peer P] [--refused] [--grep S]
arkdb trace ID [--from DIR|FILE|-]
arkdb peer P [--from DIR|FILE|-]
arkdb rows TABLE [COLUMN=VALUE]... [--sort [-]COLUMN] [-n N] [--from DIR]";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fail = |e: String| -> ! {
        eprintln!("{e}");
        std::process::exit(2)
    };
    match args.split_first() {
        Some((cmd, rest)) if cmd == "gen" => match arkdb::gen::main(rest) {
            Ok(s) => print!("{s}"),
            Err(e) => fail(e),
        },
        Some((cmd, rest)) if cmd == "rows" => match arkdb::rows::main(rest) {
            Ok(s) => print!("{s}"),
            Err(e) => fail(e),
        },
        Some((cmd, rest)) if cmd == "fuzz" => std::process::exit(arkdb::fuzz::main(rest) as i32),
        Some((cmd, rest)) if ["tail", "trace", "peer"].contains(&cmd.as_str()) => {
            if let Err(e) = arkdb::events::main(cmd, rest) {
                fail(e)
            }
        }
        _ => fail(USAGE.into()),
    }
}
