use core::mem::size_of;
use sha2::{Digest, Sha256};

/// Header binaire du format `.kald` v6 — 80 octets, little-endian.
///
/// Layout :
/// ```text
/// [0..4]   magic                b"KALD"
/// [4..6]   version              u16 = 6
/// [6..8]   variant_id           u16
/// [8..10]  epoch                u16
/// [10..12] range                u16
/// [12..16] entry_count          u32  — slots Timeline (YEAR_COUNT × 366)
/// [16..20] pool_offset          u32  — offset absolu du Secondary Pool
/// [20..24] pool_size            u32  — taille en octets
/// [24..28] registry_offset      u32  — offset absolu du Feast Registry (= 80)
/// [28..32] registry_count       u32  — nombre d'entrées FeastEntry
/// [32..34] feast_id_base        u16  — métadonnée diagnostique
/// [34..36] _reserved            u16  — nul
/// [36..68] checksum             [u8; 32] — SHA-256(Registry ∥ Timeline ∥ Pool)
/// [68..76] layout_discriminant  [u8; 8]
/// [76..80] _reserved2           [u8; 4]
/// ```
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Header {
    pub magic: [u8; 4],
    pub version: u16,
    pub variant_id: u16,
    pub epoch: u16,
    pub range: u16,
    pub entry_count: u32,
    pub pool_offset: u32,
    pub pool_size: u32,
    pub registry_offset: u32,
    pub registry_count: u32,
    pub feast_id_base: u16,
    pub _reserved: u16,
    pub checksum: [u8; 32],
    pub layout_discriminant: [u8; 8],
    pub _reserved2: [u8; 4],
}

const _: () = assert!(size_of::<Header>() == 80);

// ── Codes d'erreur internes ──────────────────────────────────────────────────

pub(crate) const ERR_BUF_TOO_SMALL: i32 = -2;
pub(crate) const ERR_MAGIC: i32 = -3;
pub(crate) const ERR_VERSION: i32 = -4;
pub(crate) const ERR_CHECKSUM: i32 = -5;
pub(crate) const ERR_FILE_SIZE: i32 = -6;
pub(crate) const ERR_SCHEMA: i32 = -10;

/// Valide les invariants structurels du header — **O(1), sans SHA-256**.
pub(crate) fn validate_header_fast(data: &[u8]) -> Result<Header, i32> {
    if data.len() < 80 {
        return Err(ERR_BUF_TOO_SMALL);
    }

    let magic = [data[0], data[1], data[2], data[3]];
    let version = u16::from_le_bytes([data[4], data[5]]);
    let variant_id = u16::from_le_bytes([data[6], data[7]]);
    let epoch = u16::from_le_bytes([data[8], data[9]]);
    let range = u16::from_le_bytes([data[10], data[11]]);
    let entry_count = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let pool_offset = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let pool_size = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
    let registry_offset = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let registry_count = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
    let feast_id_base = u16::from_le_bytes([data[32], data[33]]);

    let mut checksum = [0u8; 32];
    checksum.copy_from_slice(&data[36..68]);

    let stored_discriminant = u64::from_le_bytes([
        data[68], data[69], data[70], data[71], data[72], data[73], data[74], data[75],
    ]);

    if magic != *b"KALD" {
        return Err(ERR_MAGIC);
    }
    if version != crate::entry::KALD_FORMAT_VERSION {
        return Err(ERR_VERSION);
    }

    let registry_size = (registry_count as u64) * 4;
    let timeline_size = (entry_count as u64) * 8;
    let expected_pool_offset = 80u64 + registry_size + timeline_size;
    let expected_len = expected_pool_offset + pool_size as u64;

    if registry_offset != 80 {
        return Err(ERR_FILE_SIZE);
    }
    if pool_offset as u64 != expected_pool_offset {
        return Err(ERR_FILE_SIZE);
    }
    if data.len() as u64 != expected_len {
        return Err(ERR_FILE_SIZE);
    }

    if stored_discriminant != crate::entry::LAYOUT_DISCRIMINANT {
        return Err(ERR_SCHEMA);
    }

    Ok(Header {
        magic,
        version,
        variant_id,
        epoch,
        range,
        entry_count,
        pool_offset,
        pool_size,
        registry_offset,
        registry_count,
        feast_id_base,
        _reserved: 0,
        checksum,
        layout_discriminant: stored_discriminant.to_le_bytes(),
        _reserved2: [0; 4],
    })
}

/// Valide le header + intégrité SHA-256 — **O(payload_size)**.
pub(crate) fn validate_header(data: &[u8]) -> Result<Header, i32> {
    let header = validate_header_fast(data)?;

    let mut hasher = Sha256::new();
    hasher.update(&data[80..]);
    let computed = hasher.finalize();
    if computed.as_slice() != header.checksum {
        return Err(ERR_CHECKSUM);
    }

    Ok(header)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Construit un buffer `.kald` valide pour la version courante (`KALD_FORMAT_VERSION`).
    ///
    /// Renommé de `make_valid_kald_v5` en v6 : la version est désormais dynamique
    /// via `crate::entry::KALD_FORMAT_VERSION` — aucun hardcode.
    pub(crate) fn make_valid_kald(n_registry: u32, n_entries: u32) -> Vec<u8> {
        let registry_size = n_registry as usize * 4;
        let timeline_size = n_entries as usize * 8;
        let total = 80 + registry_size + timeline_size;
        let pool_offset = 80u32 + n_registry * 4 + n_entries * 8;

        let payload = vec![0u8; registry_size + timeline_size];
        let checksum = {
            let mut h = Sha256::new();
            h.update(&payload);
            h.finalize()
        };

        let mut buf = vec![0u8; total];
        buf[0..4].copy_from_slice(b"KALD");
        buf[4..6].copy_from_slice(&crate::entry::KALD_FORMAT_VERSION.to_le_bytes());
        buf[8..10].copy_from_slice(&1969u16.to_le_bytes());
        buf[10..12].copy_from_slice(&431u16.to_le_bytes());
        buf[12..16].copy_from_slice(&n_entries.to_le_bytes());
        buf[16..20].copy_from_slice(&pool_offset.to_le_bytes());
        buf[24..28].copy_from_slice(&80u32.to_le_bytes());
        buf[28..32].copy_from_slice(&n_registry.to_le_bytes());
        buf[36..68].copy_from_slice(checksum.as_slice());
        buf[68..76].copy_from_slice(&crate::entry::LAYOUT_DISCRIMINANT.to_le_bytes());
        buf[80..].copy_from_slice(&payload);
        buf
    }

    #[test]
    fn fast_accepts_corrupt_checksum() {
        let mut buf = make_valid_kald(2, 4);
        buf[80] = 0xFF;
        assert!(
            validate_header_fast(&buf).is_ok(),
            "validate_header_fast ne doit pas vérifier le SHA-256"
        );
        assert_eq!(validate_header(&buf), Err(ERR_CHECKSUM));
    }

    #[test]
    fn layout_header_size() {
        assert_eq!(size_of::<Header>(), 80);
    }

    #[test]
    fn valid_header_ok() {
        assert!(validate_header(&make_valid_kald(4, 8)).is_ok());
    }

    #[test]
    fn err_buf_too_small() {
        assert_eq!(validate_header(&[0u8; 79]), Err(ERR_BUF_TOO_SMALL));
    }

    #[test]
    fn err_magic() {
        let mut buf = make_valid_kald(0, 0);
        buf[0] = b'X';
        assert_eq!(validate_header(&buf), Err(ERR_MAGIC));
    }

    #[test]
    fn err_version() {
        let mut buf = make_valid_kald(0, 0);
        buf[4] = 4;
        assert_eq!(validate_header(&buf), Err(ERR_VERSION));
    }

    #[test]
    fn err_schema() {
        let mut buf = make_valid_kald(0, 0);
        buf[68] ^= 0xFF;
        assert_eq!(validate_header(&buf), Err(ERR_SCHEMA));
    }

    #[test]
    fn err_checksum() {
        let mut buf = make_valid_kald(2, 4);
        buf[80] = 0xFF;
        assert_eq!(validate_header(&buf), Err(ERR_CHECKSUM));
    }

    #[test]
    fn header_fields_roundtrip() {
        let buf = make_valid_kald(10, 366);
        let h = validate_header(&buf).unwrap();
        assert_eq!(h.registry_count, 10);
        assert_eq!(h.entry_count, 366);
        assert_eq!(h.registry_offset, 80);
        assert_eq!(h.pool_offset, 80 + 10 * 4 + 366 * 8);
    }
}
