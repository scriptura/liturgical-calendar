// header.rs — Header binaire .kald v2.0 (specification.md §3.2).
//
// Layout fixé (64 octets, #[repr(C)]) :
//   [0..4]   magic       [u8; 4]   "KALD"
//   [4..6]   version     u16       4
//   [6..8]   variant_id  u16       0 = Ordinaire
//   [8..10]  epoch       u16       1969
//   [10..12] range       u16       431 (années couvertes)
//   [12..16] entry_count u32       range × 366
//   [16..20] pool_offset u32       offset bytes → Secondary Pool
//   [20..24] pool_size   u32       taille bytes du Secondary Pool
//   [24..56] checksum    [u8; 32]  SHA-256([Data Body ∥ Secondary Pool])
//   [56..64] _reserved   [u8; 8]   0x00 × 8
//
// Toutes les valeurs numériques : Little-Endian canonique (from_le_bytes).
// Politique LE exclusive : aucun from_ne_bytes / to_ne_bytes autorisé.

use core::mem;
use sha2::{Digest, Sha256};

/// Taille fixe du header en octets.
pub const HEADER_SIZE: usize = 64;

/// Magic bytes attendus : ASCII "KALD" = 0x4B414C44.
pub const KALD_MAGIC: [u8; 4] = *b"KALD";

/// Version du format binaire .kald supportée par cet Engine.
pub const KALD_VERSION: u16 = 4;

// ─── Assertion de taille statique — vérifiée à la compilation ────────────────
// Si le layout change (ajout de champ, mauvais repr), le build échoue.
const _ASSERT_HEADER_SIZE: () = assert!(mem::size_of::<Header>() == HEADER_SIZE);

// ─── Structure Header ─────────────────────────────────────────────────────────

/// Header du fichier `.kald` v2.0 (64 octets, `#[repr(C)]`).
///
/// Tous les champs numériques sont stockés en Little-Endian dans le fichier
/// et désérialisés via `from_le_bytes` — déterminisme cross-platform garanti.
///
/// Le Build ID (`checksum[0..8]`) identifie de façon unique un build spécifique
/// sans champ supplémentaire (specification.md §3.2).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Magic bytes : `b"KALD"`.
    pub magic: [u8; 4],
    /// Version du format : doit valoir `4`.
    pub version: u16,
    /// Identifiant de variante : `0` = Ordinaire.
    pub variant_id: u16,
    /// Première année couverte : `1969`.
    pub epoch: u16,
    /// Nombre d'années couvertes : `431`.
    pub range: u16,
    /// Nombre total d'entrées : `range × 366`.
    pub entry_count: u32,
    /// Offset en octets depuis le début du fichier vers le Secondary Pool.
    /// Vaut `64 + entry_count * 8` pour un fichier sans padding.
    pub pool_offset: u32,
    /// Taille en octets du Secondary Pool.
    pub pool_size: u32,
    /// SHA-256 sur `[Data Body ∥ Secondary Pool]` (header exclu).
    pub checksum: [u8; 32],
    /// Padding réservé : doit être `0x00 × 8` à la validation.
    pub _reserved: [u8; 8],
}

// ─── Erreurs de validation du header ─────────────────────────────────────────

/// Erreurs possibles lors de la validation d'un header `.kald`.
///
/// Ordre de détection canonique : taille → magic → version → file_size
/// → reserved → checksum (specification.md §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    /// Buffer inférieur à 64 octets — impossible de lire un header complet.
    FileTooSmall,
    /// Magic bytes != `b"KALD"`.
    InvalidMagic,
    /// Version != 4. Porte la version trouvée pour diagnostic.
    UnsupportedVersion(u16),
    /// `len != 64 + entry_count * 8 + pool_size`.
    FileSizeMismatch,
    /// `_reserved != [0u8; 8]`.
    ReservedNotZero,
    /// SHA-256 calculé != `checksum` stocké dans le header.
    ChecksumMismatch,
}

// ─── Désérialisation et validation ───────────────────────────────────────────

/// Valide et désérialise un header `.kald` depuis un buffer de bytes bruts.
///
/// Validations exécutées dans l'ordre canonique (arrêt au premier échec) :
/// 1. `bytes.len() >= 64`
/// 2. `magic == b"KALD"`
/// 3. `version == 4`
/// 4. `bytes.len() == 64 + entry_count * 8 + pool_size`
/// 5. `_reserved == [0u8; 8]`
/// 6. SHA-256(`bytes[64..]`) == `checksum`
///
/// Implémentation SHA-256 streaming — aucune allocation intermédiaire (INV-W1).
pub fn validate_header(bytes: &[u8]) -> Result<Header, HeaderError> {
    // ── 1. Taille minimale ────────────────────────────────────────────────────
    if bytes.len() < HEADER_SIZE {
        return Err(HeaderError::FileTooSmall);
    }
    // À partir d'ici, bytes[0..64] est garanti accessible.
    // Les unwrap() sur try_into() ci-dessous sont infaillibles (slices exactes).

    // ── Désérialisation LE canonique ─────────────────────────────────────────
    let magic: [u8; 4] = bytes[0..4].try_into().unwrap();
    let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    let variant_id = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    let epoch = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    let range = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
    let entry_count = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let pool_offset = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let pool_size = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let checksum: [u8; 32] = bytes[24..56].try_into().unwrap();
    let reserved: [u8; 8] = bytes[56..64].try_into().unwrap();

    // ── 2. Magic ──────────────────────────────────────────────────────────────
    if magic != KALD_MAGIC {
        return Err(HeaderError::InvalidMagic);
    }

    // ── 3. Version ────────────────────────────────────────────────────────────
    if version != KALD_VERSION {
        return Err(HeaderError::UnsupportedVersion(version));
    }

    // ── 4. Cohérence de taille fichier ────────────────────────────────────────
    // Calcul en u64 pour éviter tout overflow sur les valeurs maximales :
    //   entry_count_max = 431 × 366 = 157 746
    //   data_body_max   = 157 746 × 8 = 1 261 968 octets ≈ 1.2 MiB — tient en u32.
    //   pool_size_max   = 65 535 × 2  = 131 070 octets.
    //   total_max       = 64 + 1 261 968 + 131 070 = 1 393 102 — tient en u32.
    // Promotion u64 préventive pour pool_size arbitrairement grand (fichier corrompu).
    let expected_len: u64 = 64u64
        .saturating_add(entry_count as u64 * 8)
        .saturating_add(pool_size as u64);
    if bytes.len() as u64 != expected_len {
        return Err(HeaderError::FileSizeMismatch);
    }

    // ── 5. Reserved doit être nul ─────────────────────────────────────────────
    if reserved != [0u8; 8] {
        return Err(HeaderError::ReservedNotZero);
    }

    // ── 6. SHA-256 streaming — aucune allocation (INV-W1) ─────────────────────
    // Périmètre du hash : bytes[64..] = Data Body ∥ Secondary Pool.
    // Sha256::new() et finalize() opèrent sur la pile (~208 octets d'état interne).
    let payload = &bytes[HEADER_SIZE..];
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize();
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    /// Calcul d'offset de champ sans offset_of! (stabilisé en 1.77).
    /// Utilise addr_of! sur MaybeUninit — aucun déréférencement, 100% sûr.
    macro_rules! field_offset {
        ($type:ty, $field:ident) => {{
            let uninit = core::mem::MaybeUninit::<$type>::uninit();
            let base = uninit.as_ptr() as usize;
            let field = unsafe { core::ptr::addr_of!((*uninit.as_ptr()).$field) } as usize;
            field - base
        }};
    }

    // ── Helpers de construction d'un KALD de test ─────────────────────────────

    /// Sérialise N entrées vides en Data Body LE.
    fn empty_data_body(entry_count: u32) -> Vec<u8> {
        vec![0u8; entry_count as usize * 8]
    }

    /// Construit un buffer .kald minimal valide avec les paramètres donnés.
    ///
    /// `entries_bytes` : Data Body déjà sérialisé en LE (entry_count × 8 octets).
    /// `pool_bytes`    : Secondary Pool (peut être vide).
    fn build_kald(epoch: u16, range: u16, entries_bytes: &[u8], pool_bytes: &[u8]) -> Vec<u8> {
        let entry_count = (entries_bytes.len() / 8) as u32;
        let pool_size = pool_bytes.len() as u32;
        let pool_offset = 64u32 + entry_count * 8;

        // SHA-256 sur [Data Body ∥ Secondary Pool]
        let mut h = Sha256::new();
        h.update(entries_bytes);
        h.update(pool_bytes);
        let checksum: [u8; 32] = h.finalize().into();

        let mut buf = Vec::with_capacity(64 + entries_bytes.len() + pool_bytes.len());
        buf.extend_from_slice(b"KALD");
        buf.extend_from_slice(&4u16.to_le_bytes()); // version
        buf.extend_from_slice(&0u16.to_le_bytes()); // variant_id
        buf.extend_from_slice(&epoch.to_le_bytes());
        buf.extend_from_slice(&range.to_le_bytes());
        buf.extend_from_slice(&entry_count.to_le_bytes());
        buf.extend_from_slice(&pool_offset.to_le_bytes());
        buf.extend_from_slice(&pool_size.to_le_bytes());
        buf.extend_from_slice(&checksum);
        buf.extend_from_slice(&[0u8; 8]); // _reserved
        buf.extend_from_slice(entries_bytes);
        buf.extend_from_slice(pool_bytes);
        buf
    }

    // ── Tests de layout (offsets et tailles) ─────────────────────────────────

    #[test]
    fn header_size_is_64() {
        assert_eq!(
            size_of::<Header>(),
            64,
            "Header doit faire exactement 64 octets"
        );
    }

    #[test]
    fn header_alignment_at_least_4() {
        // #[repr(C)], champ max = u32 → align = 4.
        // La spec indique "8 (ou au moins 4)".
        assert!(align_of::<Header>() >= 4);
    }

    #[test]
    fn header_field_offsets() {
        // Vérification de chaque offset conformément à specification.md §3.2.
        assert_eq!(field_offset!(Header, magic), 0);
        assert_eq!(field_offset!(Header, version), 4);
        assert_eq!(field_offset!(Header, variant_id), 6);
        assert_eq!(field_offset!(Header, epoch), 8);
        assert_eq!(field_offset!(Header, range), 10);
        assert_eq!(field_offset!(Header, entry_count), 12);
        assert_eq!(field_offset!(Header, pool_offset), 16);
        assert_eq!(field_offset!(Header, pool_size), 20);
        assert_eq!(field_offset!(Header, checksum), 24);
        assert_eq!(field_offset!(Header, _reserved), 56);
    }

    // ── Chemin nominal ────────────────────────────────────────────────────────

    #[test]
    fn validate_header_nominal() {
        // 1 an (1969), 366 entrées, pool vide.
        let body = empty_data_body(366);
        let kald = build_kald(1969, 1, &body, &[]);
        let hdr = validate_header(&kald).expect("header nominal doit être Ok");
        assert_eq!(hdr.magic, *b"KALD");
        assert_eq!(hdr.version, 4);
        assert_eq!(hdr.epoch, 1969);
        assert_eq!(hdr.range, 1);
        assert_eq!(hdr.entry_count, 366);
        assert_eq!(hdr.pool_size, 0);
        assert_eq!(hdr._reserved, [0u8; 8]);
    }

    // ── Chemins d'erreur ──────────────────────────────────────────────────────

    #[test]
    fn error_file_too_small() {
        assert_eq!(validate_header(&[0u8; 63]), Err(HeaderError::FileTooSmall));
        // Buffer vide
        assert_eq!(validate_header(&[]), Err(HeaderError::FileTooSmall));
    }

    #[test]
    fn error_invalid_magic() {
        let body = empty_data_body(366);
        let mut kald = build_kald(1969, 1, &body, &[]);
        kald[0] = b'X'; // casser le magic
        assert_eq!(validate_header(&kald), Err(HeaderError::InvalidMagic));
    }

    #[test]
    fn error_unsupported_version() {
        let body = empty_data_body(366);
        let mut kald = build_kald(1969, 1, &body, &[]);
        // Écrire version = 3 à l'offset 4 (LE)
        kald[4] = 3;
        kald[5] = 0;
        // Le SHA sera invalide aussi, mais version est vérifiée avant → UnsupportedVersion
        assert_eq!(
            validate_header(&kald),
            Err(HeaderError::UnsupportedVersion(3))
        );
    }

    #[test]
    fn error_file_size_mismatch() {
        let body = empty_data_body(366);
        let mut kald = build_kald(1969, 1, &body, &[]);
        // Ajouter un octet parasite en fin de buffer
        kald.push(0xFF);
        assert_eq!(validate_header(&kald), Err(HeaderError::FileSizeMismatch));
    }

    #[test]
    fn error_file_size_mismatch_truncated() {
        let body = empty_data_body(366);
        let kald = build_kald(1969, 1, &body, &[]);
        // Tronquer 1 octet : la taille ne correspond plus
        let truncated = &kald[..kald.len() - 1];
        assert_eq!(
            validate_header(truncated),
            Err(HeaderError::FileSizeMismatch)
        );
    }

    #[test]
    fn error_reserved_not_zero() {
        let body = empty_data_body(366);
        let mut kald = build_kald(1969, 1, &body, &[]);
        kald[56] = 0x01; // _reserved[0] non nul
                         // SHA sera invalide aussi, mais reserved est vérifié avant → ReservedNotZero
        assert_eq!(validate_header(&kald), Err(HeaderError::ReservedNotZero));
    }

    #[test]
    fn error_checksum_mismatch() {
        let body = empty_data_body(366);
        let mut kald = build_kald(1969, 1, &body, &[]);
        // Corrompre le premier octet du Data Body (après le header de 64 octets)
        kald[64] ^= 0xFF;
        assert_eq!(validate_header(&kald), Err(HeaderError::ChecksumMismatch));
    }

    #[test]
    fn error_checksum_mismatch_tampered_hash() {
        let body = empty_data_body(366);
        let mut kald = build_kald(1969, 1, &body, &[]);
        // Corrompre le checksum stocké dans le header (offset 24)
        kald[24] ^= 0xFF;
        assert_eq!(validate_header(&kald), Err(HeaderError::ChecksumMismatch));
    }

    #[test]
    fn validate_header_with_secondary_pool() {
        // Cas avec Secondary Pool non vide (4 u16 = 8 octets)
        let body: Vec<u8> = empty_data_body(366);
        let pool: Vec<u8> = vec![0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00];
        let kald = build_kald(1969, 1, &body, &pool);
        let hdr = validate_header(&kald).expect("avec pool valide");
        assert_eq!(hdr.pool_size, 8);
        assert_eq!(hdr.pool_offset, 64 + 366 * 8);
    }
}
