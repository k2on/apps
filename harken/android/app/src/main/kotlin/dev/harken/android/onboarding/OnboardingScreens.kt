// The onboarding's screens, drawn from a step and nothing else — so an app
// can render each of them, in each of its states, without a network or a
// phone (harken's screenshots do). What each looks like is `ark-ui`'s
// (`dev.arkdb.ui`, stateless and knowing nothing of a session); what is
// here is only the step read into it: the identity's mark and name, the
// server as it is shown, and which phase a sign-in is in.
package dev.harken.android.onboarding

import androidx.compose.runtime.Composable
import dev.harken.android.ui.OpeningScreen
import dev.harken.android.ui.PermissionScreen
import dev.harken.android.ui.ServerScreen
import dev.harken.android.ui.SignInPhase
import dev.harken.android.ui.SignInScreen

/** How long [Opening] waits before it says anything: a replica already here never shows it. */
public const val OPENING_DELAY_MS: Long = dev.harken.android.ui.OPENING_DELAY_MS

/**
 * Signed in just now, and the library on its way from the server (`Step.Opening`): the mark and a
 * sentence saying where from, quietly, and only after [OPENING_DELAY_MS] — the page's own ground
 * until then. Never drawn for a launch with a login remembered: the splash covers that one.
 */
@Composable
public fun Opening(identity: AppIdentity, server: String, delayMs: Long = OPENING_DELAY_MS) {
    OpeningScreen(identity.name, identity.mark, Probe.display(server), delayMs)
}

/** Step one: the server's address. */
@Composable
public fun ServerStep(
    identity: AppIdentity,
    step: Step.Server,
    onType: (String) -> Unit,
    onContinue: () -> Unit
) {
    ServerScreen(
        identity.name, identity.mark, step.typed, step.error, step.checking, onType, onContinue)
}

/** Step two: signing in, in a browser tab, on the server's own page. */
@Composable
public fun SignInStep(
    identity: AppIdentity,
    step: Step.SignIn,
    onSignIn: () -> Unit,
    onChangeServer: () -> Unit
) {
    SignInScreen(
        identity.mark,
        Probe.display(step.server),
        step.error,
        when (step.phase) {
            Step.Phase.Idle -> SignInPhase.Idle
            Step.Phase.Browser -> SignInPhase.Browser
            Step.Phase.Exchanging -> SignInPhase.Exchanging
        },
        onSignIn,
        onChangeServer)
}

/** Step three: one permission, with the app's reason, asked once. */
@Composable
public fun PermissionStep(
    identity: AppIdentity,
    step: Step.Ask,
    onAllow: () -> Unit,
    onNotNow: () -> Unit
) {
    PermissionScreen(identity.mark, step.permission.title, step.permission.why, onAllow, onNotNow)
}
