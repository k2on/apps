use super::*;
use ark_peer::store::Opening;

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
    Scanned(ModuleAnswer),
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
            Msg::Scan => vec![Command::Ask {
                call: ark_camera::scan_qr(),
                then: Some(Msg::Scanned),
            }],
            Msg::Scanned(a) => {
                s.said.push(format!("{a:?}"));
                vec![Command::Then(Msg::Add)]
            }
            Msg::Go => vec![Command::Open("settings"), Command::Back],
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
    inbox: Inbox,
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
    fn read<R>(&self, f: impl FnOnce(&Store<NoView>) -> R) -> R {
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
    fn ask(&mut self, ticket: Ticket, call: ModuleCall) {
        self.inbox.ask(&self.modules, ticket, call)
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
    assert_eq!(h.opened, vec!["settings", "back"]);
}

#[test]
fn a_later_answer_runs_its_message_then_what_it_asks() {
    let mut rt = Runtime::<Counter>::new();
    let mut h = Here::new();
    rt.dispatch(Msg::Scan, &mut h);
    assert!(rt.awaits(1));
    h.camera.scanned(Some("code".into()));
    for (t, a) in h.inbox.take() {
        rt.answer(t, a, &mut h);
    }
    assert!(!rt.awaits(1));
    assert_eq!(rt.state().n, 1);
    assert_eq!(rt.state().said, vec!["Text { value: Some(\"code\") }"]);
}

#[test]
fn commands_debug_as_what_they_ask() {
    let c: Command<Counter> = Command::Send {
        change: 3,
        then: Some(Msg::Saved),
    };
    assert_eq!(format!("{c:?}"), "Send(3, then)");
}
