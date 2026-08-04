use std::path::{Path, PathBuf};

use clap::Parser;
use liturgical_calendar_forge::{
    I18nConfig, compile, ingestion::ingest_corpus_scoped, variant_lock::VariantRegistryLock,
};

// ── Arguments CLI ─────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "kal-forge",
    version,
    about = "Compile un scope liturgique YAML → .kald (+ .lits optionnel)"
)]
struct Args {
    /// Rite à compiler.
    #[arg(short, long, default_value = "romanus")]
    rite: String,

    /// Scope à compiler. Si absent, compile tous les scopes non-DRAFT du rite.
    #[arg(short, long)]
    scope: Option<String>,

    /// Racine du corpus YAML.
    #[arg(short, long, default_value = "./corpus")]
    corpus: PathBuf,

    /// Répertoire de sortie des artefacts.
    #[arg(short, long, default_value = "./artifacts")]
    out: PathBuf,

    /// Inclure les scopes marqués DRAFT (ignorés par défaut).
    #[arg(short = 'd', long, default_value_t = false)]
    include_drafts: bool,

    /// Produit un .lits par langue découverte dans la hiérarchie i18n du rite.
    #[arg(short, long, default_value_t = false)]
    i18n: bool,
}

// ── Point d'entrée ────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    if let Err(e) = run(&args) {
        eprintln!("[kal-forge] erreur : {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), liturgical_calendar_forge::ForgeError> {
    let rite_root = args.corpus.join(&args.rite);

    std::fs::create_dir_all(&args.out).map_err(liturgical_calendar_forge::ForgeError::Io)?;

    match &args.scope {
        Some(scope) => compile_scope(args, &rite_root, scope),
        None => {
            let scopes = discover_scopes(&args.corpus, &args.rite, args.include_drafts)
                .map_err(liturgical_calendar_forge::ForgeError::Io)?;
            if scopes.is_empty() {
                eprintln!(
                    "[kal-forge] aucun scope trouvé dans {}",
                    rite_root.display()
                );
                return Ok(());
            }
            for scope_key in &scopes {
                // scope_key = "romanus/universale" → extraire la partie après le rite
                let scope = scope_key
                    .strip_prefix(&format!("{}/", args.rite))
                    .unwrap_or(scope_key);
                if let Err(e) = compile_scope(args, &rite_root, scope) {
                    eprintln!("[kal-forge] erreur sur {scope_key} : {e}");
                }
            }
            Ok(())
        }
    }
}

fn compile_scope(
    args: &Args,
    rite_root: &Path,
    scope: &str,
) -> Result<(), liturgical_calendar_forge::ForgeError> {
    // ── Résolution des chemins ────────────────────────────────────────────────
    let corpus_root = rite_root.join(scope);

    if !corpus_root.exists() {
        eprintln!("[kal-forge] scope introuvable : {}", corpus_root.display());
        std::process::exit(1);
    }

    // ── Détection sentinelle DRAFT ────────────────────────────────────────────
    if !args.include_drafts && corpus_root.join("DRAFT").exists() {
        eprintln!(
            "[kal-forge] scope ignoré (DRAFT) : {}/{}  — utilisez -d pour forcer",
            args.rite, scope
        );
        return Ok(());
    }

    // ── Chargement du variant_registry.lock ───────────────────────────────────
    let lock_path = rite_root.join("variant_registry.lock");
    let mut variant_lock = VariantRegistryLock::load(&lock_path)?;

    let scope_key = format!("{}/{}", args.rite, scope);
    let variant_id = variant_lock.allocate(&scope_key)?;
    variant_lock.save(&lock_path)?;

    eprintln!("[kal-forge] scope={scope_key}  variant_id={variant_id:#06x}");

    // ── Ingest corpus ─────────────────────────────────────────────────────────
    let registry = ingest_corpus_scoped(rite_root, scope)?;
    eprintln!("[kal-forge] {} fêtes chargées", registry.len());

    // ── Résolution chemin de sortie ───────────────────────────────────────────
    let flat_name = scope_key.replace('/', "_");
    let kald_path = args.out.join(format!("{flat_name}.kald"));

    // ── i18n config ───────────────────────────────────────────────────────────
    let i18n_config = if args.i18n {
        Some(I18nConfig {
            i18n_root: rite_root,
            scope_path: Some(scope),
            lits_dir: args.out.as_path(),
        })
    } else {
        None
    };

    // ── Compilation ───────────────────────────────────────────────────────────
    let checksum = compile(registry, &kald_path, variant_id, i18n_config, &lock_path)?;

    let build_id = u64::from_le_bytes(checksum[..8].try_into().unwrap());
    eprintln!("[kal-forge] ✓  {}", kald_path.display());
    eprintln!("[kal-forge]    build_id = {build_id:#018x}");

    // ── Renommage des .lits : {lang}.lits → {flat_name}_{lang}.lits ──────────
    if args.i18n {
        for entry in
            std::fs::read_dir(&args.out).map_err(liturgical_calendar_forge::ForgeError::Io)?
        {
            let entry = entry.map_err(liturgical_calendar_forge::ForgeError::Io)?;
            let path = entry.path();
            if path.extension().map(|e| e == "lits").unwrap_or(false) {
                let lang = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                if !lang.contains('_') {
                    let new_name = format!("{flat_name}_{lang}.lits");
                    let new_path = args.out.join(&new_name);
                    std::fs::rename(&path, &new_path)
                        .map_err(liturgical_calendar_forge::ForgeError::Io)?;
                    eprintln!("[kal-forge] ✓  {}", new_path.display());
                }
            }
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Scanne `corpus/<rite>/` et retourne les scope_keys compilables.
///
/// Couvre deux niveaux :
/// - Scopes plats : `universale` → `"romanus/universale"`
/// - Scopes imbriqués : `nationalia/FR` → `"romanus/nationalia/FR"`
///
/// Exclut les scopes marqués `DRAFT` sauf si `include_drafts = true`.
/// Résultat trié lexicographiquement — déterminisme de compilation.
fn discover_scopes(
    corpus: &Path,
    rite: &str,
    include_drafts: bool,
) -> std::io::Result<Vec<String>> {
    let rite_dir = corpus.join(rite);
    let mut scopes = Vec::new();

    for entry in std::fs::read_dir(&rite_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };

        // Exclure les fichiers de métadonnées
        if name.ends_with(".lock") {
            continue;
        }

        if path.join("DRAFT").exists() && !include_drafts {
            continue;
        }

        // Vérifier si ce répertoire contient directement des données corpus
        // (sanctorale/ ou temporale/) — scope plat.
        let has_corpus = path.join("sanctorale").exists() || path.join("temporale").exists();

        if has_corpus {
            scopes.push(format!("{rite}/{name}"));
        } else {
            // Scope conteneur (ex: nationalia/, continentalia/) — descendre d'un niveau.
            for sub in std::fs::read_dir(&path)? {
                let sub = sub?;
                let spath = sub.path();
                if !spath.is_dir() {
                    continue;
                }
                if spath.join("DRAFT").exists() && !include_drafts {
                    continue;
                }
                let subname = match spath.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_owned(),
                    None => continue,
                };
                scopes.push(format!("{rite}/{name}/{subname}"));
            }
        }
    }

    scopes.sort();
    Ok(scopes)
}
