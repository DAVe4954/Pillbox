//! Camera mirror on the Controls tab. The camera runs only while the mirror is on screen:
//! the card open on Controls, Pillbox visible and not in a game mode. Closing the card
//! also turns the mirror off, so opening Controls later never switches the camera on by
//! surprise.

use super::{App, Mode};
use crate::{
    platform::{
        camera::{Camera, Status},
        gfx::ImageSlot,
    },
    scene::Tab,
};

#[derive(Default)]
pub(super) struct Mirror {
    /// The user turned the mirror on.
    pub on: bool,
    camera: Option<Camera>,
    /// A frame has been uploaded.
    pub live: bool,
    pub message: String,
}

impl App {
    pub(super) fn toggle_mirror(&mut self) {
        self.mirror.on = !self.mirror.on;
        tracing::info!(
            "camera mirror {}",
            if self.mirror.on { "on" } else { "off" }
        );
        self.sync_mirror();
        self.dirty = true;
    }

    /// Start or stop the camera to match what is on screen.
    pub(super) fn sync_mirror(&mut self) {
        if !self.open {
            self.mirror.on = false;
        }
        let want = self.mirror.on
            && self.open
            && self.tab == Tab::Controls
            && self.mode == Mode::Normal
            && self.effective_visible();
        if want && self.mirror.camera.is_none() {
            self.mirror.camera = Some(Camera::start(self.window.waker()));
            self.mirror.live = false;
            self.mirror.message = "Starting the camera…".into();
        } else if !want && self.mirror.camera.is_some() {
            // Dropping the handle tells the camera thread to stop and release the device.
            self.mirror.camera = None;
            self.mirror.live = false;
            self.renderer.clear_image(ImageSlot::Camera);
        }
    }

    /// Take the newest camera frame (called when a worker woke the UI thread).
    pub(super) fn poll_mirror(&mut self) {
        let Some(camera) = &self.mirror.camera else {
            return;
        };
        match camera.status() {
            Status::Starting => {}
            Status::Live => {}
            Status::Failed(text) => {
                if self.mirror.message != text || self.mirror.live {
                    self.mirror.message = text;
                    self.mirror.live = false;
                    self.dirty = true;
                }
                return;
            }
        }
        if let Some(frame) = camera.take_frame() {
            self.renderer
                .set_image(ImageSlot::Camera, &frame.bgra, frame.width, frame.height);
            self.mirror.live = true;
            self.dirty = true;
        }
    }
}
