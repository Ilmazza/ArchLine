//! Strato BIM di ArchLine: zone demolite/nuove sul rilievo, locali, estrazione
//! dei DWG per stato. Logica pura e testabile senza interfaccia.
//!
//! Spec: `docs/superpowers/specs/2026-10-23-bim-stato-piante-design.md`.

pub mod abaco;
pub mod abaco_aperture;
pub mod abaco_render;
pub mod abaco_table;
pub mod aperture;
pub mod aperture_command;
pub mod aperture_quantities;
pub mod clip;
pub mod commands;
pub mod extract;
pub mod height;
pub mod migrate;
pub mod project;
pub mod quantities;
pub mod quantities_rooms;
pub mod record;
pub mod refresh;
pub mod report;
pub mod room_command;
pub mod rooms;
pub mod section;
pub mod section_draw;
pub mod units;
pub mod walls;
pub mod zone;

#[cfg(test)]
pub(crate) mod fixtures;

/// Layer delle zone demolite (ACI 2, giallo).
pub const LAYER_DEMOLIZIONI: &str = "AL-DEMOLIZIONI";
/// Layer delle zone nuove (ACI 1, rosso).
pub const LAYER_NUOVE: &str = "AL-NUOVE";
/// Layer dei locali (stato di fatto).
pub const LAYER_LOCALI: &str = "AL-LOCALI";
/// Layer dei locali di progetto.
pub const LAYER_LOCALI_PROGETTO: &str = "AL-LOCALI-PROGETTO";
/// Ogni layer che inizia così è escluso dalla geometria dei muri.
pub const LAYER_PREFIX: &str = "AL-";

/// APPID dell'XDATA delle zone.
pub const APP_ZONA: &str = "ARCHLINE_ZONA";
/// APPID dell'XDATA dei locali.
pub const APP_LOCALE: &str = "ARCHLINE_LOCALE";
/// Chiave dell'XRecord con le impostazioni del disegno (dizionario radice).
pub const RECORD_PROGETTO: &str = "ARCHLINE_PROGETTO";
/// Layer delle aperture (porte e finestre, parte 3).
pub const LAYER_APERTURE: &str = "AL-APERTURE";
/// APPID dell'XDATA delle aperture.
pub const APP_APERTURA: &str = "ARCHLINE_APERTURA";
/// Layer delle tabelle d'abaco (`AL_ABACO`).
pub const LAYER_ABACO: &str = "AL-ABACO";
/// APPID dell'XDATA delle tabelle d'abaco.
pub const APP_ABACO: &str = "ARCHLINE_ABACO";

/// Colore ACI delle zone demolite.
pub const ACI_GIALLO: i16 = 2;
/// Colore ACI delle zone nuove.
pub const ACI_ROSSO: i16 = 1;

/// Spessore massimo di un muro cercato dall'asse, in metri.
pub const MAX_WALL_THICKNESS_M: f64 = 1.5;
/// Tolleranza geometrica di base, in metri (vedi `DrawingUnit::tolerance`).
pub const BASE_TOLERANCE_M: f64 = 1.0e-6;
