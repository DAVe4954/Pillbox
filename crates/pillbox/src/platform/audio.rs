//! The visualizer's ears: a loopback capture of whatever the default speakers are playing,
//! reduced to six band levels. Runs on its own thread and only while music is playing.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use pillbox_core::music::{BANDS, FFT_SIZE, band_levels, normalize_bands};
use windows::Win32::{
    Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient, IAudioClient,
        IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole,
        eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    },
};

/// Bar heights 0..1, written by the audio thread and read by the UI thread.
#[derive(Default)]
struct Shared {
    bands: [AtomicU32; BANDS],
    stop: AtomicBool,
}

pub struct Visualizer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Visualizer {
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        let for_thread = shared.clone();
        let thread = std::thread::Builder::new()
            .name("visualizer".into())
            .spawn(move || {
                // The default output device can change or vanish (headphones unplugged);
                // start over after a pause whenever capture fails.
                while !for_thread.stop.load(Ordering::Relaxed) {
                    if let Err(e) = capture(&for_thread) {
                        tracing::debug!("visualizer capture stopped: {e:#}");
                    }
                    for b in &for_thread.bands {
                        b.store(0, Ordering::Relaxed);
                    }
                    for _ in 0..20 {
                        if for_thread.stop.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .map_err(|e| tracing::warn!("could not start the visualizer thread: {e}"))
            .ok();
        Self { shared, thread }
    }

    /// Current bar heights, 0..1.
    pub fn bands(&self) -> [f32; BANDS] {
        std::array::from_fn(|i| f32::from_bits(self.shared.bands[i].load(Ordering::Relaxed)))
    }
}

impl Drop for Visualizer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// How the device delivers samples.
#[derive(Clone, Copy)]
enum SampleKind {
    F32,
    I16,
}

/// Frees the mix-format block the audio client allocated.
struct MixFormat(*mut WAVEFORMATEX);

impl Drop for MixFormat {
    fn drop(&mut self) {
        // SAFETY: the pointer came from IAudioClient::GetMixFormat, which allocates with
        // CoTaskMemAlloc; it is freed exactly once here.
        unsafe { CoTaskMemFree(Some(self.0.cast())) };
    }
}

/// Capture until asked to stop or until the device fails.
fn capture(shared: &Shared) -> Result<()> {
    // SAFETY: COM is initialised for this thread before use (a repeat call is harmless). All
    // interface pointers are owned by windows-rs smart pointers. The format pointer is valid
    // until MixFormat drops. Packet buffers are read only between GetBuffer and ReleaseBuffer
    // and only for `frames * block_align` bytes, as the API guarantees.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .context("creating the device enumerator")?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .context("no default speakers")?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let format = MixFormat(client.GetMixFormat()?);
        let f = *format.0;
        let channels = usize::from(f.nChannels).max(1);
        let block = usize::from(f.nBlockAlign);
        let rate = f.nSamplesPerSec as f32;
        const TAG_PCM: u16 = 1;
        const TAG_FLOAT: u16 = 3;
        const TAG_EXTENSIBLE: u16 = 0xFFFE;
        let tag = if f.wFormatTag == TAG_EXTENSIBLE && usize::from(f.cbSize) >= 22 {
            // The sub-format GUID starts with the classic format tag.
            (*(format.0 as *const WAVEFORMATEXTENSIBLE)).SubFormat.data1 as u16
        } else {
            f.wFormatTag
        };
        let kind = match (tag, f.wBitsPerSample) {
            (TAG_FLOAT, 32) => SampleKind::F32,
            (TAG_PCM, 16) => SampleKind::I16,
            other => return Err(anyhow!("unsupported mix format {other:?}")),
        };
        if block != channels * usize::from(f.wBitsPerSample / 8) {
            return Err(anyhow!("unexpected block size"));
        }
        // 100 ms buffer, in 100-nanosecond units.
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                1_000_000,
                0,
                format.0,
                None,
            )
            .context("starting loopback capture")?;
        let capture: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        let mut window: Vec<f32> = Vec::with_capacity(FFT_SIZE * 2);
        let mut peak = 0.0f32;
        let mut smooth = [0.0f32; BANDS];
        while !shared.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20));
            let mut got_audio = false;
            while capture.GetNextPacketSize()? > 0 {
                let mut data = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                const SILENT: u32 = 0x2; // AUDCLNT_BUFFERFLAGS_SILENT
                let n = frames as usize;
                if flags & SILENT != 0 || data.is_null() {
                    window.extend(std::iter::repeat_n(0.0, n));
                } else {
                    let bytes = std::slice::from_raw_parts(data, n * block);
                    for frame in bytes.chunks_exact(block) {
                        // Mix the channels down to mono.
                        let sum: f32 = match kind {
                            SampleKind::F32 => frame
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|s| f32::from_ne_bytes([s[0], s[1], s[2], s[3]]))
                                .sum(),
                            SampleKind::I16 => frame
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|s| f32::from(i16::from_ne_bytes([s[0], s[1]])) / 32768.0)
                                .sum(),
                        };
                        window.push(sum / channels as f32);
                    }
                    got_audio = true;
                }
                capture.ReleaseBuffer(frames)?;
            }
            // Keep only the newest window's worth of samples.
            if window.len() > FFT_SIZE {
                window.drain(..window.len() - FFT_SIZE);
            }
            let target = if got_audio {
                normalize_bands(band_levels(&window, rate), &mut peak)
            } else {
                [0.0; BANDS]
            };
            for (i, (s, t)) in smooth.iter_mut().zip(target).enumerate() {
                // Rise quickly, fall gently: bars that snap up and sink back.
                *s = if t > *s {
                    *s + (t - *s) * 0.6
                } else {
                    *s + (t - *s) * 0.22
                };
                shared.bands[i].store(s.to_bits(), Ordering::Relaxed);
            }
        }
        let _ = client.Stop();
    }
    Ok(())
}
