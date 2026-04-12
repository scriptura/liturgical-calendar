use core::mem::size_of;
use sha2::{Digest, Sha256};

/// Header binaire du format `.kald` — 64 octets, little-endian.
///
/// Invariant de layout : `size_of::<Header>() == 64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Header {
    /// Signature : `b"KALD"`.
    pub magic: [u8; 4],
    /// Version du format : doit valoir `4`.
    pub version: u16,
    /// Identifiant de variante (`0` = Ordinaire).
    pub variant_id: u16,
    /// Année d'époque : `1969`.
    pub epoch: u16,
    /// Nombre d'années couvertes : `431`.
    pub range: u16,
    /// Nombre total d'entrées dans le Data Body.
    pub entry_count: u32,
    /// Offset en octets du Secondary Pool depuis le début du fichier.
    pub pool_offset: u32,
    /// Taille en octets du Secondary Pool.
    pub pool_size: u32,
    /// SHA-256(`Data Body ∥ Secondary Pool`).
    pub checksum: [u8; 32],
    /// Padding réservé, doit être `0x00 × 8`.
    pub _reserved: [u8; 8],
}

// Assertion statique de layout — évaluée à la compilation.
const _: () = assert!(size_of::<Header>() == 64);

// ── Codes d'erreur internes (miroir des constantes FFI) ──────────────────────
// ERR_NULL_PTR absent : le NULL check est assuré par la couche FFI avant tout
// appel à validate_header.

pub(crate) const ERR_BUF_TOO_SMALL: i32 = -2; // len < 64, spec §kal_validate_header step 2
pub(crate) const ERR_MAGIC: i32 = -3;
pub(crate) const ERR_VERSION: i32 = -4;
pub(crate) const ERR_CHECKSUM: i32 = -5;
pub(crate) const ERR_FILE_SIZE: i32 = -6;
pub(crate) const ERR_RESERVED: i32 = -9;

/// Valide le header et retourne `Ok(Header)` si toutes les vérifications passent.
///
/// Validations séquentielles (arrêt au premier échec) :
/// 1. `len >= 64`             → `ERR_BUF_TOO_SMALL`
/// 2. `magic == b"KALD"`     → `ERR_MAGIC`
/// 3. `version == 4`         → `ERR_VERSION`
/// 4. taille fichier cohérente → `ERR_FILE_SIZE`
/// 5. `_reserved == [0; 8]`  → `ERR_RESERVED`
/// 6. SHA-256 correct         → `ERR_CHECKSUM`
///
/// `data` doit être non-NULL (vérifié par l'appelant FFI).
pub(crate) fn validate_header(data: &[u8]) -> Result<Header, i32> {
    // 1. Taille minimale — spec step 2 : KAL_ERR_BUF_TOO_SMALL
    if data.len() < 64 {
        return Err(ERR_BUF_TOO_SMALL);
    }

    // Lecture des champs par `from_le_bytes` — pas de déréférencement aligné.
    let magic = [data[0], data[1], data[2], data[3]];
    let version = u16::from_le_bytes([data[4], data[5]]);
    let variant_id = u16::from_le_bytes([data[6], data[7]]);
    let epoch = u16::from_le_bytes([data[8], data[9]]);
    let range = u16::from_le_bytes([data[10], data[11]]);
    let entry_count = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let pool_offset = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let pool_size = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);

    let mut checksum = [0u8; 32];
    checksum.copy_from_slice(&data[24..56]);

    let reserved = [
        data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
    ];

    // 2. Magic
    if magic != *b"KALD" {
        return Err(ERR_MAGIC);
    }

    // 3. Version
    if version != 4 {
        return Err(ERR_VERSION);
    }

    // 4. Cohérence de taille fichier
    let body_size = (entry_count as u64) * 8;
    let expected_len = 64u64 + body_size + pool_size as u64;
    if data.len() as u64 != expected_len {
        return Err(ERR_FILE_SIZE);
    }

    // 5. Invariant structurel
    if pool_offset as u64 != 64 + (entry_count as u64) * 8 {
        return Err(ERR_FILE_SIZE);
    }

    // 6. Champ réservé nul
    if reserved != [0u8; 8] {
        return Err(ERR_RESERVED);
    }

    // 7. Checksum SHA-256(Data Body ∥ Secondary Pool)
    let payload = &data[64..];
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize(); // [u8; 32] sur la pile
    if computed.as_slice() != checksum {
        return Err(ERR_CHECKSUM);
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
        checksum,
        _reserved: reserved,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Construit un buffer `.kald` minimal valide avec `n_entries` entrées nulles.
    pub(crate) fn make_valid_kald(n_entries: u32) -> Vec<u8> {
        let body_len = n_entries as usize * 8;
        let pool_size: u32 = 0;
        let total = 64 + body_len;
        let mut buf = vec![0u8; total];

        // Checksum calculé sur le payload (Data Body ∥ pool vide) avant écriture header.
        let mut hasher = Sha256::new();
        hasher.update(&buf[64..]);
        let checksum = hasher.finalize();

        buf[0..4].copy_from_slice(b"KALD");
        buf[4..6].copy_from_slice(&4u16.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // variant_id
        buf[8..10].copy_from_slice(&1969u16.to_le_bytes());
        buf[10..12].copy_from_slice(&431u16.to_le_bytes());
        buf[12..16].copy_from_slice(&n_entries.to_le_bytes());
        buf[16..20].copy_from_slice(&(64u32 + n_entries * 8).to_le_bytes()); // pool_offset
        buf[20..24].copy_from_slice(&pool_size.to_le_bytes());
        buf[24..56].copy_from_slice(checksum.as_slice());
        // _reserved octets 56..64 = 0 (vec initialisé à 0)

        buf
    }

    #[test]
    fn layout_header_size() {
        assert_eq!(size_of::<Header>(), 64);
    }

    #[test]
    fn valid_header_ok() {
        let buf = make_valid_kald(4);
        assert!(validate_header(&buf).is_ok());
    }

    #[test]
    fn err_buf_too_small() {
        // len < 64 → ERR_BUF_TOO_SMALL (spec §kal_validate_header step 2)
        let buf = [0u8; 63];
        assert_eq!(validate_header(&buf), Err(ERR_BUF_TOO_SMALL));
    }

    #[test]
    fn err_magic() {
        let mut buf = make_valid_kald(0);
        buf[0] = b'X';
        assert_eq!(validate_header(&buf), Err(ERR_MAGIC));
    }

    #[test]
    fn err_version() {
        let mut buf = make_valid_kald(0);
        buf[4] = 3;
        assert_eq!(validate_header(&buf), Err(ERR_VERSION));
    }

    #[test]
    fn err_file_size() {
        let mut buf = make_valid_kald(2);
        // entry_count déclaré à 99 mais taille réelle = 2 entrées → mismatch
        buf[12..16].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(validate_header(&buf), Err(ERR_FILE_SIZE));
    }

    #[test]
    fn err_reserved() {
        let mut buf = make_valid_kald(0);
        buf[56] = 0xFF; // _reserved non nul
        assert_eq!(validate_header(&buf), Err(ERR_RESERVED));
    }

    #[test]
    fn err_checksum() {
        let mut buf = make_valid_kald(2);
        buf[64] = 0xFF; // corruption Data Body sans recompute
        assert_eq!(validate_header(&buf), Err(ERR_CHECKSUM));
    }
}
