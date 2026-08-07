//! Tests d'intégration — binaire `kal-read`.
//!
//! Vérifie :
//!   1. Sortie structurelle correcte sur une entrée valide (Pâques 2025).
//!   2. Padding Entry affichée correctement (doy=59, 2025 non-bissextile).
//!   3. Label + annotation affichés avec `--lits`.
//!   4. Annotation absente affichée comme `—`.
//!   5. build_id mismatch entre `.kald` et `.lits` → erreur.
//!   6. Fichier `.kald` introuvable → erreur.
//!   7. Code de retour 0 sur succès, non-0 sur erreur.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use liturgical_calendar_forge::{
    FeastRegistry, I18nConfig, canonicalization::compute_easter, compile,
    parsing::parse_feast_from_yaml, registry::Scope,
};

// ---------------------------------------------------------------------------
// Helpers partagés
// ---------------------------------------------------------------------------

fn tmp() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kal_read_integration")
}

fn kal_read_bin() -> &'static str {
    env!("CARGO_BIN_EXE_kal-read")
}

fn minimal_registry_with_annotation() -> FeastRegistry {
    let mut registry = FeastRegistry::new();

    // Pâques — label seul (pas d'annotation)
    let yaml_pascha = r#"
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
"#;
    registry.insert(
        parse_feast_from_yaml("dominica_resurrectionis", Scope::Universal, yaml_pascha)
            .expect("parse dominica_resurrectionis"),
    );

    // Dominica II Paschae — avec annotation
    let yaml_ii = r#"
version: 1
category: 0
class: lord
mobile:
  anchor: pascha
  offset: 7
history:
  - precedence: 1
    nature: dominica
    color: albus
    period: tempus_paschale
"#;
    registry.insert(
        parse_feast_from_yaml("dominica_ii_paschae", Scope::Universal, yaml_ii)
            .expect("parse dominica_ii_paschae"),
    );

    registry
}

fn setup_i18n(base_dir: &PathBuf) -> PathBuf {
    let la_dir = base_dir.join("universale").join("i18n").join("la");
    fs::create_dir_all(&la_dir).unwrap();

    // Pâques — label seul, pas d'annotation
    fs::write(
        la_dir.join("dominica_resurrectionis.yaml"),
        "version: 1\nhistory:\n  - from: 1969\n    label: \"Dominica Resurrectionis\"\n",
    )
    .unwrap();

    // Dominica II Paschae — label + annotation
    fs::write(
        la_dir.join("dominica_ii_paschae.yaml"),
        "version: 1\nhistory:\n  - from: 1969\n    label: \"Dominica II Paschæ\"\n    annotation: \"*In albis*\"\n",
    ).unwrap();

    base_dir.to_owned()
}

// ---------------------------------------------------------------------------
// Fixture — compilée une seule fois
// ---------------------------------------------------------------------------

struct Fixture {
    kald_path: PathBuf,
    lits_path: PathBuf,
    easter_doy: u16,
}

static FIXTURE: OnceLock<Fixture> = OnceLock::new();

fn fixture() -> &'static Fixture {
    FIXTURE.get_or_init(|| {
        let base = tmp();
        let lits_dir = base.join("lits");
        fs::create_dir_all(&lits_dir).unwrap();

        let rite_root = setup_i18n(&base);
        let registry = minimal_registry_with_annotation();

        let kald_path = base.join("test.kald");

        compile(
            registry,
            &kald_path,
            0,
            Some(I18nConfig {
                i18n_root: &rite_root,
                scope_path: Some("universale"),
                lits_dir: &lits_dir,
            }),
            &base.join("feast_registry.lock"),
        )
        .expect("compile doit réussir");

        // Renommer la.lits produit par compile
        let lits_src = lits_dir.join("la.lits");
        let lits_path = base.join("test_la.lits");
        fs::rename(&lits_src, &lits_path).expect("renommage la.lits");

        let easter_doy = compute_easter(2025);

        Fixture {
            kald_path,
            lits_path,
            easter_doy,
        }
    })
}

// ---------------------------------------------------------------------------
// Helpers d'assertion
// ---------------------------------------------------------------------------

fn run(args: &[&str]) -> std::process::Output {
    Command::new(kal_read_bin())
        .args(args)
        .output()
        .expect("kal-read doit s'exécuter")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// ---------------------------------------------------------------------------
// 1. Entrée valide — code de retour 0 + champs structurels
// ---------------------------------------------------------------------------

#[test]
fn kal_read_exit_ok_on_valid_entry() {
    let f = fixture();
    let out = run(&[
        "--kald",
        f.kald_path.to_str().unwrap(),
        "--year",
        "2025",
        "--doy",
        &f.easter_doy.to_string(),
    ]);
    assert!(
        out.status.success(),
        "exit code doit être 0 : {}",
        stderr(&out)
    );
}

#[test]
fn kal_read_valid_entry_contains_feast_id() {
    let f = fixture();
    let out = run(&[
        "--kald",
        f.kald_path.to_str().unwrap(),
        "--year",
        "2025",
        "--doy",
        &f.easter_doy.to_string(),
    ]);
    let s = stdout(&out);
    assert!(
        s.contains("feast_id"),
        "stdout doit contenir feast_id :\n{s}"
    );
    assert!(
        s.contains("SollemnitatesGenerales")
            || s.contains("SollemnitatesMaiores")
            || s.contains("precedence"),
        "stdout doit mentionner la précédence :\n{s}"
    );
}

// ---------------------------------------------------------------------------
// 2. Padding Entry doy=59 (2025 non-bissextile)
// ---------------------------------------------------------------------------

#[test]
fn kal_read_padding_entry_doy_59() {
    let f = fixture();
    let out = run(&[
        "--kald",
        f.kald_path.to_str().unwrap(),
        "--year",
        "2025",
        "--doy",
        "59",
    ]);
    assert!(
        out.status.success(),
        "exit code doit être 0 : {}",
        stderr(&out)
    );
    let s = stdout(&out);
    assert!(
        s.contains("Padding Entry"),
        "doy=59 doit afficher Padding Entry :\n{s}"
    );
}

// ---------------------------------------------------------------------------
// 3. --lits : label affiché
// ---------------------------------------------------------------------------

#[test]
fn kal_read_lits_shows_label() {
    let f = fixture();
    let out = run(&[
        "--kald",
        f.kald_path.to_str().unwrap(),
        "--lits",
        f.lits_path.to_str().unwrap(),
        "--year",
        "2025",
        "--doy",
        &f.easter_doy.to_string(),
    ]);
    assert!(
        out.status.success(),
        "exit code doit être 0 : {}",
        stderr(&out)
    );
    let s = stdout(&out);
    assert!(
        s.contains("Dominica Resurrectionis"),
        "stdout doit contenir le label latin :\n{s}"
    );
}

// ---------------------------------------------------------------------------
// 4. --lits : annotation affichée (Dominica II Paschae = easter_doy + 7)
// ---------------------------------------------------------------------------

#[test]
fn kal_read_lits_shows_annotation() {
    let f = fixture();
    let doy_ii = f.easter_doy + 7;
    let out = run(&[
        "--kald",
        f.kald_path.to_str().unwrap(),
        "--lits",
        f.lits_path.to_str().unwrap(),
        "--year",
        "2025",
        "--doy",
        &doy_ii.to_string(),
    ]);
    assert!(
        out.status.success(),
        "exit code doit être 0 : {}",
        stderr(&out)
    );
    let s = stdout(&out);
    assert!(
        s.contains("*In albis*"),
        "stdout doit contenir l'annotation *In albis* :\n{s}"
    );
}

// ---------------------------------------------------------------------------
// 5. --lits : annotation absente → ligne omise
// ---------------------------------------------------------------------------

#[test]
fn kal_read_lits_no_annotation_not_shown() {
    let f = fixture();
    let out = run(&[
        "--kald",
        f.kald_path.to_str().unwrap(),
        "--lits",
        f.lits_path.to_str().unwrap(),
        "--year",
        "2025",
        "--doy",
        &f.easter_doy.to_string(),
    ]);
    assert!(
        out.status.success(),
        "exit code doit être 0 : {}",
        stderr(&out)
    );
    let s = stdout(&out);
    // Le label doit être présent.
    assert!(s.contains("Dominica Resurrectionis"), "label absent :\n{s}");
    // La ligne annotation ne doit PAS apparaître quand elle est absente.
    assert!(
        !s.contains("annotation"),
        "annotation ne doit pas s'afficher quand elle est absente :\n{s}"
    );
}

// ---------------------------------------------------------------------------
// 6. build_id mismatch → erreur non-0
// ---------------------------------------------------------------------------

#[test]
fn kal_read_build_id_mismatch_returns_error() {
    let f = fixture();
    let base = tmp();

    // Fabriquer un .lits avec build_id corrompu (octet 12 muté)
    let mut lits = fs::read(&f.lits_path).unwrap();
    lits[12] ^= 0xFF;
    let bad_lits = base.join("bad.lits");
    fs::write(&bad_lits, &lits).unwrap();

    let out = run(&[
        "--kald",
        f.kald_path.to_str().unwrap(),
        "--lits",
        bad_lits.to_str().unwrap(),
        "--year",
        "2025",
        "--doy",
        &f.easter_doy.to_string(),
    ]);
    assert!(
        !out.status.success(),
        "build_id mismatch doit retourner un code non-0"
    );
    let s = stderr(&out);
    assert!(
        s.contains("build_id") || s.contains("mismatch"),
        "stderr doit mentionner le mismatch :\n{s}"
    );
}

// ---------------------------------------------------------------------------
// 7. Fichier .kald introuvable → erreur non-0
// ---------------------------------------------------------------------------

#[test]
fn kal_read_missing_kald_returns_error() {
    let out = run(&[
        "--kald",
        "/tmp/inexistant_kal_read_test.kald",
        "--year",
        "2025",
        "--doy",
        "0",
    ]);
    assert!(
        !out.status.success(),
        "fichier absent doit retourner un code non-0"
    );
}
