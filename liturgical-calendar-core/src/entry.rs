use core::mem::{offset_of, size_of};

use crate::types::{Color, DomainError, LiturgicalPeriod, Nature, Precedence};

/// Version du format `.kald` — v6 : `LiturgicalPeriod` et `liturgical_week`
/// déplacés dans `TimelineEntry`; `FeastEntry.flags[10:8]` libérés.
pub const KALD_FORMAT_VERSION: u16 = 6;

// ── FeastEntry ────────────────────────────────────────────────────────────────

/// Invariants d'une fête — 4 octets, stride constant, little-endian.
///
/// Layout `flags` (u16) v6 :
/// - bits [3:0]   → `Precedence`     (0–12)
/// - bits [7:4]   → `Color`          (0–5)
/// - bits [10:8]  → réservés, nuls   (v5 : `LiturgicalPeriod` — supprimé en v6)
/// - bits [13:11] → `Nature`         (0–4)
/// - bit  [14]    → `has_vigil_mass` — invariant corpus
/// - bit  [15]    → réservé, nul
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct FeastEntry {
    /// Identifiant corpus — vérification croisée `.lits`.
    /// Non utilisé pour le calcul d'index (positionnement dans l'array = registry_index − 1).
    pub feast_id: u16,
    /// Invariants : Precedence, Color, LiturgicalPeriod, Nature, has_vigil_mass.
    pub flags: u16,
}

const _: () = assert!(size_of::<FeastEntry>() == 4);
const _: () = assert!(offset_of!(FeastEntry, flags) == 2);

impl FeastEntry {
    /// Retourne une entrée entièrement nulle.
    pub const fn zeroed() -> Self {
        Self {
            feast_id: 0,
            flags: 0,
        }
    }

    /// Extrait la `Precedence` depuis `flags[3:0]`.
    #[inline]
    pub fn precedence(&self) -> Result<Precedence, DomainError> {
        Precedence::try_from_u8((self.flags & 0x000F) as u8)
    }

    /// Extrait la `Color` depuis `flags[7:4]`.
    #[inline]
    pub fn color(&self) -> Result<Color, DomainError> {
        Color::try_from_u8(((self.flags >> 4) & 0x000F) as u8)
    }

    /// Extrait la `Nature` depuis `flags[13:11]`.
    #[inline]
    pub fn nature(&self) -> Result<Nature, DomainError> {
        Nature::try_from_u8(((self.flags >> 11) & 0x0007) as u8)
    }

    /// `true` si la fête a une Messe de Vigile propre — invariant corpus.
    #[inline]
    pub fn has_vigil_mass(&self) -> bool {
        self.flags & (1 << 14) != 0
    }
}

impl Default for FeastEntry {
    fn default() -> Self {
        Self::zeroed()
    }
}

// ── TimelineEntry ─────────────────────────────────────────────────────────────

/// Occurrence journalière — stride constant 8 octets, little-endian.
///
/// v6 : `_reserved: u16` remplacé par `liturgical_week: u8` + `_reserved: u8`.
/// La `LiturgicalPeriod` est désormais portée par `occurrence_flags[4:2]`.
///
/// `primary_index` :
/// - `0` = Padding Entry (aucune célébration). Slots padding :
///   • DOY 59 des années non-bissextiles (29 fév fictif)
///   • Jours sans fête propre — `occurrence_flags` et `liturgical_week`
///   y sont néanmoins renseignés (primitives temporelles accessibles).
/// - `1..=registry_count` = registry_index valide dans le Feast Registry.
///
/// `occurrence_flags` :
/// - bit 0      : `has_vesperae_i` — ce soir commence les Premières Vêpres de DOY+1.
/// - bit 1      : `has_vigilia`    — ce soir a une Messe de Vigile propre (DOY+1).
///   (bits 0–1 positionnés exclusivement par `vespers_lookahead_pass`)
/// - bits [4:2] : `LiturgicalPeriod` (0–6) — positionné par `generate_year`.
/// - bits [7:5] : réservés, nuls.
///
/// `liturgical_week` :
/// - `0`    : aucun ordinal applicable (TriduumPaschale, DOY 59 non-bissextile).
/// - `1–34` : ordinal de la semaine liturgique courante dans la période.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TimelineEntry {
    /// 0 = Padding Entry. Sinon : registry_index (1-based).
    pub primary_index: u16,
    /// Offset dans le Secondary Pool (en nombre de u16).
    pub secondary_offset: u16,
    /// Bits [1:0] = vesperae_i/vigilia. Bits [4:2] = LiturgicalPeriod.
    pub occurrence_flags: u8,
    /// Nombre de célébrations secondaires dans le Secondary Pool.
    pub secondary_count: u8,
    /// Ordinal de semaine liturgique (0 = N/A, 1–34 = semaine active).
    pub liturgical_week: u8,
    /// Padding structurel — doit être nul.
    pub _reserved: u8,
}

// Assertions statiques de layout.
const _: () = assert!(size_of::<TimelineEntry>() == 8);
const _: () = assert!(offset_of!(TimelineEntry, secondary_offset) == 2);
const _: () = assert!(offset_of!(TimelineEntry, occurrence_flags) == 4);
const _: () = assert!(offset_of!(TimelineEntry, secondary_count) == 5);
const _: () = assert!(offset_of!(TimelineEntry, liturgical_week) == 6);
const _: () = assert!(offset_of!(TimelineEntry, _reserved) == 7);

/// Discriminant de layout v6 — invalide automatiquement tous les artefacts v5.
///
/// Bits [55:48] = version (6).
/// Bits [47:40] = offset `_reserved` (7).
/// Bits [39:32] = offset `liturgical_week` (6).
/// Bits [31:24] = offset `secondary_count` (5).
/// Bits [23:16] = offset `occurrence_flags` (4).
/// Bits [15:8]  = offset `secondary_offset` (2).
/// Bits [7:0]   = `size_of::<TimelineEntry>` (8).
pub const LAYOUT_DISCRIMINANT: u64 = {
    let sz = size_of::<TimelineEntry>() as u64;
    let off_sec_offset = offset_of!(TimelineEntry, secondary_offset) as u64;
    let off_occ_flags = offset_of!(TimelineEntry, occurrence_flags) as u64;
    let off_sec_count = offset_of!(TimelineEntry, secondary_count) as u64;
    let off_lit_week = offset_of!(TimelineEntry, liturgical_week) as u64;
    let off_reserved = offset_of!(TimelineEntry, _reserved) as u64;
    let version = KALD_FORMAT_VERSION as u64;

    sz ^ (off_sec_offset << 8)
        ^ (off_occ_flags << 16)
        ^ (off_sec_count << 24)
        ^ (off_lit_week << 32)
        ^ (off_reserved << 40)
        ^ (version << 48)
};

impl TimelineEntry {
    /// Retourne une entrée entièrement nulle (Padding Entry).
    pub const fn zeroed() -> Self {
        Self {
            primary_index: 0,
            secondary_offset: 0,
            occurrence_flags: 0,
            secondary_count: 0,
            liturgical_week: 0,
            _reserved: 0,
        }
    }

    /// `true` si `primary_index == 0` (aucune fête propre pour ce slot).
    #[inline]
    pub fn is_padding(&self) -> bool {
        self.primary_index == 0
    }

    /// `true` si ce soir civil commence les Premières Vêpres de DOY+1.
    #[inline]
    pub fn has_vesperae_i(&self) -> bool {
        self.occurrence_flags & (1 << 0) != 0
    }

    /// `true` si ce soir civil a une Messe de Vigile propre pour DOY+1.
    #[inline]
    pub fn has_vigilia(&self) -> bool {
        self.occurrence_flags & (1 << 1) != 0
    }

    /// Extrait la `LiturgicalPeriod` depuis `occurrence_flags[4:2]`.
    ///
    /// Pour les slots padding DOY 59 non-bissextile (toujours `occurrence_flags == 0`),
    /// retourne `Ok(TempusOrdinarium)` — le client doit ignorer ces slots via `is_padding()`.
    #[inline]
    pub fn liturgical_period(&self) -> Result<LiturgicalPeriod, DomainError> {
        LiturgicalPeriod::try_from_u8((self.occurrence_flags >> 2) & 0x07)
    }
}

impl Default for TimelineEntry {
    fn default() -> Self {
        Self::zeroed()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_feast_entry_size() {
        assert_eq!(size_of::<FeastEntry>(), 4);
    }

    #[test]
    fn layout_timeline_entry_size() {
        assert_eq!(size_of::<TimelineEntry>(), 8);
    }

    #[test]
    fn layout_timeline_offsets() {
        assert_eq!(offset_of!(TimelineEntry, occurrence_flags), 4);
        assert_eq!(offset_of!(TimelineEntry, secondary_count), 5);
        assert_eq!(offset_of!(TimelineEntry, liturgical_week), 6);
        assert_eq!(offset_of!(TimelineEntry, _reserved), 7);
    }

    #[test]
    fn timeline_zeroed_is_padding() {
        let e = TimelineEntry::zeroed();
        assert!(e.is_padding());
        assert!(!e.has_vesperae_i());
        assert!(!e.has_vigilia());
        assert_eq!(
            e.liturgical_period(),
            Ok(LiturgicalPeriod::TempusOrdinarium)
        );
        assert_eq!(e.liturgical_week, 0);
    }

    #[test]
    fn timeline_default_eq_zeroed() {
        assert_eq!(TimelineEntry::default(), TimelineEntry::zeroed());
    }

    #[test]
    fn liturgical_period_roundtrip() {
        use crate::types::LiturgicalPeriod::*;
        let periods = [
            TempusOrdinarium,
            TempusAdventus,
            TempusNativitatis,
            TempusQuadragesimae,
            TriduumPaschale,
            TempusPaschale,
            DiesSancti,
        ];
        for p in periods {
            let bits = (p as u8 & 0x07) << 2;
            let e = TimelineEntry {
                primary_index: 1,
                secondary_offset: 0,
                occurrence_flags: bits,
                secondary_count: 0,
                liturgical_week: 1,
                _reserved: 0,
            };
            assert_eq!(
                e.liturgical_period(),
                Ok(p),
                "période {:?} non roundtrip",
                p
            );
        }
    }

    #[test]
    fn occurrence_flags_period_preserves_vespers_bits() {
        // Écriture de la période dans [4:2] ne doit pas altérer [1:0].
        let period_bits = (LiturgicalPeriod::TempusPaschale as u8 & 0x07) << 2; // 0b00010100
        let vespers_bits: u8 = 0b11;
        let combined = period_bits | vespers_bits;

        let e = TimelineEntry {
            primary_index: 1,
            occurrence_flags: combined,
            ..TimelineEntry::zeroed()
        };
        assert!(e.has_vesperae_i());
        assert!(e.has_vigilia());
        assert_eq!(e.liturgical_period(), Ok(LiturgicalPeriod::TempusPaschale));
    }

    #[test]
    fn feast_entry_flags_roundtrip() {
        let p = Precedence::MemoriaeAdLibitum as u16; // 11
        let c = Color::Viridis as u16; // 2
        let n = Nature::Memoria as u16; // 3
        let vigil: u16 = 1 << 14;

        // v6 : bits [10:8] nuls (LiturgicalPeriod supprimé de FeastEntry)
        let flags = p | (c << 4) | (n << 11) | vigil;
        let fe = FeastEntry {
            feast_id: 42,
            flags,
        };

        assert_eq!(fe.precedence(), Ok(Precedence::MemoriaeAdLibitum));
        assert_eq!(fe.color(), Ok(Color::Viridis));
        assert_eq!(fe.nature(), Ok(Nature::Memoria));
        assert!(fe.has_vigil_mass());
        // bits [10:8] doivent rester nuls en v6
        assert_eq!(fe.flags & 0x0700, 0);
    }

    #[test]
    fn layout_discriminant_nonzero_and_encodes_version() {
        assert_ne!(LAYOUT_DISCRIMINANT, 0);
        let version_bits = (LAYOUT_DISCRIMINANT >> 48) & 0xFF;
        assert_eq!(version_bits, KALD_FORMAT_VERSION as u64); // 6
    }

    #[test]
    fn layout_discriminant_differs_from_v5() {
        // Valeur v5 reconstruite manuellement pour vérification de rupture.
        // sz=8, off_sec_offset=2, off_occ_flags=4, off_sec_count=5, off_reserved=6, version=5
        let v5: u64 =
            8u64 ^ (2u64 << 8) ^ (4u64 << 16) ^ (5u64 << 24) ^ (6u64 << 32) ^ (5u64 << 48);
        assert_ne!(
            LAYOUT_DISCRIMINANT, v5,
            "v6 doit invalider les artefacts v5"
        );
    }
}
