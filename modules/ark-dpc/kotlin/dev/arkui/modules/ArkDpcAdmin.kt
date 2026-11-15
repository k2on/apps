// ark-dpc's device admin receiver (modules/ark-dpc/README.md): the component an app is made the
// device owner as (`dpm set-device-owner <applicationId>/dev.arkui.modules.ArkDpcAdmin`), which
// DevicePolicyManager names every call by. module.toml declares it in the manifest, guarded by
// BIND_DEVICE_ADMIN so only the system reaches it, with its policies (res/xml/ark_dpc_admin.xml).
//
// Made the owner (`onEnabled`, or a provisioning's `onProfileProvisioningComplete`), it wakes the
// process's one runtime (`ArkProcess.wake(Enrolled, …)`): the app's update holds the phone to its
// policy at once, with no screen, rather than at the next periodic sync. The broadcast is kept
// open (`goAsync`) until the runtime is idle or its few seconds are nearly up.
package dev.arkui.modules

import android.app.admin.DeviceAdminReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import dev.arkdb.ffi.ArkProcess
import dev.arkdb.ffi.WakeDone
import dev.arkdb.ffi.WakeReason

class ArkDpcAdmin : DeviceAdminReceiver() {
    override fun onEnabled(context: Context, intent: Intent) = enrolled()

    override fun onProfileProvisioningComplete(context: Context, intent: Intent) = enrolled()

    private fun enrolled() {
        val pending = goAsync()
        ArkProcess.shared().wake(WakeReason.Enrolled, BUDGET_MS, object : WakeDone {
            override fun finished(idle: Boolean) = pending.finish()
        })
    }

    companion object {
        /** A broadcast has about ten seconds; the first sync after it is the periodic job's. */
        const val BUDGET_MS = 9_000uL

        // This receiver, as DevicePolicyManager names the admin.
        fun component(context: Context) = ComponentName(context, ArkDpcAdmin::class.java)
    }
}
