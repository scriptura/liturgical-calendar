// SPDX-License-Identifier: MIT
// liturgical-calendar-forge — Étape 1 : Rule Parsing (roadmap §2.1)
//
// Ingestion YAML → FeastRegistry.
// Validations V1–V6 appliquées à chaque fête avant insertion.
// INV-FORGE-1 : ordre d'ingestion universal → national (tri lex) → diocesan (tri lex).
// INV-FORGE-2 : BTreeMap/BTreeSet sur tout chemin influençant le .kald.

use serde::Deserialize;
use std::path::Path;

use crate::error::{ForgeError, ParseError, RegistryError};
use crate::registry::{
    Anchor, FeastDef, FeastRegistry, FeastVersion, Scope, Temporality,
    parse_color, parse_nature, parse_season,
};

// ─── Structures serde (désérialisation YAML) ──────────────────────────────────

#[derive(Deserialize, Debug)]
struct YamlFile {
    scope:          String,
    region:         Option<String>,
    from:           Option<u16>,
    to:             Option<u16>,
    format_version: u32,
    #[serde(default)]
    feasts: Vec<YamlFeast>,
}

#[derive(Deserialize, Debug)]
struct YamlFeast {
    slug:     String,
    #[serde(default)]
    id:       Option<u16>,
    scope:    String,
    #[serde(default)]
    region:   Option<String>,
    #[serde(default)]
    category: u8,
    date:     Option<YamlDate>,
    mobile:   Option<YamlMobile>,
    #[serde(default)]
    history:  Vec<YamlHistory>,
}

#[derive(Deserialize, Debug)]
struct YamlDate {
    month: u8,
    day:   u8,
}

#[derive(Deserialize, Debug)]
struct YamlMobile {
    anchor: String,
    offset: i16,
}

#[derive(Deserialize, Debug)]
struct YamlHistory {
    from:       Option<u16>,
    to:         Option<u16>,
    title:      String,
    precedence: u8,
    nature:     String,
    color:      String,
    #[serde(default)]
    season:     Option<String>,
}

// ─── Chargement du corpus depuis un répertoire (INV-FORGE-1) ─────────────────

/// Charge un corpus liturgique depuis un répertoire.
///
/// Ordre d'ingestion canonique (INV-FORGE-1) :
/// 1. `universal.yaml` (unique)
/// 2. `national-<REGION>.yaml` triés lexicographiquement
/// 3. `diocesan-<ID>.yaml` triés lexicographiquement
///
/// `fs::read_dir` est non-ordonné → collecte + tri avant ingestion.
pub fn load_corpus_from_dir(dir: &Path) -> Result<FeastRegistry, ForgeError> {
    // Collecte des chemins YAML
    let mut all_paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "yaml"))
        .collect();

    // Tri global pour déterminisme avant classification
    all_paths.sort();

    // Classification INV-FORGE-1
    let mut universal: Option<std::path::PathBuf> = None;
    let mut nationals: Vec<std::path::PathBuf>    = Vec::new();
    let mut diocesans: Vec<std::path::PathBuf>    = Vec::new();

    for path in all_paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if name == "universal.yaml" {
            universal = Some(path);
        } else if name.starts_with("national-") && name.ends_with(".yaml") {
            nationals.push(path);
        } else if name.starts_with("diocesan-") && name.ends_with(".yaml") {
            diocesans.push(path);
        }
        // Fichiers non reconnus sont silencieusement ignorés
    }

    let mut registry = FeastRegistry::new();

    // Ingestion dans l'ordre canonique
    if let Some(u) = universal {
        let content = std::fs::read_to_string(&u)?;
        parse_yaml_into_registry(&content, &mut registry)?;
    }
    // nationals est déjà trié (sort() global ci-dessus)
    for p in &nationals {
        let content = std::fs::read_to_string(p)?;
        parse_yaml_into_registry(&content, &mut registry)?;
    }
    // diocesans est déjà trié
    for p in &diocesans {
        let content = std::fs::read_to_string(p)?;
        parse_yaml_into_registry(&content, &mut registry)?;
    }

    Ok(registry)
}

// ─── Parsing d'un contenu YAML → insertion dans le registre ──────────────────

/// Parse un contenu YAML complet et insère les fêtes dans `registry`.
///
/// Applique les validations V1–V6 sur chaque fête.
pub fn parse_yaml_into_registry(
    content:  &str,
    registry: &mut FeastRegistry,
) -> Result<(), ForgeError> {
    // V1 — syntaxe YAML
    let file: YamlFile = serde_yaml::from_str(content)
        .map_err(|e| ForgeError::Parse(ParseError::MalformedYaml(e.to_string())))?;

    // V1 — format_version == 1
    if file.format_version != 1 {
        return Err(ParseError::UnsupportedSchemaVersion { found: file.format_version }.into());
    }

    // V1 — scope cohérent avec region
    if file.scope != "universal" && file.region.is_none() {
        return Err(ParseError::MalformedYaml(
            format!("scope '{}' requiert un champ region non-null", file.scope)
        ).into());
    }

    for yf in file.feasts {
        let def = convert_feast(yf)?;
        // L'id explicite a déjà été extrait — on passe None ici car défini en def.id
        // Note : convert_feast met explicit_id temporairement dans def.id si présent,
        // puis on extrait pour passer à insert().
        // Solution : explicit_id est passé séparément depuis convert_feast.
        registry.insert(def.0, def.1)?;
    }

    Ok(())
}

// ─── Conversion YamlFeast → (FeastDef, Option<u16>) ──────────────────────────

/// Convertit un `YamlFeast` en `(FeastDef, Option<explicit_id>)`.
fn convert_feast(yf: YamlFeast) -> Result<(FeastDef, Option<u16>), ForgeError> {
    let slug         = yf.slug.clone();
    let explicit_id  = yf.id;

    // Scope de la fête (le champ `scope` de la fête, pas du fichier)
    let scope = Scope::from_str(&yf.scope).ok_or_else(|| {
        ParseError::MalformedYaml(format!("scope invalide '{}' pour '{}'", yf.scope, slug))
    })?;

    // V1 — exactement un bloc de temporalité
    let temporality = match (yf.date, yf.mobile) {
        (Some(d), None) => {
            // V3a — date fixe valide
            validate_fixed_date(&slug, d.month, d.day)?;
            Temporality::Fixed { month: d.month, day: d.day }
        }
        (None, Some(m)) => {
            // V4 — ancre connue et sans cycle
            let anchor = Anchor::from_str(&m.anchor).ok_or_else(|| {
                ParseError::UnknownAnchor { slug: slug.clone(), anchor: m.anchor.clone() }
            })?;
            Temporality::Mobile { anchor, offset: m.offset }
        }
        (Some(_), Some(_)) => {
            return Err(ParseError::AmbiguousTemporalityField { slug }.into());
        }
        (None, None) => {
            return Err(ParseError::MissingTemporalityField { slug }.into());
        }
    };

    // Conversion des entrées history[]
    let mut history: Vec<FeastVersion> = Vec::with_capacity(yf.history.len());
    for yh in yf.history {
        let from = yh.from.unwrap_or(1969);
        let to   = yh.to;

        // V2-Bis — domaine de precedence [0, 12]
        if yh.precedence > 12 {
            return Err(RegistryError::InvalidPrecedenceValue(yh.precedence).into());
        }

        // V4 / V3b — cohérence et bornes des plages temporelles
        let to_val = to.unwrap_or(2399);
        if from < 1969 || to_val > 2399 || from > to_val {
            return Err(RegistryError::InvalidTemporalRange { from, to: to_val }.into());
        }

        let nature = parse_nature(&yh.nature).map_err(ForgeError::Registry)?;
        let color  = parse_color(&yh.color).map_err(ForgeError::Registry)?;
        let season = if let Some(ref s) = yh.season {
            Some(parse_season(s).map_err(ForgeError::Registry)?)
        } else {
            None
        };

        history.push(FeastVersion {
            from,
            to,
            title: yh.title,
            precedence: yh.precedence,
            nature,
            color,
            season,
        });
    }

    // Trier les entrées history[] par from croissant (spec §4 — ordre d'évaluation)
    history.sort_by_key(|v| v.from);

    // V1 / V2d — unicité temporelle dans history
    validate_temporal_uniqueness(&slug, &history)?;

    let def = FeastDef {
        slug,
        id: 0, // sera assigné par FeastRegistry::insert
        scope,
        region: yf.region,
        category: yf.category,
        temporality,
        history,
    };

    Ok((def, explicit_id))
}

// ─── Validations atomiques ────────────────────────────────────────────────────

/// V3a — valide qu'une date fixe est possible.
/// Le 29 février est admis (doy=59, Padding Entry les années non-bissextiles).
fn validate_fixed_date(slug: &str, month: u8, day: u8) -> Result<(), ForgeError> {
    if month < 1 || month > 12 {
        return Err(ParseError::InvalidDate { slug: slug.to_string(), month, day }.into());
    }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11               => 30,
        2                             => 29, // 29 fév admis
        _                             => unreachable!(),
    };
    if day < 1 || day > max_day {
        return Err(ParseError::InvalidDate { slug: slug.to_string(), month, day }.into());
    }
    Ok(())
}

/// V1 / V2d — vérifie qu'au plus une entrée history[] est active par année.
///
/// Algorithme O(n²) — acceptable car history[] << 100 entrées par fête.
fn validate_temporal_uniqueness(
    slug:    &str,
    history: &[FeastVersion],
) -> Result<(), ForgeError> {
    for (i, a) in history.iter().enumerate() {
        for b in &history[i + 1..] {
            // Deux entrées se chevauchent si a.from ≤ b.to_effective ET b.from ≤ a.to_effective
            let a_to = a.to_effective();
            let b_to = b.to_effective();
            if a.from <= b_to && b.from <= a_to {
                // Chevauchement — trouver une année en commun pour le message
                let overlap_year = a.from.max(b.from);
                return Err(RegistryError::TemporalOverlap {
                    slug:                slug.to_string(),
                    year:                overlap_year,
                    conflicting_entries: 2,
                }.into());
            }
        }
    }
    Ok(())
}

// ─── Tests unitaires ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// YAML minimal : une fête universelle fixe + une fête mobile.
    const MINIMAL_YAML: &str = r#"
scope: universal
region: ~
from: 1969
to: ~
format_version: 1
feasts:
  - slug: nativitas_domini
    scope: universal
    category: 0
    date:
      month: 12
      day: 25
    history:
      - from: 1969
        title: "In Nativitate Domini"
        precedence: 1
        nature: sollemnitas
        color: albus
        season: tempus_nativitatis

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
"#;

    #[test]
    fn parse_minimal_corpus() {
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(MINIMAL_YAML, &mut reg).unwrap();
        assert_eq!(reg.feasts.len(), 2);
        assert!(reg.feasts.contains_key("nativitas_domini"));
        assert!(reg.feasts.contains_key("dominica_resurrectionis"));
    }

    #[test]
    fn parse_rejects_bad_format_version() {
        let yaml = r#"
scope: universal
region: ~
format_version: 2
feasts: []
"#;
        let mut reg = FeastRegistry::new();
        let err = parse_yaml_into_registry(yaml, &mut reg).unwrap_err();
        assert!(matches!(err, ForgeError::Parse(ParseError::UnsupportedSchemaVersion { found: 2 })));
    }

    #[test]
    fn parse_rejects_invalid_date() {
        let yaml = r#"
scope: universal
region: ~
format_version: 1
feasts:
  - slug: fake_feast
    scope: universal
    category: 0
    date:
      month: 2
      day: 30
    history:
      - from: 1969
        title: "Fake"
        precedence: 12
        nature: feria
        color: viridis
"#;
        let mut reg = FeastRegistry::new();
        assert!(parse_yaml_into_registry(yaml, &mut reg).is_err());
    }

    #[test]
    fn parse_rejects_unknown_anchor() {
        let yaml = r#"
scope: universal
region: ~
format_version: 1
feasts:
  - slug: test_mobile
    scope: universal
    category: 0
    mobile:
      anchor: quattuor_tempora
      offset: 0
    history:
      - from: 1969
        title: "Test"
        precedence: 12
        nature: feria
        color: viridis
"#;
        let mut reg = FeastRegistry::new();
        assert!(parse_yaml_into_registry(yaml, &mut reg).is_err());
    }

    #[test]
    fn parse_national_yaml() {
        let national = r#"
scope: national
region: FR
from: 1969
to: ~
format_version: 1
feasts:
  - slug: dionysii_parisiensis
    scope: national
    region: FR
    category: 2
    date:
      month: 10
      day: 9
    history:
      - from: 1969
        title: "S. Dionysius"
        precedence: 11
        nature: memoria
        color: rubeus
"#;
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(national, &mut reg).unwrap();
        let feast = &reg.feasts["dionysii_parisiensis"];
        // National (01), category 2, sequence 1 → 0x4801
        assert_eq!(feast.id, crate::registry::FeastRegistry::encode_id(1, 2, 1));
    }
}
