// SPDX-License-Identifier: MIT
// liturgical-calendar-forge — FeastRegistry et types de domaine Forge
//
// Les types de domaine (Nature, Color, Season) sont définis ici côté Forge pour
// la PRODUCTION des .kald. Leurs valeurs numériques doivent être identiques à
// celles définies dans liturgical-calendar-core/src/types.rs (spec §4.2–4.4).
//
// INV-FORGE-2 : BTreeMap/BTreeSet/Vec ordonné uniquement sur tout chemin
//               influençant le .kald produit. HashMap interdit.

use std::collections::BTreeMap;
use crate::error::{ForgeError, RegistryError};

// ─── Types de domaine (miroir exact de l'Engine — valeurs figées) ─────────────

/// Nature liturgique (spec §4.2, bits [13:11] de `flags`).
/// Valeurs identiques à `liturgical-calendar-core::types::Nature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Nature {
    Sollemnitas  = 0,
    Festum       = 1,
    Memoria      = 2,
    Feria        = 3,
    Commemoratio = 4,
    // 5–7 : réservés
}

/// Couleur liturgique (spec §4.3, bits [7:4] de `flags`).
/// Valeurs identiques à `liturgical-calendar-core::types::Color`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Color {
    Albus     = 0,
    Rubeus    = 1,
    Viridis   = 2,
    Violaceus = 3,
    Roseus    = 4,
    Niger     = 5,
    // 6–15 : réservés
}

/// Saison liturgique (spec §4.4, bits [10:8] de `flags`).
/// Valeurs identiques à `liturgical-calendar-core::types::Season`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Season {
    TempusOrdinarium    = 0,
    TempusAdventus      = 1,
    TempusNativitatis   = 2,
    TempusQuadragesimae = 3,
    TriduumPaschale     = 4,
    TempusPaschale      = 5,
    DiesSancti          = 6,
    // 7 : réservé
}

// ─── Scope ────────────────────────────────────────────────────────────────────

/// Hiérarchie de scope (spec §5.1 — bits [15:14] du FeastID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    Universal = 0,
    National  = 1,
    Diocesan  = 2,
}

impl Scope {
    /// Bits [15:14] du FeastID.
    #[inline]
    pub fn bits(self) -> u8 { self as u8 }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "universal" => Some(Scope::Universal),
            "national"  => Some(Scope::National),
            "diocesan"  => Some(Scope::Diocesan),
            _           => None,
        }
    }
}

// ─── Ancre (dates mobiles) ────────────────────────────────────────────────────

/// Ancres admises pour les dates mobiles (liturgical-scheme.md §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Pascha,      // Dimanche de Pâques — Meeus/Jones/Butcher
    Adventus,    // Premier dimanche de l'Avent
    Pentecostes, // Pâques + 49 (alias de commodité)
}

impl Anchor {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pascha"      => Some(Anchor::Pascha),
            "adventus"    => Some(Anchor::Adventus),
            "pentecostes" => Some(Anchor::Pentecostes),
            _             => None,
        }
    }
}

// ─── Temporalité ─────────────────────────────────────────────────────────────

/// Mode de placement d'une fête dans l'année.
#[derive(Debug, Clone)]
pub enum Temporality {
    /// Fête à date grégorienne fixe.
    Fixed { month: u8, day: u8 },
    /// Fête relative à une ancre avec offset en jours (peut être négatif).
    Mobile { anchor: Anchor, offset: i16 },
}

// ─── Version d'une fête (entrée history[]) ────────────────────────────────────

/// Métadonnées d'une fête pour une plage temporelle [from, to].
#[derive(Debug, Clone)]
pub struct FeastVersion {
    pub from:       u16,
    pub to:         Option<u16>, // None = jusqu'à 2399
    pub title:      String,
    pub precedence: u8,
    pub nature:     Nature,
    pub color:      Color,
    pub season:     Option<Season>, // None = calculé par SeasonBoundaries en Étape 2
}

impl FeastVersion {
    /// Retourne la borne supérieure effective (2399 si to=None).
    #[inline]
    pub fn to_effective(&self) -> u16 {
        self.to.unwrap_or(2399)
    }

    /// Vrai si l'entrée est active pour l'année `year`.
    #[inline]
    pub fn active_in(&self, year: u16) -> bool {
        self.from <= year && year <= self.to_effective()
    }
}

// ─── Définition complète d'une fête ──────────────────────────────────────────

/// Fête liturgique complète telle qu'ingérée du YAML.
#[derive(Debug, Clone)]
pub struct FeastDef {
    pub slug:        String,
    pub id:          u16,         // FeastID alloué — 0 jusqu'à l'insertion dans le registre
    pub scope:       Scope,
    pub region:      Option<String>,
    pub category:    u8,
    pub temporality: Temporality,
    pub history:     Vec<FeastVersion>,
}

impl FeastDef {
    /// Résout la version active pour une année donnée (liturgical-scheme.md §4.2).
    pub fn resolve_for_year(&self, year: u16) -> Result<Option<&FeastVersion>, RegistryError> {
        let candidates: Vec<&FeastVersion> = self.history.iter()
            .filter(|v| v.active_in(year))
            .collect();

        match candidates.len() {
            0 => Ok(None),
            1 => Ok(Some(candidates[0])),
            n => Err(RegistryError::TemporalOverlap {
                slug:                self.slug.clone(),
                year,
                conflicting_entries: n,
            }),
        }
    }
}

// ─── FeastRegistry ────────────────────────────────────────────────────────────

/// Registre de toutes les fêtes indexé par slug (BTreeMap — INV-FORGE-2).
///
/// Allocation des FeastIDs : pour chaque (scope_bits, category), séquences 1–4095
/// en ordre lexicographique des slugs au premier build (INV-FORGE-3).
pub struct FeastRegistry {
    /// Table principale : slug → FeastDef (FeastID déjà alloué dans def.id).
    pub feasts: BTreeMap<String, FeastDef>,
    /// Compteurs d'allocation par (scope_bits, category) — séquence suivante.
    /// BTreeMap pour déterminisme de l'itération si nécessaire.
    counters:   BTreeMap<(u8, u8), u16>,
}

impl FeastRegistry {
    pub fn new() -> Self {
        FeastRegistry {
            feasts:   BTreeMap::new(),
            counters: BTreeMap::new(),
        }
    }

    // ─── Encodage FeastID (spec §5.1) ─────────────────────────────────────

    /// Encode un FeastID : bits [15:14]=scope, [13:12]=category, [11:0]=sequence.
    #[inline]
    pub fn encode_id(scope: u8, category: u8, sequence: u16) -> u16 {
        ((scope as u16) << 14) | ((category as u16) << 12) | (sequence & 0x0FFF)
    }

    /// Alloue le prochain FeastID libre pour (scope, category).
    fn allocate_next(&mut self, scope: u8, category: u8) -> Result<u16, RegistryError> {
        let key = (scope, category);
        let seq  = self.counters.entry(key).or_insert(0);
        *seq += 1;
        if *seq > 4095 {
            return Err(RegistryError::FeastIDExhausted {
                scope:    scope.to_string(),
                category,
            });
        }
        Ok(Self::encode_id(scope, category, *seq))
    }

    // ─── Insertion ────────────────────────────────────────────────────────

    /// Insère une fête dans le registre.
    ///
    /// `explicit_id` : valeur du champ `id` dans le YAML (None si absent).
    /// Applique V2a (unicité slug), V2b (unicité id explicite), V3 (capacité).
    pub fn insert(
        &mut self,
        mut def: FeastDef,
        explicit_id: Option<u16>,
    ) -> Result<(), ForgeError> {
        // V6 — syntaxe du slug
        validate_slug(&def.slug)?;

        // V2a — unicité slug par scope (scope global ici, YAML garantit la séparation)
        if self.feasts.contains_key(&def.slug) {
            return Err(RegistryError::DuplicateSlug {
                slug:  def.slug.clone(),
                scope: format!("{:?}", def.scope),
            }.into());
        }

        let scope_bits = def.scope.bits();

        let id = if let Some(explicit) = explicit_id {
            // V2b — vérifier qu'aucun autre slug ne possède déjà cet id
            for existing in self.feasts.values() {
                if existing.id == explicit {
                    return Err(RegistryError::FeastIDConflict {
                        id:     explicit,
                        slug_a: existing.slug.clone(),
                        slug_b: def.slug.clone(),
                    }.into());
                }
            }
            // Aligner le compteur si nécessaire pour éviter les collisions futures
            let seq = explicit & 0x0FFF;
            let key = (scope_bits, def.category);
            let counter = self.counters.entry(key).or_insert(0);
            if seq > *counter { *counter = seq; }
            explicit
        } else {
            self.allocate_next(scope_bits, def.category).map_err(ForgeError::from)?
        };

        def.id = id;
        self.feasts.insert(def.slug.clone(), def);
        Ok(())
    }

    // ─── Accesseurs ───────────────────────────────────────────────────────

    /// Retourne la version active d'une fête pour une année donnée.
    pub fn resolve_for_year<'a>(
        &'a self,
        slug: &str,
        year: u16,
    ) -> Result<Option<(&'a FeastDef, &'a FeastVersion)>, RegistryError> {
        let def = match self.feasts.get(slug) {
            Some(d) => d,
            None    => return Ok(None),
        };
        match def.resolve_for_year(year)? {
            Some(v) => Ok(Some((def, v))),
            None    => Ok(None),
        }
    }
}

impl Default for FeastRegistry {
    fn default() -> Self { Self::new() }
}

// ─── Validation V6 — syntaxe du slug ─────────────────────────────────────────

/// Valide la syntaxe du slug : `[a-z][a-z0-9_]*`
pub fn validate_slug(slug: &str) -> Result<(), RegistryError> {
    let mut chars = slug.chars();
    let first = match chars.next() {
        Some(c) => c,
        None    => return Err(RegistryError::InvalidSlugSyntax(slug.to_string())),
    };
    if !first.is_ascii_lowercase() {
        return Err(RegistryError::InvalidSlugSyntax(slug.to_string()));
    }
    for c in chars {
        if !matches!(c, 'a'..='z' | '0'..='9' | '_') {
            return Err(RegistryError::InvalidSlugSyntax(slug.to_string()));
        }
    }
    Ok(())
}

// ─── Normaliseurs de chaînes (spec §10 / liturgical-scheme.md §6) ────────────

/// Parse une valeur `nature` YAML → `Nature` (V5).
pub fn parse_nature(s: &str) -> Result<Nature, RegistryError> {
    match s.to_ascii_lowercase().as_str() {
        "sollemnitas"  => Ok(Nature::Sollemnitas),
        "festum"       => Ok(Nature::Festum),
        "memoria"      => Ok(Nature::Memoria),
        "feria"        => Ok(Nature::Feria),
        "commemoratio" => Ok(Nature::Commemoratio),
        _              => Err(RegistryError::UnknownNatureString(s.to_string())),
    }
}

/// Parse une valeur `color` YAML → `Color`.
pub fn parse_color(s: &str) -> Result<Color, RegistryError> {
    match s.to_ascii_lowercase().as_str() {
        "albus"     => Ok(Color::Albus),
        "rubeus"    => Ok(Color::Rubeus),
        "viridis"   => Ok(Color::Viridis),
        "violaceus" => Ok(Color::Violaceus),
        "roseus"    => Ok(Color::Roseus),
        "niger"     => Ok(Color::Niger),
        _           => Err(RegistryError::UnknownColorString(s.to_string())),
    }
}

/// Parse une valeur `season` YAML → `Season`.
pub fn parse_season(s: &str) -> Result<Season, RegistryError> {
    match s.to_ascii_lowercase().as_str() {
        "tempus_ordinarium"    => Ok(Season::TempusOrdinarium),
        "tempus_adventus"      => Ok(Season::TempusAdventus),
        "tempus_nativitatis"   => Ok(Season::TempusNativitatis),
        "tempus_quadragesimae" => Ok(Season::TempusQuadragesimae),
        "triduum_paschale"     => Ok(Season::TriduumPaschale),
        "tempus_paschale"      => Ok(Season::TempusPaschale),
        "dies_sancti"          => Ok(Season::DiesSancti),
        _                      => Err(RegistryError::UnknownSeasonString(s.to_string())),
    }
}

// ─── Tests unitaires ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feast_id_encoding() {
        // Universal (00), category 0, sequence 1 → 0x0001
        assert_eq!(FeastRegistry::encode_id(0, 0, 1), 0x0001);
        // Universal (00), category 1, sequence 1 → 0x1001
        assert_eq!(FeastRegistry::encode_id(0, 1, 1), 0x1001);
        // National (01), category 0, sequence 1 → 0x4001
        assert_eq!(FeastRegistry::encode_id(1, 0, 1), 0x4001);
        // Diocesan (10), category 2, sequence 42 → 0x802A
        assert_eq!(FeastRegistry::encode_id(2, 2, 42), 0x802A);
        // Sequence max (4095 = 0xFFF) — limit V3
        assert_eq!(FeastRegistry::encode_id(0, 0, 4095), 0x0FFF);
    }

    #[test]
    fn slug_validation_valid() {
        assert!(validate_slug("nativitas_domini").is_ok());
        assert!(validate_slug("pascha").is_ok());
        assert!(validate_slug("a1_b2").is_ok());
        assert!(validate_slug("x").is_ok());
    }

    #[test]
    fn slug_validation_invalid() {
        assert!(validate_slug("").is_err());               // vide
        assert!(validate_slug("1abc").is_err());           // commence par chiffre
        assert!(validate_slug("hello-world").is_err());    // tiret
        assert!(validate_slug("CamelCase").is_err());      // majuscule
        assert!(validate_slug("with space").is_err());     // espace
    }

    #[test]
    fn registry_allocation_sequential() {
        let mut reg = FeastRegistry::new();
        for i in 1u16..=5 {
            let slug = format!("feast_{i:03}");
            let def = FeastDef {
                slug: slug.clone(),
                id: 0,
                scope: Scope::Universal,
                region: None,
                category: 0,
                temporality: Temporality::Fixed { month: 1, day: i as u8 },
                history: vec![],
            };
            reg.insert(def, None).unwrap();
            assert_eq!(reg.feasts[&slug].id, i); // séquences 1..5
        }
    }

    #[test]
    fn parse_nature_all_variants() {
        assert!(matches!(parse_nature("sollemnitas"), Ok(Nature::Sollemnitas)));
        assert!(matches!(parse_nature("FESTUM"),      Ok(Nature::Festum)));  // insensible casse
        assert!(matches!(parse_nature("memoria"),     Ok(Nature::Memoria)));
        assert!(parse_nature("beatus").is_err());
    }
}
