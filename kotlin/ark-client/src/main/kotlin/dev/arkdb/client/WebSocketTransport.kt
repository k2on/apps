// The WebSocket transport, over OkHttp: binary frames each way, nothing
// else. OkHttp delivers its callbacks on its own threads; `Link` queues
// them and touches the client machine only from `pump()`.
package dev.arkdb.client

import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString
import java.util.concurrent.TimeUnit

public class WebSocketTransport(
    /** `ws://host:port/sync` or `wss://…`. */
    public val url: String,
    private val http: OkHttpClient = defaultClient(),
) : Transport {
    override fun open(listener: ConnectionListener): Connection {
        val request = Request.Builder().url(url).build()
        val ws = http.newWebSocket(
            request,
            object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) = listener.onOpen()

                override fun onMessage(webSocket: WebSocket, bytes: ByteString) = listener.onFrame(bytes.toByteArray())

                // The protocol is binary; a text frame is not ours and is dropped.
                override fun onMessage(webSocket: WebSocket, text: String) = Unit

                override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                    webSocket.close(1000, null)
                }

                override fun onClosed(webSocket: WebSocket, code: Int, reason: String) = listener.onClose("closed $code $reason")

                override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) =
                    listener.onClose("failed: ${t.message ?: t.javaClass.simpleName}")
            },
        )
        return object : Connection {
            override fun send(frame: ByteArray): Boolean = ws.send(frame.toByteString())

            override fun close() {
                ws.close(1000, null)
            }
        }
    }

    public companion object {
        /** A client that pings, because the engine has no clock to hang a keepalive on. */
        public fun defaultClient(): OkHttpClient = OkHttpClient.Builder()
            .pingInterval(20, TimeUnit.SECONDS)
            .readTimeout(0, TimeUnit.MILLISECONDS)
            .build()
    }
}
