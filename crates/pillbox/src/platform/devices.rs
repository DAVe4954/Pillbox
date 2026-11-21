//! Speaker volume and microphone mute at the Windows endpoint level (`IAudioEndpointVolume`),
//! so they apply to every app, the same switches the Windows sound settings flip.
//!
//! Runs on the UI thread, which is a COM single-threaded apartment (see `system::init_com`).

use anyhow::{Context, Result};
use windows::Win32::{
    Media::Audio::{
        DEVICE_STATE_ACTIVE, Endpoints::IAudioEndpointVolume, IMMDevice, IMMDeviceEnumerator,
        MMDeviceEnumerator, eCapture, eConsole, eRender,
    },
    System::Com::{CLSCTX_ALL, CoCreateInstance},
};

fn enumerator() -> Result<IMMDeviceEnumerator> {
    // SAFETY: plain COM creation on a thread with COM initialised; the result is owned.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .context("creating the audio device enumerator")
}

fn volume_of(device: &IMMDevice) -> Result<IAudioEndpointVolume> {
    // SAFETY: Activate with the interface's own IID (inferred) and no activation parameters.
    unsafe { device.Activate(CLSCTX_ALL, None) }.context("opening the endpoint volume")
}

fn default_volume(capture: bool) -> Result<IAudioEndpointVolume> {
    let flow = if capture { eCapture } else { eRender };
    // SAFETY: valid enum arguments; the device is owned.
    let device = unsafe { enumerator()?.GetDefaultAudioEndpoint(flow, eConsole) }
        .context("no default audio device")?;
    volume_of(&device)
}

/// Whether the default microphone is muted. `None` when there is no microphone.
pub fn mic_muted() -> Option<bool> {
    let v = default_volume(true).ok()?;
    // SAFETY: plain getter on a live interface.
    unsafe { v.GetMute() }.ok().map(|b| b.as_bool())
}

/// Mute or unmute every active microphone, so switching headsets cannot leave one open.
/// Returns how many were changed.
pub fn set_mic_muted(muted: bool) -> Result<u32> {
    let e = enumerator()?;
    // SAFETY: valid enum arguments; the collection and devices are owned interfaces; the
    // event-context GUID pointer may be null.
    unsafe {
        let all = e
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .context("listing microphones")?;
        let mut changed = 0;
        for i in 0..all.GetCount()? {
            let Ok(device) = all.Item(i) else { continue };
            if let Ok(v) = volume_of(&device)
                && v.SetMute(muted, std::ptr::null()).is_ok()
            {
                changed += 1;
            }
        }
        Ok(changed)
    }
}

/// Speaker volume 0..1 and mute state of the default output device.
pub fn speaker() -> Option<(f32, bool)> {
    let v = default_volume(false).ok()?;
    // SAFETY: plain getters on a live interface.
    unsafe {
        Some((
            v.GetMasterVolumeLevelScalar().ok()?,
            v.GetMute().ok()?.as_bool(),
        ))
    }
}

pub fn set_speaker_volume(level: f32) -> Result<()> {
    let v = default_volume(false)?;
    // SAFETY: level is clamped to the documented 0..1 range; null event context is allowed.
    unsafe { v.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null()) }
        .context("setting the volume")
}

pub fn set_speaker_muted(muted: bool) -> Result<()> {
    let v = default_volume(false)?;
    // SAFETY: null event context is allowed.
    unsafe { v.SetMute(muted, std::ptr::null()) }.context("muting the speakers")
}
