//! FAA UDDS GeoJSON airspace loader.
//!
//! Expects FAA UAS Facility Maps in GeoJSON format (e.g. from `faa.gov/uas/programs_partnerships/data_exchange/`).
//! Each feature should have a `CEILING` property (feet AGL).

use crate::core::types::{AirspaceGeometry, AirspaceRestrictionType, AirspaceZone};
use std::path::Path;

/// Attempt to load UDDS GeoJSON from the given path.
/// Returns `None` if the file does not exist or cannot be parsed.
pub fn try_load_udds_geojson(path: &Path) -> Option<Vec<AirspaceZone>> {
    if !path.exists() {
        return None;
    }

    let data = std::fs::read_to_string(path).ok()?;
    let geojson: serde_json::Value = serde_json::from_str(&data).ok()?;

    let features = geojson.get("features")?.as_array()?;
    let mut zones = Vec::new();

    for feature in features {
        let geometry = feature.get("geometry")?;
        let geom_type = geometry.get("type")?.as_str()?;
        let coords = geometry.get("coordinates")?;
        let props = feature.get("properties")?;

        let ceiling = props
            .get("CEILING")
            .or_else(|| props.get("ceiling"))
            .and_then(|v| v.as_f64())
            .or_else(|| {
                props.get("CEILING")
                    .or_else(|| props.get("ceiling"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
            });

        let name = props
            .get("NAME")
            .or_else(|| props.get("name"))
            .or_else(|| props.get("IDENT"))
            .and_then(|v| v.as_str())
            .unwrap_or("UDDS Zone")
            .to_string();

        match geom_type {
            "Point" => {
                let arr = coords.as_array()?;
                if arr.len() < 2 {
                    continue;
                }
                let lon = arr[0].as_f64()?;
                let lat = arr[1].as_f64()?;
                let radius = props
                    .get("RADIUS")
                    .or_else(|| props.get("radius"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1852.0);

                zones.push(AirspaceZone {
                    name: format!("{} - UDDS Height Restricted", name),
                    zone_type: AirspaceRestrictionType::HeightRestricted,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: lat,
                        center_lon: lon,
                        radius_meters: radius,
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: ceiling,
                    source: "FAA UDDS GeoJSON".to_string(),
                });
            }
            "Polygon" => {
                let rings = coords.as_array()?;
                if rings.is_empty() {
                    continue;
                }
                let exterior = rings[0].as_array()?;
                let mut vertices = Vec::new();
                for pt in exterior {
                    let pt_arr = pt.as_array()?;
                    if pt_arr.len() >= 2 {
                        vertices.push((pt_arr[1].as_f64()?, pt_arr[0].as_f64()?));
                    }
                }
                if vertices.len() >= 3 {
                    zones.push(AirspaceZone {
                        name: format!("{} - UDDS Polygon", name),
                        zone_type: AirspaceRestrictionType::HeightRestricted,
                        geometry: AirspaceGeometry::Polygon { vertices },
                        min_altitude_ft: Some(0.0),
                        max_altitude_ft: ceiling,
                        source: "FAA UDDS GeoJSON".to_string(),
                    });
                }
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
