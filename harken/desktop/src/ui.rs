//! The screens: two panes and a status line, driven by vim-ish keys. The
//! left pane is the library and the playlists; the cursor there decides
//! what the right pane *shows*, and `Enter` on a playlist makes it the
//! *target* `a` adds to. Nothing here touches the store except through the
//! generated queries in [`crate::domain`].

use std::collections::BTreeMap;

use ark::value::{hex, Id};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::domain::{self, Item, Playlist, Track};
use crate::peer::Peer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
}

/// A row of the right pane.
#[derive(Clone, Debug)]
pub enum Row {
    Track(Track),
    Item { item: Item, track: Option<Track> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asking {
    PlaylistName,
    /// `title | artist`, for a peer alone with a module that carries
    /// `add_track`.
    TrackLine,
}

#[derive(Clone, Debug)]
pub struct Prompt {
    pub asking: Asking,
    pub buf: String,
}

pub struct App {
    pub peer: Peer,
    pub pane: Pane,
    /// 0 is the library; `n` is `playlists[n - 1]`.
    pub left: usize,
    pub right: usize,
    /// The playlist `Enter` picked, that `a` adds to.
    pub target: Option<Id>,
    pub playlists: Vec<Playlist>,
    pub tracks: Vec<Track>,
    pub rows: Vec<Row>,
    pub prompt: Option<Prompt>,
    /// One line the last key left behind.
    pub note: Option<String>,
    pub quit: bool,
}

impl App {
    pub fn new(peer: Peer) -> App {
        let mut app = App {
            peer,
            pane: Pane::Left,
            left: 0,
            right: 0,
            target: None,
            playlists: vec![],
            tracks: vec![],
            rows: vec![],
            prompt: None,
            note: None,
            quit: false,
        };
        app.reload();
        app
    }

    /// Re-run the queries and rebuild both panes.
    pub fn reload(&mut self) {
        self.playlists = match self.peer.db(domain::PLAYLISTS) {
            Some(db) => domain::playlists(&db).unwrap_or_default(),
            None => vec![],
        };
        self.tracks = match self.peer.db(domain::LIBRARY) {
            Some(db) => domain::library(&db).unwrap_or_default(),
            None => vec![],
        };
        // A target that lost the rebase is no target.
        if let Some(t) = self.target {
            if !self.playlists.iter().any(|p| p.id == t) {
                self.target = None;
            }
        }
        self.left = self.left.min(self.playlists.len());
        self.rows = match self.shown() {
            None => self.tracks.iter().cloned().map(Row::Track).collect(),
            Some(pid) => {
                let by_id: BTreeMap<Id, &Track> = self.tracks.iter().map(|t| (t.id, t)).collect();
                let items = match self.peer.db(domain::PLAYLISTS) {
                    Some(db) => domain::playlist_items(&db, pid).unwrap_or_default(),
                    None => vec![],
                };
                items
                    .into_iter()
                    .map(|item| Row::Item {
                        track: by_id.get(&item.track_id).map(|t| (*t).clone()),
                        item,
                    })
                    .collect()
            }
        };
        self.right = self.right.min(self.rows.len().saturating_sub(1));
    }

    /// The playlist the right pane shows; `None` is the library.
    pub fn shown(&self) -> Option<Id> {
        if self.left == 0 {
            None
        } else {
            self.playlists.get(self.left - 1).map(|p| p.id)
        }
    }

    pub fn key(&mut self, k: KeyEvent) {
        self.note = None;
        if self.prompt.is_some() {
            self.prompt_key(k);
            return;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Tab | KeyCode::BackTab => {
                self.pane = match self.pane {
                    Pane::Left => Pane::Right,
                    Pane::Right => Pane::Left,
                }
            }
            KeyCode::Char('h') | KeyCode::Left => self.pane = Pane::Left,
            KeyCode::Char('l') | KeyCode::Right => self.pane = Pane::Right,
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::Char('g') | KeyCode::Home => self.jump(0),
            KeyCode::Char('G') | KeyCode::End => self.jump(usize::MAX),
            KeyCode::Char('n') => {
                self.prompt = Some(Prompt {
                    asking: Asking::PlaylistName,
                    buf: String::new(),
                })
            }
            KeyCode::Char('t') => {
                if !self.peer.alone() {
                    self.note = Some("tracks come from the server's scanner; `t` is for a peer alone".into());
                } else if self.peer.module.lookup_function("add_track").is_none() {
                    self.note = Some("this module has no add_track".into());
                } else {
                    self.prompt = Some(Prompt {
                        asking: Asking::TrackLine,
                        buf: String::new(),
                    })
                }
            }
            KeyCode::Enter => self.enter(),
            KeyCode::Char('a') => self.add(),
            KeyCode::Char('d') => self.remove(),
            _ => {}
        }
    }

    fn step(&mut self, by: isize) {
        match self.pane {
            Pane::Left => {
                let n = self.playlists.len() as isize;
                self.left = (self.left as isize + by).clamp(0, n) as usize;
                self.right = 0;
                self.reload();
            }
            Pane::Right => {
                let n = self.rows.len() as isize;
                if n > 0 {
                    self.right = (self.right as isize + by).clamp(0, n - 1) as usize;
                }
            }
        }
    }

    fn jump(&mut self, to: usize) {
        match self.pane {
            Pane::Left => {
                self.left = to.min(self.playlists.len());
                self.right = 0;
                self.reload();
            }
            Pane::Right => self.right = to.min(self.rows.len().saturating_sub(1)),
        }
    }

    fn enter(&mut self) {
        match self.pane {
            Pane::Left => match self.shown() {
                None => self.note = Some("Enter on a playlist makes it the one `a` adds to".into()),
                Some(pid) => {
                    self.target = if self.target == Some(pid) { None } else { Some(pid) };
                    let name = self.playlists.iter().find(|p| p.id == pid).map(|p| p.name.clone()).unwrap_or_default();
                    self.note = Some(match self.target {
                        Some(_) => format!("`a` in the library adds to {name}"),
                        None => "no target playlist".into(),
                    });
                }
            },
            Pane::Right => self.pane = Pane::Left,
        }
    }

    fn add(&mut self) {
        let Some(Row::Track(t)) = self.rows.get(self.right).cloned() else {
            self.note = Some("`a` is for a row of the library".into());
            return;
        };
        let Some(pid) = self.target else {
            self.note = Some("pick a playlist first: Enter on it in the left pane".into());
            return;
        };
        self.run(domain::add_to_playlist(pid, t.id));
    }

    fn remove(&mut self) {
        let Some(Row::Item { item, .. }) = self.rows.get(self.right).cloned() else {
            self.note = Some("`d` is for an item of a playlist".into());
            return;
        };
        match domain::remove_from_playlist(item.playlist_id, item.track_id) {
            Some(call) => self.run(call),
            None => self.note = Some("this module has no remove_from_playlist".into()),
        }
    }

    fn run(&mut self, call: domain::Call) {
        let name = call.name;
        match self.peer.call(call) {
            Ok(()) => self.note = Some(format!("{name}: ok")),
            Err(why) => self.note = Some(format!("{name}: {why}")),
        }
        self.reload();
    }

    fn prompt_key(&mut self, k: KeyEvent) {
        let Some(p) = self.prompt.as_mut() else { return };
        match k.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Backspace => {
                p.buf.pop();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => p.buf.push(c),
            KeyCode::Enter => {
                let Prompt { asking, buf } = self.prompt.take().unwrap_or(Prompt {
                    asking: Asking::PlaylistName,
                    buf: String::new(),
                });
                match asking {
                    Asking::PlaylistName => self.run(domain::create_playlist(buf)),
                    Asking::TrackLine => self.add_track(&buf),
                }
            }
            _ => {}
        }
    }

    // `title | artist`, authored by intent through the module's own closure.
    fn add_track(&mut self, line: &str) {
        let (title, artist) = match line.split_once('|') {
            Some((t, a)) => (t.trim(), a.trim()),
            None => (line.trim(), ""),
        };
        let args = domain::add_track_args(title, artist, None, 0, "");
        match self.peer.author_by_intent("add_track", args) {
            Ok(()) => self.note = Some("add_track: ok".into()),
            Err(why) => self.note = Some(format!("add_track: {why}")),
        }
        self.reload();
    }
}

// -- drawing ---------------------------------------------------------------

fn short(id: &Id) -> String {
    hex(&id[..4])
}

fn clock(ms: i64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn draw(f: &mut Frame, app: &App) {
    let [main, note, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
        .areas(f.area());
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Min(10)])
        .areas(main);
    draw_left(f, app, left);
    draw_right(f, app, right);
    draw_note(f, app, note);
    draw_status(f, app, status);
}

fn cursor_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default().add_modifier(Modifier::UNDERLINED)
    }
}

fn draw_left(f: &mut Frame, app: &App, area: Rect) {
    let mut items = vec![ListItem::new(format!("Library ({})", app.tracks.len()))];
    for p in &app.playlists {
        let mark = if app.target == Some(p.id) { "* " } else { "  " };
        items.push(ListItem::new(format!("{mark}{}", p.name)));
    }
    let focused = app.pane == Pane::Left;
    let list = List::new(items)
        .block(Block::bordered().title(" playlists "))
        .highlight_style(cursor_style(focused));
    let mut state = ListState::default().with_selected(Some(app.left));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_right(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.shown() {
        None => " library ".to_string(),
        Some(pid) => {
            let name = app.playlists.iter().find(|p| p.id == pid).map(|p| p.name.as_str()).unwrap_or("?");
            format!(" {name} ")
        }
    };
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|r| match r {
            Row::Track(t) => ListItem::new(Line::from(vec![
                Span::raw(format!("{:<32} ", t.title)),
                Span::styled(format!("{:<24} ", t.artist), Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{:<20} ", t.album.clone().unwrap_or_default()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(clock(t.duration_ms)),
            ])),
            Row::Item { item, track: Some(t) } => ListItem::new(Line::from(vec![
                Span::styled(format!("{:>3}  ", item.pos), Style::default().fg(Color::Gray)),
                Span::raw(format!("{:<32} ", t.title)),
                Span::styled(format!("{:<24} ", t.artist), Style::default().fg(Color::Gray)),
                Span::raw(clock(t.duration_ms)),
            ])),
            Row::Item { item, track: None } => ListItem::new(Line::from(vec![
                Span::styled(format!("{:>3}  ", item.pos), Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("(track {} not here yet)", short(&item.track_id)),
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
        })
        .collect();
    let focused = app.pane == Pane::Right;
    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(cursor_style(focused));
    let mut state = ListState::default().with_selected(if app.rows.is_empty() { None } else { Some(app.right) });
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_note(f: &mut Frame, app: &App, area: Rect) {
    let line = match &app.prompt {
        Some(p) => {
            let what = match p.asking {
                Asking::PlaylistName => "new playlist",
                Asking::TrackLine => "add track (title | artist)",
            };
            Line::from(vec![
                Span::styled(format!("{what}: "), Style::default().fg(Color::Yellow)),
                Span::raw(&p.buf),
                Span::raw("_"),
            ])
        }
        None => Line::from(Span::styled(
            app.note
                .clone()
                .unwrap_or_else(|| "j/k move  Tab pane  Enter target  n new playlist  a add  d remove  q quit".into()),
            Style::default().fg(Color::DarkGray),
        )),
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let s = app.peer.status();
    let mut spans = vec![
        Span::styled(format!(" {} ", s.user), Style::default().fg(Color::Black).bg(Color::Yellow)),
        Span::raw(" "),
    ];
    match &s.server {
        None => spans.push(Span::styled("alone", Style::default().fg(Color::Cyan))),
        Some(url) => {
            spans.push(Span::raw(url.clone()));
            spans.push(Span::raw(" "));
            spans.push(match (&s.denied, s.linked) {
                (Some(why), _) => Span::styled(format!("denied: {why}"), Style::default().fg(Color::Red)),
                (None, true) => Span::styled("linked", Style::default().fg(Color::Green)),
                (None, false) => Span::styled("unlinked", Style::default().fg(Color::Red)),
            });
        }
    }
    for (scope, cursor) in &s.cursors {
        spans.push(Span::raw(format!("  {scope}@{cursor}")));
    }
    spans.push(Span::raw(format!("  pending {}", s.pending)));
    if s.diverged > 0 {
        spans.push(Span::styled(format!("  diverged {}", s.diverged), Style::default().fg(Color::Red)));
    }
    if let Some(why) = &s.last_refusal {
        spans.push(Span::styled(format!("  refused: {why}"), Style::default().fg(Color::Red)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
