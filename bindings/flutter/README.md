# liturgical-calendar-flutter

Flutter FFI bindings for the [liturgical-calendar-core] engine.

Exposes a single additional C-ABI function — `kal_lits_get_label` — for
resolving a `(feast_id, year)` pair to a label and optional annotation from
a `.lits` buffer. All six Core functions (`kal_validate_header`,
`kal_validate_header_fast`, `kal_read_entry`, `kal_read_feast`,
`kal_read_secondary`, `kal_scan_flags`) are re-exported from the compiled
shared library at no overhead, as they carry `#[no_mangle]` in the statically
linked `rlib`.

**`no_std` / `no_alloc`** — the crate performs zero heap allocation.
All output pointers point directly into caller-owned buffers (zero-copy on
the Rust side).

## Architecture

```
liturgical-calendar-core  (no_std, no_alloc)
        │
        └── liturgical-calendar-flutter  (cdylib, no_std, no_alloc)
                │
                └── libkal_flutter.so / .dll / .dylib
                        │
                        └── Dart FFI  (packages/liturgical_calendar_flutter)
```

The Dart layer owns all buffers. Rust holds no state between calls.

## C-ABI surface

### Added by this crate

```c
// Resolves (feast_id, year) → (label, annotation) from a .lits buffer.
//
// Output pointers point into lits_bytes — zero-copy.
// out_annotation_ptr / out_annotation_len may be NULL (ignored if so).
// If non-NULL and annotation is absent: *out_annotation_ptr = NULL, *out_annotation_len = 0.
//
// Return codes:
//   0   KAL_ENGINE_OK
//  -1   KAL_ERR_NULL_PTR      lits_bytes, out_label_ptr or out_label_len is NULL
//  -2   KAL_ERR_BUF_TOO_SMALL buffer < 32 bytes
//  -3   KAL_ERR_MAGIC         magic != b"LITS"
//  -4   KAL_ERR_VERSION       version != 1
//  -6   KAL_ERR_FILE_SIZE     incoherent .lits layout
//  -7   KAL_ERR_INDEX_OOB     (feast_id, year) absent from corpus
int32_t kal_lits_get_label(
    const uint8_t *lits_bytes, size_t lits_len,
    uint16_t feast_id, uint16_t year,
    const uint8_t **out_label_ptr, size_t *out_label_len,
    const uint8_t **out_annotation_ptr, size_t *out_annotation_len
);
```

### Re-exported from Core

See [liturgical-calendar-core] for full documentation.

```c
int32_t kal_validate_header(const uint8_t*, size_t, KalHeader*);
int32_t kal_validate_header_fast(const uint8_t*, size_t, KalHeader*);
int32_t kal_read_entry(const uint8_t*, size_t, uint16_t year, uint16_t doy, TimelineEntry*);
int32_t kal_read_feast(const uint8_t*, size_t, uint16_t registry_index, FeastEntry*);
int32_t kal_read_secondary(const uint8_t*, size_t, uint16_t offset, uint8_t count, uint16_t*, uint8_t capacity);
int32_t kal_scan_flags(const uint8_t*, size_t, uint16_t year_from, uint16_t year_to, uint16_t mask, uint16_t value, uint32_t*, uint32_t capacity, uint32_t*);
```

## Building

```bash
# Linux / macOS (host)
cargo build --release -p liturgical-calendar-flutter

# Android arm64-v8a
cargo ndk -t arm64-v8a build --release -p liturgical-calendar-flutter

# Android x86_64 (emulator)
cargo ndk -t x86_64 build --release -p liturgical-calendar-flutter

# Windows (MSVC)
cargo build --release --target x86_64-pc-windows-msvc -p liturgical-calendar-flutter

# macOS universal (arm64 + x86_64)
cargo build --release --target aarch64-apple-darwin -p liturgical-calendar-flutter
cargo build --release --target x86_64-apple-darwin  -p liturgical-calendar-flutter
lipo -create -output libkal_flutter.dylib \
    target/aarch64-apple-darwin/release/libkal_flutter.dylib \
    target/x86_64-apple-darwin/release/libkal_flutter.dylib
```

Output artifacts:

| Platform        | File                   |
| --------------- | ---------------------- |
| Linux / Android | `libkal_flutter.so`    |
| Windows         | `kal_flutter.dll`      |
| macOS           | `libkal_flutter.dylib` |

## Dart integration

See [`packages/liturgical_calendar_flutter`] for the complete Flutter package.
The package resolves all symbols at startup via `DynamicLibrary.open()` and
exposes `LiturgicalCalendar` as the primary entry point.

```dart
import 'package:liturgical_calendar_flutter/liturgical_calendar_flutter.dart';

final cal = LiturgicalCalendar();
cal.load(kaldBytes, litsBytes);

final day = cal.getDay(2024, 12, 25);
if (day != null && !day.timeline.isPadding) {
  final label = cal.getLabel(day.primary!.feastId, 2024);
  print(label?.label); // "Nativitas Domini Nostri Iesu Christi"
}

cal.dispose();
```

## Format compatibility

Requires `.kald` v6 and `.lits` v1, produced by [liturgical-calendar-forge].
The `build_id` coherence check (first 8 bytes of the SHA-256 checksum vs.
`lits[12..20]`) is enforced by the Dart layer at load time.

## License

MIT

[liturgical-calendar-core]: https://crates.io/crates/liturgical-calendar-core
[liturgical-calendar-forge]: https://crates.io/crates/liturgical-calendar-forge
[`packages/liturgical_calendar_flutter`]: ../../packages/liturgical_calendar_flutter
