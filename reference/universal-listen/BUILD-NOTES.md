# Build notes

The app was written without a local toolchain and compiled only on CI
(`reference-listen-shots`, macos-15, Xcode 26.3, iOS 26.2 / macOS 26.2
SDKs). The fixes each CI round needed, in order:

1. **No AppIcon** — XcodeGen's default `ASSETCATALOG_COMPILER_APPICON_NAME`
   is `AppIcon`, and actool fails without one. Added a procedurally drawn
   icon (coral gradient, a dot with sound rings either side): a 1024 pt
   universal iOS image and the Mac sizes.
2. **`AdaptableTabBarPlacement.sidebar` is unavailable on macOS** — the
   Library and Podcasts tabs (`.defaultVisibility(.hidden, for: .sidebar)`)
   are iOS-only now, inside `#if os(iOS)`; the Mac never has a tab bar, so
   it never needed them.

With those two, both platforms compiled. The first screenshots then showed
these runtime faults, fixed in the next round:

3. **Mac crash on Songs and on popovers** — “No Observable object of type
   Library found”: Mac `Table` cells, popover content and menu content are
   hosted outside the view tree and did not inherit the environment's
   `@Observable` objects. Table cells now take plain values; popovers,
   sheets, the full-screen cover, context menus (`listenContextMenu`) and
   item menus (`ListenMenu`) hand the model on explicitly (`ListenModel`).
4. **iPhone tab bar showed “More”** — the sidebar-only sections counted
   towards the tab bar despite `.defaultVisibility(.hidden, for: .tabBar)`;
   they are now built only at regular width (`if !isCompact`).
5. **Mac deep links didn't push** — a `-screen album:…` path set together
   with the tab was ignored on macOS; the path is now pushed a moment later
   (`Navigator.applyLaunchPaths`).
6. **iPadOS restored the inspector** between driven launches — a launch with
   `-screen` now deletes the app's saved scene state first.
7. CI: the Mac UI test could not read the app in the job's temp folder (now
   copied to /Applications), a timed-out launch ended a driven run (the
   missing shots are now retried once), and a Mac shot whose window vanished
   copied the previous picture (now an error).
