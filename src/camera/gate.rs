//! The permission gate (plan 3.2): CAMERA is asked for on the first call
//! that needs the camera, never at launch.
//!
//! 1. Granted already: the call proceeds.
//! 2. Otherwise the call waits; if no camera request is in flight, one is
//!    made (`request_permissions(CAMERA)`), and the gate remembers its id.
//!    Every later call waits for the same answer.
//! 3. The main loop offers each permission answer to the core gate
//!    (`android::permission_gate`, shared with `media-request`) first; the
//!    camera's own is consumed (not delivered as `MobileEvent::Permissions`).
//!    Granted: every waiting call proceeds, in the order made. Denied: every
//!    waiting call fails with `PermissionDenied { settings_only }`, where
//!    `settings_only` is `!should_show_rationale(CAMERA)` read after the
//!    denial.
//! 4. A later call asks again. libmobile keeps no "never ask again" flag: the
//!    user may grant it in Settings at any time, and step 1 then holds.

#![cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android camera thread.

use super::CameraError;

/// What a call must do on reaching the gate.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Admit<T> {
    /// CAMERA is granted: go ahead.
    Proceed(T),
    /// The call waits; make the request now and tell the gate its id
    /// ([`Gate::asked`]).
    Ask,
    /// The call waits for a request already in flight.
    Wait,
}

/// The gate's answer for its waiting calls, in the order they were made.
#[derive(Debug, PartialEq)]
pub(crate) enum Answer<T> {
    Granted(Vec<T>),
    Denied(Vec<T>, CameraError),
}

/// The waiting calls and the request in flight. `T` names a call (a session
/// or capture id).
#[derive(Debug)]
pub(crate) struct Gate<T> {
    request: Option<u32>,
    waiting: Vec<T>,
}

impl<T> Default for Gate<T> {
    fn default() -> Self {
        Self {
            request: None,
            waiting: Vec::new(),
        }
    }
}

impl<T> Gate<T> {
    /// A call reaches the gate; `granted` is `permission_granted(CAMERA)` now.
    pub(crate) fn admit(&mut self, granted: bool, call: T) -> Admit<T> {
        if granted && self.request.is_none() {
            return Admit::Proceed(call);
        }
        self.waiting.push(call);
        if self.request.is_none() {
            Admit::Ask
        } else {
            Admit::Wait
        }
    }

    /// The request made for [`Admit::Ask`].
    pub(crate) fn asked(&mut self, request: u32) {
        self.request = Some(request);
    }

    /// The request in flight, if any.
    pub(crate) fn in_flight(&self) -> Option<u32> {
        self.request
    }

    /// An answer to `request`. `None` if it is not the gate's; otherwise
    /// every waiting call, granted or denied with `settings_only`.
    pub(crate) fn answer(
        &mut self,
        request: u32,
        granted: bool,
        settings_only: bool,
    ) -> Option<Answer<T>> {
        if self.request != Some(request) {
            return None;
        }
        self.request = None;
        let calls = std::mem::take(&mut self.waiting);
        Some(if granted {
            Answer::Granted(calls)
        } else {
            Answer::Denied(calls, CameraError::PermissionDenied { settings_only })
        })
    }

    /// Drop a waiting call (cancelled or stopped before the answer).
    pub(crate) fn withdraw(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.waiting.retain(|call| keep(call));
    }

    /// Forget everything (the activity is destroyed).
    pub(crate) fn clear(&mut self) {
        self.request = None;
        self.waiting.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granted_calls_proceed_at_once() {
        let mut gate = Gate::default();
        assert_eq!(gate.admit(true, 1), Admit::Proceed(1));
        assert_eq!(gate.in_flight(), None);
    }

    #[test]
    fn waiting_calls_are_answered_in_order_on_grant() {
        let mut gate = Gate::default();
        assert_eq!(gate.admit(false, 1), Admit::Ask);
        gate.asked(7);
        assert_eq!(gate.admit(false, 2), Admit::Wait);
        // Granted meanwhile (from Settings): still waits for the answer, so
        // the calls stay in order.
        assert_eq!(gate.admit(true, 3), Admit::Wait);
        assert_eq!(gate.answer(6, true, false), None, "not the gate's request");
        assert_eq!(
            gate.answer(7, true, false),
            Some(Answer::Granted(vec![1, 2, 3]))
        );
        assert_eq!(gate.in_flight(), None);
        assert_eq!(gate.answer(7, true, false), None, "answered once");
    }

    #[test]
    fn denial_fails_every_waiting_call_with_settings_only() {
        let mut gate = Gate::default();
        gate.admit(false, 1);
        gate.asked(1);
        gate.admit(false, 2);
        assert_eq!(
            gate.answer(1, false, false),
            Some(Answer::Denied(
                vec![1, 2],
                CameraError::PermissionDenied {
                    settings_only: false
                }
            ))
        );
        // A later call asks again; Android now refuses without a dialog.
        assert_eq!(gate.admit(false, 3), Admit::Ask);
        gate.asked(2);
        assert_eq!(
            gate.answer(2, false, true),
            Some(Answer::Denied(
                vec![3],
                CameraError::PermissionDenied {
                    settings_only: true
                }
            ))
        );
    }

    #[test]
    fn withdrawn_calls_get_no_answer() {
        let mut gate = Gate::default();
        gate.admit(false, 1);
        gate.asked(4);
        gate.admit(false, 2);
        gate.withdraw(|c| *c != 1);
        assert_eq!(gate.answer(4, true, false), Some(Answer::Granted(vec![2])));
        gate.admit(false, 5);
        gate.asked(5);
        gate.clear();
        assert_eq!(gate.answer(5, true, false), None);
    }
}
