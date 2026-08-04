#![allow(missing_docs)] // activé en Jalon 3

pub mod canonicalization;
pub mod error;
pub mod i18n;
pub mod id_alloc;
pub mod ingestion;
pub(crate) mod lits_writer;
pub mod lock;
pub mod materialization;
pub mod packing;
pub mod parsing;
pub mod registry;
pub mod resolution;
pub mod variant_lock;

// ── Exports publics ───────────────────────────────────────────────────────────

pub use canonicalization::{
    AnchorTable, CanonicalizedYear, MONTH_STARTS, PreResolvedTransfers, build_anchor_table,
    canonicalize_year, compute_easter, is_leap_year, resolve_adventus, resolve_epiphania,
    resolve_nativitas, resolve_tempus_ordinarium,
};
pub use error::ForgeError;
pub use id_alloc::allocate_feast_ids;
pub use ingestion::ingest_corpus;
pub(crate) use packing::build_kald;
pub use parsing::parse_feast_from_yaml;
pub use registry::FeastRegistry;
pub use variant_lock::VariantRegistryLock;

// ── Imports internes ──────────────────────────────────────────────────────────

use liturgical_calendar_core::entry::TimelineEntry;
use materialization::{PoolBuilder, build_feast_registry, generate_year, vespers_lookahead_pass};
use packing::write_kald;
use resolution::resolve_year;
use std::path::Path;

// ── Types publics ─────────────────────────────────────────────────────────────

/// Paramètres i18n pour la production des fichiers `.lits` compagnons.
pub struct I18nConfig<'a> {
    pub i18n_root: &'a Path,
    pub scope_path: Option<&'a str>,
    pub lits_dir: &'a Path,
}

// ── Pipeline de compilation ───────────────────────────────────────────────────

/// Compile un corpus YAML en fichier `.kald` v5 pour la plage 1969–2399.
/// Si `i18n` est fourni, produit également un `.lits` par langue compilée.
///
/// # Pipeline v5
///
/// - Étape 1     — Allocation des FeastIDs stables (lock)
/// - Étape 1bis  — i18n Resolution (si `i18n` fourni)
/// - Étapes 3–5a — Canonicalization → Resolution (toutes les années)
/// - Étape 5b    — Pass 1 : construction du Feast Registry (AOT)
/// - Étape 5c    — Pass 2 : génération Timeline + Pool + vespers lookahead
/// - Étape 6     — Binary Packing `.kald` puis `.lits`
pub fn compile(
    registry: FeastRegistry,
    output: &Path,
    variant_id: u16,
    i18n: Option<I18nConfig<'_>>,
    lock_path: &Path,
) -> Result<[u8; 32], ForgeError> {
    // ── Étape 1 — Allocation d'IDs stables ───────────────────────────────────
    let mut lock = crate::lock::FeastRegistryLock::load(lock_path)?;
    let feast_ids = allocate_feast_ids(&registry, &mut lock, lock_path)?;

    // ── Validation post-merge : class obligatoire ─────────────────────────────
    for feast in registry.iter() {
        if feast.class.is_none() {
            return Err(ForgeError::Parse(
                crate::error::ParseError::MissingClassAfterMerge {
                    slug: feast.slug.clone(),
                },
            ));
        }
    }

    // ── Étape 1bis — i18n Resolution ─────────────────────────────────────────
    let i18n_artifacts = match &i18n {
        Some(cfg) => {
            let mut store = i18n::DictStore::new();
            let langs = i18n::discover_and_load_i18n(cfg.i18n_root, cfg.scope_path, &mut store)?;
            i18n::remap_default_from_keys(&mut store, &registry);
            i18n::propagate_labels(&mut store, &registry);
            i18n::validate_i18n(&registry, &store)?;
            Some((store, langs))
        }
        None => None,
    };

    // ── Étapes 3–5a — Canonicalization → Resolution (toutes les années) ──────
    //
    // Les `ResolvedCalendar` sont accumulés en mémoire : la Pass 1 (build_feast_registry)
    // requiert la vue complète du corpus avant de pouvoir générer les TimelineEntry.

    let mut all_resolved: Vec<(
        resolution::ResolvedCalendar,
        canonicalization::SeasonBoundaries,
    )> = Vec::with_capacity(431);

    for year in 1969u16..=2399 {
        let canon = canonicalize_year(year, &registry)?;
        let sb = canon.season_boundaries.clone();
        let resolved = resolve_year(canon, &registry, &feast_ids)?;
        all_resolved.push((resolved, sb));
    }

    // ── Étape 5b — Pass 1 : Feast Registry ───────────────────────────────────

    let refs: Vec<(
        &resolution::ResolvedCalendar,
        &canonicalization::SeasonBoundaries,
    )> = all_resolved.iter().map(|(r, s)| (r, s)).collect();

    let feast_registry = build_feast_registry(&refs)?;

    // ── Étape 5c — Pass 2 : Timeline + Pool + vespers lookahead ──────────────

    let mut pool = PoolBuilder::new();
    let mut all_entries: Vec<[TimelineEntry; 366]> = Vec::with_capacity(431);

    for (resolved, sb) in &all_resolved {
        let entries = generate_year(resolved, &mut pool, sb, &feast_registry)?;
        all_entries.push(entries);
    }

    // Vespers lookahead — accès simultané i et i+1 via split_at_mut.
    for i in 0..all_entries.len() {
        let (left, right) = all_entries.split_at_mut(i + 1);
        let next_jan1 = right.first().map(|e| &e[0]);
        vespers_lookahead_pass(&mut left[i], feast_registry.as_slice(), next_jan1);
    }

    // ── Étape 6 — Binary Packing `.kald` ─────────────────────────────────────
    let kald_checksum = write_kald(output, all_entries, &feast_registry, pool, variant_id)?;

    // ── Étape 6 — Binary Packing `.lits` (une par langue) ────────────────────
    if let (Some(cfg), Some((store, langs))) = (&i18n, i18n_artifacts) {
        let lang_refs: Vec<&str> = langs.iter().map(String::as_str).collect();
        let table = i18n::build_label_table(&registry, &store, &feast_ids, &lang_refs);
        for lang in &lang_refs {
            let lits_path = cfg.lits_dir.join(format!("{}.lits", lang));
            crate::lits_writer::write_lits(&lits_path, &table, lang, &kald_checksum)?;
        }

        // ── Vérification de cohérence build ID ───────────────────────────────
        let expected_build_id = &kald_checksum[..8];
        for lang in &lang_refs {
            let lits_path = cfg.lits_dir.join(format!("{}.lits", lang));
            let header =
                std::fs::read(&lits_path).map_err(|e| ForgeError::ArtifactVerificationFailed {
                    kald_path: lits_path.clone(),
                    reason: e.to_string(),
                })?;
            if header.len() < 20 {
                return Err(ForgeError::ArtifactVerificationFailed {
                    kald_path: lits_path.clone(),
                    reason: format!("header tronqué ({} octets)", header.len()),
                });
            }
            let lits_build_id: [u8; 8] = header[12..20].try_into().unwrap();
            if lits_build_id != expected_build_id {
                let mut kald_id = [0u8; 8];
                kald_id.copy_from_slice(expected_build_id);
                return Err(ForgeError::ArtifactBuildIdMismatch {
                    lits_path,
                    lits_build_id,
                    kald_build_id: kald_id,
                });
            }
        }
    }

    Ok(kald_checksum)
}

// ── Helper de test ────────────────────────────────────────────────────────────

/// Compile le corpus complet en buffer binaire `.kald` sans I/O disque (hors lock).
pub fn forge_full_range(_range: std::ops::RangeInclusive<u16>) -> Result<Vec<u8>, ForgeError> {
    static LOCK_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let corpus_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../corpus/romanus")
        .canonicalize()
        .map_err(ForgeError::Io)?;

    eprintln!("[DEBUG] corpus_path = {}", corpus_path.display());
    eprintln!("[DEBUG] corpus_path exists = {}", corpus_path.exists());

    let registry = ingest_corpus(&corpus_path)?;
    eprintln!("[DEBUG] {} fêtes chargées", registry.len());

    let feast_ids = {
        let _guard = LOCK_MUTEX.lock().unwrap();
        let lock_path = std::env::temp_dir().join("liturgical_forge_test.lock");
        let mut lock = crate::lock::FeastRegistryLock::load(&lock_path)?;
        allocate_feast_ids(&registry, &mut lock, &lock_path)?
    };

    // Pass 1a : résolution de toutes les années
    let mut all_resolved: Vec<(
        resolution::ResolvedCalendar,
        canonicalization::SeasonBoundaries,
    )> = Vec::with_capacity(431);

    for year in 1969u16..=2399 {
        let canon = canonicalize_year(year, &registry)?;
        let sb = canon.season_boundaries.clone();
        let resolved = resolve_year(canon, &registry, &feast_ids)?;
        all_resolved.push((resolved, sb));
    }

    // Pass 1b : Feast Registry
    let refs: Vec<(
        &resolution::ResolvedCalendar,
        &canonicalization::SeasonBoundaries,
    )> = all_resolved.iter().map(|(r, s)| (r, s)).collect();
    let feast_registry = build_feast_registry(&refs)?;

    // Pass 2 : Timeline + Pool
    let mut pool = PoolBuilder::new();
    let mut all_entries: Vec<[TimelineEntry; 366]> = Vec::with_capacity(431);

    for (resolved, sb) in &all_resolved {
        let entries = generate_year(resolved, &mut pool, sb, &feast_registry)?;
        all_entries.push(entries);
    }

    // Vespers lookahead
    for i in 0..all_entries.len() {
        let (left, right) = all_entries.split_at_mut(i + 1);
        let next_jan1 = right.first().map(|e| &e[0]);
        vespers_lookahead_pass(&mut left[i], feast_registry.as_slice(), next_jan1);
    }

    let (_checksum, bytes) = build_kald(all_entries, &feast_registry, pool, 0)?;
    Ok(bytes)
}
