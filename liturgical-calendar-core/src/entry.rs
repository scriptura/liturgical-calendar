// liturgical-calendar-core/src/entry.rs
//
// CalendarEntry : stride constant 8 octets, #[repr(C)].
// Layout flags (u16) — spec §3.4 (canonique) :
//
//   Bits 0–3   : Precedence       (4 bits) — masque 0x000F,  shift >>  0
//   Bits 4–7   : Color            (4 bits) — masque 0x000F,  shift >>  4
//   Bits 8–10  : LiturgicalPeriod (3 bits) — masque 0x0007,  shift >>  8
//   Bits 11–13 : Nature           (3 bits) — masque 0x0007,  shift >> 11
//   Bits 14–15 : réservés (doivent être 0)
//
// Note : le brief initial inversait Nature (bits 4–6) et Color (bits 11–13).
// La spec §3.4 et le roadmap §1.3 font foi — Color est aux bits 4–7 (4 bits).

use crate::types::{Color, DomainError, LiturgicalPeriod, Nature, Precedence};

/// Entrée d'un jour liturgique dans le Data Body du `.kald`.
///
/// Stride constant : **8 octets**. Critique pour la formule d'index O(1)
/// et la compatibilité SIMD (spec §3.3).
///
/// `primary_id == 0` indique une Padding Entry (ex: 29 fév. en année non-bissextile).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarEntry {
    /// FeastID de la célébration principale. `0` = Padding Entry. Offset 0.
    pub primary_id: u16,
    /// Index dans le tableau `u16` du Secondary Pool. Ignoré si `secondary_count == 0`. Offset 2.
    pub secondary_index: u16,
    /// Champ de bits encodant Precedence, Color, LiturgicalPeriod, Nature. Offset 4.
    ///
    /// Extraction :
    /// - `Precedence = flags & 0x000F`
    /// - `Color = (flags >> 4) & 0x000F`
    /// - `LiturgicalPeriod = (flags >> 8) & 0x0007`
    /// - `Nature = (flags >> 11) & 0x0007`
    pub flags: u16,
    /// Nombre de commémorations (entrées dans le Secondary Pool). `0` = aucune. Offset 6.
    pub secondary_count: u8,
    /// Padding pour stride 64 bits. `0x00`. Offset 7.
    pub _reserved: u8,
}

// Assertions statiques de layout — vérifiées à la compilation, zéro coût runtime.
const _: () = {
    assert!(
        core::mem::size_of::<CalendarEntry>() == 8,
        "CalendarEntry : stride doit être exactement 8 octets"
    );
};

impl CalendarEntry {
    /// Construit une entrée nulle. Tous les champs à zéro.
    ///
    /// `const fn` — utilisable en contexte statique et `no_alloc` (INV-W6).
    /// Méthode canonique pour les pré-allocations déterministes.
    /// `Default::default()` ne peut pas être `const fn` en Rust stable —
    /// `zeroed()` est donc le point d'entrée recommandé.
    pub const fn zeroed() -> Self {
        Self {
            primary_id: 0,
            secondary_index: 0,
            flags: 0,
            secondary_count: 0,
            _reserved: 0,
        }
    }

    /// Retourne `true` si l'entrée est une Padding Entry ou un slot vide.
    ///
    /// Condition : `primary_id == 0`. L'Engine traite ce slot comme vide
    /// sans accéder au Secondary Pool.
    #[inline]
    pub fn is_padding(&self) -> bool {
        self.primary_id == 0
    }

    /// Décode le rang liturgique depuis `flags[0..3]`.
    ///
    /// Retourne `Err` si la valeur extraite est réservée (13–15).
    #[inline]
    pub fn precedence(&self) -> Result<Precedence, DomainError> {
        // Masque 0x000F — bits 0–3.
        Precedence::try_from_u8((self.flags & 0x000F) as u8)
    }

    /// Décode la couleur liturgique depuis `flags[4..7]`.
    ///
    /// Masque 0x000F appliqué après shift de 4 — champ 4 bits (spec §3.4 v2.0).
    #[inline]
    pub fn color(&self) -> Result<Color, DomainError> {
        // Masque 0x000F — bits 4–7.
        Color::try_from_u8(((self.flags >> 4) & 0x000F) as u8)
    }

    /// Décode la période opérationnelle depuis `flags[8..10]`.
    ///
    /// Opération bitwise unique : `(flags >> 8) & 0x0007`.
    /// Accès O(1), SIMD-compatible — invariant de performance sanctuarisé (ADR).
    #[inline]
    pub fn liturgical_period(&self) -> Result<LiturgicalPeriod, DomainError> {
        // Masque 0x0007 — bits 8–10.
        LiturgicalPeriod::try_from_u8(((self.flags >> 8) & 0x0007) as u8)
    }

    /// Décode l'axe sémantique depuis `flags[11..13]`.
    ///
    /// Retourne `Err` pour les valeurs réservées (5–7).
    #[inline]
    pub fn nature(&self) -> Result<Nature, DomainError> {
        // Masque 0x0007 — bits 11–13.
        Nature::try_from_u8(((self.flags >> 11) & 0x0007) as u8)
    }
}

impl Default for CalendarEntry {
    /// Délègue à [`CalendarEntry::zeroed`].
    fn default() -> Self {
        Self::zeroed()
    }
}

/// Encode un jeu de champs dans un champ `flags` (u16).
///
/// Utilitaire interne pour les tests — non exposé à l'ABI C.
/// Formule : `flags = prec | (color << 4) | (period << 8) | (nature << 11)`
#[cfg(test)]
pub(crate) fn encode_flags(
    prec: Precedence,
    color: Color,
    period: LiturgicalPeriod,
    nature: Nature,
) -> u16 {
    (prec as u16)
        | ((color as u16) << 4)
        | ((period as u16) << 8)
        | ((nature as u16) << 11)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests unitaires — tâche 1.3 roadmap
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn size_is_8() {
        assert_eq!(size_of::<CalendarEntry>(), 8, "stride constant critique pour l'indexation");
    }

    macro_rules! field_offset {
        ($type:ty, $field:ident) => {{
            let u = core::mem::MaybeUninit::<$type>::uninit();
            unsafe {
                core::ptr::addr_of!((*u.as_ptr()).$field) as usize
                    - u.as_ptr() as usize
            }
        }};
    }

    #[test]
    fn field_offsets() {
        assert_eq!(field_offset!(CalendarEntry, primary_id),      0);
        assert_eq!(field_offset!(CalendarEntry, secondary_index), 2);
        assert_eq!(field_offset!(CalendarEntry, flags),           4,
            "flags doit être à l'offset 4 — alignement naturel u16 sur offset pair");
        assert_eq!(field_offset!(CalendarEntry, secondary_count), 6);
        assert_eq!(field_offset!(CalendarEntry, _reserved),       7);
    }

    #[test]
    fn zeroed_is_padding() {
        let e = CalendarEntry::zeroed();
        assert!(e.is_padding());
        assert_eq!(e.flags, 0);
        assert_eq!(e.secondary_count, 0);
        assert_eq!(e._reserved, 0);
    }

    #[test]
    fn default_equals_zeroed() {
        assert_eq!(CalendarEntry::default(), CalendarEntry::zeroed());
    }

    #[test]
    fn padding_entry_primary_id_zero() {
        let e = CalendarEntry { primary_id: 0, secondary_count: 0, ..CalendarEntry::zeroed() };
        assert!(e.is_padding());
    }

    #[test]
    fn non_padding_entry() {
        let e = CalendarEntry { primary_id: 1, ..CalendarEntry::zeroed() };
        assert!(!e.is_padding());
    }

    /// Roundtrip encode/decode pour toutes les combinaisons représentatives.
    /// Un test exhaustif (13 × 6 × 7 × 5 = 2730 combinaisons) est faisable
    /// mais redondant avec les tests unitaires des enums dans types.rs.
    #[test]
    fn flags_roundtrip_representative() {
        let cases = [
            (
                Precedence::TriduumSacrum,
                Color::Rubeus,
                LiturgicalPeriod::TriduumPaschale,
                Nature::Feria,
            ),
            (
                Precedence::SollemnitatesGenerales,
                Color::Albus,
                LiturgicalPeriod::DiesSancti,
                Nature::Sollemnitas,
            ),
            (
                Precedence::FeriaePerAnnumEtMemoriaeAdLibitum,
                Color::Viridis,
                LiturgicalPeriod::TempusOrdinarium,
                Nature::Memoria,
            ),
            (
                Precedence::MemoriaeObligatoriae,
                Color::Violaceus,
                LiturgicalPeriod::TempusQuadragesimae,
                Nature::Commemoratio,
            ),
            (
                Precedence::FestaDomini,
                Color::Roseus,
                LiturgicalPeriod::TempusAdventus,
                Nature::Festum,
            ),
        ];

        for (prec, color, period, nature) in cases {
            let flags = encode_flags(prec, color, period, nature);
            let entry = CalendarEntry { flags, ..CalendarEntry::zeroed() };
            assert_eq!(entry.precedence(), Ok(prec));
            assert_eq!(entry.color(), Ok(color));
            assert_eq!(entry.liturgical_period(), Ok(period));
            assert_eq!(entry.nature(), Ok(nature));
        }
    }

    /// Vérifie que les bits réservés (14–15) n'interfèrent pas avec le décodage.
    #[test]
    fn reserved_bits_ignored_by_decoders() {
        let flags_clean = encode_flags(
            Precedence::DominicaePerAnnum,
            Color::Viridis,
            LiturgicalPeriod::TempusOrdinarium,
            Nature::Feria,
        );
        // Positionne les bits réservés 14–15
        let flags_dirty = flags_clean | 0xC000;

        let entry = CalendarEntry { flags: flags_dirty, ..CalendarEntry::zeroed() };
        // Les masques d'extraction ne lisent pas les bits 14–15 → résultat identique.
        assert_eq!(entry.precedence(), Ok(Precedence::DominicaePerAnnum));
        assert_eq!(entry.color(), Ok(Color::Viridis));
        assert_eq!(entry.liturgical_period(), Ok(LiturgicalPeriod::TempusOrdinarium));
        assert_eq!(entry.nature(), Ok(Nature::Feria));
    }

    /// Simulation d'une Padding Entry doy=59 sur année non-bissextile.
    #[test]
    fn padding_entry_doy59_non_leap() {
        // La Forge place primary_id=0, flags=0, secondary_count=0 à doy=59
        // pour les années non-bissextiles (spec §3.3, §2.1).
        let entry = CalendarEntry::zeroed();
        assert!(entry.is_padding());
        assert_eq!(entry.secondary_count, 0);
    }
}
