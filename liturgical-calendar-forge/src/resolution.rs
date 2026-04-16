//! Étape 4 — Conflict Resolution : pipeline 5 passes.
//!
//! Hypothèses sur les types Session A (ajuster si divergence) :
//!   FeastRegistry::feasts       : BTreeMap<String, FeastDef>
//!   FeastDef::feast_id          : u16
//!   FeastDef::active_version_for(year) -> Option<&FeastVersionDef>
//!   FeastVersionDef::precedence : Precedence
//!   FeastVersionDef::nature     : Nature
//!   FeastVersionDef::color      : Color
//!   FeastVersionDef::has_vigil_mass : bool
//!   FeastVersionDef::date       : Option<(u8, u8)>   — (month, day)
//!   FeastVersionDef::mobile     : Option<MobileDef>
//!   FeastVersionDef::transfers  : Vec<TransferRule>
//!   MobileDef::anchor           : String
//!   MobileDef::offset           : i32
//!   MobileDef::ordinal          : Option<u8>         — tempus_ordinarium uniquement
//!   TransferRule::collides      : String
//!   TransferRule::target        : TransferTarget
//!   TransferTarget              : Offset(u32) | Date{m,d} | Mobile{anchor,offset}
//!   CanonicalizedYear::pre_resolved_transfers : BTreeMap<(String,String), u16>
//!   SeasonBoundaries::period_of(&self, doy: u16) -> LiturgicalPeriod

#![allow(missing_docs)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use liturgical_calendar_core::{Color, LiturgicalPeriod, Nature};

use crate::{
    canonicalization::{is_leap_year, resolve_tempus_ordinarium, CanonicalizedYear, MONTH_STARTS},
    error::ForgeError,
    registry::{FeastRegistry, TransferTarget},
};

// ─── Enums de classification ─────────────────────────────────────────────────

/// Cycle liturgique — dérivé de la présence du bloc `mobile:` ou `date:` dans le YAML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cycle {
    Temporal  = 0, // `mobile:` — Proprium de Tempore
    Sanctoral = 1, // `date:`   — Sanctoral fixe
}

/// Temporalité — fixe ou calculée depuis une ancre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Temporality {
    Fixed  = 0, // `date:`
    Mobile = 1, // `mobile:`
}

// ─── ResolutionKey ────────────────────────────────────────────────────────────

/// Clé de tri canonique. Ordre lexicographique natif via `derive(Ord)`.
/// Valeur inférieure = priorité liturgique supérieure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ResolutionKey<'a> {
    pub precedence:  u8,          // [0, 12] — inférieur = plus haute priorité
    pub cycle:       Cycle,       // Temporal(0) < Sanctoral(1)
    pub temporality: Temporality, // Fixed(0) < Mobile(1)
    pub slug:        &'a str,     // tiebreaker final — ASCII stable cross-build
}

// ─── PlacedFeast ─────────────────────────────────────────────────────────────

/// Fête positionnée sur un DOY, prête pour la résolution de conflits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlacedFeast {
    pub slug:           String,
    pub feast_id:       u16,
    pub scope:          u8,   // bits [15:14] du FeastID : 0=Universal, 1=National, 2=Diocesan
    pub precedence:     u8,   // Precedence as u8 — comparaison entière pure
    pub nature:         Nature,
    pub color:          Color,
    pub has_vigil_mass: bool,
    pub cycle:          Cycle,
    pub temporality:    Temporality,
}

impl PlacedFeast {
    #[inline]
    fn key(&self) -> ResolutionKey<'_> {
        ResolutionKey {
            precedence:  self.precedence,
            cycle:       self.cycle,
            temporality: self.temporality,
            slug:        &self.slug,
        }
    }
}

impl PartialOrd for PlacedFeast {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PlacedFeast {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

// ─── ResolvedDay / ResolvedCalendar ──────────────────────────────────────────

/// Résultat de résolution pour un slot DOY : primary élu + commémorations.
#[derive(Debug, Clone)]
pub struct ResolvedDay {
    pub primary:          PlacedFeast,
    /// Triés par feast_id croissant — INV-FORGE-4.
    pub secondary_feasts: Vec<PlacedFeast>,
}

/// Table (doy → ResolvedDay) pour une année, issue de la Passe 5.
pub struct ResolvedCalendar {
    pub year: u16,
    /// Uniquement les DOY résolus — doy=59 absent pour années non-bissextiles.
    pub days: BTreeMap<u16, ResolvedDay>,
}

// ─── TransferQueue ────────────────────────────────────────────────────────────

/// Entrée dans la TransferQueue. Ordonnée par (doy_current, feast_id) — déterministe.
#[derive(Debug, Clone, Eq, PartialEq)]
struct TransferEntry {
    doy_current: u16,
    feast_id:    u16,
    depth:       u8,
    feast:       PlacedFeast,
}

impl Ord for TransferEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // BTreeSet déduplique par Ord — (doy, feast_id) suffit.
        // Un (doy, feast_id) identique avec depth différent → dedup silencieuse (premier wins).
        (self.doy_current, self.feast_id)
            .cmp(&(other.doy_current, other.feast_id))
    }
}
impl PartialOrd for TransferEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const MAX_TRANSFER_DEPTH: u8 = 7;

struct TransferQueue {
    pending: BTreeSet<TransferEntry>,
}

impl TransferQueue {
    fn new() -> Self {
        Self { pending: BTreeSet::new() }
    }

    fn enqueue(
        &mut self,
        doy_src: u16,
        feast: PlacedFeast,
        depth: u8,
        year: u16,
    ) -> Result<(), ForgeError> {
        if depth > MAX_TRANSFER_DEPTH {
            return Err(ForgeError::TransferFailed {
                slug:       feast.slug.clone(),
                origin_doy: doy_src.saturating_sub(depth as u16),
                blocked_at: doy_src,
                year,
            });
        }
        self.pending.insert(TransferEntry {
            doy_current: doy_src,
            feast_id:    feast.feast_id,
            depth,
            feast,
        });
        Ok(())
    }

    fn pop_first(&mut self) -> Option<TransferEntry> {
        let entry = self.pending.iter().next()?.clone();
        self.pending.remove(&entry);
        Some(entry)
    }

    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

// ─── Déclassement saisonnier — §3.4 ─────────────────────────────────────────

/// `true` si la fête doit être forcée en commémoration quelle que soit sa Precedence.
/// Appliqué uniquement quand le primary du slot est une fête temporelle (§3.4).
pub(crate) fn should_demote_to_commemoratio(
    feast:  &PlacedFeast,
    period: LiturgicalPeriod,
) -> bool {
    feast.precedence >= 11
        && matches!(
            period,
            LiturgicalPeriod::TempusQuadragesimae
                | LiturgicalPeriod::TempusAdventus
                | LiturgicalPeriod::TriduumPaschale
                | LiturgicalPeriod::DiesSancti
        )
}

// ─── Helper : DOY d'une fête pour une année donnée ───────────────────────────

/// Résout le DOY d'une fête depuis sa version active.
/// Retourne `None` si le slot est absorbé (tempus_ordinarium hors plage active).
fn feast_doy(
    version: &crate::registry::FeastVersionDef,
    anchors: &BTreeMap<String, u16>,
) -> Option<u16> {
    if let Some((month, day)) = version.date {
        // Fête fixe — MONTH_STARTS est une constante de compilation.
        return Some(MONTH_STARTS[month as usize - 1] + day as u16 - 1);
    }

    let mobile = version.mobile.as_ref()?;

    if mobile.anchor == "tempus_ordinarium" {
        let ordinal = mobile.ordinal?;
        let adventus = *anchors.get("adventus")?;
        // resolve_tempus_ordinarium : O(1), défini dans canonicalization.rs Session A.
        return Some(resolve_tempus_ordinarium(adventus, ordinal));
    }

    // Ancre primitive (pascha, adventus, nativitas, epiphania, pentecostes désucré).
    let anchor_doy = *anchors.get(&mobile.anchor)?;
    let doy = anchor_doy as i32 + mobile.offset;
    // Offset hors [0, 365] → slot invalide (rare, protection défensive).
    if doy < 0 || doy > 365 {
        return None;
    }
    Some(doy as u16)
}

// ─── Élection canonique d'un slot ────────────────────────────────────────────

/// Trie les candidats, élit le primary, partitionne en (secondary_feasts, to_transfer).
///
/// `temporal_primary` : le primary élu est-il de cycle Temporal ?
/// Conditionne l'application du déclassement saisonnier §3.4.
fn elect(
    mut candidates:     Vec<PlacedFeast>,
    period:             LiturgicalPeriod,
) -> (PlacedFeast, Vec<PlacedFeast>, Vec<PlacedFeast>) {
    // TRI — ordre lexicographique sur ResolutionKey.
    candidates.sort_unstable_by(|a, b| a.key().cmp(&b.key()));

    let primary = candidates.remove(0);
    let temporal_primary = primary.cycle == Cycle::Temporal;

    let mut secondary_feasts: Vec<PlacedFeast> = Vec::new();
    let mut to_transfer:      Vec<PlacedFeast> = Vec::new();

    // DÉCLASSEMENT + PARTITION — passage unique sur les candidats restants.
    for feast in candidates {
        if temporal_primary && should_demote_to_commemoratio(&feast, period) {
            // Déclassement saisonnier §3.4 — Nature ≠ Feria dans le pool.
            secondary_feasts.push(feast);
        } else if feast.precedence >= 8 {
            // Précédence ∈ [8, 12] — commémorable, versé dans le Secondary Pool.
            secondary_feasts.push(feast);
        } else if feast.precedence <= 9 && feast.nature != Nature::Feria {
            // Précédence ∈ [1, 7], non-Ferie — transfert obligatoire.
            to_transfer.push(feast);
        }
        // else : Precedence ∈ [10, 12] non déclassé + Feria → supprimé silencieusement.
    }

    // INV-FORGE-4 : secondary_feasts triés par feast_id croissant.
    secondary_feasts.sort_unstable_by_key(|f| f.feast_id);

    (primary, secondary_feasts, to_transfer)
}

// ─── Pipeline principal — resolve_year ───────────────────────────────────────

/// Résout une année calendaire complète — pipeline 5 passes.
///
/// Consomme `canonicalized` par move (INV-FORGE-MOVE).
/// `registry` est partagé entre toutes les années — passé par référence.
pub fn resolve_year(
    canonicalized: CanonicalizedYear,
    registry:      &FeastRegistry,
) -> Result<ResolvedCalendar, ForgeError> {
    let year    = canonicalized.year;
    let is_leap = is_leap_year(year);

    // ── PASSE 1 — Collecte et placement ──────────────────────────────────────
    // Résultat : BTreeMap<doy, Vec<PlacedFeast>> — liste non triée.
    // Aucun conflit résolu dans cette passe.

    let mut slots: BTreeMap<u16, Vec<PlacedFeast>> = BTreeMap::new();

    for (slug, feast_def) in registry.feasts.iter() {
        let version = match feast_def.active_version_for(year) {
            Some(v) => v,
            None    => continue,
        };

        let doy = match feast_doy(version, &canonicalized.anchors) {
            Some(d) => d,
            None    => continue, // Slot absorbé — Ok(None) normal.
        };

        // doy=59 absent en année non-bissextile (le 29 fév n'existe pas).
        if !is_leap && doy == 59 {
            continue;
        }

        let scope = (feast_def.feast_id >> 14) as u8; // bits [15:14]

        let (cycle, temporality) = if version.mobile.is_some() {
            (Cycle::Temporal, Temporality::Mobile)
        } else {
            (Cycle::Sanctoral, Temporality::Fixed)
        };

        slots.entry(doy).or_default().push(PlacedFeast {
            slug:           slug.clone(),
            feast_id:       feast_def.feast_id,
            scope,
            precedence:     version.precedence as u8,
            nature:         version.nature,
            color:          version.color,
            has_vigil_mass: version.has_vigil_mass,
            cycle,
            temporality,
        });
    }

    // ── PASSE 2 — Garde V7 + résolution de scope ─────────────────────────────
    // Opère in-place sur `slots`. Arrêt fatal sur collision irréconciliable.

    for (&doy, candidates) in slots.iter_mut() {
        // V7a : Precedence ∈ [0, 3] — deux fêtes de ce rang = corpus incohérent.
        let very_high: Vec<_> = candidates.iter()
            .filter(|f| f.precedence <= 3)
            .collect();
        if very_high.len() >= 2 {
            return Err(ForgeError::SolemnityCollision {
                slug_a:     very_high[0].slug.clone(),
                slug_b:     very_high[1].slug.clone(),
                precedence: very_high[0].precedence,
                doy,
                year,
            });
        }

        // V7b : Precedence ∈ [4, 5], même scope — irréconciliable mécaniquement.
        {
            let solemnities: Vec<_> = candidates.iter()
                .filter(|f| f.precedence >= 4 && f.precedence <= 5)
                .collect();
            for i in 0..solemnities.len() {
                for j in (i + 1)..solemnities.len() {
                    if solemnities[i].scope == solemnities[j].scope {
                        return Err(ForgeError::SolemnityCollision {
                            slug_a:     solemnities[i].slug.clone(),
                            slug_b:     solemnities[j].slug.clone(),
                            precedence: solemnities[i].precedence,
                            doy,
                            year,
                        });
                    }
                }
            }
        }

        // §3.1 — Hiérarchie de scope pour Solennités [4, 5] de scopes différents :
        // retirer les candidats de scope inférieur (diocesan > national > universal).
        let high_prec_count = candidates.iter().filter(|f| f.precedence <= 5).count();
        if high_prec_count >= 2 {
            let max_scope = candidates.iter()
                .filter(|f| f.precedence <= 5)
                .map(|f| f.scope)
                .max()
                .unwrap_or(0);
            candidates.retain(|f| {
                !(f.precedence <= 5 && f.scope < max_scope)
            });
        }
    }

    // ── PASSE 3 — Tri + Élection + Déclassement + Dispatch ───────────────────
    // Itération DOY 0→365, ordre croissant (INV-FORGE-2).
    // `pending_inserts` : inserts directs forward (target DOY > courant).
    // `retrograde_inserts` : inserts directs rétrogrades (target DOY ≤ courant).

    let mut resolved_days:      BTreeMap<u16, ResolvedDay>         = BTreeMap::new();
    let mut transfer_queue:     TransferQueue                       = TransferQueue::new();
    let mut pending_inserts:    BTreeMap<u16, Vec<PlacedFeast>>    = BTreeMap::new();
    let mut retrograde_inserts: Vec<(u16, PlacedFeast)>             = Vec::new();

    for doy in 0u16..=365u16 {
        // Fusionner : candidats initiaux + inserts directs en attente.
        let mut candidates: Vec<PlacedFeast> = slots.remove(&doy).unwrap_or_default();
        if let Some(fwd) = pending_inserts.remove(&doy) {
            candidates.extend(fwd);
        }

        if candidates.is_empty() {
            continue; // Slot vide — normal pour doy=59 non-bissextile et slots absorbés.
        }

        let period = canonicalized.season_boundaries.period_of(doy);
        let (primary, secondary_feasts, to_transfer) = elect(candidates, period);

        // DISPATCH TRANSFERTS — §3.3 Passe 3.
        for feast in to_transfer {
            // Chercher la règle de transfert active pour cette fête / ce primary.
            let active_rule = registry.feasts
                .get(&feast.slug)
                .and_then(|def| def.active_version_for(year))
                .and_then(|ver| {
                    ver.transfers.iter().find(|t| t.collides == primary.slug)
                });

            if let Some(rule) = active_rule {
                // 1. PreResolvedTransfers — cible mobile pré-calculée (Étape 3).
                //    Peut être rétrograde → NE PAS envoyer dans TransferQueue.
                let pre_key = (feast.slug.clone(), rule.collides.clone());
                if let Some(&doy_dst) = canonicalized.pre_resolved_transfers.get(&pre_key) {
                    if doy_dst <= doy {
                        retrograde_inserts.push((doy_dst, feast));
                    } else {
                        pending_inserts.entry(doy_dst).or_default().push(feast);
                    }
                    continue;
                }

                // 2. Règle offset ou date fixe — toujours forward.
                let doy_dst: u16 = match &rule.target {
                    TransferTarget::Offset(n) => doy + *n as u16,
                    TransferTarget::Date { m, d } => {
                        MONTH_STARTS[*m as usize - 1] + *d as u16 - 1
                    }
                    TransferTarget::Mobile { .. } => {
                        // Mobile sans PreResolved = bug Étape 3 ; fallback TransferQueue.
                        transfer_queue.enqueue(doy, feast, 0, year)?;
                        continue;
                    }
                };

                if doy_dst <= doy {
                    retrograde_inserts.push((doy_dst, feast));
                } else {
                    pending_inserts.entry(doy_dst).or_default().push(feast);
                }
            } else {
                // Pas de règle déclarative → TransferQueue générique.
                // doy_src = doy (le slot de conflit) — recherche dans [doy+1, doy+7].
                transfer_queue.enqueue(doy, feast, 0, year)?;
            }
        }

        resolved_days.insert(doy, ResolvedDay { primary, secondary_feasts });
    }

    // Traitement des inserts rétrogrades — "résolution à un seul niveau" §3.3.
    // Tri par doy_dst pour déterminisme (INV-FORGE-2).
    retrograde_inserts.sort_unstable_by_key(|&(d, _)| d);
    for (doy_dst, feast) in retrograde_inserts {
        let period = canonicalized.season_boundaries.period_of(doy_dst);
        if let Some(day) = resolved_days.get_mut(&doy_dst) {
            // Re-élection avec le nouvel arrivant — les `to_transfer` résultants
            // sont ignorés (un seul niveau de résolution).
            let mut all = vec![day.primary.clone(), feast];
            all.extend(day.secondary_feasts.clone());
            let (new_primary, new_secondary, _discarded) = elect(all, period);
            day.primary          = new_primary;
            day.secondary_feasts = new_secondary;
        } else {
            // Slot rétrograde vide (fête temporelle absente) → primary direct.
            resolved_days.insert(doy_dst, ResolvedDay {
                primary:          feast,
                secondary_feasts: Vec::new(),
            });
        }
    }

    // ── PASSE 4 — Exécution des transferts (clôture transitive) ──────────────
    // TransferQueue : BTreeSet, ordre (doy_src, feast_id) croissant.
    // Recherche dans [doy_src+1, doy_src+7] — strictement forward.

    while let Some(entry) = transfer_queue.pop_first() {
        let TransferEntry { doy_current, feast, depth, .. } = entry;
        let mut placed = false;

        let window_end = (doy_current + 7).min(365);
        for doy_dst in (doy_current + 1)..=window_end {
            let slot_free = match resolved_days.get(&doy_dst) {
                Some(day) => day.primary.precedence > feast.precedence,
                None      => true,
            };

            if !slot_free {
                continue;
            }

            // Insérer au slot cible avec re-tri par ResolutionKey.
            let period = canonicalized.season_boundaries.period_of(doy_dst);
            let mut all_candidates = vec![feast.clone()];
            if let Some(existing) = resolved_days.remove(&doy_dst) {
                all_candidates.push(existing.primary);
                all_candidates.extend(existing.secondary_feasts);
            }

            let (new_primary, new_secondary, displaced_transfers) =
                elect(all_candidates, period);

            // Fêtes transférables déplacées → re-enfilées avec depth+1.
            for displaced in displaced_transfers {
                transfer_queue.enqueue(doy_dst, displaced, depth + 1, year)?;
            }

            resolved_days.insert(doy_dst, ResolvedDay {
                primary:          new_primary,
                secondary_feasts: new_secondary,
            });

            placed = true;
            break;
        }

        if !placed {
            return Err(ForgeError::TransferFailed {
                slug:       feast.slug.clone(),
                origin_doy: doy_current.saturating_sub(depth as u16),
                blocked_at: doy_current,
                year,
            });
        }
    }

    debug_assert!(transfer_queue.is_empty(), "TransferQueue non vide après Passe 4");

    // ── PASSE 5 — Vérification de stabilité ──────────────────────────────────
    // Exactement un primary par slot résolu. V9 : FeastID cohérent avec le registre.

    for (&doy, day) in &resolved_days {
        if let Some(def) = registry.feasts.get(&day.primary.slug) {
            if def.feast_id != day.primary.feast_id {
                return Err(ForgeError::FeastIDMutated {
                    slug:        day.primary.slug.clone(),
                    expected_id: def.feast_id,
                    found_id:    day.primary.feast_id,
                    doy,
                    year,
                });
            }
        }
        // secondary_feasts triés par feast_id — invariant garanti par elect().
    }

    Ok(ResolvedCalendar { year, days: resolved_days })
}
