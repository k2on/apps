// ark-notifications' receivers on Android (README.md), declared by its module.toml:
//
// - [ArkNoticeReceiver], reached by its name alone: a notice's alarm going off (shown), and a
//   button pressed on one (told to the process, which wakes its runtime for it; the receiver is
//   held until the runtime is done or the system's time is nearly up).
// - [ArkNoticeBoot], the system's BOOT_COMPLETED: each notice still to come set again, since a
//   restart clears every alarm.
package dev.arkui.modules

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import uniffi.ark_notifications.NotificationEvent

class ArkNoticeReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra(ArkNotices.EXTRA_NOTICE) ?: return
        when (intent.action) {
            ArkNotices.SHOW -> ArkNotices.due(context, id)
            ArkNotices.PRESS -> {
                val action = intent.getStringExtra(ArkNotices.EXTRA_ACTION) ?: return
                // Pressed: the notice has done its work.
                ArkNotices.manager(context).cancel(id, 0)
                val pending = goAsync()
                ArkNotices.tell(NotificationEvent.Action(id, action))
                // A receiver has ten seconds; the runtime is given most of them.
                ArkNotices.settle(BUDGET_MS) { pending.finish() }
            }
        }
    }

    companion object {
        const val BUDGET_MS = 8_000L
    }
}

class ArkNoticeBoot : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) ArkNotices.rearm(context)
    }
}
