//! Says when the internet connection is lost or back. Windows reports each change
//! (`NetworkStatusChanged`) and knows itself whether there is internet; nothing polls and
//! Pillbox sends nothing to find out.

use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    time::Duration,
};

use anyhow::{Context, Result};
use windows::{
    Networking::Connectivity::{
        NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler,
    },
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx},
};

use super::window::Waker;

/// A change counts when it has held this long: switching networks or waking up drops the
/// connection for a moment.
const SETTLE: Duration = Duration::from_secs(4);

enum Msg {
    Changed,
    Stop,
}

/// Sends whether there is internet each time that changes from now on.
pub struct InternetWatch {
    rx: Receiver<bool>,
    stop: Sender<Msg>,
}

impl InternetWatch {
    pub fn start(waker: Waker) -> Option<Self> {
        let (stop, inbox) = mpsc::channel();
        let (out, rx) = mpsc::channel();
        let tx = stop.clone();
        std::thread::Builder::new()
            .name("internet".into())
            .spawn(move || {
                if let Err(e) = watch(tx, &inbox, &out, waker) {
                    tracing::warn!("internet notices are unavailable: {e:#}");
                }
            })
            .map_err(|e| tracing::warn!("could not start the internet watcher: {e}"))
            .ok()?;
        Some(Self { rx, stop })
    }

    /// The newest change since the last call.
    pub fn poll(&self) -> Option<bool> {
        self.rx.try_iter().last()
    }
}

impl Drop for InternetWatch {
    fn drop(&mut self) {
        // Not joined: the thread stops listening and ends by itself.
        let _ = self.stop.send(Msg::Stop);
    }
}

fn online() -> bool {
    NetworkInformation::GetInternetConnectionProfile()
        .and_then(|p| p.GetNetworkConnectivityLevel())
        .is_ok_and(|level| level == NetworkConnectivityLevel::InternetAccess)
}

fn watch(tx: Sender<Msg>, inbox: &Receiver<Msg>, out: &Sender<bool>, waker: Waker) -> Result<()> {
    // SAFETY: initialises COM for this thread, which makes plain WinRT calls only.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .context("initialising COM")?;
    let token = NetworkInformation::NetworkStatusChanged(&NetworkStatusChangedEventHandler::new(
        move |_| {
            let _ = tx.send(Msg::Changed);
            Ok(())
        },
    ))
    .context("listening for network changes")?;
    let mut was = online();
    tracing::debug!("internet at the start: {was}");
    'run: while let Ok(Msg::Changed) = inbox.recv() {
        loop {
            match inbox.recv_timeout(SETTLE) {
                Ok(Msg::Changed) => {}
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break 'run,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        let now = online();
        if now != was {
            was = now;
            if out.send(now).is_err() {
                break;
            }
            waker.wake();
        }
    }
    let _ = NetworkInformation::RemoveNetworkStatusChanged(token);
    Ok(())
}
