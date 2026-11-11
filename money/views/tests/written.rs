//! money's screens as the generator writes them: every file there, the
//! first run's among them, its parts the shared ones.

#[test]
fn every_screen_is_written_with_the_first_run_among_them() {
    let files = ark_ui_gen::app::files(&money_views::screens(), "dev.money.android");
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    for n in ["WelcomeView.swift", "WelcomeScreen.kt", "AccountsView.swift", "BudgetScreen.kt"] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    let swift = &files.iter().find(|(n, _)| n == "WelcomeView.swift").unwrap().1;
    let kotlin = &files.iter().find(|(n, _)| n == "WelcomeScreen.kt").unwrap().1;
    if std::env::var_os("SHOW").is_some() {
        println!("{swift}\n{kotlin}");
    }
    assert!(swift.contains("FirstRunPage(") && swift.contains("screen.send(.check)"));
    assert!(kotlin.contains("import uniffi.money_bridge.WelcomeMsg") && kotlin.contains("WelcomeMsg.Typed("));
}
