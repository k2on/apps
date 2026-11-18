# Tech Debt

What we know we owe: each entry says what it is, why it was left, and what
paying it would take. An entry goes when it is paid.

## Confirmation Is Said Twice, and a Refusal Is Only a Message

**What.** A device's pushed entry is answered twice: a verdict to that
device (`Verdict::At(seq)` or `Refused`), and the entry itself in the log
(`Logged { entry, facts }`). Acceptance needs only the second. A refusal
needs the first, but as a message it can be lost: a connection that drops
while a refusal is on its way, followed by a fill saying the device's
entries are judged up to a later one, lets the device drop the refused
entry without ever telling its user.

**How Zero does it.** Each client numbers its mutations, and the server
keeps that client's `lastMutationID`, written in the same transaction as
the mutation's own changes. Every update carries the counter, and a client
drops its pending mutations up to it. A mutation whose code fails is
rolled back but still advances the counter, so it never blocks the queue.
Its error is written to a `mutations` results table that syncs to the
client like any other rows, so a dropped connection cannot lose it.

**The fix here.**
- `Verdict::At` becomes a per-origin "last judged" count, carried with log
  updates and fills.
- Refusals become rows in a small synced table scoped to their owner,
  delivered by the subscriptions that already deliver rows reliably.
- Fills need no special handling for refusals.
- It changes the protocol in all three senders: the authority, `Serve`,
  and the `Relay`.

**Why it was left.** About 120 bytes of watch code for a protocol change
across those three. A first attempt (folding `At` into `Logged`, with
"judged up to" in the fill) lost a refusal in the convergence test and was
set aside. Do it when the protocol is next reworked.
