//! `harken-desktop [--server ws://host:port/sync] [--user NAME] [--data DIR]
//! [--module PATH]`: open the database, dial the server if there is one,
//! draw the screens until `q`.

use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event};
use harken_desktop::net::{self, Event as Net};
use harken_desktop::peer::{Config, Peer};
use harken_desktop::ui::{self, App};

const USAGE: &str = "harken-desktop [--server ws://host:port/sync] [--user NAME] [--data DIR] [--module PATH]

  --server   the sync server; without one the peer is its own authority
  --user     who you are (dev auth: a name is a login); default $USER
  --data     where the database lives; default $XDG_DATA_HOME/harken-desktop/<user>
  --module   a .ark to load instead of harken's own module";

fn default_data(user: &str) -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("harken-desktop").join(user)
}

fn parse(args: &[String]) -> Result<Config, String> {
    let mut server = None;
    let mut user = None;
    let mut data = None;
    let mut module = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--server" => server = Some(value()?),
            "--user" => user = Some(value()?),
            "--data" => data = Some(PathBuf::from(value()?)),
            "--module" => module = Some(PathBuf::from(value()?)),
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    let user = user.or_else(|| std::env::var("USER").ok()).unwrap_or_else(|| "anonymous".into());
    let data = data.unwrap_or_else(|| default_data(&user));
    Ok(Config { server, user, data, module })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let peer = match Peer::open(cfg.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("could not open the database: {e}");
            std::process::exit(1);
        }
    };
    let link = cfg.server.clone().map(net::spawn);
    let mut app = App::new(peer);
    let mut terminal = ratatui::init();
    let outcome = run(&mut terminal, &mut app, link.as_ref());
    ratatui::restore();
    if let Some(link) = link {
        link.shutdown();
    }
    if let Err(e) = outcome {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App, link: Option<&net::Link>) -> Result<(), String> {
    loop {
        terminal.draw(|f| ui::draw(f, app)).map_err(|e| e.to_string())?;
        if let Some(link) = link {
            while let Ok(ev) = link.events.try_recv() {
                match ev {
                    Net::Connected => app.peer.connected(),
                    Net::Frame(bytes) => {
                        if let Err(e) = app.peer.recv_frame(&bytes) {
                            app.note = Some(e);
                        }
                    }
                    Net::Disconnected(why) => {
                        app.peer.disconnected();
                        app.note = Some(format!("unlinked: {why}"));
                    }
                }
            }
            for frame in app.peer.take_outgoing() {
                link.send(frame);
            }
        }
        if app.peer.take_changes() {
            app.reload();
        }
        if event::poll(Duration::from_millis(50)).map_err(|e| e.to_string())? {
            if let Event::Key(k) = event::read().map_err(|e| e.to_string())? {
                if k.kind == event::KeyEventKind::Press {
                    app.key(k);
                }
            }
        }
        if let Some(link) = link {
            for frame in app.peer.take_outgoing() {
                link.send(frame);
            }
        }
        if app.quit {
            return app.peer.persist();
        }
    }
}
