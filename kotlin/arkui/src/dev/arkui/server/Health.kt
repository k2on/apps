// Whether an address is a harken server this phone can use, from what its
// `/healthz` answers.
//
// The server answers a line each (`harken-server`'s `health`): `ok`, its
// connections, the log's head and horizon, and `domain <schema hash>
// <version> current` — the domain it runs, as this library names its own
// (`harken_domain`). The decision is a function of the answer and nothing
// else, so it is held by plain JVM tests; [Probe] makes the requests.
package dev.harken.android.onboarding

/** What one `/healthz` came to. */
sealed class Health {
    /** The server runs this app's domain — or another schema of it, `newer` — and can be used. */
    data class Serving(val newer: Boolean) : Health()

    /** It answered, and it is not what this app can use: the sentence says which way. */
    data class Refused(val why: String) : Health()

    companion object {
        /**
         * The decision for an answer from `host`: `status` and `body` as they came, `domain` this
         * app's own line (`<schema hash> <version>`), `app` its name for a sentence.
         */
        fun decide(host: String, status: Int, body: String, domain: String, app: String): Health {
            if (status == 404) return Refused("$host answered, but it isn't an Ark server: it has no health page.")
            if (status != 200) return Refused("$host answered $status. It may be starting up or broken; try again in a moment.")
            val lines = body.lines().map { it.trim() }
            if (lines.firstOrNull() != "ok") return Refused("$host answered, but not the way an Ark server does.")
            val runs = lines.firstOrNull { it.startsWith("domain ") }?.removePrefix("domain ")?.removeSuffix(" current")
            val ours = domain.substringBefore(' ')
            return when {
                runs == null -> Refused("$host is an Ark server with no app on it yet.")
                runs.substringBefore(' ') == ours -> Serving(newer = false)
                else -> Serving(newer = true)
            }
        }
    }
}
