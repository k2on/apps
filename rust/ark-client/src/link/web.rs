//! The browser's `WebSocket`. Its callbacks push onto a queue the link
//! drains and then call the host's [`Wake`] — `onopen`, `onmessage` and
//! `onclose` are the only things that make a page's peer run
//! (`docs/plan-reactive.md`); the page's own event loop is the thread.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{js_sys, MessageEvent, WebSocket};

use super::{BoxTransport, Event, Transport, Wake};

/// Where the callbacks find the host's [`Wake`].
type Slot = Rc<RefCell<Option<Wake>>>;

/// Queue `e`, then wake the host — outside the borrow, since a host may
/// read the queue from inside its wake.
fn report(events: &RefCell<Vec<Event>>, slot: &Slot, e: Option<Event>) {
    if let Some(e) = e {
        events.borrow_mut().push(e);
    }
    let w = slot.borrow().clone();
    if let Some(w) = w {
        w();
    }
}

struct Web {
    socket: Option<WebSocket>,
    events: Rc<RefCell<Vec<Event>>>,
    waker: Slot,
    _on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    _on_open: Option<Closure<dyn FnMut(JsValue)>>,
    _on_close: Option<Closure<dyn FnMut(JsValue)>>,
}

impl Transport for Web {
    fn send(&mut self, frame: Vec<u8>) {
        if let Some(s) = &self.socket {
            let _ = s.send_with_u8_array(&frame);
        }
    }

    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut *self.events.borrow_mut())
    }

    fn close(&mut self) {
        if let Some(s) = self.socket.take() {
            s.set_onmessage(None);
            s.set_onopen(None);
            s.set_onclose(None);
            s.set_onerror(None);
            let _ = s.close();
        }
    }

    fn set_waker(&mut self, wake: Wake) {
        let waiting = !self.events.borrow().is_empty();
        *self.waker.borrow_mut() = Some(wake.clone());
        if waiting {
            wake();
        }
    }
}

impl Drop for Web {
    fn drop(&mut self) {
        self.close();
    }
}

/// Open a `WebSocket` to `url`; it reports `Opened`, frames, and a
/// `Closed` on close or error. A browser cannot ping; it answers the
/// server's, which is the keepalive.
pub fn dial(url: &str) -> BoxTransport {
    let events: Rc<RefCell<Vec<Event>>> = Rc::default();
    let waker: Slot = Rc::default();
    let socket = match WebSocket::new(url) {
        Ok(s) => s,
        Err(e) => {
            events.borrow_mut().push(Event::Closed(format!("{e:?}")));
            return Box::new(Web {
                socket: None,
                events,
                waker,
                _on_message: None,
                _on_open: None,
                _on_close: None,
            });
        }
    };
    socket.set_binary_type(web_sys::BinaryType::Arraybuffer);
    let on_message = {
        let (events, slot) = (events.clone(), waker.clone());
        Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                report(&events, &slot, Some(Event::Frame(js_sys::Uint8Array::new(&buf).to_vec())));
            }
        })
    };
    let on_open = {
        let (events, slot) = (events.clone(), waker.clone());
        Closure::<dyn FnMut(JsValue)>::new(move |_| report(&events, &slot, Some(Event::Opened)))
    };
    let on_close = {
        let (events, slot) = (events.clone(), waker.clone());
        Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
            let why = e
                .dyn_ref::<web_sys::CloseEvent>()
                .map(|c| format!("closed ({}) {}", c.code(), c.reason()))
                .unwrap_or_else(|| "the socket failed".into());
            // An error is followed by a close; one `Closed` is the contract.
            let first = !matches!(events.borrow().last(), Some(Event::Closed(_)));
            report(&events, &slot, first.then_some(Event::Closed(why)));
        })
    };
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    socket.set_onerror(Some(on_close.as_ref().unchecked_ref()));
    Box::new(Web {
        socket: Some(socket),
        events,
        waker,
        _on_message: Some(on_message),
        _on_open: Some(on_open),
        _on_close: Some(on_close),
    })
}
