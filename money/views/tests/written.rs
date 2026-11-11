//! money's screens as the generator writes them: every file there, the
//! first run's among them, its parts the shared ones.

#[test]
fn every_screen_is_written_with_the_first_run_among_them() {
    let files = ark_ui_gen::app::files(&money_views::screens(), "dev.money.android");
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    for n in [
        "WelcomeView.swift",
        "WelcomeScreen.kt",
        "AccountsView.swift",
        "BudgetScreen.kt",
        "TransactionsView.swift",
        "TransactionsScreen.kt",
        "RegisterView.swift",
        "RegisterScreen.kt",
        "SheetsView.swift",
        "SheetsScreen.kt",
        "InsightsView.swift",
        "InsightsScreen.kt",
    ] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    let swift = &files.iter().find(|(n, _)| n == "WelcomeView.swift").unwrap().1;
    let kotlin = &files.iter().find(|(n, _)| n == "WelcomeScreen.kt").unwrap().1;
    if let Some(show) = std::env::var_os("SHOW") {
        for (n, t) in &files {
            if n.starts_with(show.to_str().unwrap_or_default()) {
                println!("{t}");
            }
        }
    }
    let register = &files.iter().find(|(n, _)| n == "RegisterView.swift").unwrap().1;
    assert!(register.contains("DataTable(columns: [DataColumn(\"Payee\"") && register.contains("DataTableRow("));
    let register_kt = &files.iter().find(|(n, _)| n == "RegisterScreen.kt").unwrap().1;
    assert!(register_kt.contains("ColumnSlot.TrailingSub") && register_kt.contains("ColumnAlign.End"));
    assert!(swift.contains("FirstRunPage(") && swift.contains("screen.send(.check)"));
    assert!(kotlin.contains("import uniffi.money_bridge.WelcomeMsg") && kotlin.contains("WelcomeMsg.Typed("));
}
