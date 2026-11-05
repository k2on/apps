//! A desktop window photographed, for the apps' screenshots
//! (`.github/screens/apps.md`): the `desktop` beside the phones.
//!
//! **The app's own application, run as iced runs it.** An app hands over the
//! `iced::application(...)` its `main` would run, and this runs it with no
//! window: its boot, its updates, the tasks they return and its
//! subscriptions on the app's own executor, the window's events through its
//! widgets and then to its subscriptions, a redraw when a widget asks for
//! one — what iced's shell does, after iced_test's `Emulator` (which this
//! is not, as that one forgets the widgets' state once it has drawn and
//! tells no subscription of an event). So the window reads its `--server`
//! and `--user` as ever, is told its size as a window is, signs in (a dev
//! server's sign-in comes straight back to `ark_auth::client::login`'s
//! loopback, with no browser), dials the server's socket and draws what it
//! hears. Nothing in the app knows; nothing it draws is different.
//!
//! **Asked for by the environment, and only then.** `ARK_SHOTS` names the
//! app's `screens/shots.json` and `ARK_SHOTS_OUT` the directory the shots go
//! to ([`asked`]); with `ARK_SHOTS` unset an app runs its window as it always
//! does. `ARK_SHOTS_ONLY=<id>` takes one shot, for writing its steps.
//!
//! `ARK_SHOTS_FONTS=<dir>` makes that directory the machine's only fonts,
//! and `ARK_SHOTS_BACKEND` names the renderer (`wgpu`, or `tiny-skia`).
//!
//! **The same picture on every machine.** No window and no GPU: drawn by
//! wgpu, the renderer a person's window draws with, on whatever Vulkan
//! driver there is — CI's is Mesa's lavapipe, Vulkan on the CPU, pinned by
//! nix — and with the fonts the app carries and, for its default
//! sans-serif, the fonts `ARK_SHOTS_FONTS` pins (CI's are nixpkgs' DejaVu,
//! a Linux desktop's usual). Not tiny-skia, iced's fallback: its 0.14 draws
//! an image whole where a widget clips it (every album cover spills out of
//! its card). With no driver this says so rather than draw with tiny-skia.
//! A shot is taken once nothing has happened for two seconds: the sync
//! done, the pictures fetched, an animation ended.
//!
//! **Each shot** is the app opened afresh at the window's size, its steps
//! done in order, and the screen drawn in light and in dark. The steps
//! (`desktop.shots[].steps` in shots.json) name what to press by what it
//! says, as the phones' do, from the text the window lays out:
//!
//! - `{"click": "<text>"}` clicks the middle of the text that says exactly
//!   that (`a|b`: whichever is there), waiting up to 20 s for it to be;
//!   `"where": "bottom"` takes the lowest of several, `"optional": true`
//!   passes over one not there after 5 s, and `"unless": "<text>"` skips
//!   the step when that is on the screen already.
//! - `{"type": "<text>"}` types into what has the keys, and a key no
//!   widget takes goes on to the app's own (the vim keys); `{"key":
//!   "ctrl+k"}` presses one, with what is held (`press` spells them).
//! - `{"await": "<text>"}` waits up to 20 s for it to be on the screen.
//! - `{"wait": <seconds>}` lets the window run that long.
//! - `{"relaunch": true}` closes the app and opens it again.
//!
//! What a step can name is in `<id>.tree.txt` beside each shot: every text
//! the window laid out, where it is. A step that finds nothing leaves
//! `stuck.png` and `stuck.tree.txt`, what was on the screen instead.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::Theme;
use iced_test::core::renderer::{self, Headless};
use iced_test::core::widget::{self, Operation};
use iced_test::core::window::{self, RedrawRequest};
use iced_test::core::{clipboard, mouse, Event, Point, Rectangle, Size};
use iced_test::futures::futures::channel::mpsc;
use iced_test::futures::futures::executor::block_on;
use iced_test::futures::{subscription, Executor, Runtime};
use iced_test::instruction::{Interaction, Keyboard, Mouse, Target};
use iced_test::program::Program;
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_test::runtime::{self as runtime, task, Action, Task};
use iced_test::selector::{Candidate, Selector};
use serde_json::Value;

/// How long a step waits for what it names.
const PATIENCE: Duration = Duration::from_secs(20);
/// How long an optional step waits before it is passed over.
const OPTIONAL: Duration = Duration::from_secs(5);
/// How long nothing must happen before a shot is taken.
const QUIET: Duration = Duration::from_secs(2);
/// The longest a shot waits for that, before it is taken anyway (a clock
/// that never stops).
const SETTLE: Duration = Duration::from_secs(30);
/// A frame, for a widget that asks for the next one.
const FRAME: Duration = Duration::from_millis(16);

/// Shots asked for: where their steps are and where they go.
pub struct Asked {
    shots: PathBuf,
    out: PathBuf,
    only: Option<String>,
}

/// Whether this run is to take the app's shots rather than open its window:
/// `ARK_SHOTS` set.
pub fn asked() -> Option<Asked> {
    let shots = std::env::var_os("ARK_SHOTS")?;
    Some(Asked {
        shots: shots.into(),
        out: std::env::var_os("ARK_SHOTS_OUT").map(PathBuf::from).unwrap_or_else(|| "shots".into()),
        only: std::env::var("ARK_SHOTS_ONLY").ok().filter(|s| !s.is_empty()),
    })
}

impl Asked {
    /// Take the shots of `program`, the app's `iced::application`, and end
    /// the process: 0 when every shot was taken.
    pub fn run<P>(self, program: P) -> !
    where
        P: Program<Theme = Theme> + 'static,
    {
        let code = match self.take(&program) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("shots: {e}");
                1
            }
        };
        std::process::exit(code)
    }

    fn take<P>(&self, program: &P) -> Result<(), String>
    where
        P: Program<Theme = Theme> + 'static,
    {
        let read = std::fs::read_to_string(&self.shots).map_err(|e| format!("{}: {e}", self.shots.display()))?;
        let all: Value = serde_json::from_str(&read).map_err(|e| format!("{}: {e}", self.shots.display()))?;
        let desktop = &all["desktop"];
        let window = desktop["window"].as_array();
        let side = |i: usize, or: f64| window.and_then(|w| w.get(i)).and_then(Value::as_f64).unwrap_or(or) as f32;
        let size = Size::new(side(0, 1280.0), side(1, 800.0));
        let scale = desktop["scale"].as_f64().unwrap_or(2.0) as f32;
        let shots = desktop["shots"].as_array().ok_or("shots.json has no desktop.shots")?;

        if let Some(fonts) = std::env::var_os("ARK_SHOTS_FONTS") {
            only_fonts(Path::new(&fonts))?;
        }
        for font in program.settings().fonts {
            iced::advanced::graphics::text::font_system()
                .write()
                .expect("the font system")
                .load_font(font);
        }
        for theme in ["light", "dark"] {
            std::fs::create_dir_all(self.out.join(theme)).map_err(|e| e.to_string())?;
        }
        let device = serde_json::json!({
            "name": format!("Desktop, {}\u{d7}{}", size.width, size.height),
            "width": (size.width * scale).round() as u32,
            "height": (size.height * scale).round() as u32,
            "corner": 0,
            "cutout": "none",
        });
        std::fs::write(self.out.join("device.json"), format!("{device}\n")).map_err(|e| e.to_string())?;

        let mut failed = Vec::new();
        for shot in shots {
            let id = shot["id"].as_str().ok_or("a shot with no id")?;
            if self.only.as_deref().is_some_and(|only| only != id) {
                continue;
            }
            eprintln!("shots: {id}");
            let mut window = Window::open(program, size, scale);
            let mut done = Ok(());
            for step in shot["steps"].as_array().map(Vec::as_slice).unwrap_or_default() {
                if step["relaunch"].as_bool() == Some(true) {
                    // The one open goes first, as a window closed does: its
                    // replica's files and socket let go before the next opens.
                    drop(window);
                    window = Window::open(program, size, scale);
                } else if let Err(why) = window.step(step) {
                    done = Err(why);
                    break;
                }
            }
            match done {
                Ok(()) => {
                    window.settle(QUIET, SETTLE);
                    let tree = window.tree();
                    for (theme, name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
                        window.write(&theme, &self.out.join(name).join(format!("{id}.png")))?;
                        std::fs::write(self.out.join(name).join(format!("{id}.tree.txt")), &tree).map_err(|e| e.to_string())?;
                    }
                }
                Err(why) => {
                    eprintln!("shots: {id}: {why}");
                    window.write(&Theme::Light, &self.out.join("stuck.png"))?;
                    std::fs::write(self.out.join("stuck.tree.txt"), window.tree()).map_err(|e| e.to_string())?;
                    failed.push(id.to_string());
                }
            }
        }
        match failed.is_empty() {
            true => Ok(()),
            false => Err(format!("not taken: {}", failed.join(", "))),
        }
    }
}

/// The machine's fonts are `fonts` and nothing else, set before anything
/// reads them: a font configuration with that one directory. A window's
/// text that no font of the app's draws (its default, sans-serif) is drawn
/// with the system's, as on any desktop, so a run that pins them draws the
/// same on every machine. Without them that text has no font at all: iced's
/// basic shaping, which plain ASCII gets, does not fall back to the app's.
fn only_fonts(fonts: &Path) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("arkui-shots-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let fonts = std::fs::canonicalize(fonts).map_err(|e| format!("{}: {e}", fonts.display()))?;
    let conf = dir.join("fonts.conf");
    std::fs::write(
        &conf,
        format!("<?xml version=\"1.0\"?>\n<fontconfig><dir>{}</dir></fontconfig>\n", fonts.display()),
    )
    .map_err(|e| e.to_string())?;
    std::env::set_var("FONTCONFIG_FILE", &conf);
    Ok(())
}

/// A key pressed and let go, as a keyboard sends it: `Enter`, `Escape`,
/// `Tab`, `Backspace`, `Space`, `Up`, `Down`, `Left`, `Right`, or one
/// character, after any of `ctrl+`, `shift+`, `alt+`, `super+` held with
/// it (`ctrl+k`).
fn press(spelled: &str) -> Option<Vec<Event>> {
    use iced_test::core::keyboard::{self, key::Named, Modifiers};
    use iced_test::core::SmolStr;
    let mut modifiers = Modifiers::empty();
    let mut parts: Vec<&str> = spelled.split('+').collect();
    let name = parts.pop()?;
    for held in parts {
        modifiers |= match held.to_ascii_lowercase().as_str() {
            "ctrl" => Modifiers::CTRL,
            "shift" => Modifiers::SHIFT,
            "alt" => Modifiers::ALT,
            "super" | "cmd" => Modifiers::LOGO,
            _ => return None,
        };
    }
    let (key, text) = match name {
        "Enter" => (keyboard::Key::Named(Named::Enter), None),
        "Escape" => (keyboard::Key::Named(Named::Escape), None),
        "Tab" => (keyboard::Key::Named(Named::Tab), None),
        "Backspace" => (keyboard::Key::Named(Named::Backspace), None),
        "Space" => (keyboard::Key::Named(Named::Space), Some(SmolStr::new(" "))),
        "Up" => (keyboard::Key::Named(Named::ArrowUp), None),
        "Down" => (keyboard::Key::Named(Named::ArrowDown), None),
        "Left" => (keyboard::Key::Named(Named::ArrowLeft), None),
        "Right" => (keyboard::Key::Named(Named::ArrowRight), None),
        c if c.chars().count() == 1 => (keyboard::Key::Character(SmolStr::new(c)), Some(SmolStr::new(c))),
        _ => return None,
    };
    // Text is what a key types: none while Ctrl, Alt or Super is held.
    let text = text.filter(|_| !(modifiers.control() || modifiers.alt() || modifiers.logo()));
    let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
    Some(vec![
        Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            text,
            repeat: false,
        }),
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
        }),
        Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::empty())),
    ])
}

/// A text the window laid out, where it is on the screen.
#[derive(Debug, Clone)]
struct Seen {
    text: String,
    at: Rectangle,
    field: bool,
}

fn seen(candidate: Candidate<'_>) -> Option<Seen> {
    match candidate {
        Candidate::Text { content, visible_bounds, .. } => Some(Seen {
            text: content.to_string(),
            at: visible_bounds?,
            field: false,
        }),
        Candidate::TextInput { state, visible_bounds, .. } => Some(Seen {
            text: state.text().to_string(),
            at: visible_bounds?,
            field: true,
        }),
        _ => None,
    }
}

type Actions<P> = Action<<P as Program>::Message>;

/// The app, open with no window.
struct Window<'a, P: Program> {
    program: &'a P,
    state: P::State,
    runtime: Runtime<P::Executor, mpsc::Sender<Actions<P>>, Actions<P>>,
    actions: mpsc::Receiver<Actions<P>>,
    renderer: P::Renderer,
    /// The widgets' own state (a list's scroll, a field's cursor), kept from
    /// one layout to the next as a window keeps it.
    cache: user_interface::Cache,
    clipboard: clipboard::Null,
    id: window::Id,
    size: Size,
    scale: f32,
    cursor: mouse::Cursor,
    /// When a widget asked to be drawn again.
    redraw: Option<Instant>,
}

impl<'a, P> Window<'a, P>
where
    P: Program<Theme = Theme> + 'static,
{
    fn open(program: &'a P, size: Size, scale: f32) -> Self {
        let settings = program.settings();
        let backend = std::env::var("ARK_SHOTS_BACKEND").unwrap_or_else(|_| "wgpu".into());
        let renderer = block_on(P::Renderer::new(settings.default_font, settings.default_text_size, Some(&backend))).unwrap_or_else(|| {
            panic!("no {backend} renderer: wgpu needs a Vulkan driver (VK_ICD_FILENAMES, Mesa's lavapipe on a machine with no GPU)")
        });
        let (tx, actions) = mpsc::channel(1024);
        let runtime = Runtime::new(P::Executor::new().expect("the app's executor"), tx);
        let (state, task) = runtime.enter(|| program.boot());
        let mut window = Window {
            program,
            state,
            runtime,
            actions,
            renderer,
            cache: user_interface::Cache::default(),
            clipboard: clipboard::Null,
            id: window::Id::unique(),
            size,
            scale,
            cursor: mouse::Cursor::Unavailable,
            redraw: Some(Instant::now()),
        };
        window.resubscribe();
        window.spawn(task);
        // As a window is opened: it says so, with its size.
        window.events(vec![
            Event::Window(window::Event::Opened { position: None, size }),
            Event::Window(window::Event::Resized(size)),
        ]);
        window
    }

    fn resubscribe(&mut self) {
        let subscription = self.runtime.enter(|| self.program.subscription(&self.state));
        self.runtime.track(subscription::into_recipes(subscription.map(Action::Output)));
    }

    fn spawn(&mut self, task: Task<P::Message>) {
        if let Some(stream) = task::into_stream(task) {
            self.runtime.run(stream);
        }
    }

    fn update(&mut self, message: P::Message) {
        let task = self.runtime.enter(|| self.program.update(&mut self.state, message));
        self.resubscribe();
        self.spawn(task);
        self.redraw = Some(Instant::now());
    }

    /// The window's layout now, laid out from the app's view with the
    /// widgets' state, handed to `f`, and its state kept.
    fn with_ui<R>(
        &mut self,
        f: impl FnOnce(&mut UserInterface<'_, P::Message, P::Theme, P::Renderer>, &mut P::Renderer, &mut clipboard::Null) -> R,
    ) -> R {
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build(self.program.view(&self.state, self.id), self.size, cache, &mut self.renderer);
        let r = f(&mut ui, &mut self.renderer, &mut self.clipboard);
        self.cache = ui.into_cache();
        r
    }

    /// Events, as the window has them: through its widgets first, then to
    /// the subscriptions, saying whether a widget took each; then what the
    /// widgets said.
    fn events(&mut self, events: Vec<Event>) {
        for event in &events {
            if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
                self.cursor = mouse::Cursor::Available(*position);
            }
        }
        let cursor = self.cursor;
        let mut messages = Vec::new();
        let (state, statuses) = self.with_ui(|ui, renderer, clipboard| ui.update(&events, cursor, renderer, clipboard, &mut messages));
        if let user_interface::State::Updated { redraw_request, .. } = state {
            let next = match redraw_request {
                RedrawRequest::NextFrame => Some(Instant::now() + FRAME),
                RedrawRequest::At(at) => Some(at),
                RedrawRequest::Wait => None,
            };
            self.redraw = match (self.redraw, next) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        for (event, status) in events.into_iter().zip(statuses) {
            self.runtime.broadcast(subscription::Event::Interaction {
                window: self.id,
                event,
                status,
            });
        }
        for message in messages {
            self.update(message);
        }
    }

    /// What the app's tasks and subscriptions asked of the runtime.
    fn perform(&mut self, action: Actions<P>) {
        match action {
            Action::Output(message) => self.update(message),
            Action::LoadFont { bytes, channel } => {
                iced::advanced::graphics::text::font_system()
                    .write()
                    .expect("the font system")
                    .load_font(bytes);
                let _ = channel.send(Ok(()));
            }
            Action::Widget(mut operation) => {
                self.with_ui(|ui, renderer, _| loop {
                    ui.operate(renderer, operation.as_mut());
                    match operation.finish() {
                        widget::operation::Outcome::Chain(next) => operation = next,
                        _ => break,
                    }
                });
                self.redraw = Some(Instant::now());
            }
            Action::Window(action) => self.window(action),
            // No clipboard, no system to ask, no image to read back, and the
            // window stays open: what each was asked with is let go, so a
            // task waiting on it ends.
            Action::Clipboard(_) | Action::System(_) | Action::Image(_) | Action::Reload | Action::Exit => {}
        }
    }

    /// What a task asks of the window, answered as one at `size` would.
    fn window(&mut self, action: runtime::window::Action) {
        use runtime::window::Action as W;
        match action {
            W::Open(id, _, channel) => {
                self.id = id;
                let _ = channel.send(id);
            }
            W::GetOldest(channel) | W::GetLatest(channel) => {
                let _ = channel.send(Some(self.id));
            }
            W::GetSize(_, channel) => {
                let _ = channel.send(self.size);
            }
            W::GetMaximized(_, channel) => {
                let _ = channel.send(false);
            }
            W::GetMinimized(_, channel) => {
                let _ = channel.send(Some(false));
            }
            W::GetPosition(_, channel) => {
                let _ = channel.send(Some(Point::ORIGIN));
            }
            W::GetScaleFactor(_, channel) => {
                let _ = channel.send(self.scale);
            }
            W::GetMode(_, channel) => {
                let _ = channel.send(window::Mode::Windowed);
            }
            _ => {}
        }
    }

    /// Run what the app has to do — its tasks' answers, its subscriptions,
    /// the redraws its widgets ask for — until nothing has for `quiet`, or
    /// for `most` at the longest.
    fn settle(&mut self, quiet: Duration, most: Duration) {
        let start = Instant::now();
        let mut last = start;
        loop {
            if let Ok(action) = self.actions.try_recv() {
                self.perform(action);
                last = Instant::now();
                continue;
            }
            let now = Instant::now();
            if self.redraw.is_some_and(|at| at <= now) {
                self.redraw = None;
                self.events(vec![Event::Window(window::Event::RedrawRequested(now))]);
                last = now;
                continue;
            }
            if now - last >= quiet || now - start >= most {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Every text on the screen, in the order the window lays them out.
    fn texts(&mut self) -> Vec<Seen> {
        let mut find = (seen as fn(Candidate<'_>) -> Option<Seen>).find_all();
        self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut widget::operation::black_box(&mut find)));
        match find.finish() {
            widget::operation::Outcome::Some(all) => all,
            _ => Vec::new(),
        }
    }

    /// The screen's texts as `<id>.tree.txt` has them, one a line.
    fn tree(&mut self) -> String {
        self.texts()
            .iter()
            .map(|s| {
                let kind = if s.field { " [field]" } else { "" };
                format!("{:?}{kind} at {:.0},{:.0} {:.0}x{:.0}\n", s.text, s.at.x, s.at.y, s.at.width, s.at.height)
            })
            .collect()
    }

    /// The middle of the text that says `label` (`a|b`: either), the lowest
    /// of several when `bottom`.
    fn find(&mut self, label: &str, bottom: bool) -> Option<Point> {
        let names: Vec<&str> = label.split('|').collect();
        let mut found = self.texts().into_iter().filter(|s| names.contains(&s.text.as_str()));
        let one = match bottom {
            true => found.max_by(|a, b| a.at.y.total_cmp(&b.at.y)),
            false => found.next(),
        };
        one.map(|s| s.at.center())
    }

    /// `label` once it is on the screen, waiting `patience` at most.
    fn wait_for(&mut self, label: &str, bottom: bool, patience: Duration) -> Option<Point> {
        let start = Instant::now();
        loop {
            if let Some(at) = self.find(label, bottom) {
                return Some(at);
            }
            if start.elapsed() >= patience {
                return None;
            }
            self.settle(Duration::from_millis(100), Duration::from_millis(250));
        }
    }

    /// A click or keys, as iced_test spells them, and what follows them.
    fn interact(&mut self, interaction: Interaction) {
        let events = interaction
            .events(|target| match target {
                Target::Point(at) => Some(*at),
                Target::Text(_) => None,
            })
            .unwrap_or_default();
        self.events(events);
        self.settle(Duration::from_millis(300), Duration::from_secs(5));
    }

    /// One of a shot's steps (all but `relaunch`, which is the shot's own).
    fn step(&mut self, step: &Value) -> Result<(), String> {
        let optional = step["optional"].as_bool() == Some(true);
        if let Some(label) = step["click"].as_str() {
            if let Some(unless) = step["unless"].as_str() {
                if self.find(unless, false).is_some() {
                    return Ok(());
                }
            }
            let bottom = step["where"].as_str() == Some("bottom");
            match self.wait_for(label, bottom, if optional { OPTIONAL } else { PATIENCE }) {
                Some(at) => self.interact(Interaction::Mouse(Mouse::Click {
                    button: mouse::Button::Left,
                    target: Some(Target::Point(at)),
                })),
                None if optional => {}
                None => return Err(format!("nothing on the screen says {label:?}")),
            }
        } else if let Some(text) = step["type"].as_str() {
            self.interact(Interaction::Keyboard(Keyboard::Typewrite(text.to_string())));
        } else if let Some(key) = step["key"].as_str() {
            let events = press(key).ok_or_else(|| format!("no key {key:?}"))?;
            self.events(events);
            self.settle(Duration::from_millis(300), Duration::from_secs(5));
        } else if let Some(label) = step["await"].as_str() {
            if self.wait_for(label, false, PATIENCE).is_none() && !optional {
                return Err(format!("{label:?} never came"));
            }
        } else if let Some(seconds) = step["wait"].as_f64() {
            let long = Duration::from_secs_f64(seconds);
            let start = Instant::now();
            while start.elapsed() < long {
                let left = long.saturating_sub(start.elapsed());
                self.settle(left, left);
            }
        } else {
            return Err(format!("a step this window cannot do: {step}"));
        }
        Ok(())
    }

    /// The screen drawn in `theme`, as a PNG at `path`. Drawn with no
    /// pointer over it, so nothing is lit because the last click was there.
    fn write(&mut self, theme: &Theme, path: &Path) -> Result<(), String> {
        let style = self.program.style(&self.state, theme);
        let (size, scale) = (self.size, self.scale);
        let rgba = self.with_ui(|ui, renderer, clipboard| {
            let _ = ui.update(
                &[Event::Window(window::Event::RedrawRequested(Instant::now()))],
                mouse::Cursor::Unavailable,
                renderer,
                clipboard,
                &mut Vec::new(),
            );
            ui.draw(
                renderer,
                theme,
                &renderer::Style {
                    text_color: style.text_color,
                },
                mouse::Cursor::Unavailable,
            );
            let physical = Size::new((size.width * scale).round() as u32, (size.height * scale).round() as u32);
            (physical, renderer.screenshot(physical, scale, style.background_color))
        });
        let (physical, rgba) = rgba;
        let picture = image::RgbaImage::from_raw(physical.width, physical.height, rgba).ok_or("a screenshot of the wrong size")?;
        picture.save(path).map_err(|e| format!("{}: {e}", path.display()))
    }
}
