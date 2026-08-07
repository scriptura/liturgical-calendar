//! Test d'intégration, critères de sortie officiels — kald v5.
//!
//! Vérifie :
//!   1. `kal_validate_header` OK sur le `.kald` v5 produit par `compile`.
//!   2. `kal_read_entry` sans erreur sur les 366 slots de l'année 2025.
//!   3. Padding Entry à doy=59 (2025 non-bissextile → slot Feb 29 vide).
//!   4. Slot Pâques 2025 : `primary_index != 0`,
//!      `kal_read_feast` retourne nature == Sollemnitas.
//!   5. `occurrence_flags` corrects : doy du Samedi Saint a le bit HAS_VESPERAE_I.
//!   6. `LitsProvider::get(feast_id, 2025)` retourne le label latin de Pâques.
//!   7. `kald_build_id` cohérent entre `.kald` et `.lits`.

use std::fs;
use std::path::PathBuf;

use liturgical_calendar_core::{
    KAL_ENGINE_OK,
    entry::{FeastEntry, TimelineEntry},
    kal_read_entry, kal_read_feast, kal_validate_header,
    lits_provider::LitsProvider,
    types::Nature,
};

use liturgical_calendar_forge::{
    FeastRegistry, I18nConfig, canonicalization::compute_easter, compile,
    parsing::parse_feast_from_yaml, registry::Scope,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn tmp() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("conformity_2025_v5")
}

/// Corpus minimal : uniquement `dominica_resurrectionis` (Pâques).
fn minimal_registry() -> FeastRegistry {
    let mut registry = FeastRegistry::new();
    let yaml = r#"
version: 1
category: 0
class: lord
mobile:
  anchor: pascha
  offset: 0
history:
  - precedence: 2
    nature: sollemnitas
    color: albus
    period: tempus_paschale
    has_vigil_mass: false
"#;
    let feast = parse_feast_from_yaml("dominica_resurrectionis", Scope::Universal, yaml)
        .expect("parse dominica_resurrectionis");
    registry.insert(feast);
    registry
}

fn setup_i18n(base_dir: &PathBuf) -> PathBuf {
    let la_dir = base_dir.join("universale").join("i18n").join("la");
    fs::create_dir_all(&la_dir).unwrap();
    let content = "version: 1\nhistory:\n  - from: 1969\n    label: \"Dominica Resurrectionis\"\n";
    fs::write(la_dir.join("dominica_resurrectionis.yaml"), content).unwrap();
    base_dir.to_owned()
}

// ─── Fixture (OnceLock) ───────────────────────────────────────────────────────

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
        let base = tmp();
        let kald = base.join("test.kald");
        let lits_dir = base.join("lits");

        fs::create_dir_all(&lits_dir).unwrap();

        let _i18n_root = setup_i18n(&base);
        let registry = minimal_registry();

        let kald_checksum = compile(
            registry,
            &kald,
            0,
            Some(I18nConfig {
                i18n_root: &base,
                scope_path: Some("universale"),
                lits_dir: &lits_dir,
            }),
            &base.join("feast_registry.lock"),
        )
        .expect("compile doit réussir");

        let kald_bytes = fs::read(&kald).expect("lecture .kald");
        let lits_bytes = fs::read(lits_dir.join("la.lits")).expect("lecture la.lits");
        let easter_doy = compute_easter(2025);

        Fixture {
            kald_bytes,
            lits_bytes,
            kald_checksum,
            easter_doy,
        }
    })
}

// ─── 1. kal_validate_header ───────────────────────────────────────────────────

#[test]
fn kald_validate_header_ok() {
    let f = fixture();
    let rc = unsafe {
        kal_validate_header(
            f.kald_bytes.as_ptr(),
            f.kald_bytes.len(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK, "kal_validate_header doit retourner OK");
}

// ─── 2 + 3. kal_read_entry : 366 slots + Padding doy=59 ─────────────────────

#[test]
fn kald_read_entry_all_366_slots_2025() {
    let f = fixture();
    let ptr = f.kald_bytes.as_ptr();
    let len = f.kald_bytes.len();

    for doy in 0u16..=365 {
        let mut entry = TimelineEntry::zeroed();
        let rc = unsafe { kal_read_entry(ptr, len, 2025, doy, &mut entry) };
        assert_eq!(
            rc, KAL_ENGINE_OK,
            "kal_read_entry(2025, doy={}) doit retourner OK",
            doy
        );
    }
}

#[test]
fn kald_padding_entry_doy_59_non_leap() {
    let f = fixture();
    let mut entry = TimelineEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(
            f.kald_bytes.as_ptr(),
            f.kald_bytes.len(),
            2025,
            59,
            &mut entry,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert!(
        entry.is_padding(),
        "doy=59 doit être une Padding Entry pour 2025, primary_index={}",
        entry.primary_index
    );
}

// ─── 4. Pâques 2025 : primary_index ≠ 0, nature == Sollemnitas ───────────────

#[test]
fn kald_easter_2025_entry_coherent() {
    let f = fixture();
    let doy = f.easter_doy;
    let ptr = f.kald_bytes.as_ptr();
    let len = f.kald_bytes.len();

    let mut entry = TimelineEntry::zeroed();
    let rc = unsafe { kal_read_entry(ptr, len, 2025, doy, &mut entry) };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_ne!(
        entry.primary_index, 0,
        "Pâques 2025 (doy={}) ne doit pas être Padding",
        doy
    );

    // Résolution des invariants via kal_read_feast.
    let mut feast = FeastEntry::zeroed();
    let rc_feast = unsafe { kal_read_feast(ptr, len, entry.primary_index, &mut feast) };
    assert_eq!(
        rc_feast, KAL_ENGINE_OK,
        "kal_read_feast doit réussir pour Pâques"
    );

    let nature = feast.nature().expect("nature doit être valide");
    assert_eq!(nature, Nature::Sollemnitas, "Pâques est une Sollemnitas");
}

// ─── 5. occurrence_flags : Samedi Saint a HAS_VESPERAE_I ─────────────────────

#[test]
fn kald_vesperae_i_bit_sabbato_sancto() {
    let f = fixture();
    let doy = f.easter_doy;

    if doy == 0 {
        return;
    }

    let mut entry = TimelineEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(
            f.kald_bytes.as_ptr(),
            f.kald_bytes.len(),
            2025,
            doy - 1,
            &mut entry,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert!(
        entry.has_vesperae_i(),
        "Samedi Saint (doy={}) doit avoir HAS_VESPERAE_I (occurrence_flags=0b{:02b})",
        doy - 1,
        entry.occurrence_flags
    );
}

// ─── 6. LitsProvider : label latin de Pâques 2025 ────────────────────────────

#[test]
fn lits_provider_get_easter_2025() {
    let f = fixture();
    let ptr = f.kald_bytes.as_ptr();
    let len = f.kald_bytes.len();

    // Lire la TimelineEntry de Pâques.
    let mut entry = TimelineEntry::zeroed();
    let rc = unsafe { kal_read_entry(ptr, len, 2025, f.easter_doy, &mut entry) };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_ne!(entry.primary_index, 0);

    // Résoudre le feast_id via kal_read_feast.
    let mut feast = FeastEntry::zeroed();
    let rc_feast = unsafe { kal_read_feast(ptr, len, entry.primary_index, &mut feast) };
    assert_eq!(rc_feast, KAL_ENGINE_OK);
    assert_ne!(feast.feast_id, 0, "feast_id de Pâques doit être non nul");

    let provider =
        LitsProvider::new(&f.lits_bytes).expect("LitsProvider::new doit réussir sur la.lits");

    let label = provider
        .get(feast.feast_id, 2025)
        .expect("LitsProvider::get doit retourner un label pour Pâques 2025");

    assert_eq!(
        label.label, "Dominica Resurrectionis",
        "label latin inattendu : {:?}",
        label.label
    );
}

// ─── 7. kald_build_id : cohérence .kald ↔ .lits ─────────────────────────────

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

// ─── Bonus : header .lits ────────────────────────────────────────────────────

#[test]
fn lits_header_magic_and_version() {
    let f = fixture();
    assert!(f.lits_bytes.len() >= 6);
    assert_eq!(&f.lits_bytes[0..4], b"LITS", "magic .lits invalide");
    let version = u16::from_le_bytes([f.lits_bytes[4], f.lits_bytes[5]]);
    assert_eq!(version, 1u16, "version .lits doit être 1");
}
