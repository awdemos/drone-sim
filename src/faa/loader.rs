use bevy::prelude::Resource;
use crate::core::types::{AirspaceGeometry, AirspaceRestrictionType, AirspaceZone};

/// Bevy resource holding all loaded airspace restriction zones.
#[derive(Resource, Clone, Debug)]
pub struct AirspaceData {
    pub zones: Vec<AirspaceZone>,
}

impl AirspaceData {
    /// Load airspace data, preferring external files over embedded fallback.
    /// Searches for:
    /// - `data/faa/CLS_ARSP.csv` (FAA NASR Controlled Airspace)
    /// - `data/faa/udds.geojson` (FAA UAS Facility Maps)
    pub fn load() -> Self {
        let mut zones = Vec::new();

        let nasr_path = std::path::Path::new("data/faa/CLS_ARSP.csv");
        if let Some(nasr_zones) = crate::faa::csv_loader::try_load_nasr_csv(nasr_path) {
            zones.extend(nasr_zones);
        }

        let udds_path = std::path::Path::new("data/faa/udds.geojson");
        if let Some(udds_zones) = crate::faa::geojson_loader::try_load_udds_geojson(udds_path) {
            zones.extend(udds_zones);
        }

        if zones.is_empty() {
            zones = generate_airport_zones();
        }

        Self { zones }
    }

    /// Load the embedded fallback airspace database (major US airports).
    pub fn load_embedded() -> Self {
        let zones = generate_airport_zones();
        Self { zones }
    }

    /// Query zones whose center falls within the given lat/lon bounding box.
    /// Returns references to matching zones.
    pub fn query_visible(
        &self,
        min_lat: f64,
        min_lon: f64,
        max_lat: f64,
        max_lon: f64,
    ) -> Vec<&AirspaceZone> {
        self.zones
            .iter()
            .filter(|z| {
                let (clat, clon) = zone_center(z);
                clat >= min_lat && clat <= max_lat && clon >= min_lon && clon <= max_lon
            })
            .collect()
    }

    /// Query all zones (for rendering without spatial filtering).
    pub fn all_zones(&self) -> &[AirspaceZone] {
        &self.zones
    }
}

/// Extract the center lat/lon from a zone's geometry.
fn zone_center(zone: &AirspaceZone) -> (f64, f64) {
    match &zone.geometry {
        AirspaceGeometry::Circle { center_lat, center_lon, .. } => (*center_lat, *center_lon),
        AirspaceGeometry::Polygon { vertices } => {
            if vertices.is_empty() {
                (0.0, 0.0)
            } else {
                let sum_lat: f64 = vertices.iter().map(|(lat, _)| lat).sum();
                let sum_lon: f64 = vertices.iter().map(|(_, lon)| lon).sum();
                (sum_lat / vertices.len() as f64, sum_lon / vertices.len() as f64)
            }
        }
    }
}

/// Generate restriction zones for major US airports.
fn generate_airport_zones() -> Vec<AirspaceZone> {
    let mut zones = Vec::new();

    // (name, icao, lat, lon, class)
    // Class B: 5nm no-fly, 10nm height-restricted (400ft)
    // Class C: 5nm no-fly
    // Class D: 4nm no-fly
    let airports: &[(&str, &str, f64, f64, AirportClass)] = &[
        ("San Francisco Intl", "KSFO", 37.6213, -122.3790, AirportClass::B),
        ("Los Angeles Intl", "KLAX", 33.9425, -118.4081, AirportClass::B),
        ("John F Kennedy Intl", "KJFK", 40.6413, -73.7781, AirportClass::B),
        ("Chicago O'Hare Intl", "KORD", 41.9742, -87.9073, AirportClass::B),
        ("Dallas/Fort Worth Intl", "KDFW", 32.8998, -97.0403, AirportClass::B),
        ("Hartsfield-Jackson Atlanta", "KATL", 33.6407, -84.4277, AirportClass::B),
        ("Denver Intl", "KDEN", 39.8561, -104.6737, AirportClass::B),
        ("Seattle-Tacoma Intl", "KSEA", 47.4502, -122.3088, AirportClass::B),
        ("Miami Intl", "KMIA", 25.7959, -80.2870, AirportClass::B),
        ("Boston Logan Intl", "KBOS", 42.3656, -71.0096, AirportClass::B),
        ("Phoenix Sky Harbor", "KPHX", 33.4343, -112.0116, AirportClass::B),
        ("Harry Reid Intl (Las Vegas)", "KLAS", 36.0840, -115.1537, AirportClass::B),
        ("George Bush Intercontinental", "KIAH", 29.9902, -95.3368, AirportClass::B),
        ("Minneapolis-St Paul", "KMSP", 44.8848, -93.2223, AirportClass::B),
        ("Ronald Reagan Washington", "KDCA", 38.8512, -77.0402, AirportClass::B),
        ("San Diego Intl", "KSAN", 32.7336, -117.1897, AirportClass::C),
        ("Austin-Bergstrom Intl", "KAUS", 30.1975, -97.6664, AirportClass::C),
        ("Nashville Intl", "KBNA", 36.1263, -86.6774, AirportClass::C),
        ("Portland Intl", "KPDX", 45.5898, -122.5951, AirportClass::C),
        ("San Jose Intl", "KSJC", 37.3639, -121.9289, AirportClass::C),
        ("Oakland Intl", "KOAK", 37.7214, -122.2208, AirportClass::C),
        ("Raleigh-Durham Intl", "KRDU", 35.8801, -78.7880, AirportClass::C),
        ("Philadelphia Intl", "KPHL", 39.8744, -75.2424, AirportClass::B),
        ("Detroit Metro", "KDTW", 42.2124, -83.3534, AirportClass::B),
        ("Salt Lake City Intl", "KSLC", 40.7883, -111.9778, AirportClass::B),
        ("Orlando Intl", "KMCO", 28.4312, -81.3081, AirportClass::B),
        ("Tampa Intl", "KTPA", 27.9755, -82.5333, AirportClass::C),
        ("New Orleans Intl", "KMSY", 29.9934, -90.2580, AirportClass::B),
        ("Honolulu Intl", "PHNL", 21.3187, -157.9225, AirportClass::B),
        ("Anchorage Intl", "PANC", 61.1743, -149.9963, AirportClass::C),
    ];

    for (name, icao, lat, lon, class) in airports.iter() {
        match class {
            AirportClass::B => {
                // Inner no-fly zone: 5nm radius
                zones.push(AirspaceZone {
                    name: format!("{} ({}) - No Fly", name, icao),
                    zone_type: AirspaceRestrictionType::NoFly,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: *lat,
                        center_lon: *lon,
                        radius_meters: nm_to_meters(5.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: None,
                    source: "Embedded FAA Class B".to_string(),
                });
                // Outer height-restricted zone: 10nm radius, max 400ft AGL
                zones.push(AirspaceZone {
                    name: format!("{} ({}) - Height Restricted", name, icao),
                    zone_type: AirspaceRestrictionType::HeightRestricted,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: *lat,
                        center_lon: *lon,
                        radius_meters: nm_to_meters(10.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: Some(400.0),
                    source: "Embedded FAA Class B".to_string(),
                });
            }
            AirportClass::C => {
                // No-fly zone: 5nm radius
                zones.push(AirspaceZone {
                    name: format!("{} ({}) - No Fly", name, icao),
                    zone_type: AirspaceRestrictionType::NoFly,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: *lat,
                        center_lon: *lon,
                        radius_meters: nm_to_meters(5.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: None,
                    source: "Embedded FAA Class C".to_string(),
                });
            }
            AirportClass::D => {
                // No-fly zone: 4nm radius
                zones.push(AirspaceZone {
                    name: format!("{} ({}) - No Fly", name, icao),
                    zone_type: AirspaceRestrictionType::NoFly,
                    geometry: AirspaceGeometry::Circle {
                        center_lat: *lat,
                        center_lon: *lon,
                        radius_meters: nm_to_meters(4.0),
                    },
                    min_altitude_ft: Some(0.0),
                    max_altitude_ft: None,
                    source: "Embedded FAA Class D".to_string(),
                });
            }
        }
    }

    zones
}

#[derive(Clone, Copy)]
enum AirportClass {
    B,
    C,
    D,
}

/// Convert nautical miles to meters.
fn nm_to_meters(nm: f64) -> f64 {
    nm * 1852.0
}
