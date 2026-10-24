//! **What happens to the app, for its own threads.**
//!
//! [`ServiceEvent`] reports the foreground service (feature
//! `foreground-service`): it arrives on the main loop as
//! `MobileEvent::Service`.
//!
//! With the feature `system-events`, any thread can follow what happens to
//! the app without the main loop and without an async runtime:
//! [`subscribe`] returns a queue of [`SystemEvent`]s that receives every
//! event after the call, in order, never dropped. A network thread, for
//! example, reconnects when Doze ends or the network comes back:
//!
//! ```no_run
//! # #[cfg(feature = "system-events")]
//! # fn f() {
//! use libmobile::system_events::{self, SystemEvent};
//!
//! let events = system_events::subscribe();
//! std::thread::spawn(move || loop {
//!     match events.next_blocking() {
//!         SystemEvent::DeviceIdle(false) | SystemEvent::Network(Some(_)) => { /* reconnect */ }
//!         _ => {}
//!     }
//! });
//! # }
//! ```
//!
//! On Android the callbacks are registered on the Application at the first
//! [`subscribe`] and stay for the life of the process; that first call also
//! reports the state at that moment (visibility, Doze, the default network).
//! [`current`] gives the latest of each kind, for a subscriber that starts
//! late. Elsewhere nothing is reported, except what the app's own code does.
//! With the feature `tokio`, [`subscribe_tokio`] gives a tokio receiver
//! instead.

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
    #[cfg_attr(
        not(all(target_os = "android", feature = "foreground-service")),
        allow(dead_code)
    )] // Used by the Android bridge.
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

#[cfg(feature = "system-events")]
pub use subscribers::*;

#[cfg(feature = "system-events")]
mod subscribers {
    use super::ServiceEvent;
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};

    /// Something that happened to the app.
    #[derive(Clone, Debug, PartialEq)]
    #[non_exhaustive]
    pub enum SystemEvent {
        /// An activity of the app was started (`true`) or the last one
        /// stopped (`false`): the app can be seen, or not.
        Visible(bool),
        /// An activity was resumed, or the last one paused.
        Focused(bool),
        /// The foreground service (feature `foreground-service`).
        Service(ServiceEvent),
        /// The default network: `Some` when it is available or changes,
        /// `None` when there is none.
        Network(Option<NetworkInfo>),
        /// Doze entered (`true`) or left.
        DeviceIdle(bool),
        /// The battery-optimization exemption (feature `battery`): whether
        /// the app is ignoring battery optimizations. Read again each time
        /// the app becomes visible or focused, and sent when it changed; the first
        /// `subscribe()` reports the value at that moment.
        #[cfg(feature = "battery")]
        BatteryExemption(bool),
    }

    /// The default network.
    #[derive(Clone, Debug, PartialEq)]
    #[non_exhaustive]
    pub struct NetworkInfo {
        /// Android checked that it reaches the internet.
        pub validated: bool,
        /// It may cost the user money (`NET_CAPABILITY_NOT_METERED` absent).
        pub metered: bool,
        pub transport: Transport,
    }

    impl NetworkInfo {
        /// From the plugin's arguments.
        #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
        pub(crate) fn new(validated: bool, metered: bool, transport: Transport) -> Self {
            Self {
                validated,
                metered,
                transport,
            }
        }
    }

    /// How the default network is carried. A VPN is reported as `Vpn`,
    /// whatever it runs over.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum Transport {
        Wifi,
        Cellular,
        Ethernet,
        Vpn,
        Other,
    }

    impl Transport {
        /// From the plugin's code (`SystemEventsPlugin.kt`).
        #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridge.
        pub(crate) fn from_code(code: i32) -> Self {
            match code {
                0 => Transport::Wifi,
                1 => Transport::Cellular,
                2 => Transport::Ethernet,
                3 => Transport::Vpn,
                _ => Transport::Other,
            }
        }
    }

    /// The latest event of each kind; `None` for a kind not reported yet.
    #[derive(Clone, Debug, Default, PartialEq)]
    #[non_exhaustive]
    pub struct SystemState {
        pub visible: Option<bool>,
        pub focused: Option<bool>,
        /// The latest service event.
        pub service: Option<ServiceEvent>,
        /// `Some(None)`: no network.
        pub network: Option<Option<NetworkInfo>>,
        pub device_idle: Option<bool>,
        /// The battery-optimization exemption (feature `battery`).
        #[cfg(feature = "battery")]
        pub battery_exemption: Option<bool>,
    }

    impl SystemState {
        const EMPTY: SystemState = SystemState {
            visible: None,
            focused: None,
            service: None,
            network: None,
            device_idle: None,
            #[cfg(feature = "battery")]
            battery_exemption: None,
        };

        fn apply(&mut self, event: &SystemEvent) {
            match event {
                SystemEvent::Visible(v) => self.visible = Some(*v),
                SystemEvent::Focused(v) => self.focused = Some(*v),
                SystemEvent::Service(e) => self.service = Some(e.clone()),
                SystemEvent::Network(n) => self.network = Some(n.clone()),
                SystemEvent::DeviceIdle(v) => self.device_idle = Some(*v),
                #[cfg(feature = "battery")]
                SystemEvent::BatteryExemption(v) => self.battery_exemption = Some(*v),
            }
        }
    }

    /// One subscriber's queue of events. Dropping it unsubscribes.
    #[derive(Debug)]
    pub struct Events {
        queue: Arc<Queue>,
    }

    #[derive(Debug, Default)]
    struct Queue {
        items: Mutex<VecDeque<SystemEvent>>,
        ready: Condvar,
    }

    impl Events {
        /// The next event, or `None` if there is none yet.
        pub fn try_next(&self) -> Option<SystemEvent> {
            lock(&self.queue.items).pop_front()
        }

        /// The next event, waiting for one.
        pub fn next_blocking(&self) -> SystemEvent {
            let mut items = lock(&self.queue.items);
            loop {
                if let Some(event) = items.pop_front() {
                    return event;
                }
                items = self
                    .queue
                    .ready
                    .wait(items)
                    .unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    enum Sink {
        Queue(Weak<Queue>),
        #[cfg(feature = "tokio")]
        Tokio(tokio::sync::mpsc::UnboundedSender<SystemEvent>),
    }

    impl Sink {
        /// Deliver; `false` once the subscriber is gone.
        fn send(&self, event: &SystemEvent) -> bool {
            match self {
                Sink::Queue(queue) => match queue.upgrade() {
                    Some(queue) => {
                        lock(&queue.items).push_back(event.clone());
                        queue.ready.notify_all();
                        true
                    }
                    None => false,
                },
                #[cfg(feature = "tokio")]
                Sink::Tokio(sender) => sender.send(event.clone()).is_ok(),
            }
        }
    }

    /// The subscribers and the latest state, under one lock so every
    /// subscriber sees the same order.
    struct Hub {
        sinks: Vec<Sink>,
        state: SystemState,
    }

    static HUB: Mutex<Hub> = Mutex::new(Hub {
        sinks: Vec::new(),
        state: SystemState::EMPTY,
    });

    /// Poisoning is ignored: every critical section is a push or a swap.
    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every event after the call, in order, never dropped (the queue is
    /// unbounded; events are rare). The first call on Android registers the
    /// callbacks and reports the state at that moment.
    pub fn subscribe() -> Events {
        let queue = Arc::new(Queue::default());
        lock(&HUB).sinks.push(Sink::Queue(Arc::downgrade(&queue)));
        register();
        Events { queue }
    }

    /// [`subscribe`] as a tokio receiver (feature `tokio`). Needs no tokio
    /// runtime to send; receive it however the app likes.
    #[cfg(feature = "tokio")]
    pub fn subscribe_tokio() -> tokio::sync::mpsc::UnboundedReceiver<SystemEvent> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        lock(&HUB).sinks.push(Sink::Tokio(sender));
        register();
        receiver
    }

    /// The latest of each kind, for a subscriber that starts late.
    pub fn current() -> SystemState {
        lock(&HUB).state.clone()
    }

    /// Hand `event` to every subscriber, from whatever thread reports it.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Used by the Android bridges.
    pub(crate) fn publish(event: SystemEvent) {
        let mut hub = lock(&HUB);
        hub.state.apply(&event);
        hub.sinks.retain(|sink| sink.send(&event));
    }

    fn register() {
        #[cfg(target_os = "android")]
        crate::android::system_events::ensure_registered();
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::Duration;

        /// The hub is process-wide, so these tests take turns.
        static TURN: Mutex<()> = Mutex::new(());

        fn drain(events: &Events) -> Vec<SystemEvent> {
            std::iter::from_fn(|| events.try_next()).collect()
        }

        #[test]
        fn every_event_in_order() {
            let _turn = lock(&TURN);
            let events = subscribe();
            for i in 0..1000 {
                publish(SystemEvent::Visible(i % 2 == 0));
            }
            let got = drain(&events);
            assert_eq!(got.len(), 1000);
            for (i, event) in got.iter().enumerate() {
                assert_eq!(*event, SystemEvent::Visible(i % 2 == 0));
            }
            assert_eq!(events.try_next(), None);
        }

        #[test]
        fn none_lost_across_threads() {
            let _turn = lock(&TURN);
            let events = subscribe();
            let threads: Vec<_> = (0..4)
                .map(|t| {
                    std::thread::spawn(move || {
                        for _ in 0..250 {
                            publish(SystemEvent::DeviceIdle(t % 2 == 0));
                        }
                    })
                })
                .collect();
            for thread in threads {
                thread.join().unwrap();
            }
            assert_eq!(drain(&events).len(), 1000);
        }

        #[test]
        fn blocking_wait_is_woken_from_another_thread() {
            let _turn = lock(&TURN);
            let events = subscribe();
            let sender = std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(50));
                publish(SystemEvent::Network(None));
            });
            assert_eq!(events.next_blocking(), SystemEvent::Network(None));
            sender.join().unwrap();
        }

        #[test]
        fn every_subscriber_gets_each_event_and_dropped_ones_leave() {
            let _turn = lock(&TURN);
            let a = subscribe();
            let b = subscribe();
            publish(SystemEvent::Focused(true));
            assert_eq!(a.try_next(), Some(SystemEvent::Focused(true)));
            assert_eq!(b.try_next(), Some(SystemEvent::Focused(true)));
            let before = lock(&HUB).sinks.len();
            drop(b);
            publish(SystemEvent::Focused(false));
            assert_eq!(lock(&HUB).sinks.len(), before - 1);
            assert_eq!(a.try_next(), Some(SystemEvent::Focused(false)));
        }

        #[test]
        fn current_keeps_the_latest_of_each_kind() {
            let _turn = lock(&TURN);
            let wifi = NetworkInfo::new(true, false, Transport::Wifi);
            publish(SystemEvent::Network(Some(wifi.clone())));
            publish(SystemEvent::DeviceIdle(true));
            publish(SystemEvent::DeviceIdle(false));
            publish(SystemEvent::Service(ServiceEvent::Started));
            let state = current();
            assert_eq!(state.network, Some(Some(wifi)));
            assert_eq!(state.device_idle, Some(false));
            assert_eq!(state.service, Some(ServiceEvent::Started));
            publish(SystemEvent::Network(None));
            assert_eq!(current().network, Some(None));
        }

        #[cfg(feature = "battery")]
        #[test]
        fn current_keeps_the_battery_exemption() {
            let _turn = lock(&TURN);
            publish(SystemEvent::BatteryExemption(true));
            assert_eq!(current().battery_exemption, Some(true));
            publish(SystemEvent::BatteryExemption(false));
            assert_eq!(current().battery_exemption, Some(false));
        }

        #[test]
        fn transports_from_codes() {
            assert_eq!(Transport::from_code(0), Transport::Wifi);
            assert_eq!(Transport::from_code(3), Transport::Vpn);
            assert_eq!(Transport::from_code(99), Transport::Other);
        }

        #[cfg(feature = "tokio")]
        #[test]
        fn tokio_receiver_gets_every_event() {
            let _turn = lock(&TURN);
            let mut receiver = subscribe_tokio();
            publish(SystemEvent::Visible(true));
            publish(SystemEvent::Visible(false));
            assert_eq!(receiver.try_recv(), Ok(SystemEvent::Visible(true)));
            let blocked = std::thread::spawn(move || receiver.blocking_recv());
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(blocked.join().unwrap(), Some(SystemEvent::Visible(false)));
            // The receiver is gone: the next event drops its sink.
            let before = lock(&HUB).sinks.len();
            publish(SystemEvent::Visible(true));
            assert_eq!(lock(&HUB).sinks.len(), before - 1);
        }
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
