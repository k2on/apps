// ark-notifications' push on Android (README.md): UnifiedPush, the receiver protocol implemented
// directly rather than through the connector library, which is not in the offline Maven graph the
// builds replay. The protocol is a handful of broadcasts (unifiedpush.org, the Android spec):
//
//   app → distributor  org.unifiedpush.android.distributor.REGISTER    token, application, features, pi
//                      org.unifiedpush.android.distributor.UNREGISTER  token, pi
//   distributor → app  org.unifiedpush.android.connector.NEW_ENDPOINT  token, endpoint
//                      org.unifiedpush.android.connector.REGISTRATION_FAILED  token, reason
//                      org.unifiedpush.android.connector.UNREGISTERED  token
//                      org.unifiedpush.android.connector.MESSAGE       token, bytesMessage, id
//
// The distributor is the person's: ntfy, one of their own, or Google's FCM through a distributor
// app; nothing of Google's is linked. With several installed, the person's default
// (`unifiedpush://link`) is used, else the one chosen before, else the first.
//
// A message is a wake and nothing else: the server says nothing of the rows. It is Web Push
// (RFC 8291), sealed by the server to this device's keys (`webPushKeys`, kept in the module's
// private preferences, out of every backup) and opened here (`webPushWake`, in Rust), which proves
// it the app's server's. Nothing is shown for it: it is told to the process as `Pushed`, whose
// runtime dials, syncs and shows what came from its own rows, and the receiver is held until the
// runtime is done (or the system's time is nearly up).
//
// [ArkPushReceiver] is declared by ui/prebuild.py only for an app whose app.toml says
// `[background] push = true`; it is exported, as a distributor is another app, and takes nothing
// whose token is not this install's.
package dev.arkui.modules

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.Uri
import java.util.UUID
import uniffi.ark_notifications.NotificationEvent
import uniffi.ark_notifications.PushEndpoint
import uniffi.ark_notifications.webPushKeys
import uniffi.ark_notifications.webPushWake

object ArkPush {
    private const val REGISTER = "org.unifiedpush.android.distributor.REGISTER"
    private const val UNREGISTER = "org.unifiedpush.android.distributor.UNREGISTER"
    const val NEW_ENDPOINT = "org.unifiedpush.android.connector.NEW_ENDPOINT"
    const val FAILED = "org.unifiedpush.android.connector.REGISTRATION_FAILED"
    const val UNREGISTERED = "org.unifiedpush.android.connector.UNREGISTERED"
    const val MESSAGE = "org.unifiedpush.android.connector.MESSAGE"
    private const val BYTES = "org.unifiedpush.android.distributor.feature.BYTES_MESSAGE"

    private fun prefs(context: Context) = context.getSharedPreferences(ArkNotices.PREFS, Context.MODE_PRIVATE)

    /** This install's token: what the distributor names it by, and what proves a broadcast is for it. */
    fun token(context: Context): String {
        val p = prefs(context)
        return p.getString("push.token", null) ?: UUID.randomUUID().toString().also { p.edit().putString("push.token", it).apply() }
    }

    /** The distributors installed: the apps that take a REGISTER. */
    private fun distributors(context: Context): List<String> =
        context.packageManager.queryBroadcastReceivers(Intent(REGISTER), 0).map { it.activityInfo.packageName }.distinct()

    /** The distributor to use: the person's default, the one used before, or the first. */
    private fun distributor(context: Context): String? {
        val all = distributors(context)
        val default = context.packageManager.resolveActivity(Intent(Intent.ACTION_VIEW, Uri.parse("unifiedpush://link")), 0)?.activityInfo?.packageName
        val before = prefs(context).getString("push.distributor", null)
        return listOf(default, before).firstOrNull { it != null && it in all } ?: all.firstOrNull()
    }

    /** What proves to the distributor which app is asking: a PendingIntent of this package's. */
    private fun proof(context: Context): PendingIntent =
        PendingIntent.getBroadcast(context, 0, Intent("dev.arkui.push.PROOF").setPackage(context.packageName), PendingIntent.FLAG_IMMUTABLE)

    fun register(context: Context) {
        val d = distributor(context) ?: return ArkNotices.tell(NotificationEvent.Failed("There's no UnifiedPush distributor on this device. Install one, such as ntfy, to be notified."))
        prefs(context).edit().putString("push.distributor", d).apply()
        context.sendBroadcast(
            Intent(REGISTER).setPackage(d)
                .putExtra("token", token(context))
                .putExtra("application", context.packageName)
                .putExtra("features", arrayOf(BYTES))
                .putExtra("pi", proof(context)))
    }

    fun unregister(context: Context) {
        val d = prefs(context).getString("push.distributor", null) ?: return ArkNotices.tell(NotificationEvent.Unregistered)
        context.sendBroadcast(Intent(UNREGISTER).setPackage(d).putExtra("token", token(context)).putExtra("pi", proof(context)))
        prefs(context).edit().remove("push.endpoint").apply()
        ArkNotices.tell(NotificationEvent.Unregistered)
    }

    /** This device's Web Push keys: made once, kept. */
    private fun keys(context: Context): Triple<String, String, String> {
        val p = prefs(context)
        val kept = p.getString("push.keys", null)
        if (kept != null) return Triple(kept, p.getString("push.p256dh", "")!!, p.getString("push.auth", "")!!)
        val k = webPushKeys()
        p.edit().putString("push.keys", k.kept).putString("push.p256dh", k.p256dh).putString("push.auth", k.auth).apply()
        return Triple(k.kept, k.p256dh, k.auth)
    }

    fun endpoint(context: Context, url: String) {
        val (_, p256dh, auth) = keys(context)
        prefs(context).edit().putString("push.endpoint", url).apply()
        ArkNotices.tell(NotificationEvent.Registered(PushEndpoint.UnifiedPush(url, p256dh, auth)))
    }

    /** A message's body: a wake for this app, told; false when it is not one. */
    fun message(context: Context, body: ByteArray): Boolean {
        val (kept, _, _) = keys(context)
        val app = webPushWake(kept, body) ?: return false
        if (!context.packageName.endsWith(".$app")) return false
        ArkNotices.tell(NotificationEvent.Pushed)
        return true
    }
}

class ArkPushReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        // Only what names this install's token.
        if (intent.getStringExtra("token") != ArkPush.token(context)) return
        when (intent.action) {
            ArkPush.NEW_ENDPOINT -> intent.getStringExtra("endpoint")?.let { ArkPush.endpoint(context, it) }
            ArkPush.FAILED -> ArkNotices.tell(NotificationEvent.Failed(intent.getStringExtra("reason") ?: "The distributor refused to register this app."))
            ArkPush.UNREGISTERED -> ArkNotices.tell(NotificationEvent.Failed("The distributor no longer pushes to this app."))
            ArkPush.MESSAGE -> {
                val body = intent.getByteArrayExtra("bytesMessage") ?: intent.getStringExtra("message")?.toByteArray() ?: return
                val pending = goAsync()
                if (!ArkPush.message(context, body)) return pending.finish()
                ArkNotices.settle(ArkNoticeReceiver.BUDGET_MS) { pending.finish() }
            }
        }
    }
}
