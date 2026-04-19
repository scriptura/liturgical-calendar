#![allow(missing_docs)] // activé en Jalon 3

pub mod error;
pub mod registry;
pub mod parsing;
pub mod canonicalization;
pub mod resolution;
pub mod materialization;
pub mod packing;
pub mod i18n;
pub(crate) mod lits_writer;

// Re-exports publics
pub use error::ForgeError;
pub use registry::FeastRegistry;
pub use parsing::{ingest_corpus, parse_feast_from_yaml};
pub use canonicalization::{
    CanonicalizedYear, PreResolvedTransfers, AnchorTable,
    canonicalize_year, compute_easter, build_anchor_table,
    resolve_adventus, resolve_nativitas, resolve_epiphania,
    resolve_tempus_ordinarium, MONTH_STARTS, is_leap_year,
};

// ── Orchestre Session B + C ───────────────────────────────────────────────────
use std::path::Path;
use materialization::{generate_year, vespers_lookahead_pass, PoolBuilder};
use packing::write_kald;
use resolution::{assign_feast_ids, resolve_year};

/// Paramètres i18n pour la production des fichiers `.lits` compagnons.
///
/// Si `None` est passé à `compile`, aucun `.lits` n'est produit (comportement
/// Session B inchangé — `.kald` seul).
pub struct I18nConfig<'a> {
    /// Chemin vers `corpus/{rite}/i18n/` — racine de l'arborescence des dictionnaires.
    pub i18n_root: &'a Path,
    /// Répertoire de sortie pour les fichiers `.lits` produits.
    /// Un fichier `{lang}.lits` y est écrit par langue découverte.
    pub lits_dir: &'a Path,
}

/// Compile un corpus YAML en fichier `.kald` pour la plage 1969–2399.
/// Si `i18n` est fourni, produit également un `.lits` par langue compilée.
///
/// # Pipeline
///
/// - Étape 1bis — i18n Resolution (si `i18n` fourni)
/// - Étapes 3–5 — Canonicalization → Conflict Resolution → Materialization
/// - Étape 6    — Binary Packing `.kald` puis `.lits` (si `i18n` fourni)
///
/// # Retour
///
/// SHA-256 `[u8; 32]` du `.kald` produit (`checksum[..8]` = Build ID).
/// Le même Build ID est inscrit dans le header de chaque `.lits`.
pub fn compile(
    registry:   FeastRegistry,
    output:     &Path,
    variant_id: u16,
    i18n:       Option<I18nConfig<'_>>,
) -> Result<[u8; 32], ForgeError> {
    // ── Étape 1bis — i18n Resolution (AOT, avant le pipeline de résolution) ──
    // Le DictStore et la LabelTable sont construits ici mais n'influencent pas
    // le `.kald` — séparation topologie / labels (§9.1 spec).
    let i18n_artifacts = match &i18n {
        Some(cfg) => {
            let mut store = i18n::DictStore::new();
            let langs     = i18n::discover_and_load_i18n(cfg.i18n_root, &mut store)?;
            i18n::validate_i18n(&registry, &store)?;
            Some((store, langs))
        }
        None => None,
    };

    // ── FeastIDs — alloués une fois, stables sur toute la plage ──────────────
    let feast_ids = assign_feast_ids(&registry);

    // ── Étapes 3–5 ── Canonicalization → Resolution → Materialization ─────────
    let mut pool = PoolBuilder::new();
    let mut all_entries: Vec<[liturgical_calendar_core::CalendarEntry; 366]> =
        Vec::with_capacity(431);

    for year in 1969u16..=2399 {
        let canon    = canonicalize_year(year, &registry)?;
        let sb       = canon.season_boundaries.clone();
        let resolved = resolve_year(canon, &registry, &feast_ids)?;
        let entries  = generate_year(resolved, &mut pool, &sb)?;
        all_entries.push(entries);
    }

    // Vespers lookahead — accès simultané i et i+1 via split_at_mut.
    for i in 0..all_entries.len() {
        let (left, right) = all_entries.split_at_mut(i + 1);
        let next_jan1     = right.first().map(|e| &e[0]);
        vespers_lookahead_pass(&mut left[i], next_jan1);
    }

    // ── Étape 6 — Binary Packing `.kald` ─────────────────────────────────────
    let kald_checksum = write_kald(output, all_entries, pool, variant_id)?;

    // ── Étape 6 — Binary Packing `.lits` (une par langue) ────────────────────
    // Produit après le `.kald` : FeastIDs définitivement alloués, checksum connu.
    if let (Some(cfg), Some((store, langs))) = (&i18n, i18n_artifacts) {
        let lang_refs: Vec<&str> = langs.iter().map(String::as_str).collect();
        let table = i18n::build_label_table(&registry, &store, &feast_ids, &lang_refs);

        for lang in &lang_refs {
            let lits_path = cfg.lits_dir.join(format!("{}.lits", lang));
            lits_writer::write_lits(&lits_path, &table, lang, &kald_checksum)?;
        }
    }

    Ok(kald_checksum)
}

/// Helper pour les tests : compile une plage d'années en un buffer binaire unique.
/// Ce pipeline suit strictement le layout AOT défini pour la production.
pub fn forge_full_range(_range: std::ops::RangeInclusive<u16>) -> Result<Vec<u8>, ForgeError> {
    let registry = ingest_corpus(Path::new("corpus/roman"))?;
    let feast_ids = assign_feast_ids(&registry);
    
    let mut pool = PoolBuilder::new();
    // Invariant structurel : on doit TOUJOURS produire 431 années (1969-2399)
    // même si le test ne demande qu'une sous-plage.
    let mut all_entries = Vec::with_capacity(431);

    for year in 1969u16..=2399 {
        // On génère la donnée réelle si l'année est dans la plage demandée, 
        // sinon on génère une année vide/par défaut pour maintenir le layout.
        let canon    = canonicalize_year(year, &registry)?;
        let sb       = canon.season_boundaries.clone();
        let resolved = resolve_year(canon, &registry, &feast_ids)?;
        let entries  = generate_year(resolved, &mut pool, &sb)?;
        all_entries.push(entries);
    }

    for i in 0..all_entries.len() {
        let (left, right) = all_entries.split_at_mut(i + 1);
        let next_jan1     = right.first().map(|e| &e[0]);
        vespers_lookahead_pass(&mut left[i], next_jan1);
    }

    // Utilisation d'un nom unique par thread de test pour éviter les collisions
    let thread_id = std::thread::current().id();
    let temp_filename = format!("temp_test_{:?}.kald", thread_id);
    let temp_path = Path::new(&temp_filename);

    write_kald(temp_path, all_entries, pool, 0)?;
    
    let bytes = std::fs::read(temp_path).map_err(ForgeError::Io)?;
    let _ = std::fs::remove_file(temp_path); // Nettoyage
    
    Ok(bytes)
}
