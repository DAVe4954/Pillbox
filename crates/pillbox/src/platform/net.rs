//! M6 helpers for the phone server: the PC's IPv4 addresses, secure random bytes, and the
//! Downloads folder.

use std::{net::Ipv4Addr, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use pillbox_core::phone::{Adapter, AdapterKind};
use windows::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR},
    NetworkManagement::{
        IpHelper::{
            GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
            GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::{
        NetworkListManager::{
            IEnumNetworkConnections, INetworkListManager,
            NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED, NLM_NETWORK_CATEGORY_PRIVATE,
            NLM_NETWORK_CATEGORY_PUBLIC, NetworkListManager,
        },
        WinSock::{AF_INET, SOCKADDR_IN},
    },
    Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
    System::Com::{CLSCTX_ALL, CoCreateInstance},
    UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
};

// IANA interface types (ipifcons.h).
const IF_TYPE_ETHERNET_CSMACD: u32 = 6;
const IF_TYPE_IEEE80211: u32 = 71;

/// Every IPv4 address of every adapter that is up, marked trusted when Windows files its
/// network as Private or Domain. Call on a thread with COM initialised.
pub fn adapters() -> Vec<Adapter> {
    let (mut list, ids) = match read_adapters() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("network adapters: {e:#}");
            return Vec::new();
        }
    };
    match trusted_adapter_ids() {
        Ok(trusted) => {
            for (a, id) in list.iter_mut().zip(&ids) {
                a.trusted = trusted.iter().any(|t| t.eq_ignore_ascii_case(id));
            }
        }
        // Without the network categories every network counts as untrusted; the picker
        // then falls back to Wi-Fi first.
        Err(e) => tracing::warn!("network categories: {e:#}"),
    }
    list
}

/// Adapter GUIDs ("{…}", as GetAdaptersAddresses names them) of connected networks that
/// Windows files as Private or Domain (Network List Manager).
fn trusted_adapter_ids() -> Result<Vec<String>> {
    let mut out = Vec::new();
    // SAFETY: plain COM calls on interfaces we own; COM is initialised on this thread
    // (system::init_com at start-up). Each enumerated connection is used only while held.
    unsafe {
        let nlm: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL)
            .context("creating the Network List Manager")?;
        let conns: IEnumNetworkConnections = nlm
            .GetNetworkConnections()
            .context("listing network connections")?;
        loop {
            let mut slot = [None];
            let mut got = 0u32;
            if conns.Next(&mut slot, Some(&mut got)).is_err() || got == 0 {
                break;
            }
            let Some(conn) = slot[0].take() else { break };
            let (Ok(id), Ok(network)) = (conn.GetAdapterId(), conn.GetNetwork()) else {
                continue;
            };
            let category = network.GetCategory().unwrap_or(NLM_NETWORK_CATEGORY_PUBLIC);
            if category == NLM_NETWORK_CATEGORY_PRIVATE
                || category == NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED
            {
                out.push(format!("{{{id:?}}}"));
            }
        }
    }
    Ok(out)
}

/// The adapters, each with its GUID name ("{…}") in a parallel list.
fn read_adapters() -> Result<(Vec<Adapter>, Vec<String>)> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    // u64 elements keep the buffer aligned for the structures written into it.
    let mut size: u32 = 16 * 1024;
    let mut buf: Vec<u64> = Vec::new();
    for _ in 0..4 {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: buf is writable for `size` bytes (rounded up) and suitably aligned; size is
        // a valid local that receives the needed length on ERROR_BUFFER_OVERFLOW.
        let rc = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET.0),
                flags,
                None,
                Some(buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
                &mut size,
            )
        };
        if rc == NO_ERROR.0 {
            return Ok(walk(&buf));
        }
        if rc != ERROR_BUFFER_OVERFLOW.0 {
            return Err(anyhow!("GetAdaptersAddresses failed ({rc})"));
        }
    }
    drop(buf);
    Err(anyhow!("GetAdaptersAddresses kept asking for more room"))
}

/// Reads the linked list GetAdaptersAddresses wrote into `buf`.
fn walk(buf: &[u64]) -> (Vec<Adapter>, Vec<String>) {
    let mut out = Vec::new();
    let mut ids = Vec::new();
    let mut node = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: every pointer followed here (Next, FirstUnicastAddress, FirstGatewayAddress,
    // lpSockaddr, FriendlyName) points into `buf`, which GetAdaptersAddresses filled and which
    // outlives this loop; lists end with null. Socket addresses are AF_INET (we asked for that
    // family) and are checked again before being read as SOCKADDR_IN.
    unsafe {
        while let Some(a) = node.as_ref() {
            node = a.Next;
            if a.OperStatus != IfOperStatusUp {
                continue;
            }
            let kind = match a.IfType {
                IF_TYPE_IEEE80211 => AdapterKind::Wifi,
                IF_TYPE_ETHERNET_CSMACD => AdapterKind::Ethernet,
                _ => AdapterKind::Other,
            };
            let name = a.FriendlyName.to_string().unwrap_or_default();
            let id = a.AdapterName.to_string().unwrap_or_default();
            let gateway = !a.FirstGatewayAddress.is_null();
            let mut ua = a.FirstUnicastAddress;
            while let Some(u) = ua.as_ref() {
                ua = u.Next;
                let sa = u.Address.lpSockaddr;
                if sa.is_null() || (*sa).sa_family != AF_INET {
                    continue;
                }
                let sin = &*sa.cast::<SOCKADDR_IN>();
                let addr = Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr));
                out.push(Adapter {
                    name: name.clone(),
                    kind,
                    addr,
                    prefix: u.OnLinkPrefixLength,
                    gateway,
                    trusted: false,
                });
                ids.push(id.clone());
            }
        }
    }
    (out, ids)
}

/// Cryptographically secure random bytes from Windows (BCryptGenRandom, system RNG).
pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    // SAFETY: the buffer is a valid, writable local slice; no algorithm handle is needed with
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG.
    unsafe { BCryptGenRandom(None, &mut out, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
        .ok()
        .context("BCryptGenRandom")?;
    Ok(out)
}

/// The user's Downloads folder.
pub fn downloads_dir() -> Result<PathBuf> {
    // SAFETY: SHGetKnownFolderPath allocates the string with CoTaskMemAlloc; it is copied into
    // a Rust String and then freed exactly once with CoTaskMemFree.
    let path = unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None)
            .context("locating the Downloads folder")?;
        let s = p.to_string();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.context("Downloads path is not valid text")?
    };
    Ok(PathBuf::from(path))
}
