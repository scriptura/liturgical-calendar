use std::fs;
use std::path::Path;

use crate::error::{ForgeError, ParseError};
use crate::parsing::{parse_feast_from_yaml, validate_slug};
use crate::registry::{FeastRegistry, Scope};

// ---------------------------------------------------------------------------
// ingest_scope_dir — un scope (temporale/ + sanctorale/)
// INV-FORGE-1 : collecter → trier lex → ingérer
// ---------------------------------------------------------------------------

fn ingest_scope_dir(
    scope_dir: &Path,
    scope: Scope,
    registry: &mut FeastRegistry,
    is_base: bool, // true = universale, false = delta (merge)
) -> Result<(), ForgeError> {
    for sub in &["temporale", "sanctorale"] {
        let dir = scope_dir.join(sub);
        if !dir.exists() {
            continue;
        }

        // Collecte récursive — absorbe les sous-répertoires mensuels
        // (sanctorale/01/, sanctorale/03/…) sans changer la logique de tri.
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        collect_yaml_recursive(&dir, &mut files)?;
        files.sort(); // INV-FORGE-1 : ordre lexicographique global

        for path in files {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            validate_slug(&stem).map_err(ForgeError::Parse)?;

            let content = fs::read_to_string(&path)?;

            if content.trim().is_empty() {
                continue;
            }

            let def = parse_feast_from_yaml(&stem, scope.clone(), &content)?;
            if is_base {
                registry.insert(def);
            } else {
                registry.merge(def);
            }
        }
    }
    Ok(())
}

fn collect_yaml_recursive(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), ForgeError> {
    for entry in fs::read_dir(dir).map_err(ForgeError::Io)? {
        let entry = entry.map_err(ForgeError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            collect_yaml_recursive(&path, out)?;
        } else if matches!(
            path.extension().and_then(|x| x.to_str()),
            Some("yaml") | Some("yml")
        ) {
            out.push(path);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ingest_corpus — point d'entrée public
// ---------------------------------------------------------------------------

pub fn ingest_corpus(data_dir: &Path) -> Result<FeastRegistry, ForgeError> {
    let mut registry = FeastRegistry::new();

    // Universale
    let universale = data_dir.join("universale");
    if universale.exists() {
        ingest_scope_dir(&universale, Scope::Universal, &mut registry, true)?;
    }

    // Nationalia
    let nationalia = data_dir.join("nationalia");
    if nationalia.exists() {
        let iso_dirs = sorted_subdirs(&nationalia)?;
        for iso_path in iso_dirs {
            let iso = dir_name(&iso_path);
            ingest_scope_dir(&iso_path, Scope::National(iso), &mut registry, false)?;
        }
    }

    // Continentalia — traité comme National avec code continent
    let continentalia = data_dir.join("continentalia");
    if continentalia.exists() {
        let cont_dirs = sorted_subdirs(&continentalia)?;
        for cont_path in cont_dirs {
            let cont = dir_name(&cont_path);
            ingest_scope_dir(&cont_path, Scope::National(cont), &mut registry, false)?;
        }
    }

    // Dioecesana
    let dioecesana = data_dir.join("dioecesana");
    if dioecesana.exists() {
        let id_dirs = sorted_subdirs(&dioecesana)?;
        for id_path in id_dirs {
            let id = dir_name(&id_path);
            ingest_scope_dir(&id_path, Scope::Diocesan(id), &mut registry, false)?;
        }
    }

    // Ordines — traité comme Diocesan
    let ordines = data_dir.join("ordines");
    if ordines.exists() {
        let ordo_dirs = sorted_subdirs(&ordines)?;
        for ordo_path in ordo_dirs {
            // Ignorer i18n/ qui n'est pas un scope liturgique
            if dir_name(&ordo_path) == "i18n" {
                continue;
            }
            let ordo = dir_name(&ordo_path);
            ingest_scope_dir(&ordo_path, Scope::Diocesan(ordo), &mut registry, false)?;
        }
    }

    validate_collides_targets(&registry)?;

    Ok(registry)
}

/// Variante scoped d'`ingest_corpus` — ingère uniquement la chaîne de strates
/// nécessaire pour compiler le `scope_path` cible.
///
/// Règle d'ingestion :
/// - `universale` → universale uniquement.
/// - `continentalia/{ID}` → universale + continentalia/{ID}.
/// - `nationalia/{ISO}` → universale + nationalia/{ISO}.
/// - `dioecesana/{ID}` → universale + dioecesana/{ID}.
/// - `ordines/{ORDO}` → universale + ordines/{ORDO}.
///
/// Les strates intermédiaires (ex: continentalia quand scope = nationalia/FR)
/// ne sont **pas** incluses automatiquement — chaque scope est autonome.
/// Si une surcharge de continentalia est nécessaire, elle doit être intégrée
/// via un scope dédié.
///
/// `rite_root` : chemin vers `corpus/{rite}/`.
/// `scope_path` : chemin relatif du scope, ex: `"universale"`, `"nationalia/FR"`.
pub fn ingest_corpus_scoped(
    rite_root: &Path,
    scope_path: &str,
) -> Result<FeastRegistry, ForgeError> {
    let mut registry = FeastRegistry::new();

    // Toujours ingérer universale en premier (base).
    let universale = rite_root.join("universale");
    if universale.exists() {
        ingest_scope_dir(&universale, Scope::Universal, &mut registry, true)?;
    }

    // Si le scope cible est universale, on s'arrête ici.
    if scope_path == "universale" {
        validate_collides_targets(&registry)?;
        return Ok(registry);
    }

    // Résolution du scope cible : "level/id" → rite_root/level/id
    let scope_dir = rite_root.join(scope_path);
    if !scope_dir.exists() {
        return Err(ForgeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("scope introuvable : {}", scope_dir.display()),
        )));
    }

    // Déduction du Scope Rust depuis le chemin.
    let scope_variant = scope_from_path(scope_path);
    ingest_scope_dir(&scope_dir, scope_variant, &mut registry, false)?;

    validate_collides_targets(&registry)?;
    Ok(registry)
}

/// Déduit le variant `Scope` depuis un chemin de scope relatif.
/// Ex: `"nationalia/FR"` → `Scope::National("FR")`.
fn scope_from_path(scope_path: &str) -> Scope {
    let parts: Vec<&str> = scope_path.splitn(2, '/').collect();
    match parts.as_slice() {
        [level, id] => match *level {
            "nationalia" | "continentalia" => Scope::National(id.to_string()),
            "dioecesana" | "ordines" => Scope::Diocesan(id.to_string()),
            _ => Scope::Universal,
        },
        _ => Scope::Universal,
    }
}

// Helpers extraits pour éviter la répétition
fn sorted_subdirs(dir: &Path) -> Result<Vec<std::path::PathBuf>, ForgeError> {
    let mut dirs: Vec<_> = fs::read_dir(dir)
        .map_err(ForgeError::Io)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

// ---------------------------------------------------------------------------
// V-T2 — vérification post-ingestion
// ---------------------------------------------------------------------------

fn validate_collides_targets(registry: &FeastRegistry) -> Result<(), ForgeError> {
    for feast in registry.iter() {
        for entry in &feast.history {
            for transfer in &entry.transfers {
                for c in &transfer.collides {
                    if !registry.contains(c.as_str()) {
                        return Err(ParseError::UnknownCollidesTarget {
                            slug: feast.slug.clone(),
                            collides: c.clone(),
                        }
                        .into());
                    }
                }
            }
        }
    }
    Ok(())
}
