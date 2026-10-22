//! `ark-dev gen [--source DIR] [--check]`: every file the apps' IDEs read of
//! harken's domain, from its source (`ark_dev::gen::app`); `ark-dev gen
//! kotlin|swift DOMAIN [NAME [PACKAGE]]`: a domain's types and stubs for an
//! app over the bridge (`ark_dev::gen`), to stdout.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = match args.split_first() {
        Some((cmd, rest)) if cmd == "gen" => ark_dev::gen::main(rest),
        _ => Err("ark-dev gen [--source DIR] [--check] | ark-dev gen kotlin|swift DOMAIN [NAME [PACKAGE]]".into()),
    };
    match out {
        Ok(s) => print!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
