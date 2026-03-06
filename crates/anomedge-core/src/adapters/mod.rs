//! adapters/mod.rs
//! Telematics adapter module — normalises raw vehicle data into `SignalEvent`.
//!
//! Module order matches the dependency graph (base has no deps, registry depends on all).

pub mod base;
pub mod obd2;
pub mod ford_f450;
pub mod caterpillar;
pub mod john_deere_139;
pub mod registry;

// ─── Public re-exports ────────────────────────────────────────────────────────
// Callers only need to import from `adapters::*`, not from sub-modules.

pub use base::{
    AdapterError,
    CanFrame,
    RawTelematicsFrame,
    TelematicsAdapter,
    TelematicsConfig,
};

pub use obd2::Obd2Adapter;
pub use ford_f450::FordF450Adapter;
pub use caterpillar::CaterpillarAdapter;
pub use john_deere_139::JohnDeere139Adapter;

pub use registry::{
    AdapterType,
    FleetAssetConfig,
    create_adapter,
    create_fleet_adapters,
};
