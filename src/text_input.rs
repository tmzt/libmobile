//! **The soft keyboard's options, as plain values.**
//!
//! [`TextInputOptions`] says what keyboard the hidden field asks for: one
//! line or many, the keyboard type ([`TextKind`]), the action key
//! ([`TextAction`]) and autocorrect. `android::text_input` maps them to the
//! `EditText`'s `inputType` and `imeOptions` (the numbers below, Android's
//! own constants), so the mapping builds and tests on any host
//! (`docs/design/20261024_06_TEXT_INPUT_OPTIONS_PLAN.md`).

/// The hidden field's keyboard. [`Default`] is one line of text with Done
/// and autocorrect, the field's keyboard before these options existed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextInputOptions {
    /// Many lines: Enter is a newline unless `action` is not
    /// [`TextAction::None`].
    pub multiline: bool,
    /// The keyboard type.
    pub kind: TextKind,
    /// The action key.
    pub action: TextAction,
    /// Autocorrect and suggestions; `false` turns both off.
    pub autocorrect: bool,
}

impl Default for TextInputOptions {
    fn default() -> Self {
        TextInputOptions {
            multiline: false,
            kind: TextKind::Text,
            action: TextAction::Done,
            autocorrect: true,
        }
    }
}

/// The keyboard type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextKind {
    #[default]
    Text,
    /// A URL: `/` and `.com` on the keyboard.
    Url,
    Email,
    /// The number pad.
    Number,
    /// Hidden text.
    Password,
}

/// The keyboard's action key, and what [`MobileEvent::TextAction`] reports.
///
/// [`MobileEvent::TextAction`]: crate::MobileEvent::TextAction
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextAction {
    #[default]
    Done,
    Send,
    Go,
    Search,
    /// No action of its own: the key shows Enter (↵). On one line it is
    /// still reported as `TextAction(None)`; on many lines it is a newline.
    None,
}

// Android's constants (`android.text.InputType`,
// `android.view.inputmethod.EditorInfo`).
const TYPE_CLASS_TEXT: i32 = 0x1;
const TYPE_CLASS_NUMBER: i32 = 0x2;
const TYPE_TEXT_VARIATION_URI: i32 = 0x10;
const TYPE_TEXT_VARIATION_EMAIL_ADDRESS: i32 = 0x20;
const TYPE_TEXT_VARIATION_PASSWORD: i32 = 0x80;
const TYPE_TEXT_FLAG_AUTO_CORRECT: i32 = 0x8000;
const TYPE_TEXT_FLAG_MULTI_LINE: i32 = 0x2_0000;
const TYPE_TEXT_FLAG_NO_SUGGESTIONS: i32 = 0x8_0000;

const IME_ACTION_NONE: i32 = 0x1;
const IME_ACTION_GO: i32 = 0x2;
const IME_ACTION_SEARCH: i32 = 0x3;
const IME_ACTION_SEND: i32 = 0x4;
const IME_ACTION_DONE: i32 = 0x6;
const IME_FLAG_NO_EXTRACT_UI: i32 = 0x1000_0000;
const IME_FLAG_NO_ENTER_ACTION: i32 = 0x4000_0000;

impl TextInputOptions {
    /// The field's `inputType`.
    pub(crate) fn input_type(&self) -> i32 {
        if self.kind == TextKind::Number {
            // Number has no text flags: no lines, no suggestions to turn off.
            return TYPE_CLASS_NUMBER;
        }
        let variation = match self.kind {
            TextKind::Url => TYPE_TEXT_VARIATION_URI,
            TextKind::Email => TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            TextKind::Password => TYPE_TEXT_VARIATION_PASSWORD,
            TextKind::Text | TextKind::Number => 0,
        };
        let lines = if self.multiline {
            TYPE_TEXT_FLAG_MULTI_LINE
        } else {
            0
        };
        let correct = if self.autocorrect {
            TYPE_TEXT_FLAG_AUTO_CORRECT
        } else {
            TYPE_TEXT_FLAG_NO_SUGGESTIONS
        };
        TYPE_CLASS_TEXT | variation | lines | correct
    }

    /// The field's `imeOptions`. Never the full-screen editor (the field is
    /// invisible); [`TextAction::None`] is no action, with the key Enter.
    pub(crate) fn ime_options(&self) -> i32 {
        let action = match self.action {
            TextAction::Done => IME_ACTION_DONE,
            TextAction::Send => IME_ACTION_SEND,
            TextAction::Go => IME_ACTION_GO,
            TextAction::Search => IME_ACTION_SEARCH,
            TextAction::None => IME_ACTION_NONE | IME_FLAG_NO_ENTER_ACTION,
        };
        action | IME_FLAG_NO_EXTRACT_UI
    }

    /// Whether the action key (or Enter) is reported as
    /// [`MobileEvent::TextAction`](crate::MobileEvent::TextAction): always
    /// on one line, and on many lines only with an action of its own.
    pub(crate) fn reports_action(&self) -> bool {
        !self.multiline || self.action != TextAction::None
    }
}

impl TextAction {
    /// The number the Kotlin field hands back with an action.
    pub(crate) fn code(self) -> i32 {
        match self {
            TextAction::Done => 0,
            TextAction::Send => 1,
            TextAction::Go => 2,
            TextAction::Search => 3,
            TextAction::None => 4,
        }
    }

    /// [`code`](Self::code) back; an unknown number is `Done`.
    pub(crate) fn from_code(code: i32) -> TextAction {
        match code {
            1 => TextAction::Send,
            2 => TextAction::Go,
            3 => TextAction::Search,
            4 => TextAction::None,
            _ => TextAction::Done,
        }
    }
}

/// Watches the keyboard's inset while the field is active, for
/// [`MobileEvent::TextInputDismissed`](crate::MobileEvent::TextInputDismissed):
/// the inset falling to zero after the keyboard was shown is the user (Back,
/// the keyboard's hide key) or the system closing it. Reported once; showing
/// the field again re-arms it, and hiding it from the app reports nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DismissWatch {
    active: bool,
    shown: bool,
}

impl DismissWatch {
    pub(crate) const fn new() -> Self {
        DismissWatch {
            active: false,
            shown: false,
        }
    }

    /// `set_text_input_with(app, Some(..))`: the keyboard is asked for. A
    /// keyboard already up stays counted as shown.
    pub(crate) fn activate(&mut self) {
        self.active = true;
    }

    /// `set_text_input_with(app, None)`: the app hid it; nothing to report.
    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.shown = false;
    }

    /// An inset report: `true` when it is the dismissal.
    pub(crate) fn ime(&mut self, ime_bottom: f32) -> bool {
        let up = ime_bottom > 0.0;
        if !self.active {
            self.shown = up;
            return false;
        }
        let dismissed = self.shown && !up;
        self.shown = up;
        if dismissed {
            // Once: a later Some(..) shows it again and re-arms.
            self.active = false;
        }
        dismissed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(kind: TextKind, multiline: bool, autocorrect: bool) -> TextInputOptions {
        TextInputOptions {
            multiline,
            kind,
            autocorrect,
            ..TextInputOptions::default()
        }
    }

    #[test]
    fn the_default_is_one_line_of_text_with_done_and_autocorrect() {
        let o = TextInputOptions::default();
        assert!(!o.multiline);
        assert_eq!(o.kind, TextKind::Text);
        assert_eq!(o.action, TextAction::Done);
        assert!(o.autocorrect);
        assert_eq!(
            o.input_type(),
            TYPE_CLASS_TEXT | TYPE_TEXT_FLAG_AUTO_CORRECT
        );
        assert_eq!(o.ime_options(), IME_ACTION_DONE | IME_FLAG_NO_EXTRACT_UI);
        assert!(o.reports_action());
    }

    #[test]
    fn kinds_map_to_their_input_types() {
        for (kind, expected) in [
            (TextKind::Text, 0x0000_8001),
            (TextKind::Url, 0x0000_8011),
            (TextKind::Email, 0x0000_8021),
            (TextKind::Password, 0x0000_8081),
            (TextKind::Number, 0x0000_0002),
        ] {
            assert_eq!(
                options(kind, false, true).input_type(),
                expected,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn lines_and_autocorrect_are_text_flags() {
        // Many lines, no autocorrect: TYPE_CLASS_TEXT | MULTI_LINE | NO_SUGGESTIONS.
        assert_eq!(
            options(TextKind::Text, true, false).input_type(),
            0x000A_0001
        );
        assert_eq!(
            options(TextKind::Url, false, false).input_type(),
            0x0008_0011
        );
        // The number pad takes neither.
        assert_eq!(options(TextKind::Number, true, false).input_type(), 0x2);
    }

    #[test]
    fn actions_map_to_ime_options_without_the_fullscreen_editor() {
        for (action, expected) in [
            (TextAction::Done, 0x1000_0006),
            (TextAction::Send, 0x1000_0004),
            (TextAction::Go, 0x1000_0002),
            (TextAction::Search, 0x1000_0003),
            // No action of its own: the key is Enter.
            (TextAction::None, 0x5000_0001),
        ] {
            let o = TextInputOptions {
                action,
                ..TextInputOptions::default()
            };
            assert_eq!(o.ime_options(), expected, "{action:?}");
        }
    }

    #[test]
    fn one_line_always_reports_the_action_and_many_lines_only_with_one() {
        let mut o = TextInputOptions {
            action: TextAction::None,
            ..TextInputOptions::default()
        };
        assert!(o.reports_action());
        o.multiline = true;
        assert!(!o.reports_action());
        o.action = TextAction::Send;
        assert!(o.reports_action());
    }

    #[test]
    fn action_codes_round_trip() {
        for action in [
            TextAction::Done,
            TextAction::Send,
            TextAction::Go,
            TextAction::Search,
            TextAction::None,
        ] {
            assert_eq!(TextAction::from_code(action.code()), action);
        }
        assert_eq!(TextAction::from_code(99), TextAction::Done);
    }

    #[test]
    fn the_inset_falling_to_zero_while_active_is_one_dismissal() {
        let mut w = DismissWatch::default();
        w.activate();
        // Not up yet: the keyboard is still coming.
        assert!(!w.ime(0.0));
        assert!(!w.ime(400.0));
        assert!(!w.ime(900.0));
        assert!(w.ime(0.0));
        // Once.
        assert!(!w.ime(0.0));
        assert!(!w.ime(900.0));
        assert!(!w.ime(0.0));
        // Shown again: re-armed.
        w.activate();
        assert!(!w.ime(900.0));
        assert!(w.ime(0.0));
    }

    #[test]
    fn hiding_from_the_app_reports_nothing() {
        let mut w = DismissWatch::default();
        w.activate();
        assert!(!w.ime(900.0));
        w.deactivate();
        assert!(!w.ime(0.0));
    }

    #[test]
    fn a_value_replaced_while_the_keyboard_is_up_keeps_it_shown() {
        let mut w = DismissWatch::default();
        w.activate();
        assert!(!w.ime(900.0));
        w.activate();
        assert!(w.ime(0.0));
    }
}
