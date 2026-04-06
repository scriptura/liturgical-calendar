// build.rs — liturgical-calendar-core
//
// La génération de kal_engine.h est une étape manuelle, hors CI standard.
// Elle est déclenchée uniquement via :
//   cargo build -p liturgical-calendar-core --features gen-headers
//
// Spec §1.5 (roadmap) : cbindgen conditionnée par `#[cfg(feature = "gen-headers")]`.

fn main() {
    #[cfg(feature = "gen-headers")]
    generate_headers();
}

#[cfg(feature = "gen-headers")]
fn generate_headers() {
    use std::env;
    use std::path::PathBuf;

    let crate_dir = env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR non défini");

    let config = cbindgen::Config::from_file(
        PathBuf::from(&crate_dir).join("cbindgen.toml")
    )
    .expect("Impossible de lire cbindgen.toml");

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen : génération échouée")
        .write_to_file("kal_engine.h");

    println!("cargo:warning=kal_engine.h généré.");
}
