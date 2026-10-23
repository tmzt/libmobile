//! **What happens to the app, for its own threads.**
//!
//! [`ServiceEvent`] (feature `foreground-service`) reports the foreground
//! service: it arrives on the main loop as `MobileEvent::Service`.

/// Something the foreground service did, or a tap on its notification.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ServiceEvent {
    /// The service is in the foreground with its notification.
    Started,
    /// The service stopped.
    Stopped,
    /// The service could not go into the foreground; it has stopped.
    Failed(String),
    /// A type's time limit was reached (`dataSync`, 6 hours in 24 from
    /// Android 15); the service has stopped.
    TimedOut,
    /// The service runs, but notifications are off for the app (on Android
    /// 13+, `POST_NOTIFICATIONS` denied), so its notification and buttons
    /// show only in the system's running-apps list.
    NotificationHidden,
    /// The app's own button, by id.
    Action(String),
    /// The Exit button: the service has stopped and the activity's task is
    /// finishing. The app's threads should end.
    Exited,
    /// The wake lock is now held, or not.
    WakeLock(bool),
    /// The app was swiped away from Recents while
    /// `Project::foreground_service_keeps_running`; the service still runs.
    TaskRemoved,
}

impl ServiceEvent {
    /// From the plugin's event code and detail (`ForegroundServicePlugin.kt`).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
    pub(crate) fn from_code(code: i32, detail: Option<String>) -> Option<Self> {
        Some(match code {
            0 => ServiceEvent::Started,
            1 => ServiceEvent::Stopped,
            2 => ServiceEvent::Failed(detail.unwrap_or_default()),
            3 => ServiceEvent::TimedOut,
            4 => ServiceEvent::NotificationHidden,
            5 => ServiceEvent::Action(detail.unwrap_or_default()),
            6 => ServiceEvent::Exited,
            7 => ServiceEvent::WakeLock(true),
            8 => ServiceEvent::WakeLock(false),
            9 => ServiceEvent::TaskRemoved,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_plugin() {
        assert_eq!(
            ServiceEvent::from_code(0, None),
            Some(ServiceEvent::Started)
        );
        assert_eq!(
            ServiceEvent::from_code(2, Some("no".into())),
            Some(ServiceEvent::Failed("no".into()))
        );
        assert_eq!(
            ServiceEvent::from_code(5, Some("ping".into())),
            Some(ServiceEvent::Action("ping".into()))
        );
        assert_eq!(ServiceEvent::from_code(6, None), Some(ServiceEvent::Exited));
        assert_eq!(
            ServiceEvent::from_code(7, None),
            Some(ServiceEvent::WakeLock(true))
        );
        assert_eq!(
            ServiceEvent::from_code(8, None),
            Some(ServiceEvent::WakeLock(false))
        );
        assert_eq!(
            ServiceEvent::from_code(9, None),
            Some(ServiceEvent::TaskRemoved)
        );
        assert_eq!(ServiceEvent::from_code(10, None), None);
    }
}
