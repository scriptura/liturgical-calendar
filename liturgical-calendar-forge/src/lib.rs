#![allow(missing_docs)] // activé en Jalon 3

pub mod error;
pub mod registry;
pub mod parsing;
pub mod canonicalization;
pub mod resolution;
pub mod materialization;
pub mod packing;

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

// ── Orchestre Session B ───────────────────────────────────────────────────────
use std::path::Path;
use materialization::{generate_year, vespers_lookahead_pass, PoolBuilder};
use resolution::resolve_year;
use packing::write_kald;

/// Compile un corpus YAML en fichier `.kald` pour une plage 1969–2399.
///
/// Étapes exécutées : Canonicalization (Étape 3) → Conflict Resolution (Étape 4)
/// → Day Materialization + Vespers Lookahead (Étape 5) → Binary Packing (Étape 6).
///
/// Retourne le SHA-256 `[u8; 32]` du fichier produit (`checksum[..8]` = Build ID).
pub fn compile(
    registry:   FeastRegistry,
    output:     &Path,
    variant_id: u16,
) -> Result<[u8; 32], ForgeError> {
    let mut pool        = PoolBuilder::new();
    let mut all_entries = Vec::with_capacity(431);

    for year in 1969u16..=2399 {
        // `canonicalize_year` consomme par valeur — INV-FORGE-MOVE.
        let canon    = canonicalize_year(year, &registry)?;
        // Cloner SeasonBoundaries avant le move de `canon` dans resolve_year.
        let sb       = canon.season_boundaries.clone();
        let resolved = resolve_year(canon, &registry)?;
        let entries  = generate_year(resolved, &mut pool, &sb)?;
        all_entries.push(entries);
    }

    // vespers_lookahead_pass — accès simultané à l'entrée i et i+1.
    // split_at_mut évite l'emprunt simultané de deux éléments d'un Vec.
    for i in 0..all_entries.len() {
        let (left, right) = all_entries.split_at_mut(i + 1);
        let next_jan1     = right.first().map(|e| &e[0]);
        vespers_lookahead_pass(&mut left[i], next_jan1);
    }

    write_kald(output, all_entries, pool, variant_id)
}
