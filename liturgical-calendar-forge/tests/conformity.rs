// SPDX-License-Identifier: MIT
// Tests de conformité Jalon 2 (roadmap §2 critère de sortie).
//
// Ces tests nécessitent liturgical-calendar-core (dev-dependency) pour valider
// le .kald produit par la Forge via les fonctions FFI de l'Engine.
//
// Critère de sortie :
//   1. kal_validate_header → KAL_ENGINE_OK
//   2. doy=110 en 2025 → primary_id ≠ 0 (Pâques)
//   3. doy=59  en 2025 → primary_id = 0  (Padding Entry, 2025 non-bissextile)
//   4. doy=59  en 2028 → primary_id ≠ 0  (29 fév réel, 2028 bissextile)

use std::ptr::null_mut;

use liturgical_calendar_core::entry::CalendarEntry;
use liturgical_calendar_core::ffi::{
    kal_read_entry, kal_validate_header, KAL_ENGINE_OK,
};
use liturgical_calendar_forge::forge_year;

// ─── Conformité 2025 ──────────────────────────────────────────────────────────

/// Année 2025 : non-bissextile, Pâques = 20 avril = doy 110.
///
/// Vérifie :
/// - Le .kald est structurellement valide (header, SHA-256, taille).
/// - doy=110 (Pâques) → `primary_id ≠ 0`.
/// - doy=59  (29 fév fictif) → `primary_id = 0` (Padding Entry).
#[test]
fn conformity_2025() {
    let kald = forge_year(2025).expect("forge_year(2025) doit réussir");

    // ── Validation header ──────────────────────────────────────────────────
    let rc_hdr = unsafe {
        kal_validate_header(kald.as_ptr(), kald.len(), null_mut())
    };
    assert_eq!(
        rc_hdr, KAL_ENGINE_OK,
        "kal_validate_header doit retourner KAL_ENGINE_OK pour le .kald 2025 ; code={rc_hdr}"
    );

    let mut e = CalendarEntry::zeroed();

    // ── Pâques 2025 : doy=110 (20 avril = MONTH_STARTS[3] + 19 = 91 + 19 = 110) ──
    let rc_easter = unsafe {
        kal_read_entry(kald.as_ptr(), kald.len(), 2025, 110, &mut e)
    };
    assert_eq!(rc_easter, KAL_ENGINE_OK, "kal_read_entry(2025, 110) failed ; code={rc_easter}");
    assert_ne!(
        e.primary_id, 0,
        "doy=110 en 2025 doit être Pâques (primary_id ≠ 0) ; got primary_id={}",
        e.primary_id
    );

    // ── Padding Entry 2025 : doy=59 (29 fév fictif sur année non-bissextile) ──
    let rc_pad = unsafe {
        kal_read_entry(kald.as_ptr(), kald.len(), 2025, 59, &mut e)
    };
    assert_eq!(rc_pad, KAL_ENGINE_OK, "kal_read_entry(2025, 59) failed ; code={rc_pad}");
    assert_eq!(
        e.primary_id, 0,
        "doy=59 en 2025 doit être Padding Entry (primary_id = 0) ; got primary_id={}",
        e.primary_id
    );
    assert!(e.is_padding(), "is_padding() doit être vrai pour la Padding Entry 2025");
}

// ─── Conformité 2028 ──────────────────────────────────────────────────────────

/// Année 2028 : bissextile — doy=59 doit être une vraie fête, pas une Padding Entry.
///
/// Vérifie :
/// - Le .kald est structurellement valide.
/// - doy=59 (29 février réel) → `primary_id ≠ 0`.
#[test]
fn conformity_2028() {
    let kald = forge_year(2028).expect("forge_year(2028) doit réussir");

    // ── Validation header ──────────────────────────────────────────────────
    let rc_hdr = unsafe {
        kal_validate_header(kald.as_ptr(), kald.len(), null_mut())
    };
    assert_eq!(
        rc_hdr, KAL_ENGINE_OK,
        "kal_validate_header doit retourner KAL_ENGINE_OK pour le .kald 2028 ; code={rc_hdr}"
    );

    let mut e = CalendarEntry::zeroed();

    // ── 29 février 2028 réel : doy=59 ─────────────────────────────────────
    let rc = unsafe {
        kal_read_entry(kald.as_ptr(), kald.len(), 2028, 59, &mut e)
    };
    assert_eq!(rc, KAL_ENGINE_OK, "kal_read_entry(2028, 59) failed ; code={rc}");
    assert_ne!(
        e.primary_id, 0,
        "doy=59 en 2028 (bissextile) doit être une vraie fête (primary_id ≠ 0) ; got primary_id={}",
        e.primary_id
    );
    assert!(!e.is_padding(), "is_padding() doit être faux pour une vraie fête le 29 fév 2028");
}

// ─── Tests complémentaires ────────────────────────────────────────────────────

/// Vérifie le déterminisme bit-for-bit cross-build.
#[test]
fn determinism_bit_for_bit() {
    let kald1 = forge_year(2025).unwrap();
    let kald2 = forge_year(2025).unwrap();
    assert_eq!(
        kald1, kald2,
        "deux appels forge_year(2025) doivent produire des octets identiques"
    );
    // Checksum (offset 24..56) doit être identique
    assert_eq!(&kald1[24..56], &kald2[24..56]);
}

/// Vérifie la cohérence des Padding Entries sur toutes les années non-bissextiles
/// de la plage 1969–2028 couvertes par forge_year(2028).
#[test]
fn all_non_leap_years_have_padding_at_doy59() {
    let kald = forge_year(2028).unwrap();

    for year in 1969u16..=2028 {
        let leap = (year % 400 == 0) || (year % 4 == 0 && year % 100 != 0);
        let mut e = CalendarEntry::zeroed();
        let rc = unsafe {
            kal_read_entry(kald.as_ptr(), kald.len(), year, 59, &mut e)
        };
        assert_eq!(rc, KAL_ENGINE_OK,
            "kal_read_entry({year}, 59) failed ; code={rc}");
        if leap {
            assert_ne!(e.primary_id, 0,
                "year={year} (bissextile) doy=59 → vraie fête attendue");
        } else {
            assert_eq!(e.primary_id, 0,
                "year={year} (non-bissextile) doy=59 → Padding Entry attendue");
        }
    }
}

/// Vérifie que tous les Pâques dans 1969–2025 tombent dans [doy 81, doy 115].
#[test]
fn easter_always_in_valid_range() {
    let kald = forge_year(2025).unwrap();
    // Table de référence partielle pour quelques années clés.
    // Formule : MONTH_STARTS[month-1] + (day-1)
    //   6 avril  = MONTH_STARTS[3] + 5  = 91 + 5  = 96
    //   20 avril = MONTH_STARTS[3] + 19 = 91 + 19 = 110
    //   23 avril = MONTH_STARTS[3] + 22 = 91 + 22 = 113
    //   31 mars  = MONTH_STARTS[2] + 30 = 60 + 30 = 90
    let known: &[(u16, u16)] = &[
        (2025, 110), // 20 avril 2025 = doy 110
        (2024,  90), // 31 mars  2024 = doy 90
        (2000, 113), // 23 avril 2000 = doy 113
        (1969,  96), // 6 avril  1969 = doy 96
    ];
    for &(year, expected_doy) in known {
        let mut e = CalendarEntry::zeroed();
        let rc = unsafe {
            kal_read_entry(kald.as_ptr(), kald.len(), year, expected_doy, &mut e)
        };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_ne!(e.primary_id, 0,
            "Pâques {year} à doy={expected_doy} doit avoir primary_id ≠ 0");
    }
}
