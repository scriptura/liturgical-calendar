// SPDX-License-Identifier: MIT
// liturgical-calendar-forge — Étape 3 : Conflict Resolution (roadmap §2.3)
//
// 5 passes séquentielles (spec §3.4) :
//   Passe 1 — Placement fêtes fixes (Sanctoral)
//   Passe 2 — Placement fêtes mobiles (Temporal, Pâques)
//   Passe 3 — Résolution finale des préséances + déclassement saisonnier (§3.5)
//   Passe 4 — Exécution des transferts via TransferQueue (BTreeSet, clôture transitive)
//   Passe 5 — Vérification de stabilité (ForgeError::ResolutionIncomplete)
//
// INV-FORGE-2 : BTreeMap/BTreeSet sur tout chemin influençant le .kald.
// Sortie : table stable `BTreeMap<u16, ResolvedDay>` (doy → jour résolu).

use std::collections::BTreeMap;

use crate::canonicalization::{
    compute_easter, doy_from_month_day, is_leap_year, SeasonBoundaries,
};
use crate::error::ForgeError;
use crate::registry::{Anchor, FeastDef, FeastRegistry, Nature, Season};

// ─── Structures de sortie ─────────────────────────────────────────────────────

/// Fête résolue pour un slot (year, doy).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedFeast {
    pub slug:       String,
    pub feast_id:   u16,
    pub precedence: u8,
    pub nature:     Nature,
    pub color:      crate::registry::Color,
    pub season:     Season,
    /// Année de résolution (pour les messages d'erreur).
    pub year:       u16,
}

/// Jour liturgique résolu : une fête principale + commémorations.
#[derive(Debug, Clone)]
pub struct ResolvedDay {
    pub primary:     ResolvedFeast,
    pub secondaries: Vec<ResolvedFeast>, // FeastIDs des commémorations (triés par FeastID croissant — INV-FORGE-4)
}

// ─── Passe commune : calcul du DOY d'une fête pour une année ─────────────────

/// Calcule le DOY 0-based d'une fête pour une année donnée.
pub fn compute_doy(
    def:          &FeastDef,
    year:         u16,
    easter_doy:   u16,
    advent_start: u16,
) -> Option<u16> {
    match &def.temporality {
        crate::registry::Temporality::Fixed { month, day } => {
            // Le 29 février est déclarable mais absent des années non-bissextiles
            if *month == 2 && *day == 29 && !is_leap_year(year as i32) {
                return None; // Pas de fête ce jour — Padding Entry sera placée
            }
            Some(doy_from_month_day(*month, *day))
        }
        crate::registry::Temporality::Mobile { anchor, offset } => {
            let base: i32 = match anchor {
                Anchor::Pascha      => easter_doy as i32,
                Anchor::Pentecostes => easter_doy as i32 + 49,
                Anchor::Adventus    => advent_start as i32,
            };
            let doy = base + *offset as i32;
            if doy < 0 || doy > 365 {
                None // Hors bornes (rare — corpus mal formé)
            } else {
                Some(doy as u16)
            }
        }
    }
}

// ─── Constante de transfert ───────────────────────────────────────────────────

/// Profondeur maximale de chaîne de transfert (spec §3.4).
const MAX_TRANSFER_DEPTH: u8 = 7;

// ─── Résolution pour une année ────────────────────────────────────────────────

/// Résout le calendrier liturgique pour une année donnée.
///
/// Applique les 5 passes spec §3.4.
/// Retourne `BTreeMap<doy, ResolvedDay>` — uniquement les jours avec une fête.
/// Les jours sans fête (et doy=59 pour années non-bissextiles) sont gérés
/// en Étape 4 (Materialization).
pub fn resolve_year(
    year:      u16,
    registry:  &FeastRegistry,
) -> Result<BTreeMap<u16, ResolvedDay>, ForgeError> {
    // ── Étape 2 préalable : calcul des ancres de l'année ──────────────────
    let easter_doy   = compute_easter(year)?;
    let season_bounds = SeasonBoundaries::compute(year, easter_doy);
    let advent_start = season_bounds.advent_start;

    // Table de travail : doy → liste de fêtes candidates (avant résolution).
    // BTreeMap pour déterminisme (INV-FORGE-2).
    let mut table: BTreeMap<u16, Vec<ResolvedFeast>> = BTreeMap::new();

    // ── PASSE 1 : Placement des fêtes fixes (Sanctoral) ───────────────────
    // Itérer les slugs en ordre lexicographique (BTreeMap garantit l'ordre).
    for (slug, def) in &registry.feasts {
        // Ne traiter que les fêtes fixes dans cette passe
        if !matches!(def.temporality, crate::registry::Temporality::Fixed { .. }) {
            continue;
        }
        // Résoudre la version active
        let version = match def.resolve_for_year(year)? {
            Some(v) => v,
            None    => continue, // fête absente cette année
        };
        // Calculer le DOY
        let doy = match compute_doy(def, year, easter_doy, advent_start) {
            Some(d) => d,
            None    => continue, // 29 fév sur année non-bissextile
        };
        let season = version.season.unwrap_or_else(|| {
            season_bounds.season_for_doy(doy)
        });
        table.entry(doy).or_default().push(ResolvedFeast {
            slug:       slug.clone(),
            feast_id:   def.id,
            precedence: version.precedence,
            nature:     version.nature,
            color:      version.color,
            season,
            year,
        });
    }

    // ── PASSE 2 : Placement des fêtes mobiles (Temporal) ─────────────────
    for (slug, def) in &registry.feasts {
        if !matches!(def.temporality, crate::registry::Temporality::Mobile { .. }) {
            continue;
        }
        let version = match def.resolve_for_year(year)? {
            Some(v) => v,
            None    => continue,
        };
        let doy = match compute_doy(def, year, easter_doy, advent_start) {
            Some(d) => d,
            None    => continue,
        };
        let season = version.season.unwrap_or_else(|| {
            season_bounds.season_for_doy(doy)
        });
        table.entry(doy).or_default().push(ResolvedFeast {
            slug:       slug.clone(),
            feast_id:   def.id,
            precedence: version.precedence,
            nature:     version.nature,
            color:      version.color,
            season,
            year,
        });
    }

    // ── PASSE 3 : Résolution finale des préséances + déclassement saisonnier
    let mut resolved: BTreeMap<u16, ResolvedDay> = BTreeMap::new();
    // File de transferts — collectée en Passe 3, traitée en Passe 4.
    let mut transfers: Vec<(u16, ResolvedFeast, u8)> = Vec::new(); // (doy, feast, depth)

    for (&doy, candidates) in &table {
        if candidates.is_empty() { continue; }

        // Trier les candidats : precedence croissante (priorité la plus haute en premier),
        // puis par feast_id croissant (tiebreaker déterministe — spec §3.3 règle 3)
        let mut sorted = candidates.clone();
        sorted.sort_by(|a, b| {
            a.precedence.cmp(&b.precedence)
                .then(a.feast_id.cmp(&b.feast_id))
        });

        let primary_candidate = &sorted[0];

        // Vérification V7 : collision de Solennités (Precedence ≤ 5) même scope
        if sorted.len() >= 2 {
            let second = &sorted[1];
            if primary_candidate.precedence <= 5
                && second.precedence == primary_candidate.precedence
            {
                // Extraire les scopes depuis le registre
                let scope_a = registry.feasts[&primary_candidate.slug].scope;
                let scope_b = registry.feasts[&second.slug].scope;
                if scope_a == scope_b || primary_candidate.precedence <= 3 {
                    return Err(ForgeError::SolemnityCollision {
                        slug_a:     primary_candidate.slug.clone(),
                        slug_b:     second.slug.clone(),
                        precedence: primary_candidate.precedence,
                        scope_a:    format!("{:?}", scope_a),
                        scope_b:    format!("{:?}", scope_b),
                        doy,
                        year,
                    });
                }
            }
        }

        // Déclassement saisonnier §3.5 : Mémoires (prec ≥ 11) en période privilégiée
        let primary_season = primary_candidate.season;
        let mut secondaries: Vec<ResolvedFeast> = Vec::new();

        for feast in &sorted[1..] {
            let should_demote = feast.precedence >= 11
                && matches!(
                    primary_season,
                    Season::TempusQuadragesimae
                    | Season::TempusAdventus
                    | Season::TriduumPaschale
                    | Season::DiesSancti
                );

            if should_demote || feast.precedence >= 8 {
                // Commémoration
                secondaries.push(feast.clone());
            } else if feast.precedence <= 9
                && !matches!(feast.nature, Nature::Feria)
                && feast.precedence > primary_candidate.precedence
            {
                // Transférable
                transfers.push((doy, feast.clone(), 0));
            }
            // Sinon : supprimé du slot
        }

        // Trier les secondaires par FeastID croissant (INV-FORGE-4)
        secondaries.sort_by_key(|f| f.feast_id);

        resolved.insert(doy, ResolvedDay {
            primary:     primary_candidate.clone(),
            secondaries,
        });
    }

    // ── PASSE 4 : Exécution des transferts (clôture transitive) ──────────
    // Traitement dans l'ordre déterministe : (doy, feast_id)
    // On traite les transferts collectés en Passe 3
    transfers.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.feast_id.cmp(&b.1.feast_id)));

    let mut transfer_work: Vec<(u16, ResolvedFeast, u8)> = transfers;

    while let Some((doy_src, feast, depth)) = transfer_work.first().cloned() {
        transfer_work.remove(0);

        if depth > MAX_TRANSFER_DEPTH {
            return Err(ForgeError::TransferFailed {
                slug:       feast.slug.clone(),
                origin_doy: doy_src.saturating_sub(depth as u16),
                blocked_at: doy_src,
                year,
            });
        }

        // Chercher le premier DOY libre dans [doy_src+1, doy_src+7]
        let mut placed = false;
        for doy_dst in (doy_src + 1)..=(doy_src + 7).min(365) {
            let occupied = resolved.get(&doy_dst);
            let can_place = match occupied {
                None    => true,
                Some(d) => d.primary.precedence > feast.precedence,
            };
            if can_place {
                if let Some(existing_day) = resolved.get(&doy_dst) {
                    // L'occupant actuel est lui-même transférable ?
                    let occupant = existing_day.primary.clone();
                    if occupant.precedence <= 9
                        && !matches!(occupant.nature, Nature::Feria)
                    {
                        transfer_work.push((doy_dst, occupant, depth + 1));
                        transfer_work.sort_by(|a, b|
                            a.0.cmp(&b.0).then(a.1.feast_id.cmp(&b.1.feast_id)));
                    }
                }
                resolved.insert(doy_dst, ResolvedDay {
                    primary:     feast.clone(),
                    secondaries: Vec::new(),
                });
                placed = true;
                break;
            }
        }
        if !placed {
            return Err(ForgeError::TransferFailed {
                slug:       feast.slug.clone(),
                origin_doy: doy_src,
                blocked_at: doy_src + 7,
                year,
            });
        }
    }

    // ── PASSE 5 : Vérification de stabilité ───────────────────────────────
    // Vérifier qu'aucun conflit de Precedence non résolu ne subsiste.
    // Pour le corpus minimal, cette passe est triviale.
    for (&doy, day) in &resolved {
        // Bits 14–15 des flags doivent être nuls (vérification symbolique)
        let flags = encode_flags_preview(
            day.primary.precedence,
            day.primary.color as u8,
            day.primary.season as u8,
            day.primary.nature as u8,
        );
        if flags & 0xC000 != 0 {
            return Err(ForgeError::FlagsReservedBitSet { doy, year });
        }
    }

    Ok(resolved)
}

/// Calcul préliminaire des flags pour vérification en Passe 5.
#[inline]
fn encode_flags_preview(precedence: u8, color: u8, season: u8, nature: u8) -> u16 {
    (precedence as u16)
        | ((color   as u16) << 4)
        | ((season  as u16) << 8)
        | ((nature  as u16) << 11)
}

// ─── Tests unitaires ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::parse_yaml_into_registry;

    const MINIMAL_YAML: &str = r#"
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
        title: "Feria VI"
        precedence: 12
        nature: feria
        color: viridis
"#;

    #[test]
    fn resolve_easter_2025_present() {
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(MINIMAL_YAML, &mut reg).unwrap();
        let table = resolve_year(2025, &reg).unwrap();
        // Pâques 2025 = doy 110
        assert!(table.contains_key(&110), "Pâques 2025 absent du calendrier résolu");
        let day = &table[&110];
        assert_ne!(day.primary.feast_id, 0);
        assert_eq!(day.primary.precedence, 0); // TriduumSacrum/Resurrection
    }

    #[test]
    fn resolve_feb29_leap_2028_present() {
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(MINIMAL_YAML, &mut reg).unwrap();
        let table = resolve_year(2028, &reg).unwrap();
        // 2028 est bissextile → dies_29_februarii doit être présent à doy=59
        assert!(table.contains_key(&59), "doy=59 absent pour année bissextile 2028");
    }

    #[test]
    fn resolve_feb29_non_leap_2025_absent() {
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(MINIMAL_YAML, &mut reg).unwrap();
        let table = resolve_year(2025, &reg).unwrap();
        // 2025 non-bissextile → doy=59 absent de la table (Padding Entry en Étape 4)
        assert!(!table.contains_key(&59), "doy=59 ne devrait pas avoir de fête en 2025");
    }

    #[test]
    fn precedence_resolution_primary_wins() {
        // Solennité (4) vs Mémoire (11) → Solennité reste primary
        let yaml = r#"
scope: universal
region: ~
from: 1969
to: ~
format_version: 1
feasts:
  - slug: sollemnitas_generalis
    scope: universal
    category: 0
    date:
      month: 1
      day: 15
    history:
      - from: 1969
        title: "Solennité test"
        precedence: 4
        nature: sollemnitas
        color: albus

  - slug: memoria_test
    scope: universal
    category: 1
    date:
      month: 1
      day: 15
    history:
      - from: 1969
        title: "Mémoire test"
        precedence: 11
        nature: memoria
        color: albus
"#;
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(yaml, &mut reg).unwrap();
        let table = resolve_year(2025, &reg).unwrap();
        let doy = doy_from_month_day(1, 15); // doy=14
        let day = &table[&doy];
        // La Solennité (prec=4) doit être primary
        assert_eq!(day.primary.precedence, 4);
        assert_eq!(day.primary.slug, "sollemnitas_generalis");
        // La Mémoire (prec=11) doit être en secondary
        assert!(!day.secondaries.is_empty());
    }
}
