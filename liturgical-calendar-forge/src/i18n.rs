//! Étape 1bis — i18n Resolution
//!
//! Responsabilités :
//!   - Ingestion des dictionnaires `i18n/{lang}/{slug}.yaml` (§9.5 du schème).
//!   - Validation V-I1 (clé latine obligatoire) et V-I2 (pas de clé orpheline).
//!   - Fusion AOT du fallback latin → `LabelTable` plate et autonome par langue.
//!
//! Invariants :
//!   - `DictStore` et `LabelTable` utilisent `BTreeMap` (INV-FORGE-2 — ordre déterministe).
//!   - Aucune allocation post-construction dans `resolve_label` (retourne `&str`).
//!   - La `LabelTable` n'influence pas le `.kald` (séparation topologie/labels).

use std::collections::BTreeMap;
use std::path::Path;
use std::{fs, io};

use crate::error::{ForgeError, ParseError};
use crate::registry::FeastRegistry;

/// FeastID alloué définitivement par la Forge — u16 compact.
/// Identique au champ `primary_id` de `CalendarEntry` dans le `.kald`.
pub type FeastID = u16;

// ---------------------------------------------------------------------------
// DictStore — stockage brut des dictionnaires i18n
// ---------------------------------------------------------------------------
//
// Clé  : (lang, slug, from, field)  — ordre lexicographique BTreeMap.
// Valeur : String localisée.
//
// Le champ `from` est le `from` de l'entrée `history[]` correspondante.
// Le seul `field` actuellement validé est "title" (V-I1), mais d'autres
// champs (ex: "subtitle") peuvent coexister sans erreur.

pub struct DictStore {
    entries: BTreeMap<(String, String, u16, String), String>,
}

impl DictStore {
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }

    pub fn insert(&mut self, lang: &str, slug: &str, from: u16, field: &str, value: String) {
        self.entries.insert(
            (lang.to_owned(), slug.to_owned(), from, field.to_owned()),
            value,
        );
    }

    pub fn get(&self, lang: &str, slug: &str, from: u16, field: &str) -> Option<&str> {
        self.entries
            .get(&(lang.to_owned(), slug.to_owned(), from, field.to_owned()))
            .map(String::as_str)
    }

    /// Itère sur toutes les clés `(lang, slug, from, field)` du store.
    /// Ordre lexicographique garanti (BTreeMap).
    pub fn iter_keys(&self) -> impl Iterator<Item = (&str, &str, u16, &str)> {
        self.entries
            .keys()
            .map(|(lang, slug, from, field)| {
                (lang.as_str(), slug.as_str(), *from, field.as_str())
            })
    }
}

impl Default for DictStore {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// parse_dict_file — parse un fichier `i18n/{lang}/{slug}.yaml`
// ---------------------------------------------------------------------------
//
// Format YAML attendu (§9.5 du schème v1.7.0) :
//
//   {from_year}:            ← clé entière = année `from` de history[]
//     title: "..."          ← champ obligatoire (V-I1)
//     subtitle: "..."       ← champ optionnel admis, non validé par V-I1
//
// Exemple :
//   2011:
//     title: "B. Ioannes Paulus II, pp."
//   2014:
//     title: "S. Ioannes Paulus II, pp."

pub fn parse_dict_file(
    path:  &Path,
    lang:  &str,
    slug:  &str,
    store: &mut DictStore,
) -> Result<(), ForgeError> {
    let content = fs::read_to_string(path)
        .map_err(|e: io::Error| ForgeError::Io(e))?;

    let yaml: serde_yml::Value = serde_yml::from_str(&content)
        .map_err(|e| ParseError::MalformedYaml(
            format!("{}: {}", path.display(), e)
        ))?;

    let mapping = yaml.as_mapping()
        .ok_or_else(|| ParseError::MalformedYaml(
            format!("{}: le niveau racine doit être un mapping (clés = années)", path.display())
        ))?;

    for (key, value) in mapping {
        // Clé de premier niveau : entier = année `from`
        let from: u16 = match key {
            serde_yml::Value::Number(n) => {
                n.as_u64()
                    .and_then(|v| u16::try_from(v).ok())
                    .ok_or_else(|| ParseError::MalformedYaml(
                        format!("{}: clé {:?} — attendu entier u16 (année)", path.display(), key)
                    ))?
            }
            _ => return Err(ParseError::MalformedYaml(
                format!(
                    "{}: clé {:?} invalide — les clés de premier niveau doivent être des entiers (années)",
                    path.display(), key
                )
            ).into()),
        };

        // Valeur : mapping de champs
        let fields = value.as_mapping()
            .ok_or_else(|| ParseError::MalformedYaml(
                format!(
                    "{}: entrée pour l'année {} doit être un mapping {{ field: value }}",
                    path.display(), from
                )
            ))?;

        for (fkey, fval) in fields {
            let field = fkey.as_str()
                .ok_or_else(|| ParseError::MalformedYaml(
                    format!(
                        "{}: nom de champ invalide sous l'année {} — attendu une chaîne",
                        path.display(), from
                    )
                ))?;

            let val = fval.as_str()
                .ok_or_else(|| ParseError::MalformedYaml(
                    format!(
                        "{}: valeur du champ '{}' (année {}) doit être une chaîne",
                        path.display(), field, from
                    )
                ))?;

            store.insert(lang, slug, from, field, val.to_owned());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// discover_and_load_i18n — ingestion complète de l'arborescence i18n/
// ---------------------------------------------------------------------------
//
// Parcourt `i18n_root/{lang}/{slug}.yaml` en ordre lexicographique (INV-FORGE-1).
// Retourne la liste des langues découvertes, triée lexicographiquement.
//
// `i18n_root` : chemin vers `corpus/{rite}/i18n/`

pub fn discover_and_load_i18n(
    i18n_root: &Path,
    store:     &mut DictStore,
) -> Result<Vec<String>, ForgeError> {
    let mut lang_dirs: Vec<_> = fs::read_dir(i18n_root)
        .map_err(ForgeError::Io)?
        .filter_map(|res| res.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect();

    // Tri lexicographique — déterminisme inter-plateformes (INV-FORGE-2)
    lang_dirs.sort();

    let mut langs: Vec<String> = Vec::with_capacity(lang_dirs.len());

    for lang_path in lang_dirs {
        let lang = lang_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ParseError::MalformedYaml(
                format!("répertoire lang invalide : {}", lang_path.display())
            ))?
            .to_owned();

        // Collecte + tri des fichiers YAML dans ce répertoire de langue
        let mut yaml_files: Vec<_> = fs::read_dir(&lang_path)
            .map_err(ForgeError::Io)?
            .filter_map(|res| res.ok())
            .filter(|entry| {
                entry.path()
                    .extension()
                    .map(|ext| ext == "yaml")
                    .unwrap_or(false)
            })
            .map(|entry| entry.path())
            .collect();

        yaml_files.sort();

        for yaml_path in yaml_files {
            let slug = yaml_path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| ParseError::MalformedYaml(
                    format!("stem de fichier invalide : {}", yaml_path.display())
                ))?
                .to_owned();

            parse_dict_file(&yaml_path, &lang, &slug, store)?;
        }

        langs.push(lang);
    }

    Ok(langs)
}

// ---------------------------------------------------------------------------
// validate_i18n — V-I1, V-I2
// ---------------------------------------------------------------------------
//
// V-I1 : Pour chaque (slug, from) dans le FeastRegistry, la clé latine
//         `{from}.title` doit exister dans le DictStore.
//         Absence = erreur fatale (aucun fallback possible sans latin).
//
// V-I2 : Pour chaque (lang, slug, from, field) dans le DictStore,
//         `from` doit correspondre à un `from` déclaré dans history[] pour ce
//         slug dans le registry. Une clé sans slug connu ou avec un `from`
//         inconnu est une clé orpheline — erreur fatale.

pub fn validate_i18n(
    registry: &FeastRegistry,
    store:    &DictStore,
) -> Result<(), ForgeError> {
    // V-I1 — vérification exhaustive depuis le registry
    for feast in registry.iter() {
        for entry in &feast.history {
            if store.get("la", &feast.slug, entry.from, "title").is_none() {
                return Err(ParseError::I18nMissingLatinKey {
                    slug:  feast.slug.clone(),
                    from:  entry.from,
                    field: "title".to_owned(),
                }.into());
            }
        }
    }

    // V-I2 — vérification exhaustive depuis le store
    for (lang, slug, from, field) in store.iter_keys() {
        let feast = registry.get(slug)
            .ok_or_else(|| ParseError::I18nOrphanKey {
                slug:  slug.to_owned(),
                lang:  lang.to_owned(),
                from,
                field: field.to_owned(),
            })?;

        let known_from = feast.history.iter().any(|e| e.from == from);
        if !known_from {
            return Err(ParseError::I18nOrphanKey {
                slug:  slug.to_owned(),
                lang:  lang.to_owned(),
                from,
                field: field.to_owned(),
            }.into());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// resolve_label — fallback latin AOT
// ---------------------------------------------------------------------------
//
// Retourne la valeur résolue pour la clé `(slug, from, field, lang)`.
// Si la langue demandée n'a pas de valeur, retourne le latin correspondant.
// La garantie V-I1 rend l'`expect` irréachable à l'exécution post-validation.

pub fn resolve_label<'a>(
    slug:  &str,
    from:  u16,
    field: &str,
    lang:  &str,
    dicts: &'a DictStore,
) -> &'a str {
    dicts.get(lang, slug, from, field)
        .or_else(|| dicts.get("la", slug, from, field))
        .expect("V-I1 garantit l'existence de la clé latine pour tout (slug, from, title)")
}

// ---------------------------------------------------------------------------
// LabelTable — table plate post-résolution, prête pour l'écriture .lits
// ---------------------------------------------------------------------------
//
// Clé   : (FeastID, from, to, lang)
//          — (feast_id, from) identifie une tranche history[]
//          — `to` est conservé pour la construction de l'Entry Table .lits
//          — `lang` permet une LabelTable multi-langue unifiée
// Valeur : titre résolu (fallback latin déjà appliqué)
//
// Ordre BTreeMap sur (feast_id ASC, from ASC, to ASC, lang ASC) :
//   l'Entry Table .lits est produite en filtrant par lang, sans re-tri.

pub type LabelTable = BTreeMap<(FeastID, u16, u16, String), String>;

/// Construit la `LabelTable` à partir du registry, du store et des FeastIDs alloués.
///
/// `feast_ids` : BTreeMap<slug, FeastID> — produit par Session B après allocation.
///              Les slugs absents de cette map sont ignorés silencieusement
///              (fêtes dont l'allocation a échoué — erreur levée en amont).
/// `langs`     : slice des langues à compiler (doit inclure `"la"`).
pub fn build_label_table(
    registry:  &FeastRegistry,
    store:     &DictStore,
    feast_ids: &BTreeMap<String, FeastID>,
    langs:     &[&str],
) -> LabelTable {
    let mut table = LabelTable::new();

    for feast in registry.iter() {
        let Some(&feast_id) = feast_ids.get(&feast.slug) else { continue };

        for entry in &feast.history {
            for &lang in langs {
                let label = resolve_label(
                    &feast.slug, entry.from, "title", lang, store,
                );
                table.insert(
                    (feast_id, entry.from, entry.to, lang.to_owned()),
                    label.to_owned(),
                );
            }
        }
    }

    table
}
