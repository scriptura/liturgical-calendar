use liturgical_calendar_core::{
    entry::TimelineEntry,
    ffi::{KAL_ENGINE_OK, kal_read_entry, kal_validate_header},
};
use liturgical_calendar_forge::forge_full_range;
use std::{ptr::null_mut, sync::OnceLock};

// ---------------------------------------------------------------------------
// Fixture mutualisée — parsing du corpus une seule fois pour tous les tests
// ---------------------------------------------------------------------------

/// Retourne deux builds identiques du `.kald` complet (1969–2399).
/// - `.0` : utilisé par tous les tests sauf `full_range_deterministic`.
/// - `.1` : second build indépendant pour le test de déterminisme SHA-256.
///
/// L'initialisation est atomique via `OnceLock` : les 326 fêtes ne sont
/// parsées qu'une seule fois quelle que soit l'ordre d'exécution des tests.
static FULL_RANGE: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();

fn kalds() -> &'static (Vec<u8>, Vec<u8>) {
    FULL_RANGE.get_or_init(|| {
        let kald1 = forge_full_range(1969..=2399).expect("forge plage complète (build 1)");
        let kald2 = forge_full_range(1969..=2399).expect("forge plage complète (build 2)");
        (kald1, kald2)
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Lit la `TimelineEntry` pour `(year, doy)` — panique si le code de retour ≠ OK.
unsafe fn read_entry(kald: &[u8], year: u16, doy: u16) -> TimelineEntry {
    let mut e = TimelineEntry::zeroed();
    let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), year, doy, &mut e) };
    assert_eq!(
        rc, KAL_ENGINE_OK,
        "kal_read_entry({year}, {doy}) KO : code={rc}"
    );
    e
}

// ---------------------------------------------------------------------------
// 1. Validité du header sur la plage complète
// ---------------------------------------------------------------------------

#[test]
fn full_range_header_valid() {
    let kald = &kalds().0;
    let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), null_mut()) };
    assert_eq!(rc, KAL_ENGINE_OK);
}

// ---------------------------------------------------------------------------
// 2. Padding entries — doy=59 sur 431 années
// ---------------------------------------------------------------------------

/// Invariant v6 : les primitives temporelles sont toujours écrites, même au 29 février.
/// invariant v6 : primary_index peut être 0 pour un bissextile sans fête au 29 fév.
/// La présence des primitives temporelles (liturgical_week > 0) est la garantie structurelle.
/// Le padding pur (non-bissextile) reste zéro absolu.
#[test]
fn full_range_padding_entries_correct() {
    let kald = &kalds().0;

    for year in 1969u16..=2399 {
        let is_leap = is_leap_year(year as i32);
        let e = unsafe { read_entry(kald, year, 59) };

        if is_leap {
            assert!(
                e.liturgical_week > 0,
                "year {year} (bissextile) : doy=59 doit porter liturgical_week > 0 (primitives temporelles)"
            );
        } else {
            // Padding pur — zéro absolu sur tous les champs.
            assert_eq!(
                e.primary_index, 0,
                "year {year} (non-bissextile) : doy=59 doit être un Padding Entry",
            );
            assert_eq!(
                e.liturgical_week, 0,
                "year {year} (non-bissextile) : doy=59 = padding pur, liturgical_week doit être 0",
            );
            assert_eq!(
                e.occurrence_flags & 0b1111_1100,
                0,
                "year {year} (non-bissextile) : doy=59 = padding pur, bits period/reserved doivent être nuls",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Déterminisme SHA-256 — deux builds doivent produire un hash identique
// ---------------------------------------------------------------------------

/// En v5, le checksum SHA-256 est aux octets [36..68] du header.
#[test]
fn full_range_deterministic() {
    let (kald1, kald2) = kalds();
    assert_eq!(
        &kald1[36..68],
        &kald2[36..68],
        "SHA-256 (octets 36–67) doit être identique entre deux builds successifs"
    );
}

// ---------------------------------------------------------------------------
// 4. Saint Justin le 1er juin 2025 — relégation en secondary
// ---------------------------------------------------------------------------

/// Le 1er juin 2025 (doy=152), le 7e Dimanche de Pâques est prioritaire.
/// Saint Justin doit apparaître dans la liste secondary (secondary_count ≥ 1).
#[test]
fn full_range_iustini_june1_2025() {
    let kald = &kalds().0;
    let e = unsafe { read_entry(kald, 2025, 152) };
    assert!(
        e.secondary_count >= 1,
        "Saint Justin doit être en secondary le 1er juin 2025 (doy=152)"
    );
}

// ---------------------------------------------------------------------------
// 5. Triduum pascal 2025 — exactement 3 entrées Precedence 0
// ---------------------------------------------------------------------------

/// `kal_scan_flags` scanne la Timeline : pour chaque slot, résout le FeastEntry
/// et vérifie `FeastEntry.flags & flag_mask == flag_value`.
/// Masque : 0x000F (bits [3:0] = Precedence), valeur attendue : 0 (Triduum).
#[test]
fn full_range_triduum_2025_exactly_3_entries() {
    use liturgical_calendar_core::ffi::{KAL_ERR_BUF_TOO_SMALL, kal_scan_flags};

    let kald = &kalds().0;
    let mut indices = [0u32; 10];
    let mut count = 0u32;

    let rc = unsafe {
        kal_scan_flags(
            kald.as_ptr(),
            kald.len(),
            2025,
            2025,   // year_from, year_to
            0x000F, // flag_mask  : bits [3:0] = Precedence
            0,      // flag_value : Precedence 0 (Triduum)
            indices.as_mut_ptr(),
            10,
            &mut count,
        )
    };

    assert_ne!(
        rc, KAL_ERR_BUF_TOO_SMALL,
        "buffer trop petit — augmenter la capacité"
    );
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_eq!(
        count, 3,
        "Triduum pascal 2025 = exactement 3 jours Precedence-0"
    );
}

// ---------------------------------------------------------------------------
// 6. Transfer de Ss. Petri et Pauli
// ---------------------------------------------------------------------------

/// En 1973 et 1984, Pâques = 22 avril. Le Sacré-Cœur (pascha+68) atterrit le 29 juin (doy=180).
/// La résolution de préséance transfère Ss. Petri et Pauli au 30 juin (doy=181).
#[test]
fn full_range_petri_et_pauli_transfer_easter_april22() {
    let kald = &kalds().0;

    // Référence : 1970, Pâques ≠ 22 avril → Ss. Petri et Pauli en doy=180 sans conflit.
    let e_ref = unsafe { read_entry(kald, 1970, 180) };
    assert_ne!(
        e_ref.primary_index, 0,
        "Référence 1970 doy=180 : Padding Entry inattendu"
    );
    let petri_et_pauli_ridx = e_ref.primary_index;

    for year in [1973u16, 1984] {
        let e_june29 = unsafe { read_entry(kald, year, 180) };
        assert_ne!(
            e_june29.primary_index, petri_et_pauli_ridx,
            "doy=180 (29 juin {year}) : Ss. Petri et Pauli ne doit pas être en primary"
        );

        let e_june30 = unsafe { read_entry(kald, year, 181) };
        assert_eq!(
            e_june30.primary_index, petri_et_pauli_ridx,
            "doy=181 (30 juin {year}) : Ss. Petri et Pauli doit être en primary (transfert)"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Fabiani et Sebastiani — même DOY, primary + secondary
// ---------------------------------------------------------------------------

/// S. Fabianus et S. Sebastianus tombent tous deux le 20 janvier (doy=19).
/// Invariants : primary_index ≠ 0, secondary_count ≥ 1,
/// aucun registry_index secondaire identique au primaire.
#[test]
fn full_range_fabiani_et_sebastiani_same_doy() {
    use liturgical_calendar_core::ffi::kal_read_secondary;

    let kald = &kalds().0;

    for year in [2025u16, 2026, 2030] {
        let e = unsafe { read_entry(kald, year, 19) };

        assert_ne!(
            e.primary_index, 0,
            "{year} doy=19 : Padding Entry — l'une des deux mémoires doit occuper le slot"
        );
        assert!(
            e.secondary_count >= 1,
            "{year} doy=19 : secondary_count={} — Fabiani ou Sebastiani doit être en secondary",
            e.secondary_count
        );

        let mut sec_indices = vec![0u16; e.secondary_count as usize];
        let rc = unsafe {
            kal_read_secondary(
                kald.as_ptr(),
                kald.len(),
                e.secondary_offset,
                e.secondary_count,
                sec_indices.as_mut_ptr(),
                e.secondary_count,
            )
        };
        assert_eq!(rc, KAL_ENGINE_OK, "kal_read_secondary KO — {year} doy=19");

        assert!(
            sec_indices.iter().all(|&ridx| ridx != e.primary_index),
            "{year} doy=19 : un registry_index secondaire est identique au primaire"
        );
    }
}

// ---------------------------------------------------------------------------
// 8. Dominica II per annum 2026 (DOY 17)
// ---------------------------------------------------------------------------

#[test]
fn full_range_dominica_ii_2026_segment_i() {
    let kald = &kalds().0;
    let e = unsafe { read_entry(kald, 2026, 17) };
    assert_ne!(
        e.primary_index, 0,
        "Dominica II 2026 absente du .kald (doy=17)"
    );
}

// ---------------------------------------------------------------------------
// 9. Non-régression — Mercredi des Cendres 2026
// ---------------------------------------------------------------------------

/// Mercredi des Cendres 2026 (DOY=48) doit être présent ; DOY=49 doit être vide.
#[test]
fn full_range_ash_wednesday_early_easter_2026() {
    let kald = &kalds().0;

    let e48 = unsafe { read_entry(kald, 2026, 48) };
    assert_ne!(
        e48.primary_index, 0,
        "Feria IV Cinerum absente du DOY 48 en 2026"
    );

    let e49 = unsafe { read_entry(kald, 2026, 49) };
    assert_eq!(e49.primary_index, 0, "DOY 49 doit être vide en 2026");
}

// ---------------------------------------------------------------------------
// 10. Non-régression — Immaculati Cordis : flags history 1996
// ---------------------------------------------------------------------------

/// Vérifie que les flags du FeastEntry d'Immaculati Cordis reflètent l'entrée
/// `history` de 1996 (precedence interne 9 = MemoriaeObligatoriaGenerales)
/// et non l'entrée 1969 gelée (precedence 11 = MemoriaeAdLibitum).
///
/// Régression couverte : le registre AOT gelait les flags à la première
/// insertion (année 1969), ignorant les entrées `history` ultérieures.
#[test]
fn full_range_immaculati_cordis_flags_post_1996() {
    use liturgical_calendar_core::ffi::{kal_read_feast, kal_read_secondary};

    const DOY: u16 = 164; // 13 juin
    const YEAR: u16 = 2026;
    const FEAST_ID_IMMACULATI: u16 = 0x003d;
    const PREC_OBL_GEN: u8 = 9; // MemoriaeObligatoriaGenerales

    let kald = &kalds().0;
    let ptr = kald.as_ptr();
    let len = kald.len();

    let entry = unsafe { read_entry(kald, YEAR, DOY) };
    assert!(
        !entry.is_padding(),
        "DOY {DOY} {YEAR} : Padding Entry inattendu"
    );
    assert!(
        entry.secondary_count >= 1,
        "aucune secondaire — Immaculati manquant"
    );

    // Chercher Immaculati Cordis parmi primary + secondaries.
    let mut immaculati_flags: Option<u16> = None;

    // Vérifier primary
    let mut fe = liturgical_calendar_core::entry::FeastEntry::zeroed();
    unsafe { kal_read_feast(ptr, len, entry.primary_index, &mut fe) };
    if fe.feast_id == FEAST_ID_IMMACULATI {
        immaculati_flags = Some(fe.flags);
    }

    // Vérifier secondaries
    if immaculati_flags.is_none() {
        let mut sec = vec![0u16; entry.secondary_count as usize];
        unsafe {
            kal_read_secondary(
                ptr,
                len,
                entry.secondary_offset,
                entry.secondary_count,
                sec.as_mut_ptr(),
                entry.secondary_count,
            );
        };
        for &ridx in &sec {
            let mut sf = liturgical_calendar_core::entry::FeastEntry::zeroed();
            unsafe { kal_read_feast(ptr, len, ridx, &mut sf) };
            if sf.feast_id == FEAST_ID_IMMACULATI {
                immaculati_flags = Some(sf.flags);
                break;
            }
        }
    }

    let flags = immaculati_flags
        .expect("Immaculati Cordis (0x003d) introuvable en primary ni secondary pour 2026 doy=164");

    let prec = (flags & 0x000F) as u8;
    assert_eq!(
        prec, PREC_OBL_GEN,
        "Immaculati Cordis 2026 : precedence binaire={prec}, \
         attendu {PREC_OBL_GEN} (MemoriaeObligatoriaGenerales, history depuis 1996) — \
         régression : registre gelé sur l'entrée 1969 (prec=11, MemoriaeAdLibitum)"
    );
}
