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
//! [ Header     :  32 octets              ]
//! [ Entry Table: entry_count × 14 octets ]
//! [ String Pool: pool_size octets        ]
//! ```
//!
//! ## Entry Table (14 octets / entrée)
//!
//! | Offset | Champ             | Type   |
//! |--------|-------------------|--------|
//! |  0.. 2 | feast_id          | u16 LE |
//! |  2.. 4 | from              | u16 LE |
//! |  4.. 6 | to                | u16 LE |
//! |  6..10 | label_offset      | u32 LE |
//! | 10..14 | annotation_offset | u32 LE | 0xFFFF_FFFF si absent
//!
//! Tous les entiers sont Little-Endian.

/// Sentinelle inscrite par la Forge quand aucune annotation n'est définie.
const ANNOTATION_ABSENT: u32 = u32::MAX;

/// Stride d'une entrée Entry Table en octets.
const ENTRY_STRIDE: usize = 14;

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

/// Label et annotation retournés par `LitsProvider::get`.
///
/// `annotation` est `None` si la Forge a inscrit `ANNOTATION_ABSENT`
/// dans `annotation_offset` — l'Engine ne doit pas déréférencer dans ce cas.
#[derive(Debug, PartialEq, Eq)]
pub struct LitsEntry<'a> {
    /// Titre officiel de la fête pour la langue et l'année demandées.
    pub label: &'a str,
    /// Précision liturgique ou titre alternatif — `None` si absent du corpus.
    pub annotation: Option<&'a str>,
}

/// Projecteur de mémoire sur un buffer `.lits` fourni par l'appelant.
///
/// Zéro allocation. Zéro copie. Zéro état interne.
/// Le buffer doit rester valide pour toute la durée de vie `'a`.
pub struct LitsProvider<'a> {
    data: &'a [u8],
    entry_count: u32,
    pool_offset: u32,
    pool_size: u32,
}

impl<'a> LitsProvider<'a> {
    /// Construit le projecteur depuis un buffer brut.
    ///
    /// Valide : magic, version, cohérence `pool_offset` + `pool_size` vs `data.len()`.
    /// Ne valide pas le SHA-256 — responsabilité du client (§9.4 spec).
    pub fn new(data: &'a [u8]) -> Result<Self, LitsError> {
        if data.len() < 32 {
            return Err(LitsError::BufferTooShort);
        }

        if &data[0..4] != b"LITS" {
            return Err(LitsError::InvalidMagic);
        }

        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != 1 {
            return Err(LitsError::UnsupportedVersion(version));
        }

        let entry_count = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        let pool_offset = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        let pool_size = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);

        // pool_offset == 32 + entry_count × 14 (invariant spec §9.2)
        let expected_pool_offset: u64 = 32u64 + (entry_count as u64) * (ENTRY_STRIDE as u64);
        let file_end: u64 = (pool_offset as u64) + (pool_size as u64);

        if (pool_offset as u64) != expected_pool_offset || file_end != data.len() as u64 {
            return Err(LitsError::CorruptLayout);
        }

        Ok(Self {
            data,
            entry_count,
            pool_offset,
            pool_size,
        })
    }

    /// Retourne `kald_build_id` (bytes 12–19 du header).
    ///
    /// À comparer avec `kald_header.checksum[..8]` avant tout accès conjoint
    /// `.kald` + `.lits` (§9.4 spec).
    #[inline]
    pub fn build_id(&self) -> &[u8] {
        &self.data[12..20]
    }

    /// Retourne le `LitsEntry` pour `(feast_id, year)`, ou `None` si absent.
    ///
    /// Algorithme : recherche binaire sur `feast_id` → scan linéaire des tranches
    /// `[from, to]` pour trouver celle couvrant `year`.
    ///
    /// Complexité : O(log N + K), N = `entry_count`, K ≤ 10 (tranches par fête).
    pub fn get(&self, feast_id: u16, year: u16) -> Option<LitsEntry<'a>> {
        if self.entry_count == 0 {
            return None;
        }

        let n = self.entry_count as usize;

        // ── Recherche binaire sur feast_id ────────────────────────────────────
        let first = {
            let mut lo: usize = 0;
            let mut hi: usize = n;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let fid = self.read_feast_id(mid);
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

        // ── Scan linéaire des tranches [from, to] ────────────────────────────
        let mut idx = first;
        while idx < n {
            if self.read_feast_id(idx) != feast_id {
                break;
            }

            let from = self.read_from(idx);
            let to = self.read_to(idx);

            if year >= from && year <= to {
                let label_offset = self.read_label_offset(idx);
                let annotation_offset = self.read_annotation_offset(idx);

                let label = self.read_string(label_offset)?;
                let annotation = if annotation_offset == ANNOTATION_ABSENT {
                    None
                } else {
                    self.read_string(annotation_offset)
                };

                return Some(LitsEntry { label, annotation });
            }

            idx += 1;
        }

        None
    }

    // ── Accesseurs Entry Table (LE) ───────────────────────────────────────────

    #[inline]
    fn entry_base(&self, idx: usize) -> usize {
        32 + idx * ENTRY_STRIDE
    }

    #[inline]
    fn read_feast_id(&self, idx: usize) -> u16 {
        let b = self.entry_base(idx);
        u16::from_le_bytes([self.data[b], self.data[b + 1]])
    }

    #[inline]
    fn read_from(&self, idx: usize) -> u16 {
        let b = self.entry_base(idx) + 2;
        u16::from_le_bytes([self.data[b], self.data[b + 1]])
    }

    #[inline]
    fn read_to(&self, idx: usize) -> u16 {
        let b = self.entry_base(idx) + 4;
        u16::from_le_bytes([self.data[b], self.data[b + 1]])
    }

    #[inline]
    fn read_label_offset(&self, idx: usize) -> u32 {
        let b = self.entry_base(idx) + 6;
        u32::from_le_bytes([
            self.data[b],
            self.data[b + 1],
            self.data[b + 2],
            self.data[b + 3],
        ])
    }

    #[inline]
    fn read_annotation_offset(&self, idx: usize) -> u32 {
        let b = self.entry_base(idx) + 10;
        u32::from_le_bytes([
            self.data[b],
            self.data[b + 1],
            self.data[b + 2],
            self.data[b + 3],
        ])
    }

    /// Lit une chaîne UTF-8 null-terminée depuis le String Pool.
    /// `str_offset` est relatif au début du pool.
    #[inline]
    fn read_string(&self, str_offset: u32) -> Option<&'a str> {
        let pool_start = self.pool_offset as usize;
        let pool_end = pool_start + self.pool_size as usize;
        let abs_start = pool_start + str_offset as usize;

        if abs_start >= pool_end {
            return None;
        }

        let slice = &self.data[abs_start..pool_end];
        let len = slice.iter().position(|&b| b == 0x00)?;

        core::str::from_utf8(&slice[..len]).ok()
    }
}
