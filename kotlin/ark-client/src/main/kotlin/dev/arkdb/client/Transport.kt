// A transport: one way of opening a connection that carries frames. The
// engine is sans-io, so this is the only place a socket appears — and the
// in-memory one, which exchanges frames with an in-process `LocalHub`, is
// what the tests run the same `Link` over.
package dev.arkdb.client

/** One connection, as the link sees it: frames go in, and it can be closed. */
public interface Connection {
    /** Queue a binary frame; false if the connection is already gone. */
    public fun send(frame: ByteArray): Boolean

    public fun close()
}

/** What a connection tells the link. Calls may arrive on any thread; the link queues them. */
public interface ConnectionListener {
    public fun onOpen()

    public fun onFrame(frame: ByteArray)

    /** The connection is over, whatever the reason; the link decides whether to try again. */
    public fun onClose(reason: String)
}

public interface Transport {
    /** Begin one connection attempt. Exactly one of `onOpen` or `onClose` follows, then frames, then `onClose`. */
    public fun open(listener: ConnectionListener): Connection
}
