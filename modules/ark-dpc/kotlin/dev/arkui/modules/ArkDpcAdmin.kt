// ark-dpc's device admin receiver (modules/ark-dpc/README.md): the component an app is made the
// device owner as (`dpm set-device-owner <applicationId>/dev.arkui.modules.ArkDpcAdmin`), which
// DevicePolicyManager names every call by. module.toml declares it in the manifest, guarded by
// BIND_DEVICE_ADMIN so only the system reaches it, with its policies (res/xml/ark_dpc_admin.xml).
//
// It does nothing on its own yet: what the app holds the phone to is the app's, made through
// `PhoneDpc` (README.md, In the Background).
package dev.arkui.modules

import android.app.admin.DeviceAdminReceiver
import android.content.ComponentName
import android.content.Context

class ArkDpcAdmin : DeviceAdminReceiver() {
    companion object {
        // This receiver, as DevicePolicyManager names the admin.
        fun component(context: Context) = ComponentName(context, ArkDpcAdmin::class.java)
    }
}
