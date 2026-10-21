// The onboarding as a state machine: three steps — the server, signing in,
// the permissions — and the app once all three are behind it.
//
// Every transition is `next(step, event)`, a pure function returning the
// step and the effects to run: remember this, open that, ask for the other.
// [ArkHost] runs the effects and feeds back what came of them; nothing here
// touches Android, the network or a clock, so every transition is held by
// plain JVM tests (`OnboardingTests`).
//
// What it remembers decides where it starts: nothing is the server step, a
// server alone is signing in, a server and a login is the app — by way of
// any permission the app declares that has not been asked for yet.
//
// **A remembered login goes straight to the app; only a sign-in opens.**
// A launch with a login remembered is the app at once, over a session
// whose replica is already on the phone — there is nothing to wait for
// that a screen should say, and the activity holds the system splash
// over the few hundred milliseconds it takes (`holdSplash`). Signing in,
// though, is the one time the library may genuinely not be here yet: the
// replica is empty until the server's snapshot arrives. So a sign-in goes
// to [Step.Opening], which says so, and on to the app when the session has
// what the server had ([Event.Synced]). A
// server's `Denied` (a revoked or expired session) and signing out both
// come back to *Sign in* with the server kept; the replica and its pending
// intents are never the onboarding's to touch, so signing in again as the
// same person pushes them.
package dev.harken.android.onboarding

import dev.arkdb.ffi.Login
import dev.arkdb.ffi.loginUrl

/**
 * A permission an app asks for during onboarding, with the app's reason in a sentence. Open, so an
 * app with a need this module has not met declares its own.
 */
public abstract class Permission {
    /** What [Remembered] records it was asked as. */
    public abstract val key: String

    /** The platform's name for it. */
    public abstract val android: String

    /** The first API level that has it; below, it is granted by default and never asked. */
    public abstract val since: Int

    /** What the screen calls it. */
    public abstract val title: String

    /** Why the app wants it, in the app's own sentence. */
    public abstract val why: String

    /** Posting notifications: what lets a player show its notification and lock-screen controls. */
    public data class Notifications(override val why: String) : Permission() {
        override val key: String = "notifications"
        override val android: String = "android.permission.POST_NOTIFICATIONS"
        override val since: Int = 33
        override val title: String = "Notifications"
    }
}

/** Where the onboarding stands. */
public sealed class Step {
    /** The server's address: what was typed, whether it is being checked, and why it was not. */
    public data class Server(
        val typed: String = "",
        val checking: Boolean = false,
        val error: String? = null
    ) : Step()

    /** Signing in at `server`. */
    public data class SignIn(
        val server: String,
        val phase: Phase = Phase.Idle,
        val error: String? = null
    ) : Step()

    /**
     * One permission, asked once — on the way to the app over a login that was `remembered` at
     * launch, or one just signed in.
     */
    public data class Ask(
        val server: String,
        val login: Login,
        val permission: Permission,
        val remembered: Boolean = false,
    ) : Step()

    /**
     * Signed in just now: the session is open and the server's snapshot is on its way, until
     * [Event.Synced]. Never the step of a launch with a login remembered.
     */
    public data class Opening(val server: String, val login: Login) : Step()

    /** The app, over a session at `server` as `login`. */
    public data class Running(val server: String, val login: Login) : Step()

    public enum class Phase {
        /** Waiting for a press. */
        Idle,

        /** The server's sign-in is open in a browser tab. */
        Browser,

        /** A code came back and is being exchanged. */
        Exchanging,
    }
}

/** What happened. */
public sealed class Event {
    public data class Typed(val text: String) : Event()

    /** "Continue" on the server step. */
    public object Check : Event()

    /** What the server step's probe found. */
    public data class Checked(val found: Found) : Event()

    /** "Use another server", from signing in. */
    public object ChangeServer : Event()

    /** "Sign in": the browser opens on the server's sign-in. */
    public object OpenSignIn : Event()

    /** The activity came back to the front with no code: the tab was closed. */
    public object Returned : Event()

    /** `<scheme>://auth?code=…` arrived. */
    public data class Code(val code: String) : Event()

    public data class SignedIn(val login: Login) : Event()

    public data class SignInFailed(val why: String) : Event()

    /** "Allow" on a permission: the platform's dialog. */
    public object Allow : Event()

    /** The permission was answered — by the platform's dialog, or "Not now". */
    public object Answered : Event()

    /** The server turned the session away. */
    public data class Denied(val reason: String) : Event()

    /**
     * The session holds what the server had — or the server cannot be reached, and nothing more is
     * coming soon: either way, what there is to show is there.
     */
    public object Synced : Event()

    public object SignOut : Event()
}

/** What a transition asks for. */
public sealed class Effect {
    /** Ask `/healthz` of what was typed (`Probe.find`); answer with [Event.Checked]. */
    public data class FindServer(val typed: String) : Effect()

    public data class RememberServer(val url: String) : Effect()

    /** Open `url` in a Custom Tab. */
    public data class Browse(val url: String) : Effect()

    /** Trade the code at `/auth/exchange`; answer with [Event.SignedIn] or [Event.SignInFailed]. */
    public data class Exchange(val server: String, val code: String) : Effect()

    public data class RememberLogin(val login: Login) : Effect()

    public object ForgetLogin : Effect()

    /** The platform's permission dialog; answer with [Event.Answered]. */
    public data class Request(val permission: Permission) : Effect()

    public data class MarkAsked(val key: String) : Effect()

    /** Open the peer at `server` as `login`, and hand it to the app. */
    public data class OpenSession(val server: String, val login: Login) : Effect()

    /** Close the peer, written down; its storage stays. */
    public object CloseSession : Effect()
}

/** A step and what it asks for. */
public data class Move(val step: Step, val effects: List<Effect> = emptyList())

/**
 * The machine for one app: the permissions it declares, the platform's API level, and the scheme
 * the sign-in comes back on (`<scheme>://auth`).
 */
public class Onboarding(
    public val permissions: List<Permission>,
    public val sdk: Int,
    public val scheme: String,
) {
    /** Where the sign-in sends the code back to. */
    public val redirect: String
        get() = "$scheme://auth"

    /** The declared permissions this platform has and nobody has asked for yet, in order. */
    public fun pending(asked: Set<String>): List<Permission> =
        permissions.filter { sdk >= it.since && it.key !in asked }

    /** Where a launch starts, from what was remembered. */
    public fun start(server: String?, login: Login?, asked: Set<String>): Move =
        when {
            server == null -> Move(Step.Server())
            login == null -> Move(Step.SignIn(server))
            else -> onward(server, login, asked, emptyList(), remembered = true)
        }

    // Signed in: the next permission to ask, then the app — at once over a
    // login remembered, by way of opening over one just signed in.
    private fun onward(
        server: String,
        login: Login,
        asked: Set<String>,
        before: List<Effect>,
        remembered: Boolean,
    ): Move {
        val next = pending(asked).firstOrNull()
        val open = before + Effect.OpenSession(server, login)
        return when {
            next != null -> Move(Step.Ask(server, login, next, remembered), before)
            remembered -> Move(Step.Running(server, login), open)
            else -> Move(Step.Opening(server, login), open)
        }
    }

    // The server turned the session away, or the person signed out: back to
    // signing in, the server kept, the session closed and the login forgotten.
    private fun out(server: String, event: Event): Move? =
        when (event) {
            is Event.Denied ->
                Move(
                    Step.SignIn(
                        server,
                        error =
                            "The server signed you out (${event.reason}). Sign in again — what's on this phone is kept, and syncs once you do."),
                    listOf(Effect.CloseSession, Effect.ForgetLogin))
            is Event.SignOut ->
                Move(Step.SignIn(server), listOf(Effect.CloseSession, Effect.ForgetLogin))
            else -> null
        }

    /** One event, at one step; an event that means nothing there leaves it as it was. */
    public fun next(step: Step, event: Event, asked: Set<String>): Move {
        val same = Move(step)
        return when (step) {
            is Step.Server ->
                when (event) {
                    is Event.Typed -> Move(step.copy(typed = event.text, error = null))
                    is Event.Check ->
                        if (step.checking) same
                        else
                            when (val a = Address.parse(step.typed)) {
                                is Address.Invalid -> Move(step.copy(error = a.why))
                                is Address.Candidates ->
                                    Move(
                                        step.copy(checking = true, error = null),
                                        listOf(Effect.FindServer(step.typed)))
                            }
                    is Event.Checked ->
                        if (!step.checking) same
                        else
                            when (val f = event.found) {
                                is Found.Server ->
                                    Move(Step.SignIn(f.url), listOf(Effect.RememberServer(f.url)))
                                is Found.Failed -> Move(step.copy(checking = false, error = f.why))
                            }
                    else -> same
                }
            is Step.SignIn ->
                when (event) {
                    is Event.OpenSignIn ->
                        if (step.phase == Step.Phase.Exchanging) same
                        else
                            Move(
                                step.copy(phase = Step.Phase.Browser, error = null),
                                listOf(Effect.Browse(loginUrl(step.server, redirect))))
                    is Event.Returned ->
                        if (step.phase == Step.Phase.Browser)
                            Move(step.copy(phase = Step.Phase.Idle))
                        else same
                    is Event.Code ->
                        if (step.phase == Step.Phase.Exchanging) same
                        else
                            Move(
                                step.copy(phase = Step.Phase.Exchanging, error = null),
                                listOf(Effect.Exchange(step.server, event.code)))
                    is Event.SignedIn ->
                        if (step.phase != Step.Phase.Exchanging) same
                        else
                            onward(
                                step.server,
                                event.login,
                                asked,
                                listOf(Effect.RememberLogin(event.login)),
                                remembered = false)
                    is Event.SignInFailed ->
                        if (step.phase != Step.Phase.Exchanging) same
                        else Move(step.copy(phase = Step.Phase.Idle, error = event.why))
                    is Event.ChangeServer ->
                        if (step.phase == Step.Phase.Exchanging) same
                        else Move(Step.Server(typed = Probe.display(step.server)))
                    else -> same
                }
            is Step.Ask ->
                when (event) {
                    is Event.Allow -> Move(step, listOf(Effect.Request(step.permission)))
                    is Event.Answered ->
                        onward(
                            step.server,
                            step.login,
                            asked + step.permission.key,
                            listOf(Effect.MarkAsked(step.permission.key)),
                            step.remembered)
                    else -> same
                }
            is Step.Opening ->
                when (event) {
                    is Event.Synced -> Move(Step.Running(step.server, step.login))
                    else -> out(step.server, event) ?: same
                }
            is Step.Running -> out(step.server, event) ?: same
        }
    }
}
