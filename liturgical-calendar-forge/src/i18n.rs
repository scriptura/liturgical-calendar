//! Étape 1bis — i18n Resolution
//!
//! Responsabilités :
//!   - Ingestion des dictionnaires `i18n/{lang}/{slug}.yaml`.
//!   - Validation V-I1 (label latin obligatoire) et V-I2 (pas de clé orpheline).
//!   - Fusion AOT du fallback latin → `LabelTable` plate et autonome par langue.
//!
//! Invariants :
//!   - `DictStore` et `LabelTable` utilisent `BTreeMap` (INV-FORGE-2).
//!   - `resolve_label` retourne `&ResolvedLabel` — zéro allocation post-construction.
//!   - La `LabelTable` n'influence pas le `.kald`.

use std::collections::BTreeMap;
use std::path::Path;
use std::{fs, io};

use serde::Deserialize;

use crate::error::{ForgeError, ParseError};
use crate::registry::FeastRegistry;

pub type FeastID = u16;

// ---------------------------------------------------------------------------
// ResolvedLabel
// ---------------------------------------------------------------------------

/// Label et annotation résolus pour un couple `(slug, from)` dans une langue.
/// `annotation = None` → `annotation_offset = 0xFFFF_FFFF` dans le `.lits`.
#[derive(Debug, Clone)]
pub struct ResolvedLabel {
    pub label: String,
    pub annotation: Option<String>,
}

// ---------------------------------------------------------------------------
// DictStore
// ---------------------------------------------------------------------------
//
// Clé   : (lang, slug, from)
// Valeur : ResolvedLabel
//
// Le champ `to` des YAML i18n est parsé pour lisibilité du corpus mais non
// stocké : la `LabelTable` utilise le `to` du registre (source de vérité).

pub struct DictStore {
    entries: BTreeMap<(String, String, u16), ResolvedLabel>,
}

impl DictStore {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(
        &mut self,
        lang: &str,
        slug: &str,
        from: u16,
        label: String,
        annotation: Option<String>,
    ) {
        self.entries.insert(
            (lang.to_owned(), slug.to_owned(), from),
            ResolvedLabel { label, annotation },
        );
    }

    pub fn get(&self, lang: &str, slug: &str, from: u16) -> Option<&ResolvedLabel> {
        self.entries.get(&(lang.to_owned(), slug.to_owned(), from))
    }

    /// Itère sur toutes les clés `(lang, slug, from)` — ordre BTreeMap garanti.
    pub fn iter_keys(&self) -> impl Iterator<Item = (&str, &str, u16)> {
        self.entries
            .keys()
            .map(|(lang, slug, from)| (lang.as_str(), slug.as_str(), *from))
    }
}

impl Default for DictStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Structs de désérialisation YAML i18n
// ---------------------------------------------------------------------------
//
// Format attendu :
//   version: 1
//   history:
//     - from: 1969       # optionnel, défaut 1969
//       to:   2001       # optionnel, parsé pour lisibilité, non stocké
//       label: "..."     # obligatoire
//       annotation: "…"  # optionnel, Markdown admis

#[derive(Deserialize)]
struct YamlI18nFeast {
    version: u16,
    history: Vec<YamlI18nEntry>,
}

#[derive(Deserialize)]
struct YamlI18nEntry {
    #[serde(default)]
    from: Option<u16>,
    /// Parsé pour lisibilité corpus — non utilisé dans DictStore.
    #[serde(default)]
    #[allow(dead_code)]
    to: Option<u16>,
    label: String,
    annotation: Option<String>,
}

// ---------------------------------------------------------------------------
// parse_dict_file
// ---------------------------------------------------------------------------

pub fn parse_dict_file(
    path: &Path,
    lang: &str,
    slug: &str,
    store: &mut DictStore,
) -> Result<(), ForgeError> {
    let content = fs::read_to_string(path).map_err(|e: io::Error| ForgeError::Io(e))?;

    let feast: YamlI18nFeast = serde_yml::from_str(&content)
        .map_err(|e| ParseError::MalformedYaml(format!("{}: {}", path.display(), e)))?;

    if feast.version != 1 {
        return Err(ParseError::UnsupportedSchemaVersion(feast.version as u32).into());
    }

    for entry in feast.history {
        let from = entry.from.unwrap_or(1969);
        store.insert(lang, slug, from, entry.label, entry.annotation);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// discover_and_load_i18n
// ---------------------------------------------------------------------------
//
// Traverse la même chaîne de scopes qu'`ingest_corpus_scoped` :
//   - scope_path = None        → tout le rite (universale + tous les sous-scopes)
//   - scope_path = "universale" → universale uniquement
//   - scope_path = "nationalia/FR" → universale + nationalia/FR
//
// Pour chaque scope_root, scanne `{scope}/i18n/{lang}/{slug}.yaml`.
// Les scopes plus spécifiques écrasent les universels (last-write-wins).
//
// `rite_root` : chemin vers `corpus/{rite}/`.

pub fn discover_and_load_i18n(
    rite_root: &Path,
    scope_path: Option<&str>,
    store: &mut DictStore,
) -> Result<Vec<String>, ForgeError> {
    use std::collections::BTreeSet;

    let mut scope_roots: Vec<std::path::PathBuf> = Vec::new();

    // universale — toujours en premier (base)
    let universale = rite_root.join("universale");
    if universale.exists() {
        scope_roots.push(universale);
    }

    match scope_path {
        // scope cible = universale seul
        Some("universale") => {}

        // scope cible spécifique non-universel
        Some(scope) => {
            let scope_dir = rite_root.join(scope);
            if scope_dir.exists() {
                scope_roots.push(scope_dir);
            }
        }

        // mode batch : tout le rite
        None => {
            for level in &["continentalia", "nationalia", "dioecesana", "ordines"] {
                let dir = rite_root.join(level);
                if !dir.exists() {
                    continue;
                }
                let mut subs: Vec<_> = fs::read_dir(&dir)
                    .map_err(ForgeError::Io)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .filter(|p| !p.join("DRAFT").exists())
                    .collect();
                subs.sort();
                scope_roots.extend(subs);
            }
        }
    }

    let mut all_langs: BTreeSet<String> = BTreeSet::new();

    for scope_root in &scope_roots {
        let i18n_root = scope_root.join("i18n");
        if !i18n_root.exists() {
            continue;
        }
        let langs = scan_i18n_root(&i18n_root, store)?;
        all_langs.extend(langs);
    }

    Ok(all_langs.into_iter().collect())
}

/// Scanne un répertoire `i18n/` unique : `{lang}/{slug}.yaml`.
/// Retourne la liste des langues découvertes dans ce répertoire.
fn scan_i18n_root(i18n_root: &Path, store: &mut DictStore) -> Result<Vec<String>, ForgeError> {
    let mut lang_dirs: Vec<_> = fs::read_dir(i18n_root)
        .map_err(ForgeError::Io)?
        .filter_map(|res| res.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect();

    lang_dirs.sort();

    let mut langs: Vec<String> = Vec::with_capacity(lang_dirs.len());

    for lang_path in lang_dirs {
        let lang = lang_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                ParseError::MalformedYaml(format!(
                    "répertoire lang invalide : {}",
                    lang_path.display()
                ))
            })?
            .to_owned();

        let mut yaml_files: Vec<_> = fs::read_dir(&lang_path)
            .map_err(ForgeError::Io)?
            .filter_map(|res| res.ok())
            .filter(|entry| {
                entry
                    .path()
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
                .ok_or_else(|| {
                    ParseError::MalformedYaml(format!(
                        "stem de fichier invalide : {}",
                        yaml_path.display()
                    ))
                })?
                .to_owned();

            parse_dict_file(&yaml_path, &lang, &slug, store)?;
        }

        langs.push(lang);
    }

    Ok(langs)
}

// ---------------------------------------------------------------------------
// remap_default_from_keys
// ---------------------------------------------------------------------------

/// Remappage post-ingestion : corrige les clés `from=1969` produites par défaut
/// quand le slug démarre après 1969 et n'a qu'une seule entrée history.
pub fn remap_default_from_keys(store: &mut DictStore, registry: &FeastRegistry) {
    let to_remap: Vec<(String, String)> = store
        .iter_keys()
        .filter(|(_, _slug, from)| *from == 1969)
        .filter_map(|(lang, slug, _)| {
            let feast = registry.get(slug)?;
            if feast.history.len() == 1 && !feast.history.iter().any(|e| e.from == 1969) {
                Some((lang.to_owned(), slug.to_owned()))
            } else {
                None
            }
        })
        .collect();

    for (lang, slug) in to_remap {
        let real_from = registry.get(&slug).unwrap().history[0].from;
        if let Some(entry) = store.entries.remove(&(lang.clone(), slug.clone(), 1969)) {
            store.entries.insert((lang, slug, real_from), entry);
        }
    }
}

// ---------------------------------------------------------------------------
// propagate_labels
// ---------------------------------------------------------------------------

/// Pour chaque (lang, slug), propage le label de la tranche précédente
/// vers les tranches suivantes qui n'ont pas de label dans le store.
///
/// Permet au rédacteur de n'écrire le label qu'une fois quand il ne change pas
/// entre deux tranches history (ex: seule la precedence évolue).
pub fn propagate_labels(store: &mut DictStore, registry: &FeastRegistry) {
    for feast in registry.iter() {
        // Trier les tranches par from ASC — ordre canonique.
        let mut froms: Vec<u16> = feast.history.iter().map(|e| e.from).collect();
        froms.sort();

        // Pour chaque langue connue dans le store pour ce slug.
        let langs: Vec<String> = store
            .iter_keys()
            .filter(|(_, s, _)| *s == feast.slug.as_str())
            .map(|(l, _, _)| l.to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        for lang in &langs {
            let mut last_label: Option<String> = None;
            let mut last_annotation: Option<String> = None;

            for &from in &froms {
                match store.get(lang, &feast.slug, from) {
                    Some(resolved) => {
                        last_label = Some(resolved.label.clone());
                        last_annotation = resolved.annotation.clone();
                    }
                    None => {
                        if let Some(label) = &last_label {
                            store.insert(
                                lang,
                                &feast.slug,
                                from,
                                label.clone(),
                                last_annotation.clone(),
                            );
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// validate_label — V-I3
// ---------------------------------------------------------------------------

/// Valide un label brut extrait du DictStore.
/// Retourne `Some(reason)` si le label est invalide, `None` si OK.
///
/// Critères (par ordre de priorité) :
/// 1. Leading ou trailing whitespace — indique un copier-coller mal maîtrisé.
/// 2. Uniquement whitespace (couvre les chaînes vides après trim).
/// 3. Longueur < 3 caractères après trim — exclut les placeholders ("?", "—").
/// 4. Caractère de contrôle (U+0000–U+001F hors TAB) — corrompt le String Pool.
fn validate_label(label: &str) -> Option<&'static str> {
    if label != label.trim() {
        return Some("leading ou trailing whitespace");
    }
    if label.trim().is_empty() {
        return Some("label vide ou uniquement whitespace");
    }
    if label.trim().chars().count() < 3 {
        return Some("label trop court (< 3 caractères après trim)");
    }
    if label.chars().any(|c| c.is_control() && c != '\t') {
        return Some("caractère de contrôle interdit");
    }
    None
}

// ---------------------------------------------------------------------------
// validate_i18n — V-I1, V-I2, V-I3
// ---------------------------------------------------------------------------

pub fn validate_i18n(registry: &FeastRegistry, store: &DictStore) -> Result<(), ForgeError> {
    // V-I1 — chaque (slug, from) du registry doit avoir un label latin.
    for feast in registry.iter() {
        for entry in &feast.history {
            if store.get("la", &feast.slug, entry.from).is_none() {
                return Err(ParseError::I18nMissingLatinKey {
                    slug: feast.slug.clone(),
                    from: entry.from,
                    field: "label".to_owned(),
                }
                .into());
            }
        }
    }

    // V-I2 — chaque (lang, slug, from) du store doit correspondre à un (slug, from) du registry.
    for (lang, slug, from) in store.iter_keys() {
        let feast = registry
            .get(slug)
            .ok_or_else(|| ParseError::I18nOrphanKey {
                slug: slug.to_owned(),
                lang: lang.to_owned(),
                from,
                field: "label".to_owned(),
            })?;

        if !feast.history.iter().any(|e| e.from == from) {
            return Err(ParseError::I18nOrphanKey {
                slug: slug.to_owned(),
                lang: lang.to_owned(),
                from,
                field: "label".to_owned(),
            }
            .into());
        }
    }

    // V-I3 — chaque label présent dans le store doit être non-vide, ≥ 3 caractères,
    // sans whitespace superflu ni caractère de contrôle.
    for (lang, slug, from) in store.iter_keys() {
        let resolved = store
            .get(lang, slug, from)
            .expect("iter_keys() et get() cohérents");
        if let Some(reason) = validate_label(&resolved.label) {
            return Err(ParseError::I18nInvalidLabel {
                slug: slug.to_owned(),
                lang: lang.to_owned(),
                from,
                reason,
            }
            .into());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// resolve_label — fallback latin AOT
// ---------------------------------------------------------------------------

/// Retourne le `ResolvedLabel` pour `(slug, from, lang)`.
/// Si `lang` n'a pas d'entrée, retourne le label latin.
/// V-I1 garantit que le `expect` est irréachable post-validation.
pub fn resolve_label<'a>(
    slug: &str,
    from: u16,
    lang: &str,
    dicts: &'a DictStore,
) -> &'a ResolvedLabel {
    dicts
        .get(lang, slug, from)
        .or_else(|| dicts.get("la", slug, from))
        .expect("V-I1 garantit l'existence du label latin pour tout (slug, from)")
}

// ---------------------------------------------------------------------------
// LabelTable
// ---------------------------------------------------------------------------
//
// Clé   : (FeastID, from, to, lang)
// Valeur : ResolvedLabel (fallback latin déjà appliqué)
//
// Ordre BTreeMap (feast_id ASC, from ASC, to ASC, lang ASC) :
// l'Entry Table .lits est produite en filtrant par lang, sans re-tri.

pub type LabelTable = BTreeMap<(FeastID, u16, u16, String), ResolvedLabel>;

pub fn build_label_table(
    registry: &FeastRegistry,
    store: &DictStore,
    feast_ids: &BTreeMap<String, FeastID>,
    langs: &[&str],
) -> LabelTable {
    let mut table = LabelTable::new();

    for feast in registry.iter() {
        let Some(&feast_id) = feast_ids.get(&feast.slug) else {
            continue;
        };

        for entry in &feast.history {
            for &lang in langs {
                let resolved = resolve_label(&feast.slug, entry.from, lang, store);
                table.insert(
                    (feast_id, entry.from, entry.to, lang.to_owned()),
                    resolved.clone(),
                );
            }
        }
    }

    table
}
