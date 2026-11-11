// ark-dpc on Android (modules/ark-dpc/README.md): the `Dpc` interface UniFFI writes, over the
// framework's DevicePolicyManager as this app's admin, `ArkDpcAdmin`. Every call is the device
// owner's: until the app is made it (`dpm set-device-owner`), a read says `owner = false` and
// nothing else, and a change fails, said. Each change is made at once, on the caller's thread.
package dev.arkui.modules

import android.app.admin.DevicePolicyManager
import android.content.Context
import android.os.Build
import uniffi.ark_dpc.ApplyReply
import uniffi.ark_dpc.Asked
import uniffi.ark_dpc.Change
import uniffi.ark_dpc.Dpc
import uniffi.ark_dpc.Grant
import uniffi.ark_dpc.PermissionState
import uniffi.ark_dpc.ReadReply
import uniffi.ark_dpc.State

class PhoneDpc(context: Context) : Dpc {
    private val app = context.applicationContext
    private val dpm = app.getSystemService(DevicePolicyManager::class.java)
    private val admin = ArkDpcAdmin.component(app)

    private fun owner() = dpm.isDeviceOwnerApp(app.packageName)

    override fun apply(change: Change, reply: ApplyReply) {
        if (!owner()) return reply.failed("not the device owner")
        try {
            when (change) {
                is Change.AlwaysOnVpn -> dpm.setAlwaysOnVpnPackage(admin, change.app, change.app != null && change.lockdown)
                is Change.Restriction ->
                    if (change.on) dpm.addUserRestriction(admin, change.name) else dpm.clearUserRestriction(admin, change.name)
                is Change.UninstallBlocked -> dpm.setUninstallBlocked(admin, change.app, change.blocked)
                is Change.Permission -> {
                    // False when the app or the permission is not one, or the permission is not a
                    // runtime one the owner may decide.
                    if (!dpm.setPermissionGrantState(admin, change.app, change.permission, grantState(change.grant))) {
                        return reply.failed("${change.permission} of ${change.app} cannot be set")
                    }
                }
                is Change.CameraDisabled -> dpm.setCameraDisabled(admin, change.disabled)
                is Change.ScreenCaptureDisabled -> dpm.setScreenCaptureDisabled(admin, change.disabled)
            }
            reply.applied()
        } catch (e: Exception) {
            // A VPN app that is not installed (NameNotFoundException), or cannot be always on
            // (UnsupportedOperationException: it opted out); a restriction the platform refuses.
            reply.failed(e.message ?: e.toString())
        }
    }

    override fun read(asked: Asked, reply: ReadReply) {
        if (!owner()) {
            return reply.saw(
                State(
                    owner = false,
                    alwaysOnVpn = null,
                    lockdown = false,
                    restrictions = emptyList(),
                    uninstallBlocked = emptyList(),
                    permissions = emptyList(),
                    cameraDisabled = false,
                    screenCaptureDisabled = false,
                    sdk = Build.VERSION.SDK_INT,
                    securityPatch = Build.VERSION.SECURITY_PATCH,
                ),
            )
        }
        try {
            val restrictions = dpm.getUserRestrictions(admin).let { b -> b.keySet().filter { b.getBoolean(it) }.sorted() }
            reply.saw(
                State(
                    owner = true,
                    alwaysOnVpn = dpm.getAlwaysOnVpnPackage(admin),
                    lockdown = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && dpm.isAlwaysOnVpnLockdownEnabled(admin),
                    restrictions = restrictions,
                    uninstallBlocked = asked.apps.filter { dpm.isUninstallBlocked(admin, it) },
                    permissions = asked.permissions.map {
                        PermissionState(it.app, it.permission, grantOf(dpm.getPermissionGrantState(admin, it.app, it.permission)))
                    },
                    cameraDisabled = dpm.getCameraDisabled(admin),
                    screenCaptureDisabled = dpm.getScreenCaptureDisabled(admin),
                    sdk = Build.VERSION.SDK_INT,
                    securityPatch = Build.VERSION.SECURITY_PATCH,
                ),
            )
        } catch (e: Exception) {
            reply.failed(e.message ?: e.toString())
        }
    }

    private fun grantState(g: Grant) = when (g) {
        Grant.DEFAULT -> DevicePolicyManager.PERMISSION_GRANT_STATE_DEFAULT
        Grant.GRANTED -> DevicePolicyManager.PERMISSION_GRANT_STATE_GRANTED
        Grant.DENIED -> DevicePolicyManager.PERMISSION_GRANT_STATE_DENIED
    }

    private fun grantOf(state: Int) = when (state) {
        DevicePolicyManager.PERMISSION_GRANT_STATE_GRANTED -> Grant.GRANTED
        DevicePolicyManager.PERMISSION_GRANT_STATE_DENIED -> Grant.DENIED
        else -> Grant.DEFAULT
    }
}
