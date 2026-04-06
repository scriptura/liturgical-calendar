// liturgical-calendar-core/src/header.rs
//
// Header du format binaire `.kald` v2.0.
// Layout : 64 octets, align 8 (spec §3.2).
// Toutes les lectures numériques utilisent `from_le_bytes` (LE canonique).

use sha2::{Digest, Sha256};

/// Erreurs de validation du header `.kald`.
///
/// `Copy`, sans allocation — compatible `no_std` (INV-W1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Taille du buffer < 64 octets, ou incohérence `entry_count`/`pool_size`.
    FileTooSmall,
    /// `file_size != 64 + entry_count * 8 + pool_size`.
    FileSizeMismatch,
    /// Les 4 premiers octets ne sont pas `b"KALD"`.
    InvalidMagic,
    /// Version non supportée (attendu : 4).
    UnsupportedVersion(u16),
    /// Champ `_reserved` non nul.
    ReservedNotZero,
    /// SHA-256 calculé ≠ checksum stocké dans le header.
    ChecksumMismatch,
}

/// Header du fichier `.kald` v2.0.
///
/// Layout `#[repr(C, align(8))]` : 64 octets, alignement 8.
/// Toutes les valeurs numériques sont encodées en Little-Endian (LE canonique).
///
/// # Invariant de taille de fichier
/// ```text
/// file_size == 64 + (entry_count × 8) + pool_size
/// ```
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Magic bytes : `b"KALD"` (0x4B414C44). Offset 0.
    pub magic: [u8; 4],
    /// Version du format binaire. Valeur attendue : `4`. Offset 4.
    pub version: u16,
    /// Identifiant de rite. `0` = Ordinaire. Offset 6.
    pub variant_id: u16,
    /// Première année couverte. Valeur attendue : `1969`. Offset 8.
    pub epoch: u16,
    /// Nombre d'années couvertes. Valeur attendue : `431`. Offset 10.
    pub range: u16,
    /// Nombre total de `CalendarEntry`. Invariant : `range × 366`. Offset 12.
    pub entry_count: u32,
    /// Offset en octets depuis le début du fichier vers le Secondary Pool. Offset 16.
    pub pool_offset: u32,
    /// Taille en octets du Secondary Pool. Offset 20.
    pub pool_size: u32,
    /// SHA-256 sur `[Data Body ∥ Secondary Pool]` (header exclu). Offset 24.
    pub checksum: [u8; 32],
    /// Padding réservé. Doit être `0x00 × 8`. Offset 56.
    pub _reserved: [u8; 8],
}

// Assertion statique de layout — vérification à la compilation.
const _: () = {
    assert!(core::mem::size_of::<Header>() == 64, "Header doit faire exactement 64 octets");
};

/// Valide le header d'un fichier `.kald` et le désérialise.
///
/// Les validations sont séquentielles — arrêt au premier échec (spec §7.1).
///
/// # Sécurité
/// `bytes` est un buffer immutable fourni par l'appelant.
/// Aucune allocation interne.
///
/// # Erreurs
/// Voir [`HeaderError`] pour la liste exhaustive des cas d'échec.
pub fn validate_header(bytes: &[u8]) -> Result<Header, HeaderError> {
    // 1. Taille minimale (64 octets) avant tout accès.
    if bytes.len() < 64 {
        return Err(HeaderError::FileTooSmall);
    }

    // Désérialisation directe depuis le buffer (LE canonique).
    // SAFETY : bytes.len() >= 64, tous les accès sont dans les bornes.
    let magic        = [bytes[0], bytes[1], bytes[2], bytes[3]];
    let version      = u16::from_le_bytes([bytes[4], bytes[5]]);
    let variant_id   = u16::from_le_bytes([bytes[6], bytes[7]]);
    let epoch        = u16::from_le_bytes([bytes[8], bytes[9]]);
    let range        = u16::from_le_bytes([bytes[10], bytes[11]]);
    let entry_count  = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let pool_offset  = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let pool_size    = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let checksum: [u8; 32] = bytes[24..56].try_into().unwrap(); // slice de taille fixe
    let reserved: [u8; 8] = bytes[56..64].try_into().unwrap();

    // 2. Magic.
    if magic != *b"KALD" {
        return Err(HeaderError::InvalidMagic);
    }

    // 3. Version.
    if version != 4 {
        return Err(HeaderError::UnsupportedVersion(version));
    }

    // 4. Cohérence de taille de fichier.
    // file_size == 64 + entry_count * 8 + pool_size
    // Calcul en u64 pour éviter tout débordement sur les valeurs limites.
    let expected_size: u64 = 64u64
        .saturating_add(entry_count as u64 * 8)
        .saturating_add(pool_size as u64);
    if bytes.len() as u64 != expected_size {
        return Err(HeaderError::FileSizeMismatch);
    }

    // 5. Champ _reserved nul.
    if reserved != [0u8; 8] {
        return Err(HeaderError::ReservedNotZero);
    }

    // 6. SHA-256 sur [Data Body ∥ Secondary Pool] (bytes[64..]).
    // Implémentation streaming — aucune allocation (spec §7.1).
    // L'état interne du hasher (~208 octets) est sur la pile.
    let payload = &bytes[64..]; // validé : bytes.len() >= 64
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize(); // [u8; 32] sur la pile
    if computed.as_slice() != checksum {
        return Err(HeaderError::ChecksumMismatch);
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

// ─────────────────────────────────────────────────────────────────────────────
// Tests unitaires — tâche 1.2 roadmap
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};
    use sha2::{Digest, Sha256};

    /// Construit un buffer `.kald` valide minimal avec un Data Body vide.
    fn build_valid_kald(entry_count: u32, pool_size: u32) -> Vec<u8> {
        let data_body_size = entry_count as usize * 8;
        let total = 64 + data_body_size + pool_size as usize;
        let mut buf = vec![0u8; total];

        // Magic
        buf[0..4].copy_from_slice(b"KALD");
        // Version = 4
        buf[4..6].copy_from_slice(&4u16.to_le_bytes());
        // variant_id = 0
        buf[6..8].copy_from_slice(&0u16.to_le_bytes());
        // epoch = 1969
        buf[8..10].copy_from_slice(&1969u16.to_le_bytes());
        // range = 431
        buf[10..12].copy_from_slice(&431u16.to_le_bytes());
        // entry_count
        buf[12..16].copy_from_slice(&entry_count.to_le_bytes());
        // pool_offset = 64 + entry_count * 8
        let pool_offset = 64u32 + entry_count * 8;
        buf[16..20].copy_from_slice(&pool_offset.to_le_bytes());
        // pool_size
        buf[20..24].copy_from_slice(&pool_size.to_le_bytes());
        // _reserved = 0 (déjà zéro par vec![0u8])

        // SHA-256 sur bytes[64..]
        let payload = &buf[64..].to_vec(); // clone pour le calcul
        let checksum = Sha256::digest(payload);
        buf[24..56].copy_from_slice(checksum.as_slice());

        buf
    }

    #[test]
    fn header_size_is_64() {
        assert_eq!(size_of::<Header>(), 64);
    }

    #[test]
    fn header_align_at_least_8() {
        assert!(align_of::<Header>() >= 8);
    }

    /// Calcule l'offset d'un champ via addr_of! — portable Rust 1.51+.
    /// Production : utiliser offset_of! (stable 1.77, INV-W9).
    macro_rules! field_offset {
        ($type:ty, $field:ident) => {{
            let u = core::mem::MaybeUninit::<$type>::uninit();
            // SAFETY : calcul d'adresse uniquement, pas de déréférencement.
            unsafe {
                core::ptr::addr_of!((*u.as_ptr()).$field) as usize
                    - u.as_ptr() as usize
            }
        }};
    }

    #[test]
    fn header_field_offsets() {
        assert_eq!(field_offset!(Header, magic),       0);
        assert_eq!(field_offset!(Header, version),     4);
        assert_eq!(field_offset!(Header, variant_id),  6);
        assert_eq!(field_offset!(Header, epoch),       8);
        assert_eq!(field_offset!(Header, range),       10);
        assert_eq!(field_offset!(Header, entry_count), 12);
        assert_eq!(field_offset!(Header, pool_offset), 16);
        assert_eq!(field_offset!(Header, pool_size),   20);
        assert_eq!(field_offset!(Header, checksum),    24);
        assert_eq!(field_offset!(Header, _reserved),   56);
    }

    #[test]
    fn valid_header_roundtrip() {
        let buf = build_valid_kald(0, 0);
        let hdr = validate_header(&buf).expect("header valide doit passer");
        assert_eq!(hdr.magic, *b"KALD");
        assert_eq!(hdr.version, 4);
        assert_eq!(hdr.entry_count, 0);
    }

    #[test]
    fn error_file_too_small() {
        let buf = vec![0u8; 32]; // < 64
        assert_eq!(validate_header(&buf), Err(HeaderError::FileTooSmall));
    }

    #[test]
    fn error_invalid_magic() {
        let mut buf = build_valid_kald(0, 0);
        buf[0] = b'X';
        assert_eq!(validate_header(&buf), Err(HeaderError::InvalidMagic));
    }

    #[test]
    fn error_unsupported_version() {
        let mut buf = build_valid_kald(0, 0);
        buf[4..6].copy_from_slice(&3u16.to_le_bytes()); // version 3
        // Recalcul checksum non nécessaire — version check précède checksum.
        assert_eq!(
            validate_header(&buf),
            Err(HeaderError::UnsupportedVersion(3))
        );
    }

    #[test]
    fn error_file_size_mismatch() {
        let mut buf = build_valid_kald(0, 0);
        // Déclare entry_count = 1 mais le buffer ne contient pas les 8 octets d'entrée.
        buf[12..16].copy_from_slice(&1u32.to_le_bytes());
        // Pas de recalcul checksum — file size check précède checksum.
        assert_eq!(validate_header(&buf), Err(HeaderError::FileSizeMismatch));
    }

    #[test]
    fn error_reserved_not_zero() {
        let mut buf = build_valid_kald(0, 0);
        buf[56] = 0xFF; // _reserved[0] non nul
        // Recalcul du checksum pour que le SHA-256 ne soit pas la cause de l'erreur.
        // reserved check précède checksum check.
        assert_eq!(validate_header(&buf), Err(HeaderError::ReservedNotZero));
    }

    #[test]
    fn error_checksum_mismatch() {
        let mut buf = build_valid_kald(0, 0);
        buf[24] ^= 0xFF; // corrompt le premier octet du checksum
        assert_eq!(validate_header(&buf), Err(HeaderError::ChecksumMismatch));
    }
}
