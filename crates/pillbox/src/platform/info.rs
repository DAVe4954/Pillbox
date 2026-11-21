//! Small system readings for widgets: memory, network bytes, the UTC offset, idle time,
//! world clocks, ping, and the reminder chime.

use std::net::{IpAddr, ToSocketAddrs};

use anyhow::{Result, anyhow};
use pillbox_core::calendar::{Date, Minutes, minutes};
use windows::{
    Globalization::Calendar,
    Win32::{
        Foundation::HANDLE,
        Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_LOOP, SND_NODEFAULT, SND_PURGE},
        NetworkManagement::{
            IpHelper::{
                FreeMibTable, GetBestInterface, GetIfEntry2, GetIfTable2, ICMP_ECHO_REPLY,
                IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, MIB_IF_ROW2, MIB_IF_TABLE2,
            },
            Ndis::IfOperStatusUp,
        },
        System::{
            SystemInformation::{
                GetSystemTime, GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
            },
            Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION},
        },
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    },
    core::{HSTRING, PCWSTR, w},
};

/// Used and total physical memory in GB.
pub fn memory() -> Option<(f32, f32)> {
    let mut m = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: m is a valid local with its length set.
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    let gb = |b: u64| b as f32 / (1024.0 * 1024.0 * 1024.0);
    Some((gb(m.ullTotalPhys - m.ullAvailPhys), gb(m.ullTotalPhys)))
}

/// Total bytes received and sent by physical network adapters that are up.
pub fn network_bytes() -> Option<(u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table; it is read within NumEntries rows and freed
    // with FreeMibTable exactly once.
    unsafe {
        GetIfTable2(&mut table).ok().ok()?;
        if table.is_null() {
            return None;
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let (mut down, mut up) = (0u64, 0u64);
        for r in rows {
            let hardware = r.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
            if hardware && r.OperStatus == IfOperStatusUp {
                down += r.InOctets;
                up += r.OutOctets;
            }
        }
        FreeMibTable(table.cast());
        Some((down, up))
    }
}

/// The adapter Windows sends internet traffic through, by interface index. With Wi-Fi and
/// Ethernet both up it may be either (equal route metrics), so it is only a tie-break; see
/// `pillbox_core::sysinfo::NetPicker`. `None` when there is no route out.
pub fn routed_adapter() -> Option<u32> {
    // 1.1.1.1 in network byte order. Only used to ask the routing table; nothing is sent.
    const PUBLIC_ADDR: u32 = u32::from_ne_bytes([1, 1, 1, 1]);
    let mut index = 0u32;
    // SAFETY: the out-pointer is a valid local.
    (unsafe { GetBestInterface(PUBLIC_ADDR, &mut index) } == 0).then_some(index)
}

/// Name and byte counters of every physical adapter that is up (Wi-Fi, Ethernet; not VPN or
/// virtual switches). Empty when offline or the table cannot be read.
pub fn up_connections() -> Vec<pillbox_core::sysinfo::IfSample> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: as in network_bytes: allocated by GetIfTable2, read within NumEntries, freed once.
    unsafe {
        if GetIfTable2(&mut table).ok().is_err() || table.is_null() {
            return Vec::new();
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let list = rows
            .iter()
            .filter(|r| {
                r.InterfaceAndOperStatusFlags._bitfield & 1 != 0 && r.OperStatus == IfOperStatusUp
            })
            .map(|r| {
                let len = r
                    .Alias
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(r.Alias.len());
                pillbox_core::sysinfo::IfSample {
                    index: r.InterfaceIndex,
                    name: String::from_utf16_lossy(&r.Alias[..len]),
                    down: r.InOctets,
                    up: r.OutOctets,
                }
            })
            .collect();
        FreeMibTable(table.cast());
        list
    }
}

/// Network byte counters for frequent sampling (the stats strip). Listing every interface
/// (`GetIfTable2`, which also walks dozens of virtual filter interfaces) is done at most every
/// few seconds; in between only the physical adapters that were up are read (`GetIfEntry2`).
#[derive(Default)]
pub struct NetCounter {
    adapters: Vec<u32>,
    listed: Option<std::time::Instant>,
}

impl NetCounter {
    const RELIST: std::time::Duration = std::time::Duration::from_secs(5);

    /// Total bytes received and sent by the physical adapters that are up.
    pub fn bytes(&mut self) -> Option<(u64, u64)> {
        if self.listed.is_none_or(|t| t.elapsed() >= Self::RELIST) {
            self.adapters = up_adapters()?;
            self.listed = Some(std::time::Instant::now());
        }
        let (mut down, mut up) = (0u64, 0u64);
        for &index in &self.adapters {
            let mut row = MIB_IF_ROW2 {
                InterfaceIndex: index,
                ..Default::default()
            };
            // SAFETY: row is a valid local with the index set, as GetIfEntry2 requires.
            if unsafe { GetIfEntry2(&mut row) }.is_ok() && row.OperStatus == IfOperStatusUp {
                down += row.InOctets;
                up += row.OutOctets;
            }
        }
        Some((down, up))
    }
}

/// Interface indices of the physical adapters that are up.
fn up_adapters() -> Option<Vec<u32>> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: as in network_bytes: allocated by GetIfTable2, read within NumEntries, freed once.
    unsafe {
        GetIfTable2(&mut table).ok().ok()?;
        if table.is_null() {
            return None;
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let list = rows
            .iter()
            .filter(|r| {
                r.InterfaceAndOperStatusFlags._bitfield & 1 != 0 && r.OperStatus == IfOperStatusUp
            })
            .map(|r| r.InterfaceIndex)
            .collect();
        FreeMibTable(table.cast());
        Some(list)
    }
}

/// Local time minus UTC, in minutes (positive east of Greenwich), including daylight saving.
pub fn utc_offset_minutes() -> i64 {
    let mut tz = TIME_ZONE_INFORMATION::default();
    // SAFETY: tz is a valid out-struct.
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let mut bias = i64::from(tz.Bias);
    // 2 = TIME_ZONE_ID_DAYLIGHT: daylight saving time is in effect.
    if id == 2 {
        bias += i64::from(tz.DaylightBias);
    } else {
        bias += i64::from(tz.StandardBias);
    }
    -bias
}

/// Minutes since 1970 in UTC.
pub fn utc_minutes() -> Minutes {
    // SAFETY: no arguments; returns a plain struct.
    let t = unsafe { GetSystemTime() };
    minutes(
        Date {
            year: i32::from(t.wYear),
            month: u32::from(t.wMonth),
            day: u32::from(t.wDay),
        },
        u32::from(t.wHour),
        u32::from(t.wMinute),
    )
}

/// Seconds since the last keyboard or mouse input anywhere.
pub fn idle_seconds() -> u64 {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: info is a valid local with its size set.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    // SAFETY: no arguments.
    let now = unsafe { GetTickCount64() } as u32;
    u64::from(now.wrapping_sub(info.dwTime)) / 1000
}

/// Wall-clock time in a time zone ("Asia/Tokyo", "Europe/London", "UTC") as local minutes.
pub fn zone_minutes(zone: &str) -> Result<Minutes> {
    let cal = Calendar::new()?;
    cal.ChangeClock(&HSTRING::from("24HourClock"))?;
    cal.ChangeTimeZone(&HSTRING::from(zone))
        .map_err(|e| anyhow!("unknown time zone {zone}: {e}"))?;
    Ok(minutes(
        Date {
            year: cal.Year()?,
            month: cal.Month()? as u32,
            day: cal.Day()? as u32,
        },
        cal.Hour()? as u32,
        cal.Minute()? as u32,
    ))
}

/// Round-trip time to a host, in milliseconds. Blocking (up to two seconds).
pub fn ping(host: &str) -> Result<u32> {
    let addr = (host, 0)
        .to_socket_addrs()?
        .find_map(|a| match a.ip() {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
        .ok_or_else(|| anyhow!("no IPv4 address for {host}"))?;
    let ip = u32::from_ne_bytes(addr.octets());
    let payload = *b"pillbox-ping";
    let mut reply = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8];
    // SAFETY: the ICMP handle is closed before returning; the reply buffer is larger than one
    // reply plus the payload, as IcmpSendEcho requires, and is read only as ICMP_ECHO_REPLY.
    unsafe {
        let handle: HANDLE = IcmpCreateFile()?;
        let n = IcmpSendEcho(
            handle,
            ip,
            payload.as_ptr().cast(),
            payload.len() as u16,
            None,
            reply.as_mut_ptr().cast(),
            reply.len() as u32,
            2000,
        );
        let _ = IcmpCloseHandle(handle);
        if n == 0 {
            return Err(anyhow!("no answer"));
        }
        let r = std::ptr::read_unaligned(reply.as_ptr().cast::<ICMP_ECHO_REPLY>());
        if r.Status != 0 {
            return Err(anyhow!("unreachable ({})", r.Status));
        }
        Ok(r.RoundTripTime)
    }
}

/// Start the looping reminder chime (the Windows "Alarm" sound), or a one-off notification.
pub fn chime(looping: bool) {
    let flags = SND_ALIAS
        | SND_ASYNC
        | SND_NODEFAULT
        | if looping {
            SND_LOOP
        } else {
            Default::default()
        };
    let alias = if looping {
        w!("Notification.Looping.Alarm")
    } else {
        w!("Notification.Reminder")
    };
    // SAFETY: a 'static alias name; asynchronous playback copies what it needs.
    let ok = unsafe { PlaySoundW(alias, None, flags) }.as_bool();
    if !ok {
        // Fall back to the plain notification sound (always defined).
        // SAFETY: as above.
        let _ = unsafe { PlaySoundW(w!("SystemNotification"), None, SND_ALIAS | SND_ASYNC) };
    }
}

pub fn stop_chime() {
    // SAFETY: a null sound with SND_PURGE stops whatever this process is playing.
    let _ = unsafe { PlaySoundW(PCWSTR::null(), None, SND_PURGE) };
}
