//! The notification content as JNI arguments, shared by the foreground
//! service (`service`) and alerts (`notify`).

use super::jni_util::string_array;
use crate::notification::NotificationContent;
use jni::Env;
use jni::objects::{JObjectArray, JString};
use jni::sys::jint;

/// The notification content as JNI arguments.
pub(crate) struct ContentArgs<'local> {
    pub(crate) title: JString<'local>,
    pub(crate) text: JString<'local>,
    pub(crate) ids: JObjectArray<'local, JString<'local>>,
    pub(crate) labels: JObjectArray<'local, JString<'local>>,
    /// Bit `i`: action `i` opens the app.
    pub(crate) opens: jint,
    pub(crate) visibility: jint,
    /// `null` for no tag.
    pub(crate) tag: JString<'local>,
}

impl<'local> ContentArgs<'local> {
    pub(crate) fn new(
        env: &mut Env<'local>,
        content: &NotificationContent,
    ) -> jni::errors::Result<Self> {
        let ids: Vec<&str> = content.actions.iter().map(|a| a.id.as_str()).collect();
        let labels: Vec<&str> = content.actions.iter().map(|a| a.label.as_str()).collect();
        let opens = content
            .actions
            .iter()
            .enumerate()
            .fold(0, |bits, (i, a)| bits | (jint::from(a.opens_app) << i));
        Ok(Self {
            title: JString::from_str(env, &content.title)?,
            text: JString::from_str(env, &content.text)?,
            ids: string_array(env, &ids)?,
            labels: string_array(env, &labels)?,
            opens,
            visibility: content.visibility.code(),
            tag: match &content.tag {
                Some(tag) => JString::from_str(env, tag)?,
                None => JString::null(),
            },
        })
    }
}
