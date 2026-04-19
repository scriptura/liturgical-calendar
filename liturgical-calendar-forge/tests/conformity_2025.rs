//! Test d'intégration Jalon 2 — critères de sortie officiels.
//!
//! Vérifie :
//!   1. `kal_validate_header` OK sur le `.kald` produit par `compile`.
//!   2. `kal_read_entry` sans erreur sur les 366 slots de l'année 2025.
//!   3. Padding Entry à doy=59 (2025 non-bissextile → slot Feb 29 vide).
//!   4. Slot Pâques 2025 : `primary_id != 0`, nature == Sollemnitas.
//!   5. Bits vespers [14:15] corrects : doy du Samedi Saint a le bit Vigilia.
//!   6. `LitsProvider::get(pascha_feast_id, 2025)` retourne le label latin.
//!   7. `kald_build_id` cohérent entre `.kald` et `.lits`.

use std::fs;
use std::path::PathBuf;

use liturgical_calendar_core::{
    lits_provider::LitsProvider,
    kal_read_entry, kal_validate_header,
    CalendarEntry, Nature,
    KAL_ENGINE_OK,
};

use liturgical_calendar_forge::{
    compile, I18nConfig, FeastRegistry,
    canonicalization::compute_easter,
    parsing::parse_feast_from_yaml,
    registry::Scope,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn tmp() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("conformity_2025")
}

/// Corpus minimal : uniquement Dominica Resurrectionis (Pâques).
///
/// YAML precedence: 2 → interne 1 (SollemnitatesFixaeMaior),
/// conforme à la Tabella dierum liturgicorum 1969, rang I.
fn minimal_registry() -> FeastRegistry {
    let mut registry = FeastRegistry::new();

    let yaml = r#"
version: 1
category: 0
mobile:
  anchor: pascha
  offset: 0
history:
  - precedence: 2
    nature: sollemnitas
    color: albus
    season: pascha
    has_vigil_mass: false
"#;
    let feast = parse_feast_from_yaml("dominica_resurrectionis", Scope::Universal, yaml)
        .expect("parse dominica_resurrectionis");
    registry.insert(feast);

    registry
}

/// Construit l'arborescence i18n minimale dans `base_dir`.
fn setup_i18n(base_dir: &PathBuf) -> PathBuf {
    let i18n_root = base_dir.join("i18n");
    let la_dir    = i18n_root.join("la");
    fs::create_dir_all(&la_dir).unwrap();

    let content = "1969:\n  title: \"Dominica Resurrectionis\"\n";
    fs::write(la_dir.join("dominica_resurrectionis.yaml"), content).unwrap();

    i18n_root
}

// ---------------------------------------------------------------------------
// Fixture — compilée une seule fois via OnceLock.
// ---------------------------------------------------------------------------

use std::sync::OnceLock;

struct Fixture {
    kald_bytes: Vec<u8>,
    lits_bytes: Vec<u8>,
    kald_checksum: [u8; 32],
    easter_doy: u16,
}

static FIXTURE: OnceLock<Fixture> = OnceLock::new();

fn fixture() -> &'static Fixture {
    FIXTURE.get_or_init(|| {
        let base    = tmp();
        let kald    = base.join("test.kald");
        let lits_dir = base.join("lits");

        fs::create_dir_all(&lits_dir).unwrap();

        let i18n_root = setup_i18n(&base);
        let registry  = minimal_registry();

        let kald_checksum = compile(
            registry,
            &kald,
            0,
            Some(I18nConfig { i18n_root: &i18n_root, lits_dir: &lits_dir }),
        )
        .expect("compile doit réussir");

        let kald_bytes = fs::read(&kald).expect("lecture .kald");
        let lits_bytes = fs::read(lits_dir.join("la.lits")).expect("lecture la.lits");

        let easter_doy = compute_easter(2025);

        Fixture { kald_bytes, lits_bytes, kald_checksum, easter_doy }
    })
}

// ---------------------------------------------------------------------------
// 1. kal_validate_header
// ---------------------------------------------------------------------------

#[test]
fn kald_validate_header_ok() {
    let f = fixture();
    let rc = unsafe {
        kal_validate_header(f.kald_bytes.as_ptr(), f.kald_bytes.len(), std::ptr::null_mut())
    };
    assert_eq!(rc, KAL_ENGINE_OK, "kal_validate_header doit retourner OK");
}

// ---------------------------------------------------------------------------
// 2 + 3. kal_read_entry sur les 366 slots 2025 + Padding Entry doy=59
// ---------------------------------------------------------------------------

#[test]
fn kald_read_entry_all_366_slots_2025() {
    let f = fixture();
    let ptr = f.kald_bytes.as_ptr();
    let len = f.kald_bytes.len();

    for doy in 0u16..=365 {
        let mut entry = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(ptr, len, 2025, doy, &mut entry) };
        assert_eq!(
            rc, KAL_ENGINE_OK,
            "kal_read_entry(2025, doy={}) doit retourner OK", doy
        );
    }
}

#[test]
fn kald_padding_entry_doy_59_non_leap() {
    let f = fixture();
    let mut entry = CalendarEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(f.kald_bytes.as_ptr(), f.kald_bytes.len(), 2025, 59, &mut entry)
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert!(
        entry.is_padding(),
        "doy=59 doit être une Padding Entry pour 2025 (non-bissextile), primary_id={}",
        entry.primary_id
    );
}

// ---------------------------------------------------------------------------
// 4. Slot Pâques 2025 — primary_id non nul, nature == Sollemnitas
// ---------------------------------------------------------------------------

#[test]
fn kald_easter_2025_entry_coherent() {
    let f = fixture();
    let doy = f.easter_doy;
    let mut entry = CalendarEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(f.kald_bytes.as_ptr(), f.kald_bytes.len(), 2025, doy, &mut entry)
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_ne!(
        entry.primary_id, 0,
        "Pâques 2025 (doy={}) ne doit pas être une Padding Entry", doy
    );
    let nature = entry.nature().expect("nature doit être valide");
    assert_eq!(nature, Nature::Sollemnitas, "Pâques est une Sollemnitas");
}

// ---------------------------------------------------------------------------
// 5. Bits vespers [14:15] — Samedi Saint a le bit vesperae_i
// ---------------------------------------------------------------------------

#[test]
fn kald_vesperae_i_bit_sabbato_sancto() {
    let f   = fixture();
    let doy = f.easter_doy;

    if doy == 0 { return; }

    let mut entry = CalendarEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(
            f.kald_bytes.as_ptr(), f.kald_bytes.len(),
            2025, doy - 1, &mut entry,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert!(
        entry.has_vesperae_i(),
        "Samedi Saint (doy={}) doit avoir le bit vesperae_i posé (bit [14])",
        doy - 1
    );
}

// ---------------------------------------------------------------------------
// 6. LitsProvider::get — label latin de Pâques 2025
// ---------------------------------------------------------------------------

#[test]
fn lits_provider_get_easter_2025() {
    let f = fixture();

    let mut entry = CalendarEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(
            f.kald_bytes.as_ptr(), f.kald_bytes.len(),
            2025, f.easter_doy, &mut entry,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_ne!(entry.primary_id, 0, "Pâques doit avoir un FeastID non nul");

    let provider = LitsProvider::new(&f.lits_bytes)
        .expect("LitsProvider::new doit réussir sur la.lits");

    let label = provider.get(entry.primary_id, 2025)
        .expect("LitsProvider::get doit retourner un label pour Pâques 2025");

    assert_eq!(label, "Dominica Resurrectionis", "label latin inattendu : {:?}", label);
}

// ---------------------------------------------------------------------------
// 7. kald_build_id — cohérence entre .kald et .lits
// ---------------------------------------------------------------------------

#[test]
fn kald_build_id_coherent_with_lits() {
    let f = fixture();

    let expected_build_id = &f.kald_checksum[..8];

    assert!(
        f.lits_bytes.len() >= 20,
        ".lits trop court pour contenir un header valide"
    );
    let lits_build_id = &f.lits_bytes[12..20];

    assert_eq!(
        lits_build_id, expected_build_id,
        "kald_build_id incohérent : .kald={:?}, .lits={:?}",
        expected_build_id, lits_build_id
    );
}

// ---------------------------------------------------------------------------
// Bonus — header .lits : magic, version
// ---------------------------------------------------------------------------

#[test]
fn lits_header_magic_and_version() {
    let f = fixture();
    assert!(f.lits_bytes.len() >= 6);
    assert_eq!(&f.lits_bytes[0..4], b"LITS", "magic .lits invalide");
    let version = u16::from_le_bytes([f.lits_bytes[4], f.lits_bytes[5]]);
    assert_eq!(version, 1u16, "version .lits doit être 1");
}
