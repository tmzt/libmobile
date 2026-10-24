//! **The device's manufacturer, brand and model**, from `android.os.Build`.
//!
//! Platform-free: the type builds on any host, and
//! [`android::build_info`](crate::android::build_info) fills it on Android.
//!
//! Only these three public `Build` constants, which need no permission.
//! libmobile never reads the serial (`Build.getSerial()`), `ANDROID_ID`,
//! the IMEI, the advertising ID, the build fingerprint, or anything else that
//! identifies the unit or the person who owns it.

/// The device's manufacturer, brand and model. A value Android does not give
/// is an empty string.
///
/// Nothing here identifies the unit or its owner: many devices share each
/// value. libmobile reads no other `Build` field and no device identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuildInfo {
    /// `Build.MANUFACTURER` ("samsung").
    pub manufacturer: String,
    /// `Build.BRAND` ("samsung").
    pub brand: String,
    /// `Build.MODEL` ("SM-S918U").
    pub model: String,
}

impl BuildInfo {
    /// From the plugin's values; a missing one is empty.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // Built by the Android bridge.
    pub(crate) fn new(
        manufacturer: Option<String>,
        brand: Option<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            manufacturer: manufacturer.unwrap_or_default(),
            brand: brand.unwrap_or_default(),
            model: model.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BuildInfo;

    #[test]
    fn missing_values_are_empty() {
        let info = BuildInfo::new(Some("samsung".into()), None, Some("SM-S918U".into()));
        assert_eq!(info.manufacturer, "samsung");
        assert_eq!(info.brand, "");
        assert_eq!(info.model, "SM-S918U");
    }
}
