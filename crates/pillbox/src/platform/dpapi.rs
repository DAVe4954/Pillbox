//! Encryption tied to the Windows user account (DPAPI), for clipboard history on disk.
//! Only the same user on the same PC can decrypt it; no key is stored by the app.

use anyhow::{Result, anyhow};
use windows::Win32::{
    Foundation::{HLOCAL, LocalFree},
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    }
}

/// Copy an output blob into a Vec and free the memory Windows allocated for it.
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: DPAPI returned `cbData` bytes at `pbData`, allocated with LocalAlloc; they are
    // copied and then freed exactly once.
    unsafe {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        v
    }
}

pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    let input = blob(plain);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the input blob points at `plain`, which outlives the call and is only read;
    // `out` is a valid out-parameter freed in `take`.
    unsafe {
        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .map_err(|e| anyhow!("CryptProtectData: {e}"))?;
    Ok(take(out))
}

pub fn unprotect(sealed: &[u8]) -> Result<Vec<u8>> {
    let input = blob(sealed);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .map_err(|e| anyhow!("CryptUnprotectData: {e}"))?;
    Ok(take(out))
}
