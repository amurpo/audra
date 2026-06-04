//! Tiny pub/sub bus for the currently playing track path.
//!
//! Replaces the `Rc<RefCell<Option<String>>>` that views used to share, and the
//! 300 ms polling timers that detail pages used to spin to react to changes.
//! Every track list subscribes once at construction and is woken up only when
//! the path actually changes.
//!
//! Listeners return `bool`: `true` to stay subscribed, `false` to be dropped.
//! That keeps the API lock-free of `Weak` upgrades on the caller side — each
//! listener decides on its own whether the widget it cares about is still alive.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Listener = Box<dyn Fn(Option<&str>) -> bool>;

#[derive(Default)]
pub struct NowPlaying {
    path: RefCell<Option<String>>,
    /// Whether the current track is actively playing (vs paused). Track rows
    /// read this to pick the ⏸ / ▶ icon on the active row; updated through
    /// [`set_playing`](Self::set_playing) from the one place that flips the
    /// player bar's icon, so every list stays in sync without polling.
    playing: Cell<bool>,
    listeners: RefCell<Vec<Listener>>,
    /// Play/pause toggle requested from a list row's icon. Wired once by
    /// `main_window` to re-emit the player bar's play/pause button, so the row
    /// icon drives the exact same code path as the transport control and MPRIS
    /// — no duplicated player logic in the list layer.
    toggle_handler: RefCell<Option<Box<dyn Fn()>>>,
}

impl NowPlaying {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn current(&self) -> Option<String> {
        self.path.borrow().clone()
    }

    /// Whether the current track is playing (vs paused / stopped).
    pub fn is_playing(&self) -> bool {
        self.playing.get()
    }

    /// Publish a new path. Listeners that return `false` are dropped.
    pub fn set(&self, path: Option<String>) {
        // Skip no-op transitions so subscribers don't repaint for nothing.
        if *self.path.borrow() == path {
            return;
        }
        *self.path.borrow_mut() = path;
        self.notify();
    }

    /// Publish the play/paused state. Repaints subscribers so the active row's
    /// icon flips between ⏸ and ▶ in lockstep with the player bar.
    pub fn set_playing(&self, playing: bool) {
        if self.playing.get() == playing {
            return;
        }
        self.playing.set(playing);
        self.notify();
    }

    /// Install the play/pause toggle handler. Called once with a closure that
    /// re-emits the player bar's play/pause button.
    pub fn set_toggle_handler(&self, handler: impl Fn() + 'static) {
        *self.toggle_handler.borrow_mut() = Some(Box::new(handler));
    }

    /// Request a play/pause toggle (invoked when the active row's icon is
    /// clicked). No-op until a handler is installed.
    pub fn request_toggle(&self) {
        if let Some(h) = self.toggle_handler.borrow().as_ref() {
            h();
        }
    }

    /// Notify all listeners with the current path. Listeners returning `false`
    /// are dropped. Drains into a local Vec first so a listener that publishes
    /// again does not deadlock the RefCell.
    fn notify(&self) {
        let mut listeners = std::mem::take(&mut *self.listeners.borrow_mut());
        let path = self.path.borrow().clone();
        let s = path.as_deref();
        listeners.retain(|l| l(s));
        self.listeners.borrow_mut().extend(listeners);
    }

    /// Subscribe a listener. The listener is invoked once with the current value
    /// so the new subscriber starts already in sync.
    pub fn subscribe(&self, listener: impl Fn(Option<&str>) -> bool + 'static) {
        let cur = self.current();
        listener(cur.as_deref());
        self.listeners.borrow_mut().push(Box::new(listener));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn subscribe_receives_current_value_immediately() {
        let np = NowPlaying::new();
        np.set(Some("a".into()));

        let got: Rc<Cell<Option<String>>> = Rc::new(Cell::new(None));
        let g = Rc::clone(&got);
        np.subscribe(move |p| {
            g.set(p.map(String::from));
            true
        });

        assert_eq!(got.take().as_deref(), Some("a"));
    }

    #[test]
    fn set_notifies_listeners_and_skips_noop() {
        let np = NowPlaying::new();
        let count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        let c = Rc::clone(&count);
        np.subscribe(move |_| {
            c.set(c.get() + 1);
            true
        });
        assert_eq!(count.get(), 1, "subscribe fires once with current");

        np.set(Some("a".into()));
        np.set(Some("a".into())); // noop
        np.set(Some("b".into()));
        np.set(None);

        assert_eq!(count.get(), 4);
    }

    #[test]
    fn listeners_returning_false_are_dropped() {
        let np = NowPlaying::new();
        let count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        let c = Rc::clone(&count);
        np.subscribe(move |_| {
            c.set(c.get() + 1);
            false // unsubscribe after first delivery
        });
        np.set(Some("a".into()));
        np.set(Some("b".into()));
        // subscribe call (1) + the false-returning delivery (2). After that the
        // listener is gone and further sets do not increment.
        assert_eq!(count.get(), 2);
    }
}
