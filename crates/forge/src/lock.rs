use crate::error::ForgeError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Contenu du `feast_registry.lock`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeastRegistryLock {
    /// slug → FeastID alloué (actifs)
    pub entries: BTreeMap<String, u16>,
    /// slug → FeastID tombstoné (jamais réalloués)
    pub tombstones: BTreeMap<String, u16>,
}

impl FeastRegistryLock {
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

    /// Retourne le FeastID pré-alloué pour ce slug, ou `None` si absent.
    pub fn get(&self, slug: &str) -> Option<u16> {
        self.entries.get(slug).copied()
    }

    /// Enregistre un nouveau slug → FeastID.
    pub fn pin(&mut self, slug: &str, id: u16) {
        self.entries.insert(slug.to_string(), id);
    }

    /// Tombstone un slug retiré du corpus (ID jamais réalloué).
    pub fn tombstone(&mut self, slug: &str) {
        if let Some(id) = self.entries.remove(slug) {
            self.tombstones.insert(slug.to_string(), id);
        }
    }

    /// Retourne `true` si cet ID est tombstoné — ne peut pas être réalloué.
    pub fn is_tombstoned_id(&self, id: u16) -> bool {
        self.tombstones.values().any(|&v| v == id)
    }
}
