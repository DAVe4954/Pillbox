//! Says when headphones are connected or disconnected: Bluetooth, USB or the jack. Windows
//! reports every change of an audio device (`IMMNotificationClient`); nothing polls.
//!
//! The reports arrive on one of Windows' own threads, which must not call back into the audio
//! devices, so a small thread of our own looks which headphones there are then and compares. It
//! sleeps in between.

use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    time::Duration,
};

use anyhow::{Context, Result};
use pillbox_core::notices;
use windows::{
    Win32::{
        Foundation::PROPERTYKEY,
        Media::Audio::{
            DEVICE_STATE, DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator,
            IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
            PKEY_AudioEndpoint_FormFactor, eRender,
        },
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STGM_READ,
        },
    },
    core::{GUID, PCWSTR, implement},
};

use super::window::Waker;

/// `PKEY_Device_FriendlyName`: "Headphones (WH-1000XM4)".
const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

/// Devices settle for this long before they are compared: one pair of headphones is several
/// outputs that come and go one after the other.
const SETTLE: Duration = Duration::from_millis(900);

enum Msg {
    /// Some audio device changed.
    Changed,
    Stop,
}

#[implement(IMMNotificationClient)]
struct Client {
    tx: Sender<Msg>,
}

impl IMMNotificationClient_Impl for Client_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        let _ = self.tx.send(Msg::Changed);
        Ok(())
    }

    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.tx.send(Msg::Changed);
        Ok(())
    }

    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.tx.send(Msg::Changed);
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        _: EDataFlow,
        _: ERole,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Sends the name of each pair of headphones that comes (`true`) or goes (`false`) from now on.
pub struct HeadphoneWatch {
    rx: Receiver<(String, bool)>,
    stop: Sender<Msg>,
}

impl HeadphoneWatch {
    pub fn start(waker: Waker) -> Option<Self> {
        let (stop, inbox) = mpsc::channel();
        let (out, rx) = mpsc::channel();
        let tx = stop.clone();
        std::thread::Builder::new()
            .name("headphones".into())
            .spawn(move || {
                if let Err(e) = watch(tx, &inbox, &out, waker) {
                    tracing::warn!("headphone notices are unavailable: {e:#}");
                }
            })
            .map_err(|e| tracing::warn!("could not start the headphone watcher: {e}"))
            .ok()?;
        Some(Self { rx, stop })
    }

    /// The newest change since the last call.
    pub fn poll(&self) -> Option<(String, bool)> {
        self.rx.try_iter().last()
    }
}

impl Drop for HeadphoneWatch {
    fn drop(&mut self) {
        // Not joined: the thread unregisters and ends by itself.
        let _ = self.stop.send(Msg::Stop);
    }
}

fn watch(
    tx: Sender<Msg>,
    inbox: &Receiver<Msg>,
    out: &Sender<(String, bool)>,
    waker: Waker,
) -> Result<()> {
    // SAFETY: initialises COM for this thread, which makes plain COM calls only.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .context("initialising COM")?;
    // SAFETY: plain COM creation on a thread with COM initialised; the result is owned.
    let devices: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .context("creating the audio device enumerator")?;
    let client: IMMNotificationClient = Client { tx }.into();
    // SAFETY: `client` outlives the registration: it is unregistered below, before the drop.
    unsafe { devices.RegisterEndpointNotificationCallback(&client) }
        .context("listening for audio devices")?;
    let mut known = connected(&devices);
    tracing::debug!("headphones at the start: {known:?}");
    'run: while let Ok(Msg::Changed) = inbox.recv() {
        // Wait until nothing has changed for a moment.
        loop {
            match inbox.recv_timeout(SETTLE) {
                Ok(Msg::Changed) => {}
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break 'run,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        let now = connected(&devices);
        for change in notices::device_changes(&known, &now) {
            if out.send(change).is_err() {
                break 'run;
            }
            waker.wake();
        }
        known = now;
    }
    // SAFETY: registered above with the same client.
    let _ = unsafe { devices.UnregisterEndpointNotificationCallback(&client) };
    Ok(())
}

/// The names of all headphones that can be used now.
fn connected(devices: &IMMDeviceEnumerator) -> Vec<String> {
    // SAFETY: valid enum arguments; the collection and the devices are owned interfaces.
    let list = || -> windows::core::Result<Vec<IMMDevice>> {
        unsafe {
            let all = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            Ok((0..all.GetCount()?)
                .filter_map(|i| all.Item(i).ok())
                .collect())
        }
    };
    list()
        .unwrap_or_default()
        .iter()
        .filter_map(headphone)
        .collect()
}

/// The name of the device if it is headphones.
fn headphone(device: &IMMDevice) -> Option<String> {
    // SAFETY: every interface is owned; the property values are freed by their own drop.
    let (form, name) = unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let form = store
            .GetValue(&PKEY_AudioEndpoint_FormFactor)
            .ok()
            .and_then(|v| u32::try_from(&v).ok());
        let name = store.GetValue(&FRIENDLY_NAME).ok()?.to_string();
        (form, name)
    };
    notices::is_headphones(form, &name).then(|| notices::headphone_name(&name))
}
