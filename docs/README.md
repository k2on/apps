# docs

`plan-v5.md` is the design and the record of what landed; the other
`plan-*.md` files are the plans each piece was built from, and
`arkdb.md` the earlier specification, kept as history. Plans that are only
superseded history are not kept in the tree —
`plan-v4.md` (spec v4, replaced by v5), `plan-phones.md` (the Kotlin and
Swift ports, replaced by the kernel over FFI) and `scopes.md` (the scopes
design removed at spec v3) — and git keeps them: `git show
a55b0e1:docs/plan-v4.md`, and likewise for the others.

`ui/docs/motion.md` is not a plan but a standing rule: how every app moves and
answers a touch on Android, the iPhone and the desktop, mapped onto
arkui's shared springs, press, haptics and reduced motion in each toolkit,
with the checklist a change to motion is reviewed against.

`ui/docs/parts.md` lists ArkUI's generic parts — a page's header, a section's
heading, a grouped card, a pill, a pager and the rest — by the one name and
inputs each has in Compose, SwiftUI and iced. What each app draws that is
its own is `spec/money-components.md` and `spec/harken-components.md`.
