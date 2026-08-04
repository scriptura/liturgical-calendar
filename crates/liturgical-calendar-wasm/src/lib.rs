#![no_std]

use core::ptr::{addr_of, addr_of_mut};

use liturgical_calendar_core::{
    ffi::{KAL_ENGINE_OK, kal_read_entry, kal_read_feast, kal_read_secondary, kal_validate_header},
    lits_provider::LitsProvider,
};

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// ── Capacités ─────────────────────────────────────────────────────────────────
// .kald v6 : 80 octets header + Registry + Timeline + Pool.
// 431 ans × 366 slots × 8 octets ≈ 1.26 MB Timeline.
// Registry : ~4000 fêtes × 4 octets ≈ 16 KB.

const KALD_CAP: usize = 2 * 1024 * 1024;
const LITS_CAP: usize = 512 * 1024;
const SECONDARY_CAP: usize = 32;

// ── Buffers BSS ───────────────────────────────────────────────────────────────

static mut KALD_BUF: [u8; KALD_CAP] = [0u8; KALD_CAP];
static mut LITS_BUF: [u8; LITS_CAP] = [0u8; LITS_CAP];
static mut ENTRY_BUF: [u8; 8] = [0u8; 8]; // TimelineEntry v6
static mut FEAST_BUF: [u8; 4] = [0u8; 4]; // FeastEntry
static mut SECONDARY_BUF: [u16; SECONDARY_CAP] = [0u16; SECONDARY_CAP];

static mut KALD_LEN: u32 = 0;
static mut LITS_LEN: u32 = 0;
static mut BRIDGE_STATE: u8 = 0; // 0=Uninit 1=KaldLoaded 2=Ready

// ── Out-statics label + annotation ────────────────────────────────────────────

static mut LABEL_PTR: u32 = 0;
static mut LABEL_LEN: u32 = 0;
static mut ANNOTATION_PTR: u32 = 0; // 0 si annotation absente
static mut ANNOTATION_LEN: u32 = 0;

// ── Codes d'erreur bridge ─────────────────────────────────────────────────────

pub const KAL_ERR_NOT_READY: i32 = -20;
pub const KAL_ERR_BUF_OVERFLOW: i32 = -21;
pub const KAL_ERR_BUILD_ID_MISMATCH: i32 = -22;
pub const KAL_ERR_LITS_INVALID: i32 = -23;

// ── Helper interne ────────────────────────────────────────────────────────────

unsafe fn resolve_by_id(feast_id: u16, year: u16) -> i32 {
    let lits_slice: &'static [u8] = unsafe {
        core::slice::from_raw_parts(
            addr_of!(LITS_BUF) as *const u8,
            *addr_of!(LITS_LEN) as usize,
        )
    };
    let provider = match LitsProvider::new(lits_slice) {
        Ok(p) => p,
        Err(_) => return KAL_ERR_LITS_INVALID,
    };
    unsafe {
        match provider.get(feast_id, year) {
            Some(entry) => {
                let lb = entry.label.as_bytes();
                *addr_of_mut!(LABEL_PTR) = lb.as_ptr() as u32;
                *addr_of_mut!(LABEL_LEN) = lb.len() as u32;
                match entry.annotation {
                    Some(ann) => {
                        let ab = ann.as_bytes();
                        *addr_of_mut!(ANNOTATION_PTR) = ab.as_ptr() as u32;
                        *addr_of_mut!(ANNOTATION_LEN) = ab.len() as u32;
                    }
                    None => {
                        *addr_of_mut!(ANNOTATION_PTR) = 0;
                        *addr_of_mut!(ANNOTATION_LEN) = 0;
                    }
                }
                1
            }
            None => {
                *addr_of_mut!(ANNOTATION_PTR) = 0;
                *addr_of_mut!(ANNOTATION_LEN) = 0;
                0
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Phase 1 — allocation
// ═════════════════════════════════════════════════════════════════════════════

#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_alloc_kald(len: u32) -> u32 {
    if len as usize > KALD_CAP {
        return 0;
    }
    unsafe {
        *addr_of_mut!(KALD_LEN) = len;
        addr_of!(KALD_BUF) as u32
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_alloc_lits(len: u32) -> u32 {
    if len as usize > LITS_CAP {
        return 0;
    }
    unsafe {
        *addr_of_mut!(LITS_LEN) = len;
        addr_of!(LITS_BUF) as u32
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Phase 2 — commit
// ═════════════════════════════════════════════════════════════════════════════

/// Valide le header `.kald` v6 (magic, version, taille, discriminant, SHA-256).
/// Passe l'état à `KaldLoaded` si OK.
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_commit_kald() -> i32 {
    unsafe {
        let rc = kal_validate_header(
            addr_of!(KALD_BUF) as *const u8,
            *addr_of!(KALD_LEN) as usize,
            core::ptr::null_mut(),
        );
        if rc == KAL_ENGINE_OK {
            *addr_of_mut!(BRIDGE_STATE) = 1;
        }
        rc
    }
}

/// Vérifie le `build_id` et valide le header `.lits`.
/// Passe l'état à `Ready` si OK.
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_commit_lits() -> i32 {
    unsafe {
        if *addr_of!(BRIDGE_STATE) < 1 {
            return KAL_ERR_NOT_READY;
        }
        if *addr_of!(LITS_LEN) < 20 {
            return KAL_ERR_LITS_INVALID;
        }

        let kald_cs_ptr = (addr_of!(KALD_BUF) as *const u8).add(36);
        let lits_bi_ptr = (addr_of!(LITS_BUF) as *const u8).add(12);
        if core::slice::from_raw_parts(kald_cs_ptr, 8)
            != core::slice::from_raw_parts(lits_bi_ptr, 8)
        {
            return KAL_ERR_BUILD_ID_MISMATCH;
        }

        let lits_slice = core::slice::from_raw_parts(
            addr_of!(LITS_BUF) as *const u8,
            *addr_of!(LITS_LEN) as usize,
        );
        match LitsProvider::new(lits_slice) {
            Ok(_) => {
                *addr_of_mut!(BRIDGE_STATE) = 2;
                KAL_ENGINE_OK
            }
            Err(_) => KAL_ERR_LITS_INVALID,
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Lecture Timeline — pattern out-static (ENTRY_BUF)
// ═════════════════════════════════════════════════════════════════════════════

/// Lit la `TimelineEntry` pour `(year, doy)` dans `ENTRY_BUF`.
///
/// Layout v6 (8 octets, LE) :
///   [0..2] primary_index    u16  — 0 = Padding Entry
///   [2..4] secondary_offset u16
///   [4]    occurrence_flags u8   — bits [1:0] vespers/vigil, [4:2] LiturgicalPeriod
///   [5]    secondary_count  u8
///   [6]    liturgical_week  u8   — 0 = N/A, 1–34 = semaine active
///   [7]    _reserved        u8   — nul
///
/// v6 : les Padding Entries portent `occurrence_flags[4:2]` et `liturgical_week`.
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_read_day(year: u16, doy: u16) -> i32 {
    unsafe {
        if *addr_of!(BRIDGE_STATE) != 2 {
            return KAL_ERR_NOT_READY;
        }
        kal_read_entry(
            addr_of!(KALD_BUF) as *const u8,
            *addr_of!(KALD_LEN) as usize,
            year,
            doy,
            addr_of_mut!(ENTRY_BUF) as *mut _,
        )
    }
}

/// Pointeur vers `ENTRY_BUF` (8 octets, `TimelineEntry` v6).
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_entry_ptr() -> u32 {
    addr_of!(ENTRY_BUF) as u32
}

/// `primary_index` de la dernière `TimelineEntry` lue (0 = Padding Entry).
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_entry_primary_index() -> u16 {
    unsafe { u16::from_le_bytes([*addr_of!(ENTRY_BUF[0]), *addr_of!(ENTRY_BUF[1])]) }
}

/// `1` si Padding Entry (primary_index == 0), `0` sinon.
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_entry_is_padding() -> u32 {
    unsafe {
        let pi = u16::from_le_bytes([*addr_of!(ENTRY_BUF[0]), *addr_of!(ENTRY_BUF[1])]);
        if pi == 0 { 1 } else { 0 }
    }
}

/// `LiturgicalPeriod` du slot courant — bits [4:2] de `occurrence_flags`.
/// Valide pour tous les slots y compris Padding Entries.
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_entry_liturgical_period() -> u8 {
    unsafe { (*addr_of!(ENTRY_BUF[4]) >> 2) & 0x07 }
}

/// Ordinal de semaine liturgique (byte 6). 0 = N/A, 1–34 = semaine active.
/// Valide pour tous les slots y compris Padding Entries.
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_entry_liturgical_week() -> u8 {
    unsafe { *addr_of!(ENTRY_BUF[6]) }
}

/// `occurrence_flags` bruts — bits [1:0] vespers/vigil, bits [4:2] période.
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_entry_occurrence_flags() -> u8 {
    unsafe { *addr_of!(ENTRY_BUF[4]) }
}

// ═════════════════════════════════════════════════════════════════════════════
// Lecture Feast Registry — pattern out-static (FEAST_BUF)
// ═════════════════════════════════════════════════════════════════════════════

#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_read_feast(registry_index: u16) -> i32 {
    unsafe {
        if *addr_of!(BRIDGE_STATE) != 2 {
            return KAL_ERR_NOT_READY;
        }
        kal_read_feast(
            addr_of!(KALD_BUF) as *const u8,
            *addr_of!(KALD_LEN) as usize,
            registry_index,
            addr_of_mut!(FEAST_BUF) as *mut _,
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_feast_ptr() -> u32 {
    addr_of!(FEAST_BUF) as u32
}

// ═════════════════════════════════════════════════════════════════════════════
// Fêtes secondaires — pattern out-static (SECONDARY_BUF)
// ═════════════════════════════════════════════════════════════════════════════

#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_read_secondary(secondary_offset: u16, count: u8) -> i32 {
    unsafe {
        if *addr_of!(BRIDGE_STATE) != 2 {
            return KAL_ERR_NOT_READY;
        }
        kal_read_secondary(
            addr_of!(KALD_BUF) as *const u8,
            *addr_of!(KALD_LEN) as usize,
            secondary_offset,
            count,
            addr_of_mut!(SECONDARY_BUF) as *mut u16,
            SECONDARY_CAP as u8,
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_secondary_ptr() -> u32 {
    addr_of!(SECONDARY_BUF) as u32
}

// ═════════════════════════════════════════════════════════════════════════════
// Résolution label + annotation
// ═════════════════════════════════════════════════════════════════════════════

/// Résout label + annotation pour `(year, doy)`.
/// Retourne 1 (trouvé), 0 (Padding Entry ou absent), < 0 (erreur).
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_get_label(year: u16, doy: u16) -> i32 {
    unsafe {
        if *addr_of!(BRIDGE_STATE) != 2 {
            return KAL_ERR_NOT_READY;
        }

        let rc = kal_read_entry(
            addr_of!(KALD_BUF) as *const u8,
            *addr_of!(KALD_LEN) as usize,
            year,
            doy,
            addr_of_mut!(ENTRY_BUF) as *mut _,
        );
        if rc != KAL_ENGINE_OK {
            return rc;
        }

        let primary_index = u16::from_le_bytes([*addr_of!(ENTRY_BUF[0]), *addr_of!(ENTRY_BUF[1])]);
        if primary_index == 0 {
            return 0;
        }

        let rc2 = kal_read_feast(
            addr_of!(KALD_BUF) as *const u8,
            *addr_of!(KALD_LEN) as usize,
            primary_index,
            addr_of_mut!(FEAST_BUF) as *mut _,
        );
        if rc2 != KAL_ENGINE_OK {
            return rc2;
        }

        let feast_id = u16::from_le_bytes([*addr_of!(FEAST_BUF[0]), *addr_of!(FEAST_BUF[1])]);

        resolve_by_id(feast_id, year)
    }
}

/// Résout label + annotation par `feast_id` — pour les fêtes secondaires.
/// Retourne 1 (trouvé), 0 (absent), < 0 (erreur).
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_get_label_by_id(feast_id: u16, year: u16) -> i32 {
    unsafe {
        if *addr_of!(BRIDGE_STATE) != 2 {
            return KAL_ERR_NOT_READY;
        }
        resolve_by_id(feast_id, year)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_label_ptr() -> u32 {
    unsafe { *addr_of!(LABEL_PTR) }
}
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_label_len() -> u32 {
    unsafe { *addr_of!(LABEL_LEN) }
}
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_annotation_ptr() -> u32 {
    unsafe { *addr_of!(ANNOTATION_PTR) }
}
#[unsafe(no_mangle)]
pub extern "C" fn kal_wasm_annotation_len() -> u32 {
    unsafe { *addr_of!(ANNOTATION_LEN) }
}
