# Clipboard

`ark-clipboard` (`modules/clipboard`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `clipboard` in its `modules.toml` and depending on `ark-clipboard`.

Text put where the person can paste it.

## API

```rust
ark_clipboard::copy("$12.00") // -> ClipboardOp, output ()
trait Clipboard { fn copy_text(&self, text: String); }
ark_clipboard::fake::Fake // .copied()
```

## Per Platform

| Platform | Implementation | What it does |
|---|---|---|
| iPhone | `swift/PhoneClipboard.swift` | `UIPasteboard.general` |
| Mac | `swift/PhoneClipboard.swift` | `NSPasteboard.general` |
| Android | `kotlin/dev/arkui/modules/PhoneClipboard.kt` | `ClipboardManager` |
| Desktop and wasm (iced) | `Queued` (feature `desktop`) | the texts wait for the window, which writes each with `iced::clipboard::write` after the message |
| Browser (DOM) | `Browser` (feature `web`, wasm32) | `navigator.clipboard.writeText` |

## Permissions

None. (Android 13 shows its own "copied" toast; a browser writes only
from a page in a secure context, after a person's gesture.)
