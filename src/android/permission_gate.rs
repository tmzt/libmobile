//! The permission gate (media plan 4.3, camera plan 3.2): a feature asks for
//! a permission for itself, on the use that needs it, and gets the answer
//! back instead of the app.
//!
//! [`request`] makes one `request_permissions` call and files its waiter in
//! the table; the main loop ([`super::run`]'s `pump_bridges`) offers each
//! answer to [`offer`] before delivering it as `MobileEvent::Permissions`,
//! and an answer to a filed request goes to its waiter only. Waiters run on
//! the main loop's thread, after the table's lock is released, so one may
//! make another request.
//!
//! Used by the in-process camera (`camera`, for CAMERA on the first camera
//! call) and by `media-request` (for CAMERA on a camera entry when the
//! manifest declares it). Core, so neither depends on the other.

#![cfg_attr(
    not(any(feature = "camera", feature = "media-request")),
    allow(dead_code)
)] // Only features use the gate.

use std::sync::{Mutex, MutexGuard};

use android_activity::AndroidApp;

use crate::event::{PermissionRequest, PermissionsOutcome};
use crate::permission_gate::Table;

/// Called on the main loop with the answer to its request.
pub(crate) type Waiter = Box<dyn FnOnce(&AndroidApp, PermissionsOutcome) + Send>;

static TABLE: Mutex<Table<Waiter>> = Mutex::new(Table::new());

fn table() -> MutexGuard<'static, Table<Waiter>> {
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ask for `permissions` for a feature's own use: the answer goes to
/// `waiter`, not to the app. Any thread.
pub(crate) fn request(app: &AndroidApp, permissions: &[&str], waiter: Waiter) -> PermissionRequest {
    // Held across the request, so an answer the main loop drains at once (a
    // failed JNI call answers immediately) still finds its waiter.
    let mut table = table();
    let request = super::request_permissions(app, permissions);
    table.insert(request, waiter);
    request
}

/// Forget `request`: its answer, if one comes, goes to the app.
pub(crate) fn withdraw(request: PermissionRequest) {
    let _ = table().withdraw(request);
}

/// Offer an answer to the gate: `None` if it was a feature's (consumed and
/// handed to its waiter), else the outcome back for
/// `MobileEvent::Permissions`.
pub(crate) fn offer(app: &AndroidApp, outcome: PermissionsOutcome) -> Option<PermissionsOutcome> {
    let found = table().offer(outcome);
    match found {
        Ok((waiter, outcome)) => {
            waiter(app, outcome);
            None
        }
        Err(outcome) => Some(outcome),
    }
}

/// The run ended: `PermissionPlugin` drops its queue with the activity, so
/// no filed request will be answered. The waiters are dropped unanswered.
pub(crate) fn clear() {
    let dropped = table().clear();
    if !dropped.is_empty() {
        log::info!(
            "libmobile: permission gate: {} request(s) left unanswered at the end of the run",
            dropped.len()
        );
    }
}
