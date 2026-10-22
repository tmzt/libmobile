//! A capture stream on AAudio (feature `cpal`), for the input preset cpal
//! cannot set. cpal stays the way to play audio.
//!
//! The app's callback lives in an `Arc<Mutex<_>>` that only AAudio's
//! callback locks while a stream is open. Closing a stream waits for its
//! callback to return, so [`CaptureStream::set_preset`] can hand the same
//! callback to the next stream.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ndk::audio::{
    AudioCallbackResult, AudioDirection, AudioFormat, AudioInputPreset, AudioStream,
    AudioStreamBuilder,
};

use crate::audio_capture::{CaptureError, CaptureFormat, InputPreset};

type Callback = Box<dyn FnMut(&[f32], Duration) + Send>;

/// What AAudio's callback owns while a stream is open.
struct State {
    callback: Callback,
    channels: usize,
    /// The callback's clock starts here, at `open`.
    start: Instant,
}

/// The default microphone as interleaved `f32`, with an input preset.
/// Needs `RECORD_AUDIO` declared and granted.
pub struct CaptureStream {
    stream: RefCell<Option<AudioStream>>,
    state: Arc<Mutex<State>>,
    format: CaptureFormat,
    preset: Cell<InputPreset>,
    started: Cell<bool>,
}

impl CaptureStream {
    /// Open the default input with `preset` (`None`: AAudio chooses),
    /// stopped. The callback gets interleaved frames and when the first was
    /// captured, on a monotonic clock that starts here and keeps running
    /// across [`set_preset`](Self::set_preset) and [`stop`](Self::stop). It
    /// runs on AAudio's thread and must not block. A preset that fails to
    /// open falls back to `None`, with a warning.
    pub fn open(
        preset: Option<InputPreset>,
        callback: impl FnMut(&[f32], Duration) + Send + 'static,
    ) -> Result<Self, CaptureError> {
        let state = Arc::new(Mutex::new(State {
            callback: Box::new(callback),
            channels: 1,
            start: Instant::now(),
        }));
        let (stream, granted) = open_with_fallback(preset, &state, None)?;
        let format = CaptureFormat {
            sample_rate: u32::try_from(stream.sample_rate()).unwrap_or(0),
            channels: u16::try_from(stream.channel_count()).unwrap_or(1),
        };
        // No callback runs before `request_start`.
        state.lock().unwrap_or_else(|e| e.into_inner()).channels =
            usize::from(format.channels).max(1);
        log::info!("libmobile: capture: asked {preset:?}, got {granted:?}, {format:?}");
        Ok(Self {
            stream: RefCell::new(Some(stream)),
            state,
            format,
            preset: Cell::new(granted),
            started: Cell::new(false),
        })
    }

    pub fn format(&self) -> CaptureFormat {
        self.format
    }

    /// The preset AAudio reports (`VoiceRecognition` when `None` was asked).
    pub fn preset(&self) -> InputPreset {
        self.preset.get()
    }

    /// Close and reopen with `preset`, at the same format, with the same
    /// callback, started if it was started. Returns the preset granted. On
    /// failure the stream stays closed, and `start` reports it.
    pub fn set_preset(&self, preset: Option<InputPreset>) -> Result<InputPreset, CaptureError> {
        let mut slot = self.stream.borrow_mut();
        if let Some(stream) = slot.take() {
            let _ = stream.request_stop();
            // Closing waits for the callback, freeing the state.
            drop(stream);
        }
        let (stream, granted) = open_with_fallback(preset, &self.state, Some(self.format))?;
        if self.started.get() {
            stream.request_start().map_err(error)?;
        }
        log::info!("libmobile: capture: asked {preset:?}, got {granted:?}");
        self.preset.set(granted);
        *slot = Some(stream);
        Ok(granted)
    }

    pub fn start(&self) -> Result<(), CaptureError> {
        self.started.set(true);
        match &*self.stream.borrow() {
            Some(stream) => stream.request_start().map_err(error),
            None => Err(CaptureError("the capture stream is closed".into())),
        }
    }

    /// AAudio input streams have no pause: this stops, and
    /// [`start`](Self::start) resumes.
    pub fn stop(&self) -> Result<(), CaptureError> {
        self.started.set(false);
        match &*self.stream.borrow() {
            Some(stream) => stream.request_stop().map_err(error),
            None => Ok(()),
        }
    }
}

fn error(error: impl std::fmt::Debug) -> CaptureError {
    CaptureError(format!("{error:?}"))
}

/// Open with `preset`, or with `None` if that fails.
fn open_with_fallback(
    preset: Option<InputPreset>,
    state: &Arc<Mutex<State>>,
    format: Option<CaptureFormat>,
) -> Result<(AudioStream, InputPreset), CaptureError> {
    match build(preset, state, format) {
        Err(error) if preset.is_some() => {
            log::warn!("libmobile: capture: {preset:?} unavailable ({error}), using the default");
            build(None, state, format)
        }
        opened => opened,
    }
}

fn build(
    preset: Option<InputPreset>,
    state: &Arc<Mutex<State>>,
    format: Option<CaptureFormat>,
) -> Result<(AudioStream, InputPreset), CaptureError> {
    let mut builder = AudioStreamBuilder::new()
        .map_err(error)?
        .direction(AudioDirection::Input)
        .format(AudioFormat::PCM_Float);
    if let Some(preset) = preset {
        builder = builder.input_preset(AudioInputPreset::from(preset.to_raw()));
    }
    if let Some(format) = format {
        builder = builder
            .sample_rate(i32::try_from(format.sample_rate).unwrap_or(i32::MAX))
            .channel_count(i32::from(format.channels));
    }
    let callback_state = state.clone();
    let stream = builder
        .data_callback(Box::new(move |_stream, data, frames| {
            let mut state = callback_state.lock().unwrap_or_else(|e| e.into_inner());
            let state = &mut *state;
            let len = usize::try_from(frames).unwrap_or(0) * state.channels;
            // SAFETY: an input stream opened as PCM_Float hands its callback
            // `frames` interleaved frames of `channels` f32 samples at
            // `data`, valid for the length of the call.
            #[allow(unsafe_code)] // Scoped: one slice over AAudio's callback buffer.
            let samples = unsafe { std::slice::from_raw_parts(data.cast::<f32>(), len) };
            let time = state.start.elapsed();
            (state.callback)(samples, time);
            AudioCallbackResult::Continue
        }))
        .error_callback(Box::new(|_stream, error| {
            log::error!("libmobile: capture stream: {error:?}");
        }))
        .open_stream()
        .map_err(error)?;
    if stream.format() != AudioFormat::PCM_Float {
        return Err(CaptureError(format!(
            "got {:?}, not PCM_Float",
            stream.format()
        )));
    }
    let granted = InputPreset::from_raw(i32::from(stream.input_preset()))
        .unwrap_or(InputPreset::VoiceRecognition);
    Ok((stream, granted))
}
