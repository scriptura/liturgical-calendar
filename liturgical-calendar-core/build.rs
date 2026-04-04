// build.rs — Génération optionnelle du header C kal_engine.h.
//
// Activé uniquement avec : cargo build --features gen-headers
// Cela isole cbindgen en build-dependency optionnelle et ne pollue pas
// `cargo tree -p liturgical-calendar-core` en mode standard.

fn main() {
    #[cfg(feature = "gen-headers")]
    {
        let crate_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR non défini");

        let config =
            cbindgen::Config::from_file("cbindgen.toml").expect("Impossible de lire cbindgen.toml");

        cbindgen::Builder::new()
            .with_crate(&crate_dir)
            .with_config(config)
            .generate()
            .expect("Échec de génération cbindgen")
            .write_to_file("kal_engine.h");

        println!("cargo:warning=kal_engine.h généré avec succès.");
    }

    // Invalider le cache si les sources FFI changent.
    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!("cargo:rerun-if-changed=src/header.rs");
    println!("cargo:rerun-if-changed=src/entry.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
}
