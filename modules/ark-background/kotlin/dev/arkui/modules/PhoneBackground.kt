// ark-background on Android (README.md): the `NativeBackground` interface UniFFI writes, over a
// periodic `JobScheduler` job ([ArkSyncJob]) — the platform's own scheduler, which WorkManager
// itself runs on: WorkManager is not in the offline Maven graph the builds replay. Persisted, so
// it outlives a restart (the RECEIVE_BOOT_COMPLETED it needs, and the job service, are written by
// ui/prebuild.py only for an app whose app.toml says `[background] sync`).
//
// Off until the app asks (`set`), and the app asks only once the person turned it on; what was
// asked is kept in the module's private preferences. Each run is a wake of the process's one
// runtime (`ArkProcess.wake(Sync, …)`), the job finished when the runtime is idle or its time is
// nearly up.
package dev.arkui.modules

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import uniffi.ark_background.NativeBackground
import uniffi.ark_background.WantedReply
import dev.arkdb.ffi.ArkProcess
import dev.arkdb.ffi.WakeDone
import dev.arkdb.ffi.WakeReason

private const val PREFS = "ark-background"
private const val EVERY = "every"
private const val JOB = 0x41524b // "ARK"

class PhoneBackground(context: Context) : NativeBackground {
    private val app = context.applicationContext

    override fun set(everyS: UInt?) {
        app.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putLong(EVERY, everyS?.toLong() ?: 0L).apply()
        val jobs = app.getSystemService(JobScheduler::class.java)
        if (everyS == null) return jobs.cancel(JOB)
        val ms = maxOf(everyS.toLong(), 15 * 60L) * 1000
        jobs.schedule(
            JobInfo.Builder(JOB, ComponentName(app, ArkSyncJob::class.java))
                .setPeriodic(ms)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setPersisted(true)
                .build())
    }

    override fun wanted(reply: WantedReply) {
        val every = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getLong(EVERY, 0L)
        reply.wanted(if (every > 0) every.toUInt() else null)
    }
}

/** A run of the periodic sync: a wake of the process's runtime, finished when it is idle. */
class ArkSyncJob : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        ArkProcess.shared().wake(WakeReason.Sync, BUDGET_MS, object : WakeDone {
            override fun finished(idle: Boolean) = jobFinished(params, !idle)
        })
        return true
    }

    // The system took the time back: the runtime goes on until it is idle, the job is asked again.
    override fun onStopJob(params: JobParameters): Boolean = true

    companion object {
        /** A job has ten minutes; a sync needs a fraction of them. */
        const val BUDGET_MS = 60_000uL
    }
}

/** The device started: a wake of the runtime (app.toml's `[background] boot`). */
class ArkBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        ArkNotices.rearm(context)
        val pending = goAsync()
        ArkProcess.shared().wake(WakeReason.Boot, 8_000uL, object : WakeDone {
            override fun finished(idle: Boolean) = pending.finish()
        })
    }
}
