//! M6: read-only look at Windows Defender Firewall, to tell the user when it will drop the
//! phone's connection. Nothing here changes a firewall setting.

use anyhow::{Context, Result};
pub use pillbox_core::phone::Firewall;
use pillbox_core::phone::{FwRule, firewall_verdict};
use windows::{
    Win32::{
        NetworkManagement::WindowsFirewall::{
            INetFwPolicy2, INetFwRule, NET_FW_ACTION_ALLOW, NET_FW_IP_PROTOCOL_ANY,
            NET_FW_IP_PROTOCOL_TCP, NET_FW_PROFILE_TYPE2, NET_FW_PROFILE2_PRIVATE,
            NET_FW_PROFILE2_PUBLIC, NET_FW_RULE_DIR_IN, NetFwPolicy2,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize, IDispatch,
            },
            Ole::IEnumVARIANT,
            Variant::VARIANT,
        },
    },
    core::Interface,
};

/// Will the firewall let a phone in on a Private (`trusted`) or Public network? Call on a
/// worker thread: listing the rules takes a few hundred COM calls.
pub fn check(trusted: bool) -> Result<Firewall> {
    let exe = std::env::current_exe().context("finding pillbox.exe")?;
    let exe = exe.to_string_lossy().to_lowercase();
    let profile = if trusted {
        NET_FW_PROFILE2_PRIVATE
    } else {
        NET_FW_PROFILE2_PUBLIC
    };
    // SAFETY: COM is initialised for this thread here and uninitialised once on the way out,
    // after every interface below has been dropped (they live inside `read`).
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let result = read(&exe, profile);
    if hr.is_ok() {
        // SAFETY: balances the successful CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
    result
}

fn read(exe: &str, profile: NET_FW_PROFILE_TYPE2) -> Result<Firewall> {
    let mut rules = Vec::new();
    // SAFETY: plain COM calls on interfaces we own; every VARIANT from the enumerator is read
    // through windows-rs's checked conversion and dropped (VariantClear) at the end of its loop.
    unsafe {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)
            .context("opening the firewall policy")?;
        let enabled = policy.get_FirewallEnabled(profile)?.as_bool();
        let block_all = policy.get_BlockAllInboundTraffic(profile)?.as_bool();
        let default_allow = policy.get_DefaultInboundAction(profile)? == NET_FW_ACTION_ALLOW;
        if enabled && !block_all && !default_allow {
            let list = policy.Rules().context("reading firewall rules")?;
            let walker: IEnumVARIANT = list._NewEnum()?.cast()?;
            loop {
                let mut slot = [VARIANT::default()];
                let mut got = 0u32;
                if walker.Next(&mut slot, &mut got).is_err() || got == 0 {
                    break;
                }
                let Ok(rule) = IDispatch::try_from(&slot[0]).and_then(|d| d.cast::<INetFwRule>())
                else {
                    continue;
                };
                let app = rule
                    .ApplicationName()
                    .map(|b| b.to_string().to_lowercase())
                    .unwrap_or_default();
                let ports = rule.LocalPorts().map(|b| b.to_string()).unwrap_or_default();
                let protocol = rule.Protocol().unwrap_or(0);
                rules.push(FwRule {
                    enabled: rule.Enabled().map(|b| b.as_bool()).unwrap_or(false),
                    inbound: rule.Direction().is_ok_and(|d| d == NET_FW_RULE_DIR_IN),
                    allow: rule.Action().is_ok_and(|a| a == NET_FW_ACTION_ALLOW),
                    app_matches: app.is_empty() || app == exe,
                    tcp: protocol == NET_FW_IP_PROTOCOL_TCP.0
                        || protocol == NET_FW_IP_PROTOCOL_ANY.0,
                    profile_matches: rule.Profiles().unwrap_or(0) & profile.0 != 0,
                    any_port: ports.is_empty() || ports == "*",
                    names_app: !app.is_empty(),
                });
            }
        }
        Ok(firewall_verdict(enabled, block_all, default_allow, &rules))
    }
}
