// liturgical-calendar-forge/src/lib.rs
//
// Forge — compilateur AOT de droit liturgique.
// Produit les artefacts `.kald` consommés par l'Engine.
//
// INV-W7 : missing_docs autorisé en Jalon 1 et 2 pour la Forge.
// Sera activé en Jalon 3 (stabilisation de l'API publique).
#![allow(missing_docs)]

// INV-W4 : la Forge peut importer l'Engine pour validation.
// L'Engine ne doit jamais importer la Forge.
pub use liturgical_calendar_core as engine;
