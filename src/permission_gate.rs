//! The permission gate's table (media plan 4.3; the camera plan's 3.2 gate,
//! moved here from the camera so features can share it without depending
//! on each other): which `request_permissions` calls a feature made for
//! itself, and who waits for each answer.
//!
//! The main loop offers every `PermissionsOutcome` to the table before
//! delivering it as `MobileEvent::Permissions`; an answer to a request in the
//! table goes to its waiter only, and the app never sees it. The Android
//! side (`android::permission_gate`) holds one table of boxed waiters.
//!
//! Platform-free, so the routing is host-tested.

#![cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.

use crate::event::{PermissionRequest, PermissionsOutcome};

/// The requests in flight and their waiters, oldest first.
#[derive(Debug)]
pub(crate) struct Table<W> {
    entries: Vec<(PermissionRequest, W)>,
}

impl<W> Table<W> {
    pub(crate) const fn new() -> Self {
        Table {
            entries: Vec::new(),
        }
    }

    /// `request` was made for `waiter`.
    pub(crate) fn insert(&mut self, request: PermissionRequest, waiter: W) {
        self.entries.push((request, waiter));
    }

    /// An answer: `Ok` with its waiter (the answer is consumed), or the
    /// outcome back when the request is not in the table.
    pub(crate) fn offer(
        &mut self,
        outcome: PermissionsOutcome,
    ) -> Result<(W, PermissionsOutcome), PermissionsOutcome> {
        match self.entries.iter().position(|(r, _)| *r == outcome.request) {
            Some(i) => Ok((self.entries.remove(i).1, outcome)),
            None => Err(outcome),
        }
    }

    /// Forget `request` (its caller gave up); a later answer goes to the app.
    pub(crate) fn withdraw(&mut self, request: PermissionRequest) -> Option<W> {
        let i = self.entries.iter().position(|(r, _)| *r == request)?;
        Some(self.entries.remove(i).1)
    }

    /// Forget every request (the run ended), giving their waiters back.
    pub(crate) fn clear(&mut self) -> Vec<W> {
        self.entries.drain(..).map(|(_, w)| w).collect()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "media-request")]
    use crate::media::{MediaError, camera_gate};

    const CAMERA: &str = "android.permission.CAMERA";

    fn outcome(id: u32, granted: bool) -> PermissionsOutcome {
        PermissionsOutcome::from_result(
            PermissionRequest(id),
            vec![CAMERA.into()],
            vec![CAMERA.into()],
            &[granted],
        )
    }

    /// A media request waiting for CAMERA, as `android::media` registers it.
    #[derive(Debug, PartialEq)]
    struct MediaWaiter(u32);

    #[cfg(feature = "media-request")]
    /// What the main loop does with an outcome: the waiter's verdict, or
    /// `MobileEvent::Permissions` for the app. `rationale` stands for
    /// `should_show_rationale(CAMERA)` after the answer.
    fn pump(
        table: &mut Table<MediaWaiter>,
        outcome: PermissionsOutcome,
        rationale: bool,
    ) -> Result<(u32, Result<(), MediaError>), PermissionsOutcome> {
        let (waiter, outcome) = table.offer(outcome)?;
        Ok((waiter.0, camera_gate(outcome.granted(CAMERA), rationale)))
    }

    #[cfg(feature = "media-request")]
    #[test]
    fn media_waiter_granted() {
        let mut table = Table::new();
        table.insert(PermissionRequest(3), MediaWaiter(1));
        assert_eq!(pump(&mut table, outcome(3, true), false), Ok((1, Ok(()))));
        assert_eq!(table.len(), 0);
    }

    #[cfg(feature = "media-request")]
    #[test]
    fn media_waiter_denied_with_and_without_settings_only() {
        let mut table = Table::new();
        table.insert(PermissionRequest(4), MediaWaiter(2));
        table.insert(PermissionRequest(5), MediaWaiter(3));
        assert_eq!(
            pump(&mut table, outcome(4, false), true),
            Ok((
                2,
                Err(MediaError::PermissionDenied {
                    settings_only: false
                })
            ))
        );
        assert_eq!(
            pump(&mut table, outcome(5, false), false),
            Ok((
                3,
                Err(MediaError::PermissionDenied {
                    settings_only: true
                })
            ))
        );
    }

    #[test]
    fn gate_answers_are_not_the_apps_and_others_are() {
        let mut table = Table::new();
        table.insert(PermissionRequest(7), MediaWaiter(1));
        // The app's own request passes through to `MobileEvent::Permissions`.
        let app = outcome(6, true);
        assert_eq!(table.offer(app.clone()).map(|(w, _)| w), Err(app));
        // The gate's is consumed, once.
        let (waiter, answer) = table.offer(outcome(7, true)).unwrap();
        assert_eq!(waiter, MediaWaiter(1));
        assert!(answer.granted(CAMERA));
        let again = outcome(7, true);
        assert_eq!(table.offer(again.clone()).map(|(w, _)| w), Err(again));
    }

    #[test]
    fn withdrawn_and_cleared_requests_go_to_the_app() {
        let mut table = Table::new();
        table.insert(PermissionRequest(1), MediaWaiter(1));
        table.insert(PermissionRequest(2), MediaWaiter(2));
        assert_eq!(table.withdraw(PermissionRequest(1)), Some(MediaWaiter(1)));
        assert_eq!(table.withdraw(PermissionRequest(1)), None);
        assert!(table.offer(outcome(1, true)).is_err());
        assert_eq!(table.len(), 1);
        assert_eq!(table.clear(), vec![MediaWaiter(2)]);
        assert_eq!(table.len(), 0);
        assert!(table.offer(outcome(2, true)).is_err());
    }
}
