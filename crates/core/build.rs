// La génération de kal_engine.h s'effectue via invocation manuelle de cbindgen,
// sans dépendance build-time, pour respecter l'invariant :
//   cargo tree -p liturgical-calendar-core → seule dépendance = sha2
//
// Commande manuelle :
//   cbindgen --config cbindgen.toml \
//            --crate liturgical-calendar-core \
//            --output kal_engine.h

fn main() {
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!("cargo:rerun-if-changed=src/header.rs");
    println!("cargo:rerun-if-changed=src/entry.rs");
}
