import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// What a transport tells the `Link` about one connection, from whatever
/// thread it lives on. The link queues these and reads them on `pump()`.
public enum LinkEvent {
    /// The connection is open; the engine's `connected()` follows.
    case opened
    /// One binary frame arrived.
    case frame([UInt8])
    /// The connection is gone, with the reason if there was one. A
    /// transport may say it more than once; the link takes the first.
    case closed(String?)
}

/// One connection's worth of socket. A transport is dialled once, opened
/// once and closed once; the `Link` dials a new one for every attempt.
public protocol LinkTransport: AnyObject {
    /// Start connecting. Events go through `events`, from any thread, until
    /// `.closed` has been delivered.
    func open(_ events: @escaping (LinkEvent) -> Void)
    /// Send one binary frame. A failure surfaces as `.closed`.
    func send(_ frame: [UInt8])
    /// Close; `.closed` may or may not follow, and the link does not wait for it.
    func close()
}

/// How a link gets a transport for an address: `WebSocketTransport.dial`
/// for a real server, `MemoryExchange.dial` for one in this process.
public typealias LinkDial = (URL) -> LinkTransport

/// A WebSocket over `URLSessionWebSocketTask`, sending and receiving binary
/// frames. On Linux Foundation only the `async` send/receive are public, so
/// the receive loop and each send run in a `Task`; on Apple platforms the
/// same code is the ordinary way to use the task.
public final class WebSocketTransport: NSObject, LinkTransport, URLSessionWebSocketDelegate {
    public let url: URL
    private var session: URLSession?
    private var task: URLSessionWebSocketTask?
    private var events: ((LinkEvent) -> Void)?
    private let lock = NSLock()
    private var closedSaid = false

    public init(url: URL) {
        self.url = url
        super.init()
    }

    /// The `LinkDial` for a real server.
    public static func dial(_ url: URL) -> LinkTransport { return WebSocketTransport(url: url) }

    public func open(_ events: @escaping (LinkEvent) -> Void) {
        lock.lock()
        self.events = events
        let config = URLSessionConfiguration.default
        config.waitsForConnectivity = false
        let s = URLSession(configuration: config, delegate: self, delegateQueue: nil)
        let t = s.webSocketTask(with: url)
        session = s
        task = t
        lock.unlock()
        t.resume()
        receiveLoop(t)
    }

    private func emit(_ e: LinkEvent) {
        lock.lock()
        let f = events
        if case .closed = e {
            if closedSaid { lock.unlock(); return }
            closedSaid = true
        }
        lock.unlock()
        f?(e)
    }

    private func receiveLoop(_ t: URLSessionWebSocketTask) {
        Task { [weak self] in
            while true {
                do {
                    let m = try await t.receive()
                    switch m {
                    case .data(let d): self?.emit(.frame([UInt8](d)))
                    case .string: break // the protocol is binary; a text frame is not ours
                    @unknown default: break
                    }
                } catch {
                    self?.emit(.closed("\(error)"))
                    return
                }
            }
        }
    }

    public func send(_ frame: [UInt8]) {
        lock.lock()
        let t = task
        lock.unlock()
        guard let t = t else { return }
        Task { [weak self] in
            do {
                try await t.send(.data(Data(frame)))
            } catch {
                self?.emit(.closed("\(error)"))
            }
        }
    }

    /// A ping, so a proxy between here and the server sees traffic while
    /// nothing else is said. Failure surfaces as `.closed`.
    public func ping() {
        lock.lock()
        let t = task
        lock.unlock()
        t?.sendPing { [weak self] err in
            if let e = err { self?.emit(.closed("\(e)")) }
        }
    }

    public func close() {
        lock.lock()
        let t = task
        let s = session
        task = nil
        lock.unlock()
        t?.cancel(with: .goingAway, reason: nil)
        s?.invalidateAndCancel()
        emit(.closed(nil))
    }

    // MARK: URLSessionWebSocketDelegate

    public func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didOpenWithProtocol protocol: String?) {
        emit(.opened)
    }

    public func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didCloseWith closeCode: URLSessionWebSocketTask.CloseCode, reason: Data?) {
        emit(.closed("closed \(closeCode.rawValue)"))
    }

    public func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        emit(.closed(error.map { "\($0)" }))
    }
}
