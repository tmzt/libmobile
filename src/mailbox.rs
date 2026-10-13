//! **The cross-thread handoff every JNI callback uses.**
//!
//! Kotlin calls into Rust on threads `android_main` does not own — the UI
//! thread for lifecycle and inset callbacks, a Credential Manager executor for
//! the login result. None of those may touch the app's state directly, so each
//! bridge pushes into a process-wide static here and the main loop drains it
//! once per tick.
//!
//! Two shapes, because two kinds of thing cross:
//! * [`Mailbox`] — a queue of *commands* (an intent, a login outcome). Every
//!   item matters, in order, exactly once.
//! * [`Latest`] — a current *value* (the safe area). Only the newest report
//!   means anything, so several callbacks between two ticks coalesce into one.
//!
//! Poisoning is ignored (`into_inner`): every critical section is a single
//! push or swap, so a panic elsewhere cannot leave the data half-written.

use std::sync::Mutex;

/// A FIFO of items produced on foreign threads, drained on the main loop.
pub struct Mailbox<T> {
    queue: Mutex<Vec<T>>,
}

impl<T> Mailbox<T> {
    pub const fn new() -> Self {
        Mailbox {
            queue: Mutex::new(Vec::new()),
        }
    }

    pub fn push(&self, item: T) {
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(item);
    }

    /// Everything pushed since the last call, oldest first.
    pub fn take(&self) -> Vec<T> {
        std::mem::take(&mut *self.queue.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A single slot holding the newest value reported since the last drain.
pub struct Latest<T> {
    slot: Mutex<Option<T>>,
}

impl<T> Latest<T> {
    pub const fn new() -> Self {
        Latest {
            slot: Mutex::new(None),
        }
    }

    /// Replace the pending value. Returns the value that was pending, if any.
    pub fn set(&self, value: T) -> Option<T> {
        self.slot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(value)
    }

    /// The newest value since the last call, or `None` if nothing changed.
    pub fn take(&self) -> Option<T> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn mailbox_drains_in_order_once() {
        let mailbox = Mailbox::new();
        mailbox.push(1);
        mailbox.push(2);
        assert_eq!(mailbox.take(), vec![1, 2]);
        assert!(mailbox.take().is_empty());
    }

    #[test]
    fn mailbox_collects_from_other_threads() {
        static BOX: Mailbox<usize> = Mailbox::new();
        let handles: Vec<_> = (0..4)
            .map(|i| std::thread::spawn(move || BOX.push(i)))
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let mut got = BOX.take();
        got.sort();
        assert_eq!(got, vec![0, 1, 2, 3]);
    }

    #[test]
    fn latest_coalesces_to_newest() {
        let latest = Arc::new(Latest::new());
        assert_eq!(latest.set(1), None);
        assert_eq!(latest.set(2), Some(1));
        assert_eq!(latest.take(), Some(2));
        assert_eq!(latest.take(), None);
    }
}
