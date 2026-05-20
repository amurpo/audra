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
use std::cell::RefCell;
use std::rc::Rc;

type Listener = Box<dyn Fn(Option<&str>) -> bool>;

#[derive(Default)]
pub struct NowPlaying {
    path: RefCell<Option<String>>,
    listeners: RefCell<Vec<Listener>>,
}

impl NowPlaying {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn current(&self) -> Option<String> {
        self.path.borrow().clone()
    }

    /// Publish a new path. Listeners that return `false` are dropped.
    pub fn set(&self, path: Option<String>) {
        // Skip no-op transitions so subscribers don't repaint for nothing.
        if *self.path.borrow() == path {
            return;
        }
        *self.path.borrow_mut() = path.clone();

        // Drain into a local Vec so a listener that publishes again does not
        // deadlock the RefCell.
        let mut listeners = std::mem::take(&mut *self.listeners.borrow_mut());
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
