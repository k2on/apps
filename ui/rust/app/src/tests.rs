use super::*;
use ark_peer::store::{Opening, Store};

/// A counter: the smallest app with every kind of command.
struct Counter;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NoView;

impl ViewKey for NoView {
    fn open(&self) -> Option<Opening> {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Add,
    Save,
    Saved(Result<(), String>),
    Scan,
    Scanned(Answer<Option<String>>),
    Go,
}

#[derive(Default)]
struct State {
    n: i64,
    said: Vec<String>,
}

impl App for Counter {
    type Key = NoView;
    type State = State;
    type Msg = Msg;
    type Change = i64;
    type Route = &'static str;
    type Request = &'static str;
    type Screen = String;

    fn holds(_: &State) -> Vec<NoView> {
        vec![]
    }

    fn update(s: &mut State, msg: Msg, data: &Data<'_, NoView>) -> Vec<Command<Self>> {
        match msg {
            Msg::Add => {
                s.n += 1;
                vec![Command::ask(ark_haptics::perform(ark_haptics::Haptic::Selection))]
            }
            Msg::Save => vec![Command::Send {
                change: s.n,
                then: Some(Msg::Saved),
            }],
            Msg::Saved(r) => {
                s.said.push(format!("{r:?} at {}", data.now_ms));
                vec![]
            }
            Msg::Scan => vec![Command::ask_then(ark_camera::scan_qr(), Msg::Scanned)],
            Msg::Scanned(a) => {
                s.said.push(format!("{a:?}"));
                vec![Command::Then(Msg::Add)]
            }
            Msg::Go => vec![Command::Open("settings"), Command::Back, Command::Request("sync")],
        }
    }

    fn screen(s: &State, _: &Data<'_, NoView>) -> String {
        format!("{} {}", s.n, s.said.join("; "))
    }
}

/// A host over an empty store, the fakes for modules.
struct Here {
    store: Store<NoView>,
    haptics: ark_haptics::fake::Fake,
    camera: ark_camera::Fake,
    modules: Modules,
    inbox: Inbox<Msg>,
    sent: Vec<i64>,
    opened: Vec<String>,
}

impl Here {
    fn new() -> Here {
        let haptics = ark_haptics::fake::Fake::new();
        let camera = ark_camera::Fake::new();
        let mut modules = Modules::new();
        haptics.register(&mut modules);
        camera.register(&mut modules);
        Here {
            store: Store::new(),
            haptics,
            camera,
            modules,
            inbox: Inbox::default(),
            sent: vec![],
            opened: vec![],
        }
    }
}

impl Host<Counter> for Here {
    fn read<R>(&self, f: impl FnOnce(&dyn Views<NoView>) -> R) -> R {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        42
    }
    fn send(&mut self, change: i64) -> Result<(), String> {
        self.sent.push(change);
        if change > 1 {
            Err("too many".into())
        } else {
            Ok(())
        }
    }
    fn open(&mut self, route: &'static str) {
        self.opened.push(route.into())
    }
    fn back(&mut self) {
        self.opened.push("back".into())
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<Msg>) {
        self.inbox.ask(&self.modules, ticket, ask)
    }
    fn request(&mut self, r: &'static str) {
        self.opened.push(r.into())
    }
}

#[test]
fn a_message_updates_and_grows_the_version() {
    let mut rt = Runtime::<Counter>::new();
    let mut h = Here::new();
    assert_eq!(rt.version(), 0);
    rt.dispatch(Msg::Add, &mut h);
    assert_eq!(rt.state().n, 1);
    assert!(rt.version() > 0);
    assert_eq!(h.haptics.performed(), vec![ark_haptics::Haptic::Selection]);
    assert_eq!(rt.screen(&h), "1 ");
}

#[test]
fn what_became_of_a_change_comes_back_as_a_message() {
    let mut rt = Runtime::<Counter>::new();
    let mut h = Here::new();
    rt.dispatch(Msg::Save, &mut h);
    rt.dispatch(Msg::Add, &mut h);
    rt.dispatch(Msg::Add, &mut h);
    rt.dispatch(Msg::Save, &mut h);
    assert_eq!(h.sent, vec![0, 2]);
    assert_eq!(rt.state().said, vec!["Ok(()) at 42", "Err(\"too many\") at 42"]);
}

#[test]
fn routes_are_handed_to_the_host() {
    let mut rt = Runtime::<Counter>::new();
    let mut h = Here::new();
    rt.dispatch(Msg::Go, &mut h);
    assert_eq!(h.opened, vec!["settings", "back", "sync"]);
}

#[test]
fn a_later_answer_runs_its_message_then_what_it_asks() {
    let mut rt = Runtime::<Counter>::new();
    let mut h = Here::new();
    rt.dispatch(Msg::Scan, &mut h);
    assert!(rt.awaits(1));
    assert!(!rt.is_idle(&h.store), "a scan awaited is not idle");
    h.camera.scanned(Some("code".into()));
    let inbox = h.inbox.clone();
    inbox.drain(&mut rt, &mut h);
    assert!(!rt.awaits(1));
    assert_eq!(rt.state().n, 1);
    assert_eq!(rt.state().said, vec!["Ok(Some(\"code\"))"]);
    // The haptic the answer asked for was answered at once.
    assert!(rt.is_idle(&h.store));
}

/// An operation is data: it compares and prints as itself and its
/// `then`, whatever its output's type. Falsified by an `Ask` that compared
/// equal to another module's or another `then`'s.
#[test]
fn an_ask_compares_and_prints_as_its_operation() {
    let a: Command<Counter> = Command::ask(ark_haptics::perform(ark_haptics::Haptic::Selection));
    assert_eq!(a, Command::ask(ark_haptics::perform(ark_haptics::Haptic::Selection)));
    assert_ne!(a, Command::ask(ark_haptics::perform(ark_haptics::Haptic::Success)));
    assert_ne!(a, Command::ask(ark_camera::scan_qr()));
    let scan: Command<Counter> = Command::ask_then(ark_camera::scan_qr(), Msg::Scanned);
    assert_eq!(scan, Command::ask_then(ark_camera::scan_qr(), Msg::Scanned));
    assert_ne!(scan, Command::ask(ark_camera::scan_qr()));
    assert_eq!(format!("{a:?}"), "Ask(Perform(Selection))");
    assert_eq!(format!("{scan:?}"), "Ask(ScanQr, then)");
    let Command::Ask(ask) = scan else { unreachable!() };
    assert_eq!(ask.module(), "camera");
    assert_eq!(ask.op::<ark_camera::ScanQr>(), Some(&ark_camera::ScanQr));
    assert!(ask.op::<ark_camera::TakePhoto>().is_none());
}

/// Idle is no answer awaited and every view held: a host with no module
/// answers `Unavailable`, so it is idle again at once.
#[test]
fn a_runtime_is_idle_once_nothing_is_left_to_come() {
    let mut rt = Runtime::<Counter>::new();
    let mut h = Here::new();
    assert!(rt.is_idle(&h.store));
    h.modules = Modules::new();
    rt.dispatch(Msg::Scan, &mut h);
    let v = rt.version();
    let inbox = h.inbox.clone();
    inbox.drain(&mut rt, &mut h);
    assert!(rt.is_idle(&h.store));
    assert_eq!(rt.state().said, vec!["Err(Unavailable)"]);
    assert!(rt.version() > v);
}

#[test]
fn commands_debug_as_what_they_ask() {
    let c: Command<Counter> = Command::Send {
        change: 3,
        then: Some(Msg::Saved),
    };
    assert_eq!(format!("{c:?}"), "Send(3, then)");
}

#[test]
fn launch_arguments_are_read_as_pairs() {
    use crate::shell::Launch;
    let l = Launch::parse(&[
        "Money",
        "-screen",
        "account:Rewards Visa",
        "-inspector",
        "1",
        "--appearance",
        "Dark",
        "-NSDocumentRevisionsDebugMode",
        "YES",
    ]);
    assert_eq!(l.screen.as_deref(), Some("account"));
    assert_eq!(l.item.as_deref(), Some("Rewards Visa"));
    assert_eq!(l.inspector, Some(true));
    assert_eq!(l.appearance.as_deref(), Some("dark"));
    assert_eq!(l.pane, None);
    assert!(Launch::parse(&["money-iced", "--server", "http://x", "--user", "alex"]).is_empty());
    let s = Launch::parse(&["-screen", "settings", "-settingsTab", "banks"]);
    assert_eq!((s.screen.as_deref(), s.pane.as_deref()), (Some("settings"), Some("banks")));
}
