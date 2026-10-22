//! **Audio capture types** (feature `cpal`), platform-free so wrappers can
//! map onto them on any host. The stream itself is
//! `android::CaptureStream`.

use std::fmt;

/// AAudio's input presets (`AAUDIO_INPUT_PRESET_*`): what the platform does
/// to the microphone signal before the app sees it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum InputPreset {
    /// The plain microphone; `dumpsys audio` reports it as `MIC`.
    Generic,
    Camcorder,
    /// AAudio's default.
    VoiceRecognition,
    /// Echo cancellation, noise suppression and gain control.
    VoiceCommunication,
    /// The microphone without processing, where the device offers it.
    Unprocessed,
    /// Low latency, for live monitoring (API 29).
    VoicePerformance,
}

/// `(preset, AAUDIO_INPUT_PRESET_* value)`, the one table both directions
/// use. Raw values, so `VoicePerformance` needs no ndk API-level feature
/// beyond the one presets need.
const RAW: [(InputPreset, i32); 6] = [
    (InputPreset::Generic, 1),
    (InputPreset::Camcorder, 5),
    (InputPreset::VoiceRecognition, 6),
    (InputPreset::VoiceCommunication, 7),
    (InputPreset::Unprocessed, 9),
    (InputPreset::VoicePerformance, 10),
];

impl InputPreset {
    /// The `AAUDIO_INPUT_PRESET_*` value.
    pub fn to_raw(self) -> i32 {
        RAW.iter()
            .find(|(preset, _)| *preset == self)
            .map_or(0, |(_, raw)| *raw)
    }

    /// The preset for an `AAUDIO_INPUT_PRESET_*` value, if it is one.
    pub fn from_raw(raw: i32) -> Option<Self> {
        RAW.iter()
            .find(|(_, value)| *value == raw)
            .map(|(preset, _)| *preset)
    }
}

/// Why a capture stream did not open or start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureError(pub String);

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CaptureError {}

/// A capture stream's format, chosen by AAudio when it opens. Samples are
/// always interleaved `f32`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

#[cfg(test)]
mod tests {
    use super::{InputPreset, RAW};

    #[test]
    fn raw_values_round_trip() {
        for (preset, raw) in RAW {
            assert_eq!(preset.to_raw(), raw);
            assert_eq!(InputPreset::from_raw(raw), Some(preset));
        }
        assert_eq!(InputPreset::VoiceCommunication.to_raw(), 7);
        assert_eq!(InputPreset::from_raw(0), None);
        assert_eq!(InputPreset::from_raw(8), None);
    }
}
