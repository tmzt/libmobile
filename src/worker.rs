//! **Background work that never blocks the main loop.**
//!
//! The main loop is a non-blocking poll: anything slow (a network exchange, a
//! file decode) runs elsewhere and reports back through an `mpsc` channel the
//! loop drains with `try_recv` once per tick. [`Workers`] is that channel plus
//! the bookkeeping, so an app writes
//!
//! ```
//! # use libmobile::worker::Workers;
//! let mut workers: Workers<String> = Workers::new();
//! workers.spawn("fetch", || "done".to_string());
//! # std::thread::sleep(std::time::Duration::from_millis(50));
//! for result in workers.drain() {
//!     // fold `result` into app state; redraw
//! #   assert_eq!(result, "done");
//! }
//! ```
//!
//! [`Workers::spawn`] uses a plain OS thread. For an async runtime (`smol`,
//! `tokio`, ...) hand the task a [`Workers::sender`] instead — the channel is
//! the whole contract, so libmobile depends on no executor:
//!
//! ```ignore
//! let tx = workers.sender();
//! tokio_handle.spawn(async move { let _ = tx.send(fetch().await); });
//! ```

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;

/// A channel of results from background tasks, drained on the main loop.
pub struct Workers<T> {
    tx: Sender<T>,
    rx: Receiver<T>,
}

impl<T: Send + 'static> Workers<T> {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Workers { tx, rx }
    }

    /// A sender for tasks running on some other executor.
    pub fn sender(&self) -> Sender<T> {
        self.tx.clone()
    }

    /// Run `task` on a new named OS thread; its return value arrives through
    /// [`Self::drain`]. A task that panics delivers nothing.
    pub fn spawn<F>(&self, name: &str, task: F) -> std::io::Result<JoinHandle<()>>
    where
        F: FnOnce() -> T + Send + 'static,
    {
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                // The receiver only goes away with the main loop itself.
                let _ = tx.send(task());
            })
    }

    /// Every result that has arrived since the last call, without waiting.
    pub fn drain(&mut self) -> Vec<T> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(item) => out.push(item),
                // `Disconnected` cannot happen while `self.tx` is alive; treat
                // it like an empty channel regardless.
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }
}

impl<T: Send + 'static> Default for Workers<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_never_blocks() {
        let mut workers: Workers<u8> = Workers::new();
        assert!(workers.drain().is_empty());
    }

    #[test]
    fn spawned_results_arrive() {
        let mut workers = Workers::new();
        workers.spawn("a", || 1).unwrap().join().unwrap();
        workers.spawn("b", || 2).unwrap().join().unwrap();
        assert_eq!(workers.drain(), vec![1, 2]);
    }

    #[test]
    fn foreign_senders_feed_the_same_channel() {
        let mut workers = Workers::new();
        let tx = workers.sender();
        std::thread::spawn(move || tx.send(7).unwrap())
            .join()
            .unwrap();
        assert_eq!(workers.drain(), vec![7]);
    }

    #[test]
    fn panicking_task_delivers_nothing() {
        let mut workers: Workers<u8> = Workers::new();
        let handle = workers.spawn("boom", || panic!("boom")).unwrap();
        assert!(handle.join().is_err());
        assert!(workers.drain().is_empty());
    }
}
