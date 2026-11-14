// ark-notifications on Android (modules/ark-notifications/README.md): the `NativeNotifications`
// interface UniFFI writes, over
//
// - `NotificationManager`: a notice shown now, its id the notification's tag so a second of one
//   id replaces the first; a channel per kind (`Notice.channel`, in the person's words, listed in
//   the app's settings); its buttons each a broadcast to [ArkNoticeReceiver], so pressing one
//   never opens the app;
// - `AlarmManager` for a later time (inexact and allowed while idle: no exact-alarm permission),
//   the notice kept in the module's private preferences until it is shown (set again after a
//   restart only in an app that declares `[background]`: ark-background's boot receiver);
// - UnifiedPush ([ArkPush], ArkPush.kt), the receiver protocol implemented directly.
//
// A tap opens the notice's route through the app's links: a VIEW of `<scheme>://<path>` at the
// app's own activity, exactly as a link from anywhere else, so the app resolves the route; the
// scheme is the manifest's `dev.arkui.scheme` (ui/prebuild.py). The activity hands the intent to
// [ArkNotices.opened] to have the tap said. A button and a push are each told to the process's
// listener; the process's runtime makes a wake-up of them and says when it is done ([ArkNotices.settle]).
//
// The permission (Android 13 and later) is asked by [NotificationsPermission], put once at the
// app's root, as the camera's page is.
package dev.arkui.modules

import android.Manifest
import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.json.JSONArray
import org.json.JSONObject
import dev.arkdb.ffi.ArkProcess
import dev.arkdb.ffi.WakeDone
import uniffi.ark_notifications.AuthorizeReply
import uniffi.ark_notifications.NativeNotifications
import uniffi.ark_notifications.Notice
import uniffi.ark_notifications.NoticeAction
import uniffi.ark_notifications.NotificationEvent
import uniffi.ark_notifications.NotificationListener

/** What the process's notifications share: the listener, what waits for one, the runtime's settle. */
object ArkNotices {
    /** The module's private preferences: the notices still to come, the push keys and endpoint. */
    const val PREFS = "ark-notifications"
    const val EXTRA_NOTICE = "dev.arkui.notice"
    const val EXTRA_ACTION = "dev.arkui.notice.action"
    const val SHOW = "dev.arkui.notice.SHOW"
    const val PRESS = "dev.arkui.notice.PRESS"

    @Volatile private var listener: NotificationListener? = null
    private val early = mutableListOf<NotificationEvent>()

    /**
     * How the process's runtime says it is done with a wake-up: called with the most it may take,
     * in milliseconds, and what to do then — the process's own (`ArkProcess.settle`), unless a test
     * sets another.
     */
    @Volatile var settle: (budgetMs: Long, done: () -> Unit) -> Unit = { budget, done ->
        ArkProcess.shared().settle(budget.toULong(), object : WakeDone {
            override fun finished(idle: Boolean) = done()
        })
    }

    fun listen(l: NotificationListener?) {
        val told = synchronized(early) {
            listener = l
            if (l == null) emptyList() else early.toList().also { early.clear() }
        }
        told.forEach { l?.heard(it) }
    }

    /**
     * `e` told to the listener the app's runtime set; with none, to the process itself
     * (`ArkProcess.notices`: a push or a button a wake, an endpoint the session's push row), and a
     * tap kept until there is one.
     */
    fun tell(e: NotificationEvent) {
        val l = synchronized(early) { listener ?: run { if (e is NotificationEvent.Opened) early.add(e); null } }
        if (l != null) return l.heard(e)
        if (e !is NotificationEvent.Opened) ArkProcess.shared().notices().heard(e)
    }

    /** An intent the app's activity was started or resumed with: a tap on a notice, said. */
    fun opened(intent: Intent?) {
        val id = intent?.getStringExtra(EXTRA_NOTICE) ?: return
        intent.removeExtra(EXTRA_NOTICE)
        tell(NotificationEvent.Opened(id, intent.data?.let { "/" + (it.host.orEmpty() + it.path.orEmpty()).trimStart('/') }))
    }

    // * Showing

    fun manager(context: Context) = context.getSystemService(NotificationManager::class.java)

    /** The channel of `name` (none: the app's general one), made the first time. */
    fun channel(context: Context, name: String?): String {
        val id = "ark." + (name ?: "general").lowercase().replace(Regex("[^a-z0-9]+"), "-")
        val m = manager(context)
        if (m.getNotificationChannel(id) == null) {
            val label = name ?: context.applicationInfo.loadLabel(context.packageManager).toString()
            m.createNotificationChannel(NotificationChannel(id, label, NotificationManager.IMPORTANCE_DEFAULT))
        }
        return id
    }

    /** The app's scheme, as the manifest says it (`dev.arkui.scheme`). */
    private fun scheme(context: Context): String? {
        val info = context.packageManager.getApplicationInfo(context.packageName, PackageManager.GET_META_DATA)
        return info.metaData?.getString("dev.arkui.scheme")
    }

    /** `path` as a link to the app's own activity: `<scheme>://<path>`, or the launcher's with none. */
    private fun link(context: Context, path: String?, id: String): Intent? {
        val scheme = scheme(context)
        val intent = when {
            path != null && scheme != null -> Intent(Intent.ACTION_VIEW, Uri.parse("$scheme://${path.trimStart('/')}")).setPackage(context.packageName)
            else -> context.packageManager.getLaunchIntentForPackage(context.packageName)
        }
        return intent?.putExtra(EXTRA_NOTICE, id)?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    }

    /** A request code of its own for each notice and button, so their intents are not one. */
    private fun code(vararg parts: String) = parts.joinToString("\u0000").hashCode()

    /** `n` shown now. Nothing when the person said no. */
    fun show(context: Context, n: Notice) {
        if (!allowed(context)) return
        val icon = context.resources.getIdentifier("mark", "drawable", context.packageName).takeIf { it != 0 } ?: context.applicationInfo.icon
        val b = NotificationCompat.Builder(context, channel(context, n.channel))
            .setSmallIcon(icon)
            .setContentTitle(n.title)
            .setContentText(n.body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(n.body))
            .setAutoCancel(true)
        link(context, n.opens, n.id)?.let {
            b.setContentIntent(PendingIntent.getActivity(context, code(n.id), it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        }
        for (a in n.actions.take(3)) {
            val press = Intent(context, ArkNoticeReceiver::class.java).setAction(PRESS).putExtra(EXTRA_NOTICE, n.id).putExtra(EXTRA_ACTION, a.id)
            b.addAction(0, a.title, PendingIntent.getBroadcast(context, code(n.id, a.id), press, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        }
        manager(context).notify(n.id, 0, b.build())
    }

    fun allowed(context: Context): Boolean =
        NotificationManagerCompat.from(context).areNotificationsEnabled() &&
            (Build.VERSION.SDK_INT < 33 || ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED)

    // * Later

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun alarm(context: Context, id: String): PendingIntent {
        val i = Intent(context, ArkNoticeReceiver::class.java).setAction(SHOW).putExtra(EXTRA_NOTICE, id)
        return PendingIntent.getBroadcast(context, code("show", id), i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    /** `n` shown at its time: kept, and its alarm set. */
    fun later(context: Context, n: Notice) {
        prefs(context).edit().putString("notice.${n.id}", json(n).toString()).apply()
        arm(context, n)
    }

    private fun arm(context: Context, n: Notice) {
        val at = n.atMs ?: return
        context.getSystemService(AlarmManager::class.java).setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, alarm(context, n.id))
    }

    /** The notice of `id` taken back: its alarm, what is kept of it, and what is shown. */
    fun cancel(context: Context, id: String) {
        context.getSystemService(AlarmManager::class.java).cancel(alarm(context, id))
        prefs(context).edit().remove("notice.$id").apply()
        manager(context).cancel(id, 0)
    }

    /** The notice of `id` kept for later, its time come: shown, and no longer kept. */
    fun due(context: Context, id: String) {
        val n = prefs(context).getString("notice.$id", null)?.let(::notice) ?: return
        prefs(context).edit().remove("notice.$id").apply()
        show(context, n)
    }

    /** After a restart: each notice still to come set again, and one whose time passed shown. */
    fun rearm(context: Context) {
        val now = System.currentTimeMillis()
        for ((k, v) in prefs(context).all) {
            if (!k.startsWith("notice.")) continue
            val n = (v as? String)?.let(::notice) ?: continue
            if ((n.atMs ?: 0) <= now) due(context, n.id) else arm(context, n)
        }
    }

    private fun json(n: Notice): JSONObject = JSONObject()
        .put("id", n.id).put("title", n.title).put("body", n.body)
        .put("at", n.atMs ?: JSONObject.NULL).put("opens", n.opens ?: JSONObject.NULL).put("channel", n.channel ?: JSONObject.NULL)
        .put("actions", JSONArray(n.actions.map { JSONObject().put("id", it.id).put("title", it.title) }))

    private fun notice(text: String): Notice? = runCatching {
        val o = JSONObject(text)
        val a = o.getJSONArray("actions")
        Notice(
            id = o.getString("id"),
            title = o.getString("title"),
            body = o.getString("body"),
            atMs = if (o.isNull("at")) null else o.getLong("at"),
            opens = if (o.isNull("opens")) null else o.getString("opens"),
            actions = (0 until a.length()).map { NoticeAction(a.getJSONObject(it).getString("id"), a.getJSONObject(it).getString("title")) },
            channel = if (o.isNull("channel")) null else o.getString("channel"),
        )
    }.getOrNull()

    // * Asking

    private val _asking = MutableStateFlow<AuthorizeReply?>(null)
    /** The permission being asked for, while it is: [NotificationsPermission] asks it. */
    val asking: StateFlow<AuthorizeReply?> = _asking

    fun ask(context: Context, reply: AuthorizeReply) {
        if (Build.VERSION.SDK_INT < 33 || allowed(context)) return reply.allowed(allowed(context))
        _asking.value?.allowed(false)
        _asking.value = reply
    }

    fun answered(allowed: Boolean) {
        _asking.value?.allowed(allowed)
        _asking.value = null
    }
}

/** The phone's notifications, for the process: one, made with the application's context. */
class PhoneNotifications(context: Context) : NativeNotifications {
    private val app = context.applicationContext

    override fun schedule(notice: Notice) {
        val later = (notice.atMs ?: 0) > System.currentTimeMillis() + 1_000
        if (later) ArkNotices.later(app, notice) else ArkNotices.show(app, notice)
    }

    override fun cancel(id: String) = ArkNotices.cancel(app, id)

    override fun authorize(reply: AuthorizeReply) = ArkNotices.ask(app, reply)

    override fun registerPush() = ArkPush.register(app)

    override fun unregisterPush() = ArkPush.unregister(app)

    override fun setListener(listener: NotificationListener?) = ArkNotices.listen(listener)
}

/** The system's permission dialog, while the notifications ask for it: put once at the app's root. */
@Composable
fun NotificationsPermission() {
    val asking by ArkNotices.asking.collectAsState()
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok -> ArkNotices.answered(ok) }
    LaunchedEffect(asking) {
        if (asking != null && Build.VERSION.SDK_INT >= 33) ask.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
}
