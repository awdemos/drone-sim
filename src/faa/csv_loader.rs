//! FAA NASR CSV airspace loader.
//!
//! Expects the FAA NASR 28-day subscription `CLS_ARSP.csv` (Controlled Airspace).
//! Falls back gracefully if the file is missing or malformed.

use crate::core::types::{AirspaceGeometry, AirspaceRestrictionType, AirspaceZone};
use std::path::Path;

/// Attempt to load FAA NASR `CLS_ARSP.csv` from the given path.
/// Returns `None` if the file does not exist or cannot be parsed.
pub fn try_load_nasr_csv(path: &Path) -> Option<Vec<AirspaceZone>> {
    if !path.exists() {
        return None;
    }

    let file = std::fs::File::open(path).ok()?;
    let mut reader = csv::Reader::from_reader(file);

    let mut zones = Vec::new();

    for result in reader.records() {
        let record = result.ok()?;

        // NASR CLS_ARSP columns (simplified — index into known positions):
        // 0: AIRSPACE_CENTER, 1: STATE_CODE, 2: CITY_NAME
        // 3: AIRSPACE_TYPE (B/C/D), 4: AIRSPACE_CLASSIFICATION
        // 5: LATITUDE, 6: LONGITUDE
        // 7: AIRSPACE_UPPER_LIMIT, 8: AIRSPACE_UPPER_LIMIT_UOM
        let center = record.get(0)?.trim();
        let airspace_type = record.get(3)?.trim();
        let lat: f64 = record.get(5)?.trim().parse().ok()?;
        let lon: f64 = record.get(6)?.trim().parse().ok()?;
        let upper_limit: f64 = record.get(7)?.trim().parse().unwrap_or(0.0);
        let upper_uom = record.get(8).unwrap_or("FT").trim();

        let max_alt_ft = if upper_limit > 0.0 {
            Some(if upper_uom.eq_ignore_ascii_case("M") {
                upper_limit * 3.28084
            } else {
                upper_limit
            })
        } else {
            None
        };

        match airspace_type {
            "B" => {
                zones.push(AirspaceZone {
                    name: format!("{} - Class B No-Fly", center),
                    zone_type: AirspaceRestrictionType::NoFly,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: lat,
                        center_lon: lon,
                        radius_meters: nm_to_meters(5.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: None,
                    source: "FAA NASR CLS_ARSP".to_string(),
                });
                zones.push(AirspaceZone {
                    name: format!("{} - Class B Height Restricted", center),
                    zone_type: AirspaceRestrictionType::HeightRestricted,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: lat,
                        center_lon: lon,
                        radius_meters: nm_to_meters(10.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: max_alt_ft.or(Some(400.0)),
                    source: "FAA NASR CLS_ARSP".to_string(),
                });
            }
            "C" => {
                zones.push(AirspaceZone {
                    name: format!("{} - Class C No-Fly", center),
                    zone_type: AirspaceRestrictionType::NoFly,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: lat,
                        center_lon: lon,
                        radius_meters: nm_to_meters(5.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: None,
                    source: "FAA NASR CLS_ARSP".to_string(),
                });
            }
            "D" => {
                zones.push(AirspaceZone {
                    name: format!("{} - Class D No-Fly", center),
                    zone_type: AirspaceRestrictionType::NoFly,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: lat,
                        center_lon: lon,
                        radius_meters: nm_to_meters(4.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: None,
                    source: "FAA NASR CLS_ARSP".to_string(),
                });
            }
            _ => {}
        }
    }

    if zones.is_empty() {
        None
    } else {
        Some(zones)
    }
}

fn nm_to_meters(nm: f64) -> f64 {
    nm * 1852.0
}
