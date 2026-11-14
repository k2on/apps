// The onboarding is the sign-in machine every Ark app shares
// (`ark_auth::onboarding`, through the bridge: `dev.arkdb.auth`'s
// `Onboarding`, `Step`, `Event` and `Effect`), whose rules are written and
// tested once, in Rust: a server with nobody signed in goes to sign in by
// itself, once a launch and again when the app comes back to the front; a
// sign-in goes by way of `Step.Opening`, which says the library is on its
// way, until `Event.Synced`; a denial or signing out comes back to signing
// in with the server kept; a join link is asked about first. [ArkHost] runs
// its effects.
//
// What is Android's alone is here: the permissions an app asks for, once,
// on its way to the app — over a session already opening beneath them.
package dev.harken.android.onboarding

/**
 * A permission an app asks for during onboarding, with the app's reason in a sentence. Open, so an
 * app with a need this module has not met declares its own.
 */
public abstract class Permission {
    /** What the remembered store records it was asked as. */
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

/** The declared permissions this platform (`sdk`) has and nobody has asked for yet, in order. */
public fun pending(permissions: List<Permission>, sdk: Int, asked: Set<String>): List<Permission> =
    permissions.filter { sdk >= it.since && it.key !in asked }
