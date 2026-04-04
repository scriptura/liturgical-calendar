// entry.rs — CalendarEntry binaire .kald v2.0 (specification.md §3.3–3.4).
//
// Layout fixé (8 octets, stride constant, #[repr(C)]) :
//   [0..2]  primary_id      u16   FeastID principal. 0 = Padding Entry.
//   [2..4]  secondary_index u16   Index dans le tableau u16 du Secondary Pool.
//   [4..6]  flags           u16   Champs encodés (§3.4).
//   [6]     secondary_count u8    Nombre de commémorations (0 = aucune).
//   [7]     _reserved       u8    0x00 — padding pour stride 64 bits.
//
// Layout flags (u16, specification.md §3.4) :
//   bits  0– 3  Precedence (4 bits) — rang effectif 0–15
//   bits  4– 7  Color      (4 bits) — index couleur liturgique 0–6
//   bits  8–10  Season     (3 bits) — index saison liturgique 0–6
//   bits 11–13  Nature     (3 bits) — type liturgique 0–4
//   bits 14–15  Reserved   (2 bits) — doit être 0
//
// Encodage : flags = P | (C << 4) | (S << 8) | (N << 11)
//
// Justification du layout : flags (u16) à l'offset pair 4 garantit l'alignement
// naturel sur 2 octets. Placer secondary_count (u8) avant flags aurait produit
// un offset impair (5) pour flags — UB potentiel sur architectures strictes.

use crate::types::{Color, DomainError, Nature, Precedence, Season};

// ─── Assertions de layout statiques ──────────────────────────────────────────
use core::mem;
const _ASSERT_ENTRY_SIZE: () = assert!(mem::size_of::<CalendarEntry>() == 8);
const _ASSERT_ENTRY_STRIDE: () = assert!(mem::size_of::<CalendarEntry>() == 8);

// ─── Masques d'extraction des champs depuis flags ─────────────────────────────

/// Masque Precedence : bits 0–3.
const MASK_PRECEDENCE: u16 = 0x000F;
/// Masque Color : bits 4–7.
const MASK_COLOR: u16 = 0x000F;
/// Masque Season : bits 8–10.
const MASK_SEASON: u16 = 0x0007;
/// Masque Nature : bits 11–13.
const MASK_NATURE: u16 = 0x0007;

// ─── CalendarEntry ────────────────────────────────────────────────────────────

/// Entrée du calendrier liturgique — stride 8 octets, `#[repr(C)]`.
///
/// Unité atomique du Data Body du `.kald`. Chaque entrée représente un jour
/// `(year, doy)` dans la plage 1969–2399.
///
/// **Padding Entry** : `primary_id == 0`, `secondary_count == 0`, `flags == 0`.
/// Placée à `doy = 59` par la Forge pour chaque année non-bissextile.
/// L'Engine retourne cette entrée normalement (`KAL_ENGINE_OK`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarEntry {
    /// FeastID de la célébration principale.
    /// `0` = Padding Entry (slot vide — 29 février d'une année non-bissextile).
    pub primary_id: u16,

    /// Index dans le **tableau `u16`** du Secondary Pool (unité : éléments, pas octets).
    /// Ignoré si `secondary_count == 0`.
    pub secondary_index: u16,

    /// Champs encodés sur 16 bits. Voir `encode_flags` / méthodes d'extraction.
    /// Bits 14–15 réservés — doivent être nuls dans tout fichier valide.
    pub flags: u16,

    /// Nombre de commémorations. `0` = aucune — pas d'accès au Secondary Pool.
    pub secondary_count: u8,

    /// Padding pour aligner l'entrée sur 8 octets (stride 64 bits).
    /// Non validé par l'Engine au niveau entrée (tolérance en lecture).
    pub _reserved: u8,
}

impl CalendarEntry {
    /// Extrait la `Precedence` depuis `flags[0..3]`.
    ///
    /// Retourne `Err(DomainError)` si la valeur encodée dépasse 12 (réservée).
    pub fn precedence(&self) -> Result<Precedence, DomainError> {
        let raw = (self.flags & MASK_PRECEDENCE) as u8;
        Precedence::try_from_u8(raw)
    }

    /// Extrait la `Color` depuis `flags[4..7]`.
    ///
    /// Retourne `Err(DomainError)` si la valeur encodée dépasse 5 (réservée).
    pub fn color(&self) -> Result<Color, DomainError> {
        let raw = ((self.flags >> 4) & MASK_COLOR) as u8;
        Color::try_from_u8(raw)
    }

    /// Extrait la `Season` depuis `flags[8..10]`.
    ///
    /// Retourne `Err(DomainError)` si la valeur encodée vaut 7 (réservée).
    pub fn season(&self) -> Result<Season, DomainError> {
        let raw = ((self.flags >> 8) & MASK_SEASON) as u8;
        Season::try_from_u8(raw)
    }

    /// Extrait la `Nature` depuis `flags[11..13]`.
    ///
    /// Retourne `Err(DomainError)` si la valeur encodée dépasse 4 (réservée).
    pub fn nature(&self) -> Result<Nature, DomainError> {
        let raw = ((self.flags >> 11) & MASK_NATURE) as u8;
        Nature::try_from_u8(raw)
    }

    /// Retourne `true` si cette entrée est une **Padding Entry** :
    /// slot 29 février d'une année non-bissextile (`primary_id == 0`).
    ///
    /// L'appelant ne doit pas accéder au Secondary Pool pour une Padding Entry.
    #[inline(always)]
    pub fn is_padding(&self) -> bool {
        self.primary_id == 0
    }

    /// Entrée nulle (zéro tous champs) — utilisable comme valeur initiale dans les tests.
    #[cfg(test)]
    pub fn zeroed() -> Self {
        CalendarEntry {
            primary_id: 0,
            secondary_index: 0,
            flags: 0,
            secondary_count: 0,
            _reserved: 0,
        }
    }
}

/// Encode `flags` depuis les quatre champs de domaine.
///
/// Formule canonique (specification.md §3.4) :
/// `flags = P | (C << 4) | (S << 8) | (N << 11)`
///
/// Utilisée par la Forge (Étape 5) et les tests de roundtrip de l'Engine.
#[inline(always)]
pub fn encode_flags(p: Precedence, c: Color, s: Season, n: Nature) -> u16 {
    (p as u16) | ((c as u16) << 4) | ((s as u16) << 8) | ((n as u16) << 11)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    /// Calcul d'offset de champ sans offset_of! (stabilisé en 1.77).
    macro_rules! field_offset {
        ($type:ty, $field:ident) => {{
            let uninit = core::mem::MaybeUninit::<$type>::uninit();
            let base = uninit.as_ptr() as usize;
            let field = unsafe { core::ptr::addr_of!((*uninit.as_ptr()).$field) } as usize;
            field - base
        }};
    }

    // ── Tests de layout (stride et offsets) ───────────────────────────────────

    #[test]
    fn entry_size_is_8() {
        // Stride constant = 8 octets. Critique pour l'indexation O(1).
        assert_eq!(size_of::<CalendarEntry>(), 8);
    }

    #[test]
    fn entry_flags_at_offset_4() {
        // flags doit être à l'offset 4 — alignement naturel u16 sur offset pair.
        assert_eq!(field_offset!(CalendarEntry, flags), 4);
    }

    #[test]
    fn entry_field_offsets() {
        assert_eq!(field_offset!(CalendarEntry, primary_id), 0);
        assert_eq!(field_offset!(CalendarEntry, secondary_index), 2);
        assert_eq!(field_offset!(CalendarEntry, flags), 4);
        assert_eq!(field_offset!(CalendarEntry, secondary_count), 6);
        assert_eq!(field_offset!(CalendarEntry, _reserved), 7);
    }

    // ── Tests de Padding Entry ─────────────────────────────────────────────────

    #[test]
    fn padding_entry_detected() {
        let e = CalendarEntry {
            primary_id: 0,
            secondary_index: 0,
            flags: 0,
            secondary_count: 0,
            _reserved: 0,
        };
        assert!(e.is_padding(), "primary_id=0 doit être Padding Entry");
        assert_eq!(e.secondary_count, 0);
    }

    #[test]
    fn non_padding_entry() {
        let e = CalendarEntry {
            primary_id: 0x0042,
            secondary_index: 0,
            flags: 0,
            secondary_count: 0,
            _reserved: 0,
        };
        assert!(!e.is_padding());
    }

    // ── Tests de roundtrip encode/decode flags ─────────────────────────────────

    #[test]
    fn flags_roundtrip_precedence() {
        use Precedence::*;
        let variants = [
            TriduumSacrum,
            SollemnitatesFixaeMaior,
            DominicaePrivilegiataeMaior,
            FeriaePrivilegiataeMaior,
            SollemnitatesGenerales,
            SollemnitatesPropria,
            FestaDomini,
            DominicaePerAnnum,
            FestaBMVEtSanctorumGenerales,
            FestaPropria,
            FeriaeAdventusEtOctavaNativitatis,
            MemoriaeObligatoriae,
            FeriaePerAnnumEtMemoriaeAdLibitum,
        ];
        for p in variants {
            let flags = encode_flags(p, Color::Viridis, Season::TempusOrdinarium, Nature::Feria);
            let entry = CalendarEntry {
                primary_id: 1,
                secondary_index: 0,
                flags,
                secondary_count: 0,
                _reserved: 0,
            };
            assert_eq!(entry.precedence(), Ok(p), "roundtrip Precedence::{p:?}");
        }
    }

    #[test]
    fn flags_roundtrip_color() {
        use Color::*;
        for c in [Albus, Rubeus, Viridis, Violaceus, Roseus, Niger] {
            let flags = encode_flags(
                Precedence::MemoriaeObligatoriae,
                c,
                Season::TempusOrdinarium,
                Nature::Memoria,
            );
            let entry = CalendarEntry {
                primary_id: 1,
                secondary_index: 0,
                flags,
                secondary_count: 0,
                _reserved: 0,
            };
            assert_eq!(entry.color(), Ok(c), "roundtrip Color::{c:?}");
        }
    }

    #[test]
    fn flags_roundtrip_season() {
        use Season::*;
        for s in [
            TempusOrdinarium,
            TempusAdventus,
            TempusNativitatis,
            TempusQuadragesimae,
            TriduumPaschale,
            TempusPaschale,
            DiesSancti,
        ] {
            let flags = encode_flags(
                Precedence::FeriaePerAnnumEtMemoriaeAdLibitum,
                Color::Viridis,
                s,
                Nature::Feria,
            );
            let entry = CalendarEntry {
                primary_id: 1,
                secondary_index: 0,
                flags,
                secondary_count: 0,
                _reserved: 0,
            };
            assert_eq!(entry.season(), Ok(s), "roundtrip Season::{s:?}");
        }
    }

    #[test]
    fn flags_roundtrip_nature() {
        use Nature::*;
        for n in [Sollemnitas, Festum, Memoria, Feria, Commemoratio] {
            let flags = encode_flags(
                Precedence::SollemnitatesGenerales,
                Color::Albus,
                Season::TempusPaschale,
                n,
            );
            let entry = CalendarEntry {
                primary_id: 1,
                secondary_index: 0,
                flags,
                secondary_count: 0,
                _reserved: 0,
            };
            assert_eq!(entry.nature(), Ok(n), "roundtrip Nature::{n:?}");
        }
    }

    #[test]
    fn flags_roundtrip_combined() {
        // Test d'une combinaison représentative :
        // TriduumSacrum (0), Violaceus (3), TriduumPaschale (4), Feria (3)
        // flags = 0 | (3 << 4) | (4 << 8) | (3 << 11)
        //       = 0x0000 | 0x0030 | 0x0400 | 0x1800
        //       = 0x1C30
        let p = Precedence::TriduumSacrum;
        let c = Color::Violaceus;
        let s = Season::TriduumPaschale;
        let n = Nature::Feria;
        let flags = encode_flags(p, c, s, n);
        assert_eq!(flags, 0x1C30);

        let entry = CalendarEntry {
            primary_id: 0x0010,
            secondary_index: 0,
            flags,
            secondary_count: 0,
            _reserved: 0,
        };
        assert_eq!(entry.precedence(), Ok(p));
        assert_eq!(entry.color(), Ok(c));
        assert_eq!(entry.season(), Ok(s));
        assert_eq!(entry.nature(), Ok(n));
        assert!(!entry.is_padding()); // primary_id != 0
    }

    #[test]
    fn flags_reserved_bits_ignored_in_read() {
        // Les bits 14–15 réservés ne doivent pas perturber l'extraction.
        // L'Engine lit les bits 0–13 uniquement, bits 14–15 sont masqués.
        let mut entry = CalendarEntry::zeroed();
        entry.primary_id = 1;
        entry.flags = encode_flags(
            Precedence::FestaDomini,
            Color::Albus,
            Season::TempusNativitatis,
            Nature::Festum,
        );
        // Forcer les bits réservés 14–15 à 1 (simulate fichier légèrement corrompu)
        entry.flags |= 0xC000;
        // Les extracteurs masquent correctement — pas d'erreur
        assert_eq!(entry.precedence(), Ok(Precedence::FestaDomini));
        assert_eq!(entry.color(), Ok(Color::Albus));
        assert_eq!(entry.season(), Ok(Season::TempusNativitatis));
        assert_eq!(entry.nature(), Ok(Nature::Festum));
    }

    #[test]
    fn flags_invalid_precedence_reserved() {
        // Precedence = 13 (réservé) dans flags → DomainError
        let mut entry = CalendarEntry::zeroed();
        entry.primary_id = 1;
        entry.flags = 13u16; // bits 0–3 = 13
        assert!(entry.precedence().is_err());
    }
}
