use crate::error::ForgeError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Contenu du `variant_registry.lock`.
///
/// Garantit la stabilité des `variant_id` entre compilations successives :
/// un scope supprimé du corpus voit son ID tombstoné — jamais réalloué.
///
/// Clé : chemin de scope normalisé (`"romanus/universale"`, `"romanus/nationalia/FR"`).
/// Valeur : `variant_id` u16 inscrit dans `header[6..8]` du `.kald`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VariantRegistryLock {
    /// scope → variant_id actifs.
    pub entries: BTreeMap<String, u16>,
    /// scope → variant_id tombstonés (jamais réalloués).
    pub tombstones: BTreeMap<String, u16>,
}

impl VariantRegistryLock {
    /// Charge depuis le disque. Retourne `Default` si le fichier est absent (premier build).
    pub fn load(path: &Path) -> Result<Self, ForgeError> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str::<Self>(&s)
                .map_err(|e: toml::de::Error| ForgeError::LockFileMalformed(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ForgeError::Io(e)),
        }
    }

    /// Persiste sur le disque (écrasement atomique via fichier temporaire).
    pub fn save(&self, path: &Path) -> Result<(), ForgeError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e: toml::ser::Error| ForgeError::LockFileMalformed(e.to_string()))?;
        let tmp = path.with_extension("lock.tmp");
        std::fs::write(&tmp, content).map_err(ForgeError::Io)?;
        std::fs::rename(&tmp, path).map_err(ForgeError::Io)
    }

    /// Retourne le `variant_id` pré-alloué pour ce scope, ou `None` si absent.
    pub fn get(&self, scope: &str) -> Option<u16> {
        self.entries.get(scope).copied()
    }

    /// Enregistre un nouveau scope → variant_id.
    pub fn pin(&mut self, scope: &str, id: u16) {
        self.entries.insert(scope.to_string(), id);
    }

    /// Tombstone un scope retiré du corpus (ID jamais réalloué).
    pub fn tombstone(&mut self, scope: &str) {
        if let Some(id) = self.entries.remove(scope) {
            self.tombstones.insert(scope.to_string(), id);
        }
    }

    /// Retourne `true` si cet ID est tombstoné — ne peut pas être réalloué.
    pub fn is_tombstoned_id(&self, id: u16) -> bool {
        self.tombstones.values().any(|&v| v == id)
    }

    /// Alloue un `variant_id` stable pour `scope`.
    ///
    /// - Si déjà présent dans `entries` : retourne l'ID existant (idempotent).
    /// - Sinon : alloue le plus petit u16 ≥ 1 absent de `entries` et `tombstones`,
    ///   puis appelle `pin()`.
    ///
    /// Retourne `ForgeError::FeastIDExhausted { scope: 0, category: 0 }` si
    /// tous les u16 sont épuisés (cas théorique, > 65 535 scopes).
    pub fn allocate(&mut self, scope: &str) -> Result<u16, ForgeError> {
        if let Some(id) = self.get(scope) {
            return Ok(id);
        }

        let occupied: std::collections::HashSet<u16> = self
            .entries
            .values()
            .chain(self.tombstones.values())
            .copied()
            .collect();

        let id = (1u32..=u16::MAX as u32)
            .map(|n| n as u16)
            .find(|n| !occupied.contains(n))
            .ok_or(ForgeError::VariantIDExhausted)?;

        self.pin(scope, id);
        Ok(id)
    }
}
