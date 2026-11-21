//! Camera mirror: frames from the first webcam through a Media Foundation source reader.
//!
//! The camera runs only while the mirror is on screen. A worker thread reads frames
//! (blocking `ReadSample`), converts them to mirrored, opaque BGRA at most 640 pixels wide,
//! and leaves the newest one for the UI thread. Stopping sets a flag; the thread notices on
//! its next frame and releases the camera (its light goes off). The UI never waits for it.

use std::{
    sync::mpsc::{self, RecvTimeoutError, Sender},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use windows::{
    Win32::{
        Foundation::E_ACCESSDENIED,
        Media::MediaFoundation::{
            IMF2DBuffer, IMFActivate, IMFAttributes, IMFMediaEvent, IMFMediaSource, IMFSample,
            IMFSourceReader, IMFSourceReaderCallback, IMFSourceReaderCallback_Impl,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ASYNC_CALLBACK,
            MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
            MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR, MF_VERSION, MFCreateAttributes,
            MFCreateMediaType, MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources,
            MFMediaType_Video, MFSTARTUP_FULL, MFShutdown, MFStartup, MFVideoFormat_RGB32,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
    },
    core::{HRESULT, Interface, Ref, implement},
};

use super::window::Waker;

/// Widest frame kept; larger cameras are halved until they fit.
const MAX_WIDTH: u32 = 640;

/// A mirrored, opaque BGRA frame.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// What the mirror shows while there is no picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Starting,
    Live,
    /// A message for the user (no camera, access blocked, camera busy).
    Failed(String),
}

struct Shared {
    frame: Option<Frame>,
    status: Status,
}

pub struct Camera {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
}

impl Camera {
    pub fn start(waker: Waker) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared {
            frame: None,
            status: Status::Starting,
        }));
        let (stop2, shared2) = (stop.clone(), shared.clone());
        let spawned = std::thread::Builder::new()
            .name("camera".into())
            .spawn(move || {
                let result = run(&stop2, &shared2, waker);
                if let Err(e) = result {
                    tracing::warn!("camera: {e:#}");
                    let text = explain(&e);
                    if let Ok(mut s) = shared2.lock() {
                        s.status = Status::Failed(text);
                    }
                    waker.wake();
                }
                tracing::info!("camera released");
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start the camera thread: {e}");
            if let Ok(mut s) = shared.lock() {
                s.status = Status::Failed("Camera could not start".into());
            }
        }
        Self { stop, shared }
    }

    /// The newest frame since the last call, if any.
    pub fn take_frame(&self) -> Option<Frame> {
        self.shared.lock().ok()?.frame.take()
    }

    pub fn status(&self) -> Status {
        self.shared
            .lock()
            .map(|s| s.status.clone())
            .unwrap_or(Status::Failed("Camera stopped".into()))
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A short, human message for a failure.
fn explain(e: &anyhow::Error) -> String {
    let access_denied = e
        .chain()
        .filter_map(|c| c.downcast_ref::<windows::core::Error>())
        .any(|w| w.code() == E_ACCESSDENIED);
    if access_denied {
        "Camera access is off for desktop apps (Windows privacy settings)".into()
    } else if e.to_string().contains("no camera") {
        "No camera found".into()
    } else if e.to_string().contains("no picture") {
        "The camera sends no picture. Is its switch or shutter closed?".into()
    } else {
        "Camera is busy or could not start".into()
    }
}

/// Media Foundation and COM for the life of the thread.
struct MfSession;

impl MfSession {
    fn start() -> Result<Self> {
        // SAFETY: initialises COM for this worker thread only; balanced in Drop.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .context("COM")?;
        // SAFETY: plain call; balanced by MFShutdown in Drop.
        if let Err(e) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            // SAFETY: balances the CoInitializeEx above.
            unsafe { CoUninitialize() };
            return Err(e).context("Media Foundation");
        }
        Ok(Self)
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        // SAFETY: balances MFStartup and CoInitializeEx from `start`, on the same thread.
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

fn first_camera() -> Result<IMFMediaSource> {
    let mut attrs: Option<IMFAttributes> = None;
    // SAFETY: out-pointer to a local Option.
    unsafe { MFCreateAttributes(&mut attrs, 1) }?;
    let attrs = attrs.ok_or_else(|| anyhow!("no attributes"))?;
    // SAFETY: setting a GUID attribute on a live attribute store.
    unsafe {
        attrs.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )
    }?;
    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: MFEnumDeviceSources allocates an array of `count` activation objects with
    // CoTaskMemAlloc. Each element is moved out exactly once (so each is released once) and
    // the array itself is freed with CoTaskMemFree.
    let activates: Vec<IMFActivate> = unsafe {
        MFEnumDeviceSources(&attrs, &mut list, &mut count)?;
        let mut out = Vec::new();
        if !list.is_null() {
            for i in 0..count as usize {
                if let Some(a) = std::ptr::replace(list.add(i), None) {
                    out.push(a);
                }
            }
            CoTaskMemFree(Some(list as *const _));
        }
        out
    };
    let first = activates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no camera"))?;
    // SAFETY: activating a device returned by the enumeration.
    let source: IMFMediaSource = unsafe { first.ActivateObject() }.context("opening the camera")?;
    Ok(source)
}

/// Messages from the reader callback (a Media Foundation thread) to the camera thread.
enum Msg {
    Frame(Frame),
    /// A read finished without a picture (a stream tick); ask for the next one.
    Empty,
    Failed(String),
}

/// Receives finished reads. Converting happens here, on Media Foundation's thread, so only
/// plain pixels cross over to the camera thread.
#[implement(IMFSourceReaderCallback)]
struct OnRead {
    tx: Mutex<Sender<Msg>>,
    /// Frame width << 32 | height, set once the format is agreed.
    size: Arc<AtomicU64>,
}

impl OnRead {
    fn send(&self, msg: Msg) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(msg);
        }
    }
}

impl IMFSourceReaderCallback_Impl for OnRead_Impl {
    fn OnReadSample(
        &self,
        status: HRESULT,
        _stream: u32,
        flags: u32,
        _time: i64,
        sample: Ref<'_, IMFSample>,
    ) -> windows::core::Result<()> {
        if status.is_err() {
            self.send(Msg::Failed(format!("camera read failed: {status}")));
            return Ok(());
        }
        if flags & (MF_SOURCE_READERF_ERROR.0 as u32 | MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0
        {
            self.send(Msg::Failed("the camera stopped sending pictures".into()));
            return Ok(());
        }
        let size = self.size.load(Ordering::Relaxed);
        let (w, h) = ((size >> 32) as u32, (size & 0xFFFF_FFFF) as u32);
        let msg = match sample.as_ref() {
            Some(s) if w > 0 && h > 0 => match convert(s, w, h) {
                Ok(frame) => Msg::Frame(frame),
                Err(e) => Msg::Failed(format!("{e:#}")),
            },
            _ => Msg::Empty,
        };
        self.send(msg);
        Ok(())
    }

    fn OnFlush(&self, _stream: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnEvent(&self, _stream: u32, _event: Ref<'_, IMFMediaEvent>) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Open an asynchronous reader that delivers RGB32 to `callback`. Returns it with the frame size.
fn open_reader(
    source: &IMFMediaSource,
    callback: &IMFSourceReaderCallback,
) -> Result<(IMFSourceReader, u32, u32)> {
    let mut attrs: Option<IMFAttributes> = None;
    // SAFETY: out-pointer to a local Option.
    unsafe { MFCreateAttributes(&mut attrs, 2) }?;
    let attrs = attrs.ok_or_else(|| anyhow!("no attributes"))?;
    // SAFETY: setting values on a live attribute store; the store keeps a reference to the
    // callback for as long as the reader needs it.
    unsafe {
        // Let the reader convert whatever the camera delivers (usually NV12 or MJPEG) to RGB32.
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        attrs.SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, callback)?;
    }
    // SAFETY: both interfaces are live; the reader keeps its own references.
    let reader = unsafe { MFCreateSourceReaderFromMediaSource(source, &attrs) }?;
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    // SAFETY: a fresh media type, configured and handed to the reader.
    unsafe {
        let wanted = MFCreateMediaType()?;
        wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader
            .SetCurrentMediaType(stream, None, &wanted)
            .context("the camera cannot deliver RGB frames")?;
    }
    // SAFETY: reading back the negotiated type from the live reader.
    let size = unsafe {
        reader
            .GetCurrentMediaType(stream)?
            .GetUINT64(&MF_MT_FRAME_SIZE)?
    };
    let (w, h) = ((size >> 32) as u32, (size & 0xFFFF_FFFF) as u32);
    if w == 0 || h == 0 {
        return Err(anyhow!("camera reported an empty frame size"));
    }
    Ok((reader, w, h))
}

/// How long the camera may take to deliver its first (or next) picture before the mirror
/// says so and lets go of it. Some cameras open fine but never stream while their privacy
/// switch or shutter is closed.
const NO_PICTURE: Duration = Duration::from_secs(6);

fn run(stop: &AtomicBool, shared: &Mutex<Shared>, waker: Waker) -> Result<()> {
    let _session = MfSession::start()?;
    let source = first_camera()?;
    let (tx, rx) = mpsc::channel();
    let size = Arc::new(AtomicU64::new(0));
    let callback: IMFSourceReaderCallback = OnRead {
        tx: Mutex::new(tx),
        size: size.clone(),
    }
    .into();
    let result = (|| -> Result<()> {
        let (reader, w, h) = open_reader(&source, &callback)?;
        size.store((u64::from(w) << 32) | u64::from(h), Ordering::Relaxed);
        tracing::info!("camera open: {w}x{h}");
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        // SAFETY: an asynchronous read: no out-pointers; the result goes to the callback.
        let next = || unsafe { reader.ReadSample(stream, 0, None, None, None, None) };
        next()?;
        let mut last_picture = Instant::now();
        let mut first = true;
        while !stop.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(150)) {
                Ok(Msg::Frame(frame)) => {
                    last_picture = Instant::now();
                    if first {
                        tracing::info!("camera streaming");
                    }
                    if let Ok(mut s) = shared.lock() {
                        s.status = Status::Live;
                        let was_taken = s.frame.is_none();
                        s.frame = Some(frame);
                        // Wake the UI only when it has taken the previous frame (or on the
                        // first one), so a busy UI is never flooded with messages.
                        if was_taken || first {
                            waker.wake();
                        }
                    }
                    first = false;
                    next()?;
                }
                Ok(Msg::Empty) => next()?,
                Ok(Msg::Failed(e)) => return Err(anyhow!(e)),
                Err(RecvTimeoutError::Timeout) => {
                    if last_picture.elapsed() > NO_PICTURE {
                        return Err(anyhow!("the camera sends no picture"));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Err(anyhow!("camera reader gone")),
            }
        }
        Ok(())
    })();
    // SAFETY: shuts the device down, which also ends any read still pending, so the camera
    // light goes off right away.
    let _ = unsafe { source.Shutdown() };
    result
}
/// Copy a sample out as mirrored, opaque BGRA, halving the size until it is at most
/// `MAX_WIDTH` wide.
fn convert(sample: &IMFSample, w: u32, h: u32) -> Result<Frame> {
    let mut step = 1u32;
    while w / step > MAX_WIDTH {
        step *= 2;
    }
    let (ow, oh) = (w / step, h / step);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    // SAFETY: the buffer is locked for the duration of the copy and unlocked after. Rows are
    // read through the pitch the buffer reports (negative for bottom-up images), never past
    // `h` rows of `w` 4-byte pixels, which is the size of an RGB32 frame of the type we set.
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer()?;
        if let Ok(b2) = buffer.cast::<IMF2DBuffer>() {
            let mut scan0: *mut u8 = std::ptr::null_mut();
            let mut pitch = 0i32;
            b2.Lock2D(&mut scan0, &mut pitch)?;
            copy_rows(scan0, pitch as isize, w, h, step, &mut out, ow, oh);
            let _ = b2.Unlock2D();
        } else {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut data, None, Some(&mut len))?;
            if (len as u64) < u64::from(w) * u64::from(h) * 4 {
                let _ = buffer.Unlock();
                return Err(anyhow!("short camera frame"));
            }
            // A plain RGB32 buffer is stored bottom-up, like a DIB.
            let last = data.add(((h - 1) * w * 4) as usize);
            copy_rows(last, -((w * 4) as isize), w, h, step, &mut out, ow, oh);
            let _ = buffer.Unlock();
        }
    }
    Ok(Frame {
        width: ow,
        height: oh,
        bgra: out,
    })
}

/// # Safety
/// `scan0` must point at the first (top) row of an image of `h` rows of `w` 4-byte pixels,
/// with `pitch` bytes from one row to the next.
#[allow(clippy::too_many_arguments)]
unsafe fn copy_rows(
    scan0: *const u8,
    pitch: isize,
    w: u32,
    h: u32,
    step: u32,
    out: &mut [u8],
    ow: u32,
    oh: u32,
) {
    for y in 0..oh {
        let sy = (y * step).min(h - 1);
        // SAFETY: sy < h, so the row lies inside the locked image (caller's contract).
        let row = unsafe {
            std::slice::from_raw_parts(scan0.offset(sy as isize * pitch), (w * 4) as usize)
        };
        let dst = &mut out[(y * ow * 4) as usize..((y + 1) * ow * 4) as usize];
        for x in 0..ow {
            // Mirror: the left of the picture comes from the right of the camera image.
            let sx = ((ow - 1 - x) * step).min(w - 1) as usize * 4;
            let d = x as usize * 4;
            dst[d] = row[sx];
            dst[d + 1] = row[sx + 1];
            dst[d + 2] = row[sx + 2];
            dst[d + 3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::copy_rows;

    #[test]
    fn frames_are_mirrored_opaque_and_halved() {
        // 4x2 image, pixel value = column number in every channel, alpha 0.
        let mut src = Vec::new();
        for _ in 0..2 {
            for x in 0..4u8 {
                src.extend_from_slice(&[x, x, x, 0]);
            }
        }
        let mut out = vec![0u8; 4 * 2 * 4];
        // SAFETY: src holds 2 rows of 4 four-byte pixels, top row first.
        unsafe { copy_rows(src.as_ptr(), 16, 4, 2, 1, &mut out, 4, 2) };
        let first_row: Vec<u8> = out[..16].chunks(4).map(|p| p[0]).collect();
        assert_eq!(first_row, [3, 2, 1, 0], "left and right are swapped");
        assert!(out.chunks(4).all(|p| p[3] == 255), "alpha is opaque");
        let mut half = vec![0u8; 2 * 4];
        // Bottom-up: start at the last row with a negative pitch.
        // SAFETY: as above; the pointer is the start of the last row.
        unsafe { copy_rows(src.as_ptr().add(16), -16, 4, 2, 2, &mut half, 2, 1) };
        assert_eq!([half[0], half[4]], [2, 0]);
    }
}
