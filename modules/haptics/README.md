# Haptics

`ark-haptics` (`modules/haptics`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `haptics` in its `modules.toml` and depending on `ark-haptics`.

The phone's answer in the hand: a success, a selection step, a warning
(`ui/docs/motion.md`, Haptics). Fired on the change itself, never on a tap
that might change nothing.

## API

```rust
ark_haptics::perform(Haptic::Success) // -> ModuleCall, answered Done
trait Haptics { fn perform(&self, haptic: Haptic); }
ark_haptics::fake::Fake // .performed()
```

## Per Platform

| Platform | Implementation | What it does |
|---|---|---|
| iPhone | `swift/PhoneHaptics.swift` | `UINotificationFeedbackGenerator` (success, warning), `UISelectionFeedbackGenerator` (selection) |
| Mac | `swift/PhoneHaptics.swift` | `NSHapticFeedbackManager` on a Force Touch trackpad: `.alignment` for a selection, `.levelChange` for the others |
| Android | `kotlin/dev/arkui/modules/PhoneHaptics.kt` | ArkUI's `View.perform` (`Feedback.Selection`, `Feedback.Success`; a warning is a success's weight) |
| Desktop (iced) | none | answered `Unavailable` |
| Browser | none | answered `Unavailable` |

## Permissions

None.
