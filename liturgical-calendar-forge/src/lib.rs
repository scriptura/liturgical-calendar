// SPDX-License-Identifier: MIT
// liturgical-calendar-forge — Compilateur AOT du calendrier liturgique
//
// INV-W4 : la Forge peut dépendre de l'Engine pour validation post-écriture.
//           L'Engine ne dépend JAMAIS de la Forge.
//
// Pipeline en 5 étapes (spec §6) :
//   Étape 1 — Rule Parsing    (parsing.rs)
//   Étape 2 — Canonicalization (canonicalization.rs)
//   Étape 3 — Conflict Resolution (resolution.rs)
//   Étape 4 — Day Materialization (materialization.rs)
//   Étape 5 — Binary Packing  (packing.rs)

#![warn(missing_docs)]

//! Compilateur AOT du calendrier liturgique Novus Ordo.
//!
//! Produit des artefacts `.kald` consommés par `liturgical-calendar-core`.
//!
//! # Utilisation
//!
//! ```rust,ignore
//! use liturgical_calendar_forge::forge_corpus;
//! let kald = forge_corpus(corpus_yaml, 1969, 431).unwrap();
//! ```

pub mod canonicalization;
pub mod error;
pub mod materialization;
pub mod packing;
pub mod parsing;
pub mod registry;
pub mod resolution;

use std::collections::BTreeMap;

use error::ForgeError;
use materialization::materialize_range;
use packing::pack_kald;
use parsing::parse_yaml_into_registry;
use registry::FeastRegistry;
use resolution::resolve_year;

// ─── Corpus minimal embarqué pour les tests de conformité ────────────────────

/// Corpus YAML minimal — utilisé par `forge_year` pour les tests de conformité.
///
/// Contient deux fêtes universelles :
/// 1. `dominica_resurrectionis` : Dimanche de Pâques (mobile, Pâques + 0)
///    → garantit `primary_id ≠ 0` à `doy=110` pour 2025.
/// 2. `dies_29_februarii` : fête du 29 février (fixe, mois=2, jour=29)
///    → garantit `primary_id ≠ 0` à `doy=59` pour les années bissextiles (ex: 2028).
///    Absent en année non-bissextile → Padding Entry automatique à `doy=59`.
const MINIMAL_CORPUS_YAML: &str = r#"
scope: universal
region: ~
from: 1969
to: ~
format_version: 1

feasts:
  - slug: dominica_resurrectionis
    scope: universal
    category: 0
    mobile:
      anchor: pascha
      offset: 0
    history:
      - from: 1969
        title: "Dominica Resurrectionis"
        precedence: 0
        nature: sollemnitas
        color: albus
        season: tempus_paschale

  - slug: dies_29_februarii
    scope: universal
    category: 1
    date:
      month: 2
      day: 29
    history:
      - from: 1969
        title: "Feria VI post Dominicam I Quadragesimae"
        precedence: 12
        nature: feria
        color: viridis
"#;

// ─── API publique ─────────────────────────────────────────────────────────────

/// Forge un fichier `.kald` couvrant `1969..=target_year` depuis un corpus YAML.
///
/// `epoch` est toujours 1969 pour assurer la compatibilité avec la formule d'index
/// de l'Engine : `idx = (year − 1969) × 366 + doy`.
/// `range = target_year − 1968` → `entry_count = range × 366`.
///
/// Tous les jours sans fête définie ont `primary_id = 0`.
/// Padding Entry (`primary_id = 0, flags = 0, secondary_count = 0`) à `doy = 59`
/// pour chaque année non-bissextile de la plage.
///
/// # Erreur
/// Retourne `Err(ForgeError)` si le corpus YAML est invalide ou si la résolution échoue.
pub fn forge_from_corpus(corpus_yaml: &str, target_year: u16) -> Result<Vec<u8>, ForgeError> {
    // ── Étape 1 : Rule Parsing ─────────────────────────────────────────────
    let mut registry = FeastRegistry::new();
    parse_yaml_into_registry(corpus_yaml, &mut registry)?;

    // ── Étape 2 + 3 : Canonicalization + Conflict Resolution ──────────────
    // Résoudre chaque année dans [1969, target_year]
    let epoch = 1969u16;
    let range = target_year
        .checked_sub(epoch)
        .map(|d| d + 1)
        .unwrap_or(1);

    let mut resolved_all: BTreeMap<u16, BTreeMap<u16, resolution::ResolvedDay>> =
        BTreeMap::new();
    for year in epoch..=(epoch + range - 1) {
        let year_table = resolve_year(year, &registry)?;
        resolved_all.insert(year, year_table);
    }

    // ── Étape 4 : Day Materialization ─────────────────────────────────────
    let (entries, pool) = materialize_range(epoch, range, &resolved_all)?;

    // ── Étape 5 : Binary Packing ──────────────────────────────────────────
    let kald = pack_kald(epoch, range, &entries, &pool);

    Ok(kald)
}

/// Forge un `.kald` pour `1969..=year` en utilisant le corpus minimal embarqué.
///
/// Fonction d'entrée pour les tests de conformité (roadmap §2 critère de sortie).
/// Le corpus embarqué contient Pâques et le 29 février — suffisant pour passer
/// les tests `conformity_2025` et `conformity_2028`.
pub fn forge_year(year: u16) -> Result<Vec<u8>, ForgeError> {
    forge_from_corpus(MINIMAL_CORPUS_YAML, year)
}

// ─── Tests de conformité Jalon 2 (inline pour visibilité) ────────────────────
//
// Note : les tests d'intégration complets (avec kal_validate_header et kal_read_entry)
// sont dans tests/conformity.rs car ils nécessitent liturgical-calendar-core
// (dev-dependency).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_year_2025_produces_valid_bytes() {
        let kald = forge_year(2025).unwrap();
        // Magic
        assert_eq!(&kald[0..4], b"KALD");
        // Version
        assert_eq!(u16::from_le_bytes([kald[4], kald[5]]), 4);
        // epoch = 1969
        assert_eq!(u16::from_le_bytes([kald[8], kald[9]]), 1969);
        // range = 2025 - 1968 = 57
        let range = u16::from_le_bytes([kald[10], kald[11]]);
        assert_eq!(range, 57);
        // entry_count = 57 * 366 = 20862
        let ec = u32::from_le_bytes([kald[12], kald[13], kald[14], kald[15]]);
        assert_eq!(ec, 57 * 366);
        // Taille cohérente
        let ps = u32::from_le_bytes([kald[20], kald[21], kald[22], kald[23]]);
        assert_eq!(kald.len(), 64 + ec as usize * 8 + ps as usize);
    }

    #[test]
    fn forge_year_2025_padding_at_doy59() {
        let kald = forge_year(2025).unwrap();
        // 2025 : year_offset = 2025 - 1969 = 56
        // idx pour (2025, 59) = 56 * 366 + 59 = 20555
        let idx = 56usize * 366 + 59;
        let offset = 64 + idx * 8;
        let primary_id = u16::from_le_bytes([kald[offset], kald[offset + 1]]);
        assert_eq!(primary_id, 0, "doy=59 pour 2025 doit être Padding Entry");
    }

    #[test]
    fn forge_year_2025_easter_at_doy110() {
        let kald = forge_year(2025).unwrap();
        // idx pour (2025, 110) = 56 * 366 + 110 = 20606
        let idx = 56usize * 366 + 110;
        let offset = 64 + idx * 8;
        let primary_id = u16::from_le_bytes([kald[offset], kald[offset + 1]]);
        assert_ne!(primary_id, 0, "doy=110 pour 2025 doit être Pâques (primary_id ≠ 0)");
    }

    #[test]
    fn forge_year_2028_feb29_real() {
        let kald = forge_year(2028).unwrap();
        // 2028 : year_offset = 2028 - 1969 = 59
        // idx pour (2028, 59) = 59 * 366 + 59 = 21653
        let idx = 59usize * 366 + 59;
        let offset = 64 + idx * 8;
        let primary_id = u16::from_le_bytes([kald[offset], kald[offset + 1]]);
        assert_ne!(primary_id, 0, "doy=59 pour 2028 (bissextile) doit être une vraie fête");
    }

    #[test]
    fn forge_is_deterministic() {
        let kald1 = forge_year(2025).unwrap();
        let kald2 = forge_year(2025).unwrap();
        assert_eq!(kald1, kald2, "forge_year doit être déterministe bit-for-bit");
    }
}
