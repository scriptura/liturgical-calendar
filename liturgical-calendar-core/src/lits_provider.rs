//! `LitsProvider` — Projecteur zero-copy sur un buffer `.lits`.
//!
//! Ce module appartient au crate `liturgical_calendar_core`.
//! Contraintes : `no_std`, `no_alloc` — aucune allocation, aucune copie.
//!
//! # Protocole d'accès
//!
//! 1. Le client charge le buffer `.lits` (responsabilité client).
//! 2. Le client vérifie la cohérence `.kald` / `.lits` via `build_id()` (§9.4 spec).
//! 3. `LitsProvider::get(feast_id, year)` : O(log N + K).
//!
//! # Layout binaire attendu
//!
//! ```text
//! [ Header   :  32 octets              ]
//! [ Entry Table : entry_count × 10 B  ]
//! [ String Pool : pool_size octets     ]
//! ```
//!
//! Tous les entiers sont Little-Endian.

/// Erreurs de construction du projecteur.
#[derive(Debug, PartialEq, Eq)]
pub enum LitsError {
    /// Buffer trop court pour contenir un header valide (minimum 32 octets).
    BufferTooShort,
    /// Magic bytes invalides — attendu `b"LITS"`.
    InvalidMagic,
    /// Version non supportée — attendu 1.
    UnsupportedVersion(u16),
    /// `pool_offset` ou `pool_size` incohérent avec la taille du buffer.
    CorruptLayout,
}

/// Projecteur de mémoire sur un buffer `.lits` fourni par l'appelant.
///
/// Zéro allocation. Zéro copie. Zéro état interne.
/// Le buffer doit rester valide pour toute la durée de vie `'a`.
pub struct LitsProvider<'a> {
    data: &'a [u8],
    /// Nombre d'entrées dans l'Entry Table (lu depuis le header).
    entry_count: u32,
    /// Offset absolu du début du String Pool dans `data`.
    pool_offset: u32,
    /// Taille du String Pool en octets.
    pool_size: u32,
}

impl<'a> LitsProvider<'a> {
    /// Construit le projecteur depuis un buffer brut.
    ///
    /// Valide : magic, version, cohérence `pool_offset` + `pool_size` vs `data.len()`.
    /// Ne valide pas le SHA-256 — responsabilité du client (§9.4 spec, INV-W5).
    pub fn new(data: &'a [u8]) -> Result<Self, LitsError> {
        if data.len() < 32 {
            return Err(LitsError::BufferTooShort);
        }

        // magic [0..4]
        if &data[0..4] != b"LITS" {
            return Err(LitsError::InvalidMagic);
        }

        // version [4..6]
        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != 1 {
            return Err(LitsError::UnsupportedVersion(version));
        }

        // entry_count [20..24]
        let entry_count = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);

        // pool_offset [24..28]
        let pool_offset = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);

        // pool_size [28..32]
        let pool_size = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);

        // Cohérence du layout
        // pool_offset == 32 + entry_count * 10 (invariant spec §9.2)
        let expected_pool_offset: u64 = 32u64 + (entry_count as u64) * 10;
        let file_end: u64 = (pool_offset as u64) + (pool_size as u64);

        if (pool_offset as u64) != expected_pool_offset
            || file_end != data.len() as u64
        {
            return Err(LitsError::CorruptLayout);
        }

        Ok(Self { data, entry_count, pool_offset, pool_size })
    }

    /// Retourne `kald_build_id` (bytes 12–19 du header).
    ///
    /// À comparer avec `kald_header.checksum[..8]` avant tout accès conjoint
    /// `.kald` + `.lits` (§9.4 spec).
    #[inline]
    pub fn build_id(&self) -> &[u8] {
        &self.data[12..20]
    }

    /// Retourne le label (titre) pour `(feast_id, year)`.
    ///
    /// Algorithme : recherche binaire sur `feast_id` → scan linéaire
    /// des tranches `[from, to]` pour trouver la tranche couvrant `year`.
    ///
    /// Retourne `None` si aucune entrée ne couvre `(feast_id, year)`.
    /// Ce n'est pas une erreur — le client gère l'affichage (fallback, "?", ID brut).
    ///
    /// Complexité : O(log N + K), N = `entry_count`, K ≤ 10 (tranches par fête).
    pub fn get(&self, feast_id: u16, year: u16) -> Option<&'a str> {
        if self.entry_count == 0 {
            return None;
        }

        // L'Entry Table commence à l'offset 32 dans le buffer.
        // Chaque entrée : feast_id(u16) + from(u16) + to(u16) + str_offset(u32) = 10 B.
        let table_base: usize = 32;

        // ── Recherche binaire sur feast_id ────────────────────────────────────
        // Trouve l'index de la première entrée avec feast_id == feast_id cible.
        // Les entrées sont triées par (feast_id ASC, from ASC).

        let n = self.entry_count as usize;

        let first = {
            let mut lo: usize = 0;
            let mut hi: usize = n;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let fid = self.read_entry_feast_id(table_base, mid);
                if fid < feast_id {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            lo
        };

        if first >= n {
            return None;
        }

        // ── Scan linéaire des tranches [from, to] pour ce feast_id ────────────
        let mut idx = first;
        while idx < n {
            let fid = self.read_entry_feast_id(table_base, idx);
            if fid != feast_id {
                break; // plus d'entrées pour ce feast_id
            }

            let from = self.read_entry_from(table_base, idx);
            let to   = self.read_entry_to(table_base, idx);

            if year >= from && year <= to {
                // Tranche trouvée — lecture de la chaîne dans le String Pool
                let str_offset = self.read_entry_str_offset(table_base, idx);
                return self.read_string(str_offset);
            }

            idx += 1;
        }

        None
    }

    // ── Accesseurs internes (LE, no bounds-check en release) ─────────────────

    #[inline]
    fn entry_base(&self, table_base: usize, idx: usize) -> usize {
        table_base + idx * 10
    }

    #[inline]
    fn read_entry_feast_id(&self, table_base: usize, idx: usize) -> u16 {
        let b = self.entry_base(table_base, idx);
        u16::from_le_bytes([self.data[b], self.data[b + 1]])
    }

    #[inline]
    fn read_entry_from(&self, table_base: usize, idx: usize) -> u16 {
        let b = self.entry_base(table_base, idx) + 2;
        u16::from_le_bytes([self.data[b], self.data[b + 1]])
    }

    #[inline]
    fn read_entry_to(&self, table_base: usize, idx: usize) -> u16 {
        let b = self.entry_base(table_base, idx) + 4;
        u16::from_le_bytes([self.data[b], self.data[b + 1]])
    }

    #[inline]
    fn read_entry_str_offset(&self, table_base: usize, idx: usize) -> u32 {
        let b = self.entry_base(table_base, idx) + 6;
        u32::from_le_bytes([self.data[b], self.data[b+1], self.data[b+2], self.data[b+3]])
    }

    /// Lit une chaîne UTF-8 null-terminée depuis le String Pool.
    /// `str_offset` = offset depuis le début du pool (pas du fichier).
    #[inline]
    fn read_string(&self, str_offset: u32) -> Option<&'a str> {
        let pool_start = self.pool_offset as usize;
        let pool_end   = pool_start + self.pool_size as usize;
        let abs_start  = pool_start + str_offset as usize;

        if abs_start >= pool_end {
            return None;
        }

        // Recherche du null-terminator
        let slice = &self.data[abs_start..pool_end];
        let len   = slice.iter().position(|&b| b == 0x00)?;

        // SAFETY : le contenu a été produit par la Forge avec des chaînes UTF-8 valides.
        // En l'absence de `unsafe`, on utilise `from_utf8` avec conversion gracieuse.
        core::str::from_utf8(&slice[..len]).ok()
    }
}
