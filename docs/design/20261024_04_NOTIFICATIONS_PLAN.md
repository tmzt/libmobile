---
title: Alert notifications, tap tags, and updating the ongoing notification
date: 2026-10-24
status: Draft
covers:
  - "Feature `notifications`: `android::notify::{post, cancel, cancel_all}`
    for alert notifications on the app's own channels, separate from the
    foreground service's"
  - "`NotificationContent::tag`, and events naming what was tapped: the
    ongoing notification's body, an alert's body, an alert's button"
  - "Updating the ongoing notification from any thread while the activity
    is suspended, as a documented and checked guarantee"
sources:
  - "handlr (request, 2026-10-24, with the owner's \"yes\", from the handlr
    session): alerts when an agent asks a question, needs an approval or
    finishes, while the app is not on screen, on their own channel with
    normal priority and sound; a tap naming what was tapped, so handlr
    can open that agent; it already asks for POST_NOTIFICATIONS from its
    own setting. Owner: \"updating the ongoing notification should be
    supported\": handlr calls `update` and `update_text` from a non-UI
    thread while the activity is suspended, since no frames are drawn
    then, changing the title, text, actions and tap tag."
  - "libmobile 0.2.4: `android::service` (`start`, `update`,
    `update_text`, `stop`), `NotificationContent { title, text, actions,
    visibility }` (`#[non_exhaustive]`), `ServiceEvent::Action(id)` for
    button taps, and a body tap that only opens the app
    (`ForegroundServicePlugin.openApp(context, null, ...)`)."
  - "Android: `NotificationChannel` (API 26), whose importance and sound
    cannot be changed by the app once created; `NotificationManager.notify`
    and `cancel` by tag and id; `PendingIntent` extras reaching
    `MobileNativeActivity` through `onCreate` or `onNewIntent`;
    `POST_NOTIFICATIONS` (API 33); notifications are posted from any
    thread."
related:
  - docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md
  - libmorph docs/design/20261024_04_NOTIFICATIONS_PLAN.md (wraps it)
---

# Alert notifications, tap tags, and updating the ongoing notification

## 1. Tap tags (core to both kinds)

`NotificationContent` gains one field (additive: the struct is
`#[non_exhaustive]` and built with `new`):

```rust
/// Set on the body's tap: the app gets it back, to know what was tapped
/// (for example an agent's id). At most 256 bytes.
pub tag: Option<String>,
```

A body tap opens the app's activity, as now, with the notification's id
and tag as intent extras (`libmobile.notification`, `libmobile.tag`).
`IntentPlugin` recognises them and libmobile delivers, instead of a plain
`MobileEvent::Intent`:
- for the ongoing notification, `ServiceEvent::Tapped(Option<String>)`
  (through `MobileEvent::Service` and system events);
- for an alert, `NotifyEvent::Tapped { id, tag }` (section 2).

A tap that cold-starts the app arrives with the launch intent, so it is
queued like any other bridge event and reaches the app after it exists.

## 2. Alert notifications

Feature `notifications`. It adds `POST_NOTIFICATIONS` to the manifest and
a Kotlin `notify.NotifyPlugin`, reached from any thread through the
Application context.

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Channel {
    /// Stable id, 1 to 64 of [a-z0-9._-]; not "libmobile.foreground".
    pub id: String,
    /// Shown in the system's settings.
    pub name: String,
    pub importance: Importance,
}
impl Channel { pub fn new(id: impl Into<String>, name: impl Into<String>, importance: Importance) -> Self; }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Importance { Low, Default, High }   // Default: sound, no heads-up; High: heads-up

pub fn post(id: &str, content: &NotificationContent, channel: &Channel) -> Result<(), NotifyError>;
pub fn cancel(id: &str) -> Result<(), NotifyError>;
pub fn cancel_all() -> Result<(), NotifyError>;   // the app's alerts, not the service's

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyError { BadId, PermissionDenied, TooManyActions, Platform(String) }

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyEvent {
    Tapped { id: String, tag: Option<String> },
    Action { id: String, action: String },
}
```

- `id` names the alert (1 to 64 of `[A-Za-z0-9._-]`); posting the same id
  again replaces it, so an alert can be updated.
- The channel is created on its first post. Android keeps a channel's
  importance and sound once created, so changing `importance` later
  needs a new channel id; the rustdoc says so.
- `PermissionDenied` when `POST_NOTIFICATIONS` is not granted; the app
  asks for it itself, as it does now.
- Alerts are dismissable and auto-cancel on tap. Buttons work as the
  service's do (`NotificationAction`, `opens_app`), delivered as
  `NotifyEvent::Action`.
- `NotifyEvent` reaches the main loop as `MobileEvent::Notify(NotifyEvent)`
  and, with `system-events`, `SystemEvent::Notify`.

## 3. Updating the ongoing notification

`service::update(&content)` and `update_text` already exist. This plan
makes them a checked guarantee:
- they may be called from any thread, including while the activity is
  stopped or its window destroyed (no frames drawn), as long as the
  service runs (`NotRunning` otherwise);
- `update` replaces the title, text, buttons and tag; `update_text`
  only the text;
- updates are posted at once, without sound or heads-up
  (`setOnlyAlertOnce`), so frequent updates do not disturb.

The rustdoc of both says this, and step 3 checks it.

## 4. Steps

1. **Tags and alerts.** `NotificationContent::tag`, the tap extras and
   events, feature `notifications` with `NotifyPlugin`, `Channel`,
   `post`, `cancel`, `cancel_all`, `NotifyEvent`; host tests for id and
   channel validation and the extras.
   **Done when:** tests and clippy pass on the host and Android with every
   feature, and a generated project with `notifications` builds.
2. **Example.** The `background` example posts an alert (`--es cmd
   alert` or a timer) on a `high` channel with a tag and a button, and
   updates the ongoing notification every 30 s from its network thread
   with a new title, text, button set and tag.
   **Done when:** it builds and runs on the S23.
3. **On the S23.**
   **Done when:** on the owner's S23 Ultra:
   - the alert appears with sound on its own channel, separate from the
     service's in the app's notification settings;
   - tapping its body opens the app and logs `Tapped` with its id and
     tag; its button logs `Action`; posting the same id replaces it;
     `cancel` removes it;
   - tapping the ongoing notification's body logs
     `ServiceEvent::Tapped(Some(tag))`, with the tag from the latest
     update;
   - with the screen off and the activity stopped, the ongoing
     notification's title, text and buttons change at each update
     (seen on waking the screen, and in `dumpsys notification
     --noredact`), without sound;
   - with the app killed, tapping an alert cold-starts it and the tap is
     delivered after start;
   - with `POST_NOTIFICATIONS` revoked, `post` gives `PermissionDenied`.
4. **Release** in the next libmobile release, on the owner's word.
   Additive.

## 5. Open questions for the owner

1. **One event type or two.** The ongoing notification's tap is a
   `ServiceEvent::Tapped`, alerts' taps a `NotifyEvent`. Recommended: as
   written; the service and alerts are separate features.
2. **Channel per call.** `post` takes the channel each time, created on
   first use. Recommended: yes; an app usually has one or two.
