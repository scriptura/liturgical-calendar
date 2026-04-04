// SPDX-License-Identifier: MIT
// liturgical-calendar-forge — Étape 5 : Binary Packing (roadmap §2.5)
//
// Sérialisation LE canonique → fichier .kald v2.0.
// Calcul SHA-256 sur [Data Body ∥ Secondary Pool] (header exclu — spec §3.2).
// Construction du Header 64 octets.
//
// Format :
//   [ Header : 64 octets ]
//   [ Data Body : entry_count × 8 octets ]
//   [ Secondary Pool : pool_size octets ]

use sha2::{Digest, Sha256};

use crate::materialization::{CalendarEntry, PoolBuilder};

// ─── Constantes du format .kald ───────────────────────────────────────────────

pub const MAGIC:          [u8; 4] = *b"KALD"; // 0x4B414C44
pub const FORMAT_VERSION: u16     = 4;
pub const HEADER_SIZE:    usize   = 64;

// ─── Encodage des flags (spec §3.4 + §7 Mapping) ─────────────────────────────

/// Encode les 4 composantes dans `flags u16`.
///
/// ```
/// flags = Precedence | (Color << 4) | (Season << 8) | (Nature << 11)
/// ```
///
/// Bits [15:14] doivent être nuls — `& 0x3FFF` appliqué pour sécurité.
#[inline]
pub fn encode_flags(precedence: u8, color: u8, season: u8, nature: u8) -> u16 {
    ((precedence as u16)
        | ((color   as u16) << 4)
        | ((season  as u16) << 8)
        | ((nature  as u16) << 11))
    & 0x3FFF // bits 14–15 toujours nuls
}

// ─── Sérialisation du Header (spec §3.2) ─────────────────────────────────────

/// Construit le header 64 octets en LE.
///
/// Champs :
///  [0..4]   magic       : b"KALD"
///  [4..6]   version     : 4 (LE)
///  [6..8]   variant_id  : 0 (Ordinaire)
///  [8..10]  epoch       : 1969 (LE) — première année couverte
///  [10..12] range       : `range` (LE)
///  [12..16] entry_count : `range * 366` (LE)
///  [16..20] pool_offset : 64 + entry_count * 8 (LE, en octets depuis début fichier)
///  [20..24] pool_size   : taille pool en octets (LE)
///  [24..56] checksum    : SHA-256 sur [Data Body ∥ Secondary Pool]
///  [56..64] _reserved   : 0x00 × 8
pub fn build_header(
    epoch:       u16,
    range:       u16,
    entry_count: u32,
    pool_offset: u32,
    pool_size:   u32,
    checksum:    [u8; 32],
) -> [u8; 64] {
    let mut hdr = [0u8; 64];

    hdr[0..4].copy_from_slice(&MAGIC);
    hdr[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    hdr[6..8].copy_from_slice(&0u16.to_le_bytes());         // variant_id = 0
    hdr[8..10].copy_from_slice(&epoch.to_le_bytes());
    hdr[10..12].copy_from_slice(&range.to_le_bytes());
    hdr[12..16].copy_from_slice(&entry_count.to_le_bytes());
    hdr[16..20].copy_from_slice(&pool_offset.to_le_bytes());
    hdr[20..24].copy_from_slice(&pool_size.to_le_bytes());
    hdr[24..56].copy_from_slice(&checksum);
    // hdr[56..64] = 0x00 (déjà zéro par construction)

    hdr
}

// ─── Sérialisation complète → Vec<u8> ────────────────────────────────────────

/// Sérialise le calendrier en binaire .kald v2.0.
///
/// # Paramètres
/// - `epoch`   : première année couverte (1969 pour un fichier standard)
/// - `range`   : nombre d'années couvertes
/// - `entries` : `range * 366` `CalendarEntry` dans l'ordre (année × DOY)
/// - `pool`    : Secondary Pool construit par `PoolBuilder`
///
/// # Retour
/// `Vec<u8>` contenant le fichier .kald complet, validable par `kal_validate_header`.
pub fn pack_kald(
    epoch:   u16,
    range:   u16,
    entries: &[CalendarEntry],
    pool:    &PoolBuilder,
) -> Vec<u8> {
    let entry_count = range as u32 * 366;
    debug_assert_eq!(
        entries.len() as u32, entry_count,
        "entries.len()={} ≠ entry_count={entry_count}",
        entries.len()
    );

    // ── Sérialisation Data Body ────────────────────────────────────────────
    let mut data_body = Vec::with_capacity(entries.len() * 8);
    for entry in entries {
        data_body.extend_from_slice(&entry.to_le_bytes());
    }

    // ── Sérialisation Secondary Pool ──────────────────────────────────────
    let pool_bytes = pool.to_le_bytes();
    let pool_size  = pool_bytes.len() as u32;
    let pool_offset = HEADER_SIZE as u32 + data_body.len() as u32;

    // ── SHA-256 sur [Data Body ∥ Secondary Pool] (header exclu — spec §3.2) ─
    let mut hasher = Sha256::new();
    hasher.update(&data_body);
    hasher.update(&pool_bytes);
    let checksum: [u8; 32] = hasher.finalize().into();

    // ── Construction du header ─────────────────────────────────────────────
    let hdr = build_header(epoch, range, entry_count, pool_offset, pool_size, checksum);

    // ── Assemblage final : Header + Data Body + Secondary Pool ────────────
    let total = HEADER_SIZE + data_body.len() + pool_bytes.len();
    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(&hdr);
    output.extend_from_slice(&data_body);
    output.extend_from_slice(&pool_bytes);

    output
}

// ─── Tests unitaires ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Vérifie la cohérence structurelle d'un .kald minimal.
    #[test]
    fn pack_kald_structure() {
        // 1 an, 0 fêtes, 0 commémorations
        let entries = vec![CalendarEntry::ZERO; 366];
        let pool    = PoolBuilder::new();
        let kald    = pack_kald(1969, 1, &entries, &pool);

        // Taille attendue : 64 + 366*8 + 0 = 64 + 2928 = 2992
        assert_eq!(kald.len(), 64 + 366 * 8);

        // Magic
        assert_eq!(&kald[0..4], b"KALD");
        // Version
        assert_eq!(u16::from_le_bytes([kald[4], kald[5]]), 4);
        // variant_id = 0
        assert_eq!(u16::from_le_bytes([kald[6], kald[7]]), 0);
        // epoch = 1969
        assert_eq!(u16::from_le_bytes([kald[8], kald[9]]), 1969);
        // range = 1
        assert_eq!(u16::from_le_bytes([kald[10], kald[11]]), 1);
        // entry_count = 366
        assert_eq!(u32::from_le_bytes([kald[12], kald[13], kald[14], kald[15]]), 366);
        // pool_offset = 64 + 366*8 = 2992
        assert_eq!(u32::from_le_bytes([kald[16], kald[17], kald[18], kald[19]]), 2992);
        // pool_size = 0
        assert_eq!(u32::from_le_bytes([kald[20], kald[21], kald[22], kald[23]]), 0);
        // reserved = 0
        assert_eq!(&kald[56..64], &[0u8; 8]);
    }

    #[test]
    fn pack_kald_checksum_deterministic() {
        let entries = vec![CalendarEntry::ZERO; 366];
        let pool    = PoolBuilder::new();
        let kald1   = pack_kald(1969, 1, &entries, &pool);
        let kald2   = pack_kald(1969, 1, &entries, &pool);
        // Déterminisme bit-for-bit
        assert_eq!(kald1, kald2);
        // Checksum identique
        assert_eq!(&kald1[24..56], &kald2[24..56]);
    }

    #[test]
    fn pack_kald_entry_count_formula() {
        // Vérifier entry_count = range * 366 pour différentes plages
        for range in [1u16, 10, 57, 431] {
            let entries = vec![CalendarEntry::ZERO; range as usize * 366];
            let pool    = PoolBuilder::new();
            let kald    = pack_kald(1969, range, &entries, &pool);
            let ec = u32::from_le_bytes([kald[12], kald[13], kald[14], kald[15]]);
            assert_eq!(ec, range as u32 * 366);
        }
    }

    #[test]
    fn encode_flags_nativitas_example() {
        // Spec §9.1 : Nativitas Domini → flags = 0x0201
        // Precedence=1, Color=0 (Albus), Season=2 (TempusNativitatis), Nature=0 (Sollemnitas)
        let flags = encode_flags(1, 0, 2, 0);
        assert_eq!(flags, 0x0201);
    }

    #[test]
    fn encode_flags_reserved_bits_zero() {
        // Quelle que soit la combinaison, bits 14–15 doivent être nuls
        let flags = encode_flags(12, 5, 6, 4); // valeurs max admises
        assert_eq!(flags & 0xC000, 0);
    }

    #[test]
    fn file_size_invariant() {
        // file_size == 64 + entry_count * 8 + pool_size
        let range   = 57u16;
        let entries = vec![CalendarEntry::ZERO; range as usize * 366];
        let mut pool = PoolBuilder::new();
        pool.insert(vec![0x0001, 0x0002]).unwrap();
        let kald = pack_kald(1969, range, &entries, &pool);

        let entry_count = u32::from_le_bytes([kald[12], kald[13], kald[14], kald[15]]);
        let pool_size   = u32::from_le_bytes([kald[20], kald[21], kald[22], kald[23]]);
        let expected    = 64 + entry_count as usize * 8 + pool_size as usize;
        assert_eq!(kald.len(), expected);
    }
}
