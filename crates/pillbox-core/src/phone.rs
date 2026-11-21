//! M6: phone transfer over the local network. The pure parts: picking the network address,
//! checking requests (token, host, subnet), cleaning file names from the phone, the QR matrix
//! and the pages the phone sees. The server itself lives in the app crate.

use std::net::Ipv4Addr;

// ---- network address ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Wifi,
    Ethernet,
    Other,
}

/// One IPv4 address of a network adapter that is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    pub kind: AdapterKind,
    pub addr: Ipv4Addr,
    /// On-link prefix length (24 for 255.255.255.0).
    pub prefix: u8,
    /// Has a default gateway: a real network, not a host-only virtual switch.
    pub gateway: bool,
    /// Windows files the network under Private (or Domain), so a firewall rule for private
    /// networks lets the phone in. On a Public network Windows blocks it by default.
    pub trusted: bool,
}

/// The address the phone server listens on: a private (RFC 1918) address of an adapter with
/// a gateway. Networks Windows calls Private come first; then Wi-Fi before Ethernet before
/// anything else. Virtual switches without a gateway (Hyper-V, WSL) and public or VPN
/// addresses (e.g. 100.64/10) are never chosen.
pub fn pick_adapter(list: &[Adapter]) -> Option<&Adapter> {
    let rank = |a: &Adapter| {
        let kind = match a.kind {
            AdapterKind::Wifi => 0,
            AdapterKind::Ethernet => 1,
            AdapterKind::Other => 2,
        };
        (!a.trusted, kind)
    };
    list.iter()
        .filter(|a| a.gateway && a.addr.is_private() && (8..=30).contains(&a.prefix))
        .min_by_key(|a| rank(a))
}

/// Are both addresses on the same subnet?
pub fn same_subnet(a: Ipv4Addr, b: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = u32::MAX << (32 - u32::from(prefix.min(32)));
    (u32::from(a) & mask) == (u32::from(b) & mask)
}

/// The Host header must name the address and port the server listens on (blocks DNS
/// rebinding: a web page elsewhere cannot make the phone's browser talk to us by name).
pub fn host_ok(host: &str, addr: Ipv4Addr, port: u16) -> bool {
    host.eq_ignore_ascii_case(&format!("{addr}:{port}"))
}

// ---- firewall ----------------------------------------------------------------------------------

/// Will Windows Firewall let a phone reach Pillbox on the chosen network?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firewall {
    /// Firewall off for that network type, inbound allowed by default, or a rule allows us.
    Open,
    /// A rule blocks pillbox.exe, or "block all incoming connections" is on.
    Blocked,
    /// No rule for pillbox.exe: Windows drops the phone's connection (the usual first-time case).
    NoRule,
}

/// One firewall rule, reduced to what matters for the phone server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FwRule {
    pub enabled: bool,
    pub inbound: bool,
    pub allow: bool,
    /// The program it names is pillbox.exe (or it names none).
    pub app_matches: bool,
    /// TCP or any protocol.
    pub tcp: bool,
    /// Covers the network type Pillbox listens on.
    pub profile_matches: bool,
    /// All local ports (Pillbox's port is random).
    pub any_port: bool,
    /// The rule names a program (not an every-program rule).
    pub names_app: bool,
}

/// Windows' rule order: "block all" beats everything, block rules beat allow rules, and with no
/// matching rule the default inbound action applies (block, unless changed).
pub fn firewall_verdict(
    enabled: bool,
    block_all: bool,
    default_allow: bool,
    rules: &[FwRule],
) -> Firewall {
    if !enabled {
        return Firewall::Open;
    }
    if block_all {
        return Firewall::Blocked;
    }
    // Only rules that name pillbox.exe count. Rules without a program are nearly always limited
    // in ways this API does not show (a Store app's package, a service): on this PC 29 of them
    // look like "allow everything" but let nothing reach pillbox.exe.
    let applies = |r: &&FwRule| {
        r.enabled && r.inbound && r.tcp && r.profile_matches && r.app_matches && r.names_app
    };
    let matching: Vec<&FwRule> = rules.iter().filter(applies).collect();
    // A block rule for pillbox.exe (e.g. "Cancel" on Windows' prompt makes one) stops it.
    if matching.iter().any(|r| !r.allow) {
        return Firewall::Blocked;
    }
    // The port is random, so only a rule for all ports lets the phone in.
    if default_allow || matching.iter().any(|r| r.allow && r.any_port) {
        Firewall::Open
    } else {
        Firewall::NoRule
    }
}

// ---- token -------------------------------------------------------------------------------------

/// Lower-case hex of some bytes (the token in the link).
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    s
}

/// Compares a token from a request with the real one in time that does not depend on where
/// they differ.
pub fn token_ok(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---- file names --------------------------------------------------------------------------------

/// A file name from the phone made safe to create in the receive folder: only the last path
/// part, no characters Windows forbids, no reserved device names, no trailing dots or spaces,
/// at most 120 characters (the extension is kept).
pub fn safe_file_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name: String = last
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    name = name
        .trim_matches(|c: char| c == ' ' || c == '.')
        .to_string();
    if name.is_empty() {
        name = "file".into();
    }
    let (stem, ext) = split_ext(&name);
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit());
    let stem = if reserved {
        format!("_{stem}")
    } else {
        stem.to_string()
    };
    let ext: String = ext.chars().take(16).collect();
    let room = 120usize.saturating_sub(ext.chars().count()).max(1);
    let stem: String = stem.chars().take(room).collect();
    format!("{stem}{ext}")
}

/// ("photo", ".jpg"); a name without a dot (or only a leading one) has no extension.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    }
}

/// `name`, or "name (2).ext", "name (3).ext", ... — the first that `taken` says is free.
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    (2..10_000)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| format!("{stem} (copy){ext}"))
}

/// Content-Disposition for a download: an ASCII fallback plus the exact UTF-8 name.
pub fn content_disposition(name: &str, inline: bool) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for b in name.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(b) {
            encoded.push(char::from(*b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    let kind = if inline { "inline" } else { "attachment" };
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// MIME type from the file extension (enough for the phone to preview or save it well).
pub fn content_type(name: &str) -> &'static str {
    let ext = split_ext(name).1.to_ascii_lowercase();
    match ext.as_str() {
        ".jpg" | ".jpeg" => "image/jpeg",
        ".png" => "image/png",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".heic" => "image/heic",
        ".bmp" => "image/bmp",
        ".mp4" => "video/mp4",
        ".mov" => "video/quicktime",
        ".mp3" => "audio/mpeg",
        ".m4a" => "audio/mp4",
        ".pdf" => "application/pdf",
        ".txt" | ".log" | ".md" => "text/plain; charset=utf-8",
        ".zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

pub fn is_image(name: &str) -> bool {
    content_type(name).starts_with("image/") && !name.to_ascii_lowercase().ends_with(".heic")
}

/// "1.4 MB", "820 KB", "12 B".
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.1} GB", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

// ---- QR code -------------------------------------------------------------------------------------

/// A QR code as a square of modules, row by row; `true` = dark. No quiet zone included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qr {
    pub width: usize,
    pub dark: Vec<bool>,
}

impl Qr {
    pub fn new(text: &str) -> Option<Self> {
        let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
            .ok()?;
        let width = code.width();
        let dark = code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect();
        Some(Self { width, dark })
    }

    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && self.dark.get(y * self.width + x).copied().unwrap_or(false)
    }

    /// Horizontal runs of dark modules: (row, first column, length). Drawing runs instead of
    /// single modules keeps the rectangle count low and leaves no seams between modules.
    pub fn runs(&self) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        for y in 0..self.width {
            let mut x = 0;
            while x < self.width {
                if self.is_dark(x, y) {
                    let start = x;
                    while x < self.width && self.is_dark(x, y) {
                        x += 1;
                    }
                    out.push((y, start, x - start));
                } else {
                    x += 1;
                }
            }
        }
        out
    }
}

// ---- pages ---------------------------------------------------------------------------------------

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Content-Security-Policy for the pages: nothing from elsewhere, only our own inline script.
pub fn csp(nonce: &str) -> String {
    format!(
        "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; script-src 'nonce-{nonce}'; \
         connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
    )
}

const STYLE: &str = "\
:root{color-scheme:dark}*{box-sizing:border-box}\
body{margin:0;font:16px/1.4 system-ui,-apple-system,Segoe UI,Roboto,sans-serif;background:#0b0b0f;color:#f2f2f5;\
display:flex;min-height:100vh;align-items:center;justify-content:center;padding:20px}\
main{width:100%;max-width:420px;background:#17171d;border-radius:28px;padding:24px;box-shadow:0 10px 40px #0008}\
h1{font-size:15px;font-weight:600;color:#9a9aa8;margin:0 0 16px;letter-spacing:.02em}\
.name{font-size:20px;font-weight:600;word-break:break-word;margin:0 0 4px}.dim{color:#9a9aa8;font-size:14px}\
img{width:100%;border-radius:16px;margin:16px 0 4px;background:#0006}\
.btn{display:block;width:100%;margin-top:18px;padding:15px;border:0;border-radius:999px;background:#7c5cff;color:#fff;\
font:600 17px system-ui,sans-serif;text-align:center;text-decoration:none}\
.btn:disabled{opacity:.45}.pick{background:#2a2a33}\
textarea{width:100%;min-height:96px;margin-top:14px;padding:12px 14px;border-radius:16px;border:1px solid #2e2e38;\
background:#0f0f14;color:#f2f2f5;font:16px system-ui,sans-serif;resize:vertical}\
input[type=file]{display:none}\
progress{width:100%;height:8px;margin-top:16px;accent-color:#7c5cff}\
#msg{margin-top:12px;min-height:20px;font-size:14px;color:#b9b9c6}";

fn page(title: &str, nonce: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\"><title>{title}</title>\
         <style nonce=\"{nonce}\">{STYLE}</style></head><body><main>{body}</main></body></html>",
        title = html_escape(title),
    )
}

/// The page a phone sees for a file sent from the shelf.
pub fn send_page(name: &str, size: u64, nonce: &str) -> String {
    let n = html_escape(name);
    let preview = if is_image(name) {
        "<img src=\"file?inline=1\" alt=\"\">".to_string()
    } else {
        String::new()
    };
    page(
        "From your PC",
        nonce,
        &format!(
            "<h1>FROM YOUR PC · PILLBOX</h1><p class=\"name\">{n}</p>\
             <p class=\"dim\">{size}</p>{preview}\
             <a class=\"btn\" href=\"file\" download=\"{n}\">Download</a>\
             <p class=\"dim\" style=\"margin-top:14px\">The link stops working when the code \
             is closed on the PC.</p>",
            size = human_size(size),
        ),
    )
}

/// The page a phone sees to send photos, files and text to the PC.
pub fn receive_page(nonce: &str, max_bytes: u64) -> String {
    page(
        "Send to your PC",
        nonce,
        &format!(
            "<h1>SEND TO YOUR PC · PILLBOX</h1>\
             <form id=\"f\" method=\"post\" action=\"upload\" enctype=\"multipart/form-data\">\
             <label class=\"btn pick\" for=\"files\">Choose photos or files</label>\
             <input id=\"files\" name=\"files\" type=\"file\" multiple>\
             <p id=\"chosen\" class=\"dim\" style=\"margin:10px 0 0\">Nothing chosen</p>\
             <textarea name=\"text\" placeholder=\"Or type text to copy on the PC\"></textarea>\
             <button class=\"btn\" id=\"send\" type=\"submit\">Send</button>\
             <progress id=\"bar\" max=\"1\" value=\"0\" hidden></progress><p id=\"msg\"></p></form>\
             <p class=\"dim\">Up to {max} in total. Same Wi-Fi only; the link stops working \
             when the code is closed on the PC.</p>\
             <script nonce=\"{nonce}\">const MAX={max_bytes},MAXT='{max}';{script}</script>",
            max = human_size(max_bytes),
            script = RECEIVE_SCRIPT,
        ),
    )
}

/// Upload with a progress bar; plain form posting still works without it.
const RECEIVE_SCRIPT: &str = "\
const f=document.getElementById('f'),files=document.getElementById('files'),\
chosen=document.getElementById('chosen'),bar=document.getElementById('bar'),\
msg=document.getElementById('msg'),send=document.getElementById('send');\
files.onchange=()=>{const n=files.files.length;chosen.textContent=n?n+(n==1?' file':' files')+' chosen':'Nothing chosen'};\
f.onsubmit=e=>{e.preventDefault();const d=new FormData(f);\
if(!files.files.length&&!f.text.value.trim()){msg.textContent='Choose a file or type some text.';return}\
let size=0;for(const x of files.files)size+=x.size;if(size>MAX){msg.textContent='Too big: the limit is '+MAXT+'.';return}\
const x=new XMLHttpRequest();x.open('POST','upload');send.disabled=true;bar.hidden=false;bar.value=0;msg.textContent='Sending…';\
x.upload.onprogress=p=>{if(p.lengthComputable)bar.value=p.loaded/p.total};\
x.onload=()=>{send.disabled=false;bar.hidden=true;msg.textContent=x.status==200?x.responseText:'Not sent ('+x.status+'). '+x.responseText;\
if(x.status==200){f.reset();chosen.textContent='Nothing chosen'}};\
x.onerror=()=>{send.disabled=false;bar.hidden=true;msg.textContent='Not sent: the PC stopped listening.'};\
x.send(d)};";

/// The plain answer after an upload (shown by the script, or as the page without it).
pub fn upload_summary(files: usize, text: bool) -> String {
    match (files, text) {
        (0, true) => "Text sent ✓".to_string(),
        (n, false) => format!("{n} file{} sent ✓", if n == 1 { "" } else { "s" }),
        (n, true) => format!("{n} file{} and text sent ✓", if n == 1 { "" } else { "s" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ad(kind: AdapterKind, addr: [u8; 4], gateway: bool) -> Adapter {
        Adapter {
            name: format!("{kind:?}"),
            kind,
            addr: Ipv4Addr::from(addr),
            prefix: 24,
            gateway,
            trusted: true,
        }
    }

    #[test]
    fn a_private_network_beats_wifi_on_a_public_one() {
        // This PC: Wi-Fi and Ethernet on the same router, Wi-Fi filed as Public.
        let mut wifi = ad(AdapterKind::Wifi, [192, 168, 68, 116], true);
        wifi.trusted = false;
        let list = vec![wifi, ad(AdapterKind::Ethernet, [192, 168, 68, 115], true)];
        assert_eq!(
            pick_adapter(&list).map(|a| a.kind),
            Some(AdapterKind::Ethernet)
        );
        // Only a public network: still used (the firewall may ask or block).
        let mut wifi = ad(AdapterKind::Wifi, [192, 168, 68, 116], true);
        wifi.trusted = false;
        assert_eq!(pick_adapter(&[wifi]).map(|a| a.trusted), Some(false));
    }

    #[test]
    fn picks_wifi_on_a_private_network() {
        let list = vec![
            ad(AdapterKind::Other, [172, 20, 0, 1], false), // WSL / Hyper-V switch
            ad(AdapterKind::Ethernet, [192, 168, 1, 20], true),
            ad(AdapterKind::Other, [100, 70, 3, 4], true), // VPN, not private
            ad(AdapterKind::Wifi, [10, 0, 0, 7], true),
        ];
        assert_eq!(
            pick_adapter(&list).map(|a| a.addr),
            Some(Ipv4Addr::new(10, 0, 0, 7))
        );
        let list = vec![ad(AdapterKind::Ethernet, [192, 168, 1, 20], true)];
        assert_eq!(
            pick_adapter(&list).map(|a| a.kind),
            Some(AdapterKind::Ethernet)
        );
        let list = vec![
            ad(AdapterKind::Other, [172, 20, 0, 1], false),
            ad(AdapterKind::Wifi, [8, 8, 8, 8], true),
        ];
        assert_eq!(pick_adapter(&list), None);
    }

    #[test]
    fn firewall_verdicts() {
        let allow = FwRule {
            enabled: true,
            inbound: true,
            allow: true,
            app_matches: true,
            tcp: true,
            profile_matches: true,
            any_port: true,
            names_app: true,
        };
        let block = FwRule {
            allow: false,
            ..allow
        };
        // This PC before the fix: firewall on, default block, no rule for pillbox.exe.
        assert_eq!(firewall_verdict(true, false, false, &[]), Firewall::NoRule);
        assert_eq!(
            firewall_verdict(true, false, false, &[allow]),
            Firewall::Open
        );
        assert_eq!(
            firewall_verdict(false, true, false, &[]),
            Firewall::Open,
            "firewall off"
        );
        assert_eq!(
            firewall_verdict(true, false, true, &[]),
            Firewall::Open,
            "default allow"
        );
        assert_eq!(
            firewall_verdict(true, true, false, &[allow]),
            Firewall::Blocked,
            "block all"
        );
        assert_eq!(
            firewall_verdict(true, false, false, &[allow, block]),
            Firewall::Blocked
        );
        // Rules that do not apply: disabled, outbound, another network type, UDP, one port.
        for r in [
            FwRule {
                enabled: false,
                ..allow
            },
            FwRule {
                inbound: false,
                ..allow
            },
            FwRule {
                profile_matches: false,
                ..allow
            },
            FwRule {
                tcp: false,
                ..allow
            },
            FwRule {
                any_port: false,
                ..allow
            },
            FwRule {
                app_matches: false,
                ..allow
            },
        ] {
            assert_eq!(
                firewall_verdict(true, false, false, &[r]),
                Firewall::NoRule,
                "{r:?}"
            );
        }
        // Rules naming no program (on this PC: Store-app rules scoped to their package) are
        // ignored either way: an "allow everything" one must not hide the warning.
        let package_allow = FwRule {
            names_app: false,
            ..allow
        };
        assert_eq!(
            firewall_verdict(true, false, false, &[package_allow]),
            Firewall::NoRule
        );
        let package_block = FwRule {
            names_app: false,
            ..block
        };
        assert_eq!(
            firewall_verdict(true, false, false, &[allow, package_block]),
            Firewall::Open
        );
    }

    #[test]
    fn subnets_and_hosts() {
        let a = Ipv4Addr::new(192, 168, 1, 20);
        assert!(same_subnet(a, Ipv4Addr::new(192, 168, 1, 99), 24));
        assert!(!same_subnet(a, Ipv4Addr::new(192, 168, 2, 99), 24));
        assert!(same_subnet(a, Ipv4Addr::new(192, 168, 2, 99), 16));
        assert!(same_subnet(a, Ipv4Addr::new(1, 2, 3, 4), 0));
        assert!(host_ok("192.168.1.20:50123", a, 50123));
        assert!(!host_ok("192.168.1.20:50124", a, 50123));
        assert!(!host_ok("evil.example:50123", a, 50123));
        assert!(!host_ok("192.168.1.20", a, 50123));
    }

    #[test]
    fn tokens() {
        assert_eq!(hex(&[0, 15, 16, 255]), "000f10ff");
        assert!(token_ok("abc123", "abc123"));
        assert!(!token_ok("abc123", "abc124"));
        assert!(!token_ok("abc123", "abc12"));
        assert!(!token_ok("", ""));
    }

    #[test]
    fn file_names_from_the_phone_are_made_safe() {
        assert_eq!(safe_file_name("IMG_0001.HEIC"), "IMG_0001.HEIC");
        assert_eq!(safe_file_name("../../Windows/win.ini"), "win.ini");
        assert_eq!(safe_file_name("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(safe_file_name("C:\\x\\a:b?.txt"), "a_b_.txt");
        assert_eq!(safe_file_name("..."), "file");
        assert_eq!(safe_file_name(""), "file");
        assert_eq!(safe_file_name("con.txt"), "_con.txt");
        assert_eq!(safe_file_name("COM1"), "_COM1");
        assert_eq!(safe_file_name("COMET.txt"), "COMET.txt");
        assert_eq!(safe_file_name("name. "), "name");
        assert_eq!(safe_file_name("a\u{0}b\nc.png"), "a_b_c.png");
        assert_eq!(safe_file_name(".bashrc"), "bashrc");
        let long = format!("{}.jpg", "x".repeat(300));
        let safe = safe_file_name(&long);
        assert_eq!(safe.chars().count(), 120);
        assert!(safe.ends_with(".jpg"));
    }

    #[test]
    fn unique_names() {
        let taken = ["a.jpg", "a (2).jpg", "notes"];
        let t = |n: &str| taken.contains(&n);
        assert_eq!(unique_name("b.jpg", t), "b.jpg");
        assert_eq!(unique_name("a.jpg", t), "a (3).jpg");
        assert_eq!(unique_name("notes", t), "notes (2)");
    }

    #[test]
    fn downloads_and_types() {
        assert_eq!(
            content_disposition("Fotka č. 1.jpg", false),
            "attachment; filename=\"Fotka _. 1.jpg\"; filename*=UTF-8''Fotka%20%C4%8D.%201.jpg"
        );
        assert!(content_disposition("a\"b.png", true).starts_with("inline; filename=\"a_b.png\""));
        assert_eq!(content_type("X.JPG"), "image/jpeg");
        assert_eq!(content_type("noext"), "application/octet-stream");
        assert!(is_image("a.png") && !is_image("a.heic") && !is_image("a.pdf"));
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(1536), "2 KB");
        assert_eq!(human_size(3 * 1024 * 1024 / 2), "1.5 MB");
    }

    #[test]
    fn pages_escape_names_and_carry_the_nonce() {
        let p = send_page("<script>alert(1)</script>.png", 2048, "N0NCE");
        assert!(!p.contains("<script>alert"));
        assert!(p.contains("&lt;script&gt;"));
        assert!(p.contains("file?inline=1"), "pictures get a preview");
        let p = receive_page("N0NCE", 1 << 30);
        assert!(p.contains("<script nonce=\"N0NCE\">"));
        assert!(p.contains("1.0 GB"));
        assert!(csp("N0NCE").contains("script-src 'nonce-N0NCE'"));
        assert_eq!(upload_summary(2, false), "2 files sent ✓");
        assert_eq!(upload_summary(0, true), "Text sent ✓");
    }

    #[test]
    fn qr_round_trips_through_a_decoder() {
        let url = "http://192.168.1.20:51234/0123456789abcdef0123456789abcdef";
        let qr = Qr::new(url).expect("encodes");
        assert!(qr.width >= 21 && qr.dark.len() == qr.width * qr.width);
        // Draw it 4 px per module with a 4-module quiet zone, as Pillbox does, and decode.
        let scale = 4;
        let side = (qr.width + 8) * scale;
        let mut img = rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| {
            let (mx, my) = (x / scale, y / scale);
            if mx >= 4 && my >= 4 && qr.is_dark(mx - 4, my - 4) {
                0
            } else {
                255
            }
        });
        let grids = img.detect_grids();
        assert_eq!(grids.len(), 1);
        let (_, content) = grids[0].decode().expect("decodes");
        assert_eq!(content, url);
        // Runs cover exactly the dark modules.
        let dark_in_runs: usize = qr.runs().iter().map(|r| r.2).sum();
        assert_eq!(dark_in_runs, qr.dark.iter().filter(|d| **d).count());
    }
}
