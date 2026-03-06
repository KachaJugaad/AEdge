//! adapters/registry.rs
//! Factory functions for instantiating the correct adapter for any vehicle type.
//!
//! The registry is the single entry point for all adapter construction.
//! No caller should import a concrete adapter type directly — use `create_adapter`.

use super::base::{AdapterError, TelematicsAdapter, TelematicsConfig};
use super::obd2::Obd2Adapter;
use super::ford_f450::FordF450Adapter;
use super::caterpillar::CaterpillarAdapter;
use super::john_deere_139::JohnDeere139Adapter;

// ─── AdapterType ──────────────────────────────────────────────────────────────

/// All supported vehicle/adapter types.
/// Matches the `SignalSource` variants in `types.rs` for the adapters that
/// correspond to real telematics protocols.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AdapterType {
    Obd2Generic,
    FordF450,
    CatHeavy,
    JohnDeere139,
}

impl AdapterType {
    /// Parse from the string representation used in fleet config YAML/JSON.
    /// Case-insensitive. Returns an error for unknown strings.
    pub fn parse(s: &str) -> Result<Self, AdapterError> {
        match s.to_uppercase().as_str() {
            "OBD2_GENERIC" | "OBD2" => Ok(AdapterType::Obd2Generic),
            "FORD_F450"    | "FORD" => Ok(AdapterType::FordF450),
            "CAT_HEAVY"    | "CAT"  => Ok(AdapterType::CatHeavy),
            "JOHN_DEERE_139" | "JD" | "JOHN_DEERE" => Ok(AdapterType::JohnDeere139),
            other => Err(AdapterError::MalformedFrame(
                format!("Unknown adapter type: '{other}'"),
            )),
        }
    }
}

// ─── create_adapter ───────────────────────────────────────────────────────────

/// Instantiate the correct adapter for the given type and config.
///
/// Returns a `Box<dyn TelematicsAdapter>` so the caller works with the trait
/// without knowing the concrete type. The adapter is `Send + Sync` by contract.
pub fn create_adapter(
    adapter_type: AdapterType,
    config: TelematicsConfig,
) -> Box<dyn TelematicsAdapter> {
    match adapter_type {
        AdapterType::Obd2Generic  => Box::new(Obd2Adapter::new(config)),
        AdapterType::FordF450     => Box::new(FordF450Adapter::new(config)),
        AdapterType::CatHeavy     => Box::new(CaterpillarAdapter::new(config)),
        AdapterType::JohnDeere139 => Box::new(JohnDeere139Adapter::new(config)),
    }
}

// ─── FleetAssetConfig ─────────────────────────────────────────────────────────

/// Configuration entry for a single asset in a fleet.
/// Typically loaded from `fleet.yaml` or the cloud asset registry.
#[derive(Debug, Clone)]
pub struct FleetAssetConfig {
    pub asset_id:    String,
    pub driver_id:   String,
    pub adapter:     AdapterType,
    pub description: String,
}

// ─── create_fleet_adapters ────────────────────────────────────────────────────

/// Instantiate adapters for every asset in a fleet configuration.
///
/// Returns a `Vec<(asset_id, Box<dyn TelematicsAdapter>)>` rather than a HashMap
/// so the caller can choose its own lookup structure without forcing a dependency
/// on a specific hash map crate.
pub fn create_fleet_adapters(
    assets: Vec<FleetAssetConfig>,
) -> Vec<(String, Box<dyn TelematicsAdapter>)> {
    assets
        .into_iter()
        .map(|asset| {
            let config = TelematicsConfig {
                asset_id:      asset.asset_id.clone(),
                driver_id:     asset.driver_id,
                vehicle_class: format!("{:?}", asset.adapter).to_lowercase(),
            };
            let adapter = create_adapter(asset.adapter, config);
            (asset.asset_id, adapter)
        })
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SignalSource;

    fn test_config(asset: &str) -> TelematicsConfig {
        TelematicsConfig {
            asset_id:      asset.into(),
            driver_id:     "DRV-TEST".into(),
            vehicle_class: "test".into(),
        }
    }

    #[test]
    fn test_create_obd2_adapter_source() {
        let adapter = create_adapter(AdapterType::Obd2Generic, test_config("TRUCK-001"));
        assert_eq!(adapter.source(), SignalSource::Obd2Generic);
    }

    #[test]
    fn test_create_ford_adapter_source() {
        let adapter = create_adapter(AdapterType::FordF450, test_config("F450-001"));
        assert_eq!(adapter.source(), SignalSource::FordF450);
    }

    #[test]
    fn test_create_cat_adapter_source() {
        let adapter = create_adapter(AdapterType::CatHeavy, test_config("CAT-001"));
        assert_eq!(adapter.source(), SignalSource::CatHeavy);
    }

    #[test]
    fn test_create_jd_adapter_source() {
        let adapter = create_adapter(AdapterType::JohnDeere139, test_config("JD-001"));
        assert_eq!(adapter.source(), SignalSource::JohnDeere139);
    }

    #[test]
    fn test_adapter_type_from_str_obd2() {
        assert_eq!(AdapterType::parse("OBD2_GENERIC").unwrap(), AdapterType::Obd2Generic);
        assert_eq!(AdapterType::parse("obd2").unwrap(), AdapterType::Obd2Generic);
    }

    #[test]
    fn test_adapter_type_from_str_ford() {
        assert_eq!(AdapterType::parse("FORD_F450").unwrap(), AdapterType::FordF450);
        assert_eq!(AdapterType::parse("ford").unwrap(), AdapterType::FordF450);
    }

    #[test]
    fn test_adapter_type_from_str_cat() {
        assert_eq!(AdapterType::parse("CAT_HEAVY").unwrap(), AdapterType::CatHeavy);
        assert_eq!(AdapterType::parse("cat").unwrap(), AdapterType::CatHeavy);
    }

    #[test]
    fn test_adapter_type_from_str_jd() {
        assert_eq!(AdapterType::parse("JOHN_DEERE_139").unwrap(), AdapterType::JohnDeere139);
        assert_eq!(AdapterType::parse("jd").unwrap(), AdapterType::JohnDeere139);
    }

    #[test]
    fn test_adapter_type_from_str_unknown_returns_error() {
        assert!(AdapterType::parse("UNKNOWN_VEHICLE").is_err());
    }

    #[test]
    fn test_create_fleet_adapters_returns_correct_count() {
        let assets = vec![
            FleetAssetConfig {
                asset_id:    "TRUCK-001".into(),
                driver_id:   "DRV-001".into(),
                adapter:     AdapterType::FordF450,
                description: "Ford F450 truck".into(),
            },
            FleetAssetConfig {
                asset_id:    "CAT-001".into(),
                driver_id:   "DRV-002".into(),
                adapter:     AdapterType::CatHeavy,
                description: "Cat 320 excavator".into(),
            },
            FleetAssetConfig {
                asset_id:    "JD-001".into(),
                driver_id:   "DRV-003".into(),
                adapter:     AdapterType::JohnDeere139,
                description: "JD 460E haul truck".into(),
            },
        ];
        let fleet = create_fleet_adapters(assets);
        assert_eq!(fleet.len(), 3);
    }

    #[test]
    fn test_fleet_adapters_have_correct_asset_ids() {
        let assets = vec![
            FleetAssetConfig {
                asset_id:    "ASSET-A".into(),
                driver_id:   "DRV-A".into(),
                adapter:     AdapterType::Obd2Generic,
                description: "Generic OBD2 vehicle".into(),
            },
            FleetAssetConfig {
                asset_id:    "ASSET-B".into(),
                driver_id:   "DRV-B".into(),
                adapter:     AdapterType::CatHeavy,
                description: "Cat excavator".into(),
            },
        ];
        let fleet = create_fleet_adapters(assets);
        let ids: Vec<&str> = fleet.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"ASSET-A"));
        assert!(ids.contains(&"ASSET-B"));
    }
}
