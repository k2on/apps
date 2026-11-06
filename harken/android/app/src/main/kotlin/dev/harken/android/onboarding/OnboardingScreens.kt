// The onboarding's screens, drawn from a step and nothing else — so an app
// can render each of them, in each of its states, without a network or a
// phone (harken's screenshots do). What each looks like is `ark-ui`'s
// (`dev.arkdb.ui`, stateless and knowing nothing of a session); what is
// here is only the step read into it: the identity's mark and name, the
// server as it is shown, and which phase a sign-in is in.
package dev.harken.android.onboarding

import androidx.compose.runtime.Composable
import dev.arkdb.auth.Phase
import dev.arkdb.auth.Step
import dev.arkui.server.Probe
import dev.arkui.OpeningScreen
import dev.arkui.PermissionScreen
import dev.arkui.ServerScreen
import dev.arkui.SignInPhase
import dev.arkui.SignInScreen

/** How long [Opening] waits before it says anything: a replica already here never shows it. */
public const val OPENING_DELAY_MS: Long = dev.arkui.OPENING_DELAY_MS

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
            Phase.IDLE -> SignInPhase.Idle
            Phase.BROWSER -> SignInPhase.Browser
            Phase.EXCHANGING -> SignInPhase.Exchanging
        },
        onSignIn,
        onChangeServer)
}

/** On the way to the app: one permission, with the app's reason, asked once. */
@Composable
public fun PermissionStep(
    identity: AppIdentity,
    permission: Permission,
    onAllow: () -> Unit,
    onNotNow: () -> Unit
) {
    PermissionScreen(identity.mark, permission.title, permission.why, onAllow, onNotNow)
}
