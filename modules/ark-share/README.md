# Share

`ark-share` (`modules/ark-share`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `share` in its `modules.toml` and depending on `ark-share`.

Text, and a link when there is one, handed to the system's share sheet.

## API

```rust
ark_share::share("Checking", Some(url)) // -> ShareOp, output ()
trait Share { fn share(&self, text: String, url: Option<String>); }
ark_share::Fake // .shared()
```

## Per Platform

| Platform | Implementation | What it does |
|---|---|---|
| iPhone, iPad | `swift/PhoneShare.swift` | `UIActivityViewController` over whatever is in front (a popover from the window's middle on an iPad) |
| Mac | `swift/PhoneShare.swift` | `NSSharingServicePicker` over the key window |
| Android | none yet | answered `Unavailable` (an `Intent.ACTION_SEND` chooser is the plan) |
| Desktop (iced), browser | none | answered `Unavailable` |

## Permissions

None.
