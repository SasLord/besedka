//! Minimal MIT keytab (`.keytab`) reader for the AD-SSO service key.
//!
//! Ported verbatim from trackly `crates/trackly-infra/src/ad/keytab.rs`
//! (spike 002) — the parsing is deterministic, network-free and fully
//! unit-tested, which is exactly why it survives the port unchanged.
//!
//! `sspi` validates an incoming Kerberos ticket by decrypting it with the
//! service's long-term key, and it wants that key as **raw bytes**
//! (`ServerProperties.ticket_decryption_key`) — it does not parse `.keytab`
//! files itself. On Windows/AD the admin produces the file with
//! `ktpass … /crypto AES256-SHA1 /out server.keytab`. This module reads that
//! file and pulls out the AES256-CTS-HMAC-SHA1-96 key (enctype 18) for the
//! configured SPN.
//!
//! Format reference: MIT krb5 keytab, version `0x0502` (big-endian) — the
//! format `ktpass` emits. Layout:
//! ```text
//! keytab       = u16 version (0x0502) , entry*
//! entry        = i32 size , entry_body   (size<=0 ⇒ a deleted "hole" of |size| bytes)
//! entry_body   = u16 num_components , realm , component{num_components} ,
//!                u32 name_type , u32 timestamp , u8 vno8 , keyblock , [u32 vno]
//! counted_str  = u16 len , u8[len]
//! keyblock     = u16 enctype , counted_str key
//! ```
//!
//! Read-only and total: any malformed input yields `Err`, never a panic.

/// AES256-CTS-HMAC-SHA1-96 — the enctype `ktpass /crypto AES256-SHA1` writes,
/// and the modern default across Windows Server 2008 R2+ / Windows 10-11
/// clients.
pub const ENCTYPE_AES256_CTS_HMAC_SHA1_96: u16 = 18;

const KEYTAB_VERSION_0502: u16 = 0x0502;

/// One decoded keytab key entry (only the fields we need to select the key).
#[derive(Debug, Clone)]
pub struct KeytabKey {
    /// Realm, e.g. `EXAMPLE.LOCAL`.
    pub realm: String,
    /// Principal name components, e.g. `["HTTP", "web.example.local"]`.
    pub components: Vec<String>,
    /// Kerberos enctype (18 = AES256-CTS-HMAC-SHA1-96).
    pub enctype: u16,
    /// Key version number.
    pub kvno: u32,
    /// Raw long-term key bytes (32 bytes for AES256).
    pub key: Vec<u8>,
}

impl KeytabKey {
    /// The SPN in `SERVICE/host` form (components joined by `/`), no realm.
    pub fn spn(&self) -> String {
        self.components.join("/")
    }
}

/// Error decoding a keytab blob. Deliberately does not carry secret bytes.
#[derive(Debug, thiserror::Error)]
pub enum KeytabError {
    #[error("keytab too short at offset {0} (truncated)")]
    Truncated(usize),
    #[error("unsupported keytab version {0:#06x} (only 0x0502 big-endian is supported)")]
    UnsupportedVersion(u16),
    #[error("no AES256 (enctype 18) key found for SPN {0:?} in keytab")]
    SpnNotFound(String),
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], KeytabError> {
        if self.remaining() < n {
            return Err(KeytabError::Truncated(self.pos));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, KeytabError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i32(&mut self) -> Result<i32, KeytabError> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u32(&mut self) -> Result<u32, KeytabError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u8(&mut self) -> Result<u8, KeytabError> {
        Ok(self.take(1)?[0])
    }
    /// A `counted_octet_string`: u16 length prefix + bytes.
    fn counted(&mut self) -> Result<&'a [u8], KeytabError> {
        let len = self.u16()? as usize;
        self.take(len)
    }
    fn counted_string(&mut self) -> Result<String, KeytabError> {
        Ok(String::from_utf8_lossy(self.counted()?).into_owned())
    }
}

/// Parse all key entries from a keytab byte blob (version 0x0502 / big-endian).
///
/// Deleted "hole" records (size ≤ 0) are skipped. A single malformed entry
/// aborts the whole parse with `Err` rather than silently returning a partial
/// set — a keytab we can only half-read is not one we should trust a login to.
pub fn parse_keytab(bytes: &[u8]) -> Result<Vec<KeytabKey>, KeytabError> {
    let mut cur = Cursor::new(bytes);
    let version = cur.u16()?;
    if version != KEYTAB_VERSION_0502 {
        return Err(KeytabError::UnsupportedVersion(version));
    }

    let mut keys = Vec::new();
    while cur.remaining() >= 4 {
        let size = cur.i32()?;
        if size <= 0 {
            // Hole / deleted record: skip |size| bytes and continue.
            let skip = size.unsigned_abs() as usize;
            cur.take(skip)?;
            continue;
        }
        // Bound the entry body to `size` so a corrupt inner length can't read
        // past it.
        let body = cur.take(size as usize)?;
        keys.push(parse_entry(body)?);
    }
    Ok(keys)
}

fn parse_entry(body: &[u8]) -> Result<KeytabKey, KeytabError> {
    let mut cur = Cursor::new(body);
    let num_components = cur.u16()? as usize;
    let realm = cur.counted_string()?;
    let mut components = Vec::with_capacity(num_components);
    for _ in 0..num_components {
        components.push(cur.counted_string()?);
    }
    let _name_type = cur.u32()?;
    let _timestamp = cur.u32()?;
    let vno8 = cur.u8()? as u32;
    let enctype = cur.u16()?;
    let key = cur.counted()?.to_vec();
    // Optional 4-byte kvno overrides vno8 when present.
    let kvno = if cur.remaining() >= 4 {
        cur.u32()?
    } else {
        vno8
    };

    Ok(KeytabKey {
        realm,
        components,
        enctype,
        kvno,
        key,
    })
}

/// Select the AES256 (enctype 18) key matching `spn` (case-insensitive on
/// components), choosing the highest `kvno` when several are present. `spn` is
/// `SERVICE/host` (e.g. `HTTP/web.example.local`), realm-agnostic.
pub fn aes256_key_for_spn(bytes: &[u8], spn: &str) -> Result<Vec<u8>, KeytabError> {
    let want = spn.to_ascii_lowercase();
    let best = parse_keytab(bytes)?
        .into_iter()
        .filter(|k| {
            k.enctype == ENCTYPE_AES256_CTS_HMAC_SHA1_96 && k.spn().to_ascii_lowercase() == want
        })
        .max_by_key(|k| k.kvno);
    match best {
        Some(k) => Ok(k.key),
        None => Err(KeytabError::SpnNotFound(spn.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid 0x0502 keytab with a single AES256 entry for
    /// `HTTP/web.example.local@EXAMPLE.LOCAL`, kvno 3, with a 32-byte key of
    /// 0xAB.
    fn sample_keytab() -> Vec<u8> {
        fn counted(s: &[u8]) -> Vec<u8> {
            let mut v = (s.len() as u16).to_be_bytes().to_vec();
            v.extend_from_slice(s);
            v
        }
        let mut body = Vec::new();
        body.extend_from_slice(&2u16.to_be_bytes()); // num_components
        body.extend_from_slice(&counted(b"EXAMPLE.LOCAL")); // realm
        body.extend_from_slice(&counted(b"HTTP")); // component 1
        body.extend_from_slice(&counted(b"web.example.local")); // component 2
        body.extend_from_slice(&1u32.to_be_bytes()); // name_type (KRB5_NT_PRINCIPAL)
        body.extend_from_slice(&0u32.to_be_bytes()); // timestamp
        body.push(3u8); // vno8
        body.extend_from_slice(&18u16.to_be_bytes()); // enctype AES256
        body.extend_from_slice(&counted(&[0xABu8; 32])); // key
        body.extend_from_slice(&3u32.to_be_bytes()); // optional u32 kvno

        let mut out = Vec::new();
        out.extend_from_slice(&0x0502u16.to_be_bytes()); // version
        out.extend_from_slice(&(body.len() as i32).to_be_bytes()); // entry size
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn parses_single_aes256_entry() {
        let kt = sample_keytab();
        let keys = parse_keytab(&kt).expect("parse");
        assert_eq!(keys.len(), 1);
        let k = &keys[0];
        assert_eq!(k.realm, "EXAMPLE.LOCAL");
        assert_eq!(k.spn(), "HTTP/web.example.local");
        assert_eq!(k.enctype, ENCTYPE_AES256_CTS_HMAC_SHA1_96);
        assert_eq!(k.kvno, 3);
        assert_eq!(k.key, vec![0xABu8; 32]);
    }

    #[test]
    fn selects_key_by_spn_case_insensitively() {
        let kt = sample_keytab();
        let key = aes256_key_for_spn(&kt, "http/WEB.example.LOCAL").expect("key");
        assert_eq!(key, vec![0xABu8; 32]);
    }

    #[test]
    fn missing_spn_is_error_not_panic() {
        let kt = sample_keytab();
        let err = aes256_key_for_spn(&kt, "HTTP/other.host").unwrap_err();
        assert!(matches!(err, KeytabError::SpnNotFound(_)));
    }

    #[test]
    fn wrong_version_rejected() {
        let bad = 0x0501u16.to_be_bytes().to_vec();
        assert!(matches!(
            parse_keytab(&bad),
            Err(KeytabError::UnsupportedVersion(0x0501))
        ));
    }

    #[test]
    fn truncated_input_is_error_not_panic() {
        // Version present, entry size says 100 bytes but body is missing.
        let mut bad = 0x0502u16.to_be_bytes().to_vec();
        bad.extend_from_slice(&100i32.to_be_bytes());
        assert!(matches!(parse_keytab(&bad), Err(KeytabError::Truncated(_))));
    }

    #[test]
    fn hole_record_is_skipped() {
        // A negative-size hole between the version and a real entry must be
        // skipped.
        let real = sample_keytab();
        let entry = &real[2..]; // size+body of the real entry (drop version)
        let mut kt = 0x0502u16.to_be_bytes().to_vec();
        kt.extend_from_slice(&(-8i32).to_be_bytes()); // hole: skip 8 bytes
        kt.extend_from_slice(&[0u8; 8]); // hole payload
        kt.extend_from_slice(entry); // then the real entry
        let keys = parse_keytab(&kt).expect("parse");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].spn(), "HTTP/web.example.local");
    }
}
