// Whether an address is an Ark server serving this app, from what its
// `/healthz` answers.
//
// The server answers JSON when asked for it (`rust/ark-server`'s
// `health_json`): `{"status":"ok", …, "module": "<hex>" | null,
// "modules": ["<hex>", …], …}` — the module it runs now, by the hash every
// client computes of the module it loads (`Domain.hash`), and every module
// it has run. The decision is a function of the answer and nothing else, so
// it is held by plain JVM tests; [Probe] makes the requests.
package dev.arkdb.android

import dev.arkdb.Hex
import dev.arkdb.Json
import dev.arkdb.Value

/** What one `/healthz` came to. */
public sealed class Health {
    /** The server runs this app's module — or ran it, and runs a newer one now. */
    public data class Serving(val newer: Boolean) : Health()

    /** It answered, and it is not what this app can use: the sentence says which way. */
    public data class Refused(val why: String) : Health()

    public companion object {
        /**
         * The decision for an answer from `host`: `status` and `body` as they came, `module` the
         * hash of the module this app loads, `app` its name for a sentence.
         */
        public fun decide(
            host: String,
            status: Int,
            body: String,
            module: ByteArray,
            app: String
        ): Health {
            if (status == 404)
                return Refused("$host answered, but it isn't an Ark server: it has no health page.")
            if (status != 200)
                return Refused(
                    "$host answered $status. It may be starting up or broken; try again in a moment.")
            val v =
                try {
                    Json.decode(body)
                } catch (e: Exception) {
                    null
                }
            val s = (v as? Value.VStruct)
            if (s == null ||
                s["status"].let { it.isNull() || it !is Value.VText || it.asText() != "ok" })
                return Refused("$host answered, but not the way an Ark server does.")
            val ours = Hex.encode(module)
            val now = s["module"].let { if (it is Value.VText) it.asText().lowercase() else null }
            val ran =
                (s["modules"] as? Value.VList)?.items.orEmpty().mapNotNull {
                    (it as? Value.VText)?.value?.lowercase()
                }
            return when {
                now == ours -> Serving(newer = false)
                ours in ran -> Serving(newer = true)
                now == null -> Refused("$host is an Ark server with no app on it yet.")
                else ->
                    Refused(
                        "$host is an Ark server, but it serves an app this $app doesn't know — another app, or a newer $app than this one.")
            }
        }
    }
}
