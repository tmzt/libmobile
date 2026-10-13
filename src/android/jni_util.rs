//! JNI helpers shared by the bridges.

use jni::objects::JString;

/// A nullable Java string argument as an `Option`.
pub(crate) fn opt_string(value: &JString<'_>) -> Option<String> {
    (!value.is_null()).then(|| value.to_string())
}
