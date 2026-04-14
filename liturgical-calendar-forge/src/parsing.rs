use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::fs;

use crate::error::{ForgeError, ParseError, RegistryError};
use crate::registry::{
    Color, FeastDef, FeastHistoryEntry, FeastRegistry, LiturgicalPeriod,
    Nature, Scope, Temporality, TransferDef, TransferTarget,
};

// ---------------------------------------------------------------------------
// Structs de désérialisation YAML — deny_unknown_fields partout
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlFeast {
    version:   u32,
    category:  u8,
    id:        Option<u16>,
    date:      Option<YamlDate>,
    mobile:    Option<YamlMobile>,
    history:   Vec<YamlHistoryEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlDate { month: u8, day: u8 }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlMobile {
    anchor:  String,
    offset:  Option<i32>,
    ordinal: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlTransfer {
    collides: String,
    offset:   Option<u32>,
    date:     Option<YamlDate>,
    mobile:   Option<YamlMobileDst>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlMobileDst {
    anchor: String,
    offset: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlHistoryEntry {
    from:           Option<u16>,
    to:             Option<u16>,
    precedence:     u8,
    nature:         String,
    color:          String,
    season:         Option<String>,
    has_vigil_mass: Option<bool>,
    transfers:      Option<Vec<YamlTransfer>>,  // scoped à cette tranche temporelle
}

// ---------------------------------------------------------------------------
// V6 — validation slug : [a-z][a-z0-9_]*
// ---------------------------------------------------------------------------

fn validate_slug(stem: &str) -> Result<(), ParseError> {
    let mut chars = stem.chars();
    match chars.next() {
        None | Some('0'..='9') | Some('_') => {
            return Err(ParseError::InvalidSlugSyntax(stem.to_string()))
        }
        Some(c) if !c.is_ascii_lowercase() => {
            return Err(ParseError::InvalidSlugSyntax(stem.to_string()))
        }
        _ => {}
    }
    for c in chars {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_' {
            return Err(ParseError::InvalidSlugSyntax(stem.to_string()));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// V5 — parse Nature (avec hint sur valeurs informelles)
// ---------------------------------------------------------------------------

fn parse_nature(s: &str) -> Result<Nature, RegistryError> {
    match s {
        "sollemnitas"  => Ok(Nature::Sollemnitas),
        "festum"       => Ok(Nature::Festum),
        "memoria"      => Ok(Nature::Memoria),
        "feria"        => Ok(Nature::Feria),
        "commemoratio" => Ok(Nature::Commemoratio),
        other => {
            let hint = match other {
                "solemnity" | "solemnnitas" | "solemnitas" => " (hint: 'sollemnitas')",
                "feast"    => " (hint: 'festum')",
                "memorial" | "memory" => " (hint: 'memoria')",
                "commemoration" => " (hint: 'commemoratio')",
                _ => "",
            };
            Err(RegistryError::UnknownNatureString(format!("{}{}", other, hint)))
        }
    }
}

// ---------------------------------------------------------------------------
// Parse Color
// ---------------------------------------------------------------------------

fn parse_color(s: &str) -> Result<Color, RegistryError> {
    match s {
        "white" | "albus"              => Ok(Color::Albus),
        "red"   | "rubeus"             => Ok(Color::Rubeus),
        "green" | "viridis"            => Ok(Color::Viridis),
        "purple"| "violet"|"violaceus" => Ok(Color::Violaceus),
        "rose"  | "rosaceus"           => Ok(Color::Rosaceus),
        "black" | "niger"              => Ok(Color::Niger),
        "gold"  | "aureus"             => Ok(Color::Aureus),
        other => Err(RegistryError::UnknownColorString(other.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Parse LiturgicalPeriod
// ---------------------------------------------------------------------------

fn parse_season(s: &str) -> Result<LiturgicalPeriod, RegistryError> {
    match s {
        "adventus"                              => Ok(LiturgicalPeriod::Adventus),
        "nativitas"                             => Ok(LiturgicalPeriod::Nativitas),
        "epiphania"                             => Ok(LiturgicalPeriod::Epiphania),
        "quadragesima"                          => Ok(LiturgicalPeriod::Quadragesima),
        "pascha"                                => Ok(LiturgicalPeriod::Pascha),
        "tempus_ordinarium"|"temporis_ordinarii"=> Ok(LiturgicalPeriod::TemporisOrdinarii),
        other => Err(RegistryError::UnknownSeasonString(other.to_string())),
    }
}

// ---------------------------------------------------------------------------
// V3a — validation date (mois/jour cohérents ; Feb 29 admis statiquement)
// ---------------------------------------------------------------------------

fn validate_date(slug: &str, month: u8, day: u8) -> Result<(), ParseError> {
    if !(1..=12).contains(&month) {
        return Err(ParseError::InvalidDate { slug: slug.to_string(), month, day });
    }
    let max_day: u8 = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 29, // Feb 29 admis (l'année n'est pas connue à ce stade)
        _ => unreachable!(),
    };
    if !(1..=max_day).contains(&day) {
        return Err(ParseError::InvalidDate { slug: slug.to_string(), month, day });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// V-T5 — ancres primitives valides pour transfers.mobile
// ---------------------------------------------------------------------------

const PRIMITIVE_ANCHORS: &[&str] = &[
    "pascha", "adventus", "pentecostes", "nativitas", "epiphania",
];

// ---------------------------------------------------------------------------
// Parsing de la temporalité mobile (V4a + desucrage pentecostes)
// ---------------------------------------------------------------------------

fn parse_mobile_temporality(slug: &str, m: &YamlMobile) -> Result<Temporality, ForgeError> {
    if m.anchor == "tempus_ordinarium" {
        // V4a — tempus_ordinarium : offset interdit, ordinal obligatoire [1,34]
        if m.offset.is_some() {
            return Err(ParseError::OffsetOnOrdinalAnchor { slug: slug.to_string() }.into());
        }
        let ordinal = m.ordinal
            .ok_or_else(|| ParseError::MissingOrdinal { slug: slug.to_string() })?;
        if !(1..=34).contains(&ordinal) {
            return Err(ParseError::OrdinalOutOfRange { slug: slug.to_string(), ordinal }.into());
        }
        Ok(Temporality::Ordinal { ordinal })
    } else {
        // V4a — ancre ordinaire : ordinal interdit
        if m.ordinal.is_some() {
            return Err(ParseError::OrdinalOnNonOrdinalAnchor {
                slug:   slug.to_string(),
                anchor: m.anchor.clone(),
            }.into());
        }
        let offset = m.offset.unwrap_or(0);
        // Desugaring pentecostes → pascha + 49
        let (anchor, offset) = if m.anchor == "pentecostes" {
            ("pascha".to_string(), offset + 49)
        } else {
            (m.anchor.clone(), offset)
        };
        Ok(Temporality::Mobile { anchor, offset })
    }
}

// ---------------------------------------------------------------------------
// Parsing history (V2-Bis, V3b, V2d, V5, V-Natura-Memoria, V-Vigilia)
// ---------------------------------------------------------------------------

fn parse_history(slug: &str, entries: &[YamlHistoryEntry])
    -> Result<Vec<FeastHistoryEntry>, ForgeError>
{
    let mut result: Vec<FeastHistoryEntry> = Vec::with_capacity(entries.len());

    for entry in entries {
        let from = entry.from.unwrap_or(1969);
        let to   = entry.to.unwrap_or(2399);

        // V2-Bis — precedence ∈ [0,12]
        if entry.precedence > 12 {
            return Err(RegistryError::InvalidPrecedenceValue(entry.precedence).into());
        }
        // V3b — plages temporelles
        if from < 1969 || to > 2399 || from > to {
            return Err(RegistryError::InvalidTemporalRange { from, to }.into());
        }

        let nature = parse_nature(&entry.nature)?;
        let color  = parse_color(&entry.color)?;
        let season = entry.season.as_deref().map(parse_season).transpose()?;
        let has_vigil_mass = entry.has_vigil_mass.unwrap_or(false);

        // V-Natura-Memoria
        if nature == Nature::Memoria && entry.precedence != 11 && entry.precedence != 12 {
            return Err(ParseError::InvalidMemoriaPrecedence {
                slug: slug.to_string(),
                from,
                found_precedence: entry.precedence,
            }.into());
        }
        // V-Vigilia
        if has_vigil_mass && nature != Nature::Sollemnitas {
            return Err(ParseError::VigiliaNonSollemnitas {
                slug:   slug.to_string(),
                from,
                nature: entry.nature.clone(),
            }.into());
        }

        // V-T* — transfers scoped à cette entrée history
        let transfers = entry.transfers
            .as_deref()
            .map(|ts| parse_transfers(slug, from, ts))
            .transpose()?
            .unwrap_or_default();

        result.push(FeastHistoryEntry {
            from, to,
            precedence: entry.precedence,
            nature, color, season, has_vigil_mass,
            transfers,
        });
    }

    // V2d — chevauchement temporel : tri par `from`, détection intervalle
    check_temporal_overlap(slug, &result)?;

    Ok(result)
}

fn check_temporal_overlap(_slug: &str, entries: &[FeastHistoryEntry])
    -> Result<(), ForgeError>
// TODO: slug dans error ? RegistryError::TemporalOverlap ne porte pas de champ slug (spec actuelle).
// Le paramètre est conservé pour cohérence d'appel depuis parse_history.
{
    let mut sorted: Vec<&FeastHistoryEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.from);
    for i in 1..sorted.len() {
        if sorted[i].from <= sorted[i - 1].to {
            return Err(RegistryError::TemporalOverlap.into());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Parsing transfers (V-T1..V-T5, desucrage pentecostes dans mobile)
// ---------------------------------------------------------------------------

fn parse_transfers(slug: &str, from: u16, transfers: &[YamlTransfer])
    -> Result<Vec<TransferDef>, ForgeError>
{
    let mut result: Vec<TransferDef> = Vec::with_capacity(transfers.len());
    // INV-FORGE-2 : BTreeSet pour détecter doublons (pas HashMap)
    let mut seen: BTreeSet<&str> = BTreeSet::new();

    for t in transfers {
        // V-T3 — unicité de collides dans cette tranche temporelle
        if !seen.insert(t.collides.as_str()) {
            return Err(ParseError::TransferDuplicateCollides {
                slug:     slug.to_string(),
                from,
                collides: t.collides.clone(),
            }.into());
        }

        // V-T1 — exactement une option
        let count = t.offset.is_some() as u8
                  + t.date.is_some() as u8
                  + t.mobile.is_some() as u8;
        match count {
            0 => return Err(ParseError::TransferEmpty {
                slug: slug.to_string(), collides: t.collides.clone()
            }.into()),
            2.. => return Err(ParseError::TransferAmbiguous {
                slug: slug.to_string(), collides: t.collides.clone()
            }.into()),
            _ => {}
        }

        let target = if let Some(offset) = t.offset {
            // V-T4 — offset ≥ 1 (u32, seule valeur invalide = 0)
            if offset == 0 {
                return Err(ParseError::TransferOffsetNotPositive {
                    slug: slug.to_string(), collides: t.collides.clone(), offset,
                }.into());
            }
            TransferTarget::Offset(offset)

        } else if let Some(ref d) = t.date {
            validate_date(slug, d.month, d.day)?;
            TransferTarget::Date { month: d.month, day: d.day }

        } else if let Some(ref m) = t.mobile {
            // V-T5 — ancre primitive uniquement
            if !PRIMITIVE_ANCHORS.contains(&m.anchor.as_str()) {
                return Err(ParseError::TransferMobileInvalidAnchor {
                    slug:    slug.to_string(),
                    collides: t.collides.clone(),
                    anchor:  m.anchor.clone(),
                }.into());
            }
            // Desugaring pentecostes → pascha + 49
            let (anchor, offset) = if m.anchor == "pentecostes" {
                ("pascha".to_string(), m.offset + 49)
            } else {
                (m.anchor.clone(), m.offset)
            };
            TransferTarget::Mobile { anchor, offset }

        } else {
            unreachable!()
        };

        result.push(TransferDef { collides: t.collides.clone(), target });
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// parse_feast_from_yaml — noyau testable (expose pour tests)
// ---------------------------------------------------------------------------

pub fn parse_feast_from_yaml(
    slug:    &str,
    scope:   Scope,
    content: &str,
) -> Result<FeastDef, ForgeError> {
    // V1 — parsing YAML
    let yaml: YamlFeast = serde_yaml::from_str(content)
        .map_err(|e| ParseError::MalformedYaml(e.to_string()))?;

    if yaml.version != 1 {
        return Err(ParseError::UnsupportedSchemaVersion(yaml.version).into());
    }

    // Temporalité — exactement un bloc
    let temporality = match (yaml.date.as_ref(), yaml.mobile.as_ref()) {
        (Some(_), Some(_)) =>
            return Err(ParseError::AmbiguousTemporalityField { slug: slug.to_string() }.into()),
        (None, None) =>
            return Err(ParseError::MissingTemporalityField { slug: slug.to_string() }.into()),
        (Some(d), None) => {
            validate_date(slug, d.month, d.day)?;
            Temporality::Fixed { month: d.month, day: d.day }
        }
        (None, Some(m)) => parse_mobile_temporality(slug, m)?,
    };

    let history = parse_history(slug, &yaml.history)?;

    Ok(FeastDef {
        slug:        slug.to_string(),
        scope,
        category:    yaml.category,
        id:          yaml.id,
        temporality,
        history,
    })
}

// ---------------------------------------------------------------------------
// parse_feast_file — lecture disque + appel parse_feast_from_yaml
// ---------------------------------------------------------------------------

fn parse_feast_file(path: &Path, slug: &str, scope: Scope)
    -> Result<FeastDef, ForgeError>
{
    let content = fs::read_to_string(path)?;
    parse_feast_from_yaml(slug, scope, &content)
}

// ---------------------------------------------------------------------------
// ingest_scope_dir — un scope (temporale/ + sanctorale/)
// INV-FORGE-1 : collecter → trier lex → ingérer
// ---------------------------------------------------------------------------

fn ingest_scope_dir(
    scope_dir: &Path,
    scope:     Scope,
    registry:  &mut FeastRegistry,
) -> Result<(), ForgeError> {
    for sub in &["temporale", "sanctorale"] {
        let dir = scope_dir.join(sub);
        if !dir.exists() { continue; }

        // INV-FORGE-1 : fs::read_dir non ordonné → collecter + trier
        let mut files: Vec<std::path::PathBuf> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("yaml") | Some("yml")
            ))
            .collect();
        files.sort();

        for path in files {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            // V6 — slug avant parsing
            validate_slug(&stem).map_err(ForgeError::Parse)?;

            let def = parse_feast_file(&path, &stem, scope.clone())?;
            registry.insert(def);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ingest_corpus — point d'entrée public
// Ordre : universale → nationalia (lex) → dioecesana (lex)
// Post-ingestion : V-T2 (collides targets)
// ---------------------------------------------------------------------------

pub fn ingest_corpus(data_dir: &Path) -> Result<FeastRegistry, ForgeError> {
    let mut registry = FeastRegistry::new();

    // Universale
    let universale = data_dir.join("universale");
    if universale.exists() {
        ingest_scope_dir(&universale, Scope::Universal, &mut registry)?;
    }

    // Nationalia — lex sort sur code ISO
    let nationalia = data_dir.join("nationalia");
    if nationalia.exists() {
        let mut iso_dirs: Vec<std::path::PathBuf> = fs::read_dir(&nationalia)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        iso_dirs.sort();

        for iso_path in iso_dirs {
            let iso = iso_path
                .file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            ingest_scope_dir(&iso_path, Scope::National(iso), &mut registry)?;
        }
    }

    // Dioecesana — lex sort sur identifiant
    let dioecesana = data_dir.join("dioecesana");
    if dioecesana.exists() {
        let mut id_dirs: Vec<std::path::PathBuf> = fs::read_dir(&dioecesana)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        id_dirs.sort();

        for id_path in id_dirs {
            let id = id_path
                .file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            ingest_scope_dir(&id_path, Scope::Diocesan(id), &mut registry)?;
        }
    }

    // V-T2 — post-ingestion : tous les slugs collides doivent exister
    validate_collides_targets(&registry)?;

    Ok(registry)
}

// ---------------------------------------------------------------------------
// V-T2 — vérification post-ingestion
// ---------------------------------------------------------------------------

fn validate_collides_targets(registry: &FeastRegistry) -> Result<(), ForgeError> {
    for feast in registry.iter() {
        for entry in &feast.history {
            for transfer in &entry.transfers {
                if !registry.contains(&transfer.collides) {
                    return Err(ParseError::UnknownCollidesTarget {
                        slug:    feast.slug.clone(),
                        collides: transfer.collides.clone(),
                    }.into());
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests unitaires
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- V6 ---

    #[test]
    fn slug_valid() {
        assert!(validate_slug("iosephi").is_ok());
        assert!(validate_slug("a1_b").is_ok());
    }

    #[test]
    fn slug_invalid_starts_digit() {
        assert!(matches!(
            validate_slug("1abc"),
            Err(ParseError::InvalidSlugSyntax(_))
        ));
    }

    #[test]
    fn slug_invalid_uppercase() {
        assert!(matches!(
            validate_slug("Abc"),
            Err(ParseError::InvalidSlugSyntax(_))
        ));
    }

    // --- V4a ---

    #[test]
    fn v4a_offset_on_ordinal_anchor() {
        let yaml = r#"
version: 1
category: 0
mobile:
  anchor: tempus_ordinarium
  offset: 7
  ordinal: 3
history:
  - precedence: 1
    nature: sollemnitas
    color: white
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(err, ForgeError::Parse(ParseError::OffsetOnOrdinalAnchor { .. })));
    }

    #[test]
    fn v4a_ordinal_on_non_ordinal_anchor() {
        let yaml = r#"
version: 1
category: 0
mobile:
  anchor: pascha
  ordinal: 1
history:
  - precedence: 1
    nature: sollemnitas
    color: white
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(err, ForgeError::Parse(ParseError::OrdinalOnNonOrdinalAnchor { .. })));
    }

    // --- V-Natura-Memoria ---

    #[test]
    fn v_natura_memoria_invalid_precedence() {
        let yaml = r#"
version: 1
category: 1
date:
  month: 5
  day: 1
history:
  - precedence: 9
    nature: memoria
    color: white
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(
            err,
            ForgeError::Parse(ParseError::InvalidMemoriaPrecedence {
                found_precedence: 9, ..
            })
        ));
    }

    #[test]
    fn v_natura_memoria_valid_precedence_11() {
        let yaml = r#"
version: 1
category: 1
date:
  month: 5
  day: 1
history:
  - precedence: 11
    nature: memoria
    color: white
"#;
        assert!(parse_feast_from_yaml("test_slug", Scope::Universal, yaml).is_ok());
    }

    // --- V-Vigilia ---

    #[test]
    fn v_vigilia_non_sollemnitas() {
        // has_vigil_mass: true sur une natura != sollemnitas → VigiliaNonSollemnitas
        let yaml = r#"
version: 1
category: 1
date:
  month: 5
  day: 1
history:
  - precedence: 11
    nature: memoria
    color: white
    has_vigil_mass: true
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(err, ForgeError::Parse(ParseError::VigiliaNonSollemnitas { .. })));
    }

    // --- Desugaring pentecostes (temporalité) ---

    #[test]
    fn desugaring_pentecostes_temporality() {
        let yaml = r#"
version: 1
category: 0
mobile:
  anchor: pentecostes
  offset: 0
history:
  - precedence: 1
    nature: sollemnitas
    color: white
"#;
        let def = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap();
        match def.temporality {
            Temporality::Mobile { anchor, offset } => {
                assert_eq!(anchor, "pascha");
                assert_eq!(offset, 49);
            }
            _ => panic!("expected Mobile"),
        }
    }

    // --- V-T1 ---

    #[test]
    fn transfer_ambiguous() {
        // offset ET mobile simultanément dans le même transfer (scoped history)
        let yaml = r#"
version: 1
category: 1
date:
  month: 3
  day: 19
history:
  - precedence: 1
    nature: sollemnitas
    color: white
    transfers:
      - collides: other_slug
        offset: 2
        mobile:
          anchor: pascha
          offset: 3
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(err, ForgeError::Parse(ParseError::TransferAmbiguous { .. })));
    }

    // --- V-T5 ---

    #[test]
    fn transfer_mobile_invalid_anchor_tempus_ordinarium() {
        let yaml = r#"
version: 1
category: 1
date:
  month: 3
  day: 19
history:
  - precedence: 1
    nature: sollemnitas
    color: white
    transfers:
      - collides: other_slug
        mobile:
          anchor: tempus_ordinarium
          offset: 0
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(
            err,
            ForgeError::Parse(ParseError::TransferMobileInvalidAnchor { .. })
        ));
    }

    // --- V-T4 ---

    #[test]
    fn transfer_offset_zero_rejected() {
        let yaml = r#"
version: 1
category: 1
date:
  month: 3
  day: 19
history:
  - precedence: 1
    nature: sollemnitas
    color: white
    transfers:
      - collides: other_slug
        offset: 0
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(
            err,
            ForgeError::Parse(ParseError::TransferOffsetNotPositive { offset: 0, .. })
        ));
    }

    // --- Desugaring pentecostes dans transfer.mobile ---

    #[test]
    fn desugaring_pentecostes_in_transfer_mobile() {
        let yaml = r#"
version: 1
category: 1
date:
  month: 3
  day: 19
history:
  - precedence: 1
    nature: sollemnitas
    color: white
    transfers:
      - collides: other_slug
        mobile:
          anchor: pentecostes
          offset: 3
"#;
        let def = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap();
        // transfers scoped à history[0]
        let t = &def.history[0].transfers[0];
        match &t.target {
            TransferTarget::Mobile { anchor, offset } => {
                assert_eq!(anchor, "pascha");
                assert_eq!(*offset, 52); // 49 + 3
            }
            _ => panic!("expected Mobile"),
        }
    }

    // --- UnsupportedSchemaVersion ---

    #[test]
    fn unsupported_schema_version() {
        let yaml = r#"
version: 2
category: 1
date:
  month: 1
  day: 1
history:
  - precedence: 1
    nature: sollemnitas
    color: white
"#;
        let err = parse_feast_from_yaml("test_slug", Scope::Universal, yaml).unwrap_err();
        assert!(matches!(err, ForgeError::Parse(ParseError::UnsupportedSchemaVersion(2))));
    }

    // --- Iosephi — transfers scoped (schème v1.7.0) ---

    const YAML_IOSEPHI: &str = r#"
version: 1
category: 1
date:
  month: 3
  day: 19

history:
  - from: 1969
    to: 2007
    precedence: 4
    nature: sollemnitas
    color: albus

  - from: 2008
    precedence: 4
    nature: sollemnitas
    color: albus
    transfers:
      - collides: dominica_in_palmis
        mobile:
          anchor: pascha
          offset: -8
      - collides: feria_ii_in_hebdomada_sancta
        mobile:
          anchor: pascha
          offset: -8
      - collides: feria_iii_in_hebdomada_sancta
        mobile:
          anchor: pascha
          offset: -8
      - collides: feria_iv_in_hebdomada_sancta
        mobile:
          anchor: pascha
          offset: -8
"#;

    #[test]
    fn parse_iosephi_scoped_transfers() {
        // V-T2 (collides targets) est post-ingestion : non exécutée ici
        let feast = parse_feast_from_yaml(
            "iosephi_sponsi_beatae_mariae_virginis",
            Scope::Universal,
            YAML_IOSEPHI,
        ).expect("parse doit réussir");

        assert_eq!(feast.history.len(), 2);

        let v1969 = &feast.history[0];
        assert_eq!(v1969.from, 1969);
        assert_eq!(v1969.to, 2007);
        assert!(v1969.transfers.is_empty(), "période 1969–2007 sans transfers");

        let v2008 = &feast.history[1];
        assert_eq!(v2008.from, 2008);
        assert_eq!(v2008.to, 2399); // to absent → défaut
        assert_eq!(v2008.transfers.len(), 4, "4 collides en Semaine Sainte");

        for t in &v2008.transfers {
            match &t.target {
                TransferTarget::Mobile { anchor, offset } => {
                    assert_eq!(anchor, "pascha");
                    assert_eq!(*offset, -8i32);
                }
                _ => panic!("attendu TransferTarget::Mobile"),
            }
        }
    }
}
