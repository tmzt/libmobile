//! **Sign in with Google results** (feature `google-auth`).
//!
//! `android::auth::start_google_login` opens Android's Credential Manager
//! account-picker sheet — Google's own system UI, so the app still draws no
//! platform UI of its own — and the answer comes back as a
//! [`MobileEvent::GoogleLogin`](crate::MobileEvent::GoogleLogin).
//!
//! **Every path produces an outcome.** A cancelled sheet, a missing client ID,
//! a Play-services error and a malformed credential all arrive as
//! [`LoginOutcome::Failed`], so an app waiting on the sheet can always leave
//! that state.

/// What the Credential Manager sheet produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginOutcome {
    /// A Google ID token, for the app's backend to verify.
    IdToken(String),
    /// Cancelled, misconfigured, or refused; the message is for logs.
    Failed(String),
}

impl LoginOutcome {
    /// Map the Kotlin callback's `(idToken?, error?)` pair. Exactly one should
    /// be present; neither is reported as a failure rather than dropped.
    pub fn from_parts(id_token: Option<String>, error: Option<String>) -> Self {
        match (id_token, error) {
            (Some(token), _) => LoginOutcome::IdToken(token),
            (None, Some(error)) => LoginOutcome::Failed(error),
            (None, None) => LoginOutcome::Failed(
                "credential manager returned neither a token nor an error".into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_wins() {
        assert_eq!(
            LoginOutcome::from_parts(Some("t".into()), None),
            LoginOutcome::IdToken("t".into())
        );
    }

    #[test]
    fn nothing_is_a_failure() {
        assert!(matches!(
            LoginOutcome::from_parts(None, None),
            LoginOutcome::Failed(_)
        ));
    }
}
