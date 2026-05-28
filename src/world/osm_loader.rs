use bevy::prelude::*;
use std::collections::HashMap;
use crate::core::gps::GeoReference;
use crate::core::types::GpsCoord;

fn parse_osm_length(value: &str) -> Option<f32> {
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }

    let numeric: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    let val = numeric.parse::<f32>().ok()?;

    let rest = trimmed[numeric.len()..].trim();
    let meters = match rest {
        "m" | "meters" | "meter" | "" => val,
        "cm" | "centimeters" | "centimeter" => val / 100.0,
        "mm" | "millimeters" | "millimeter" => val / 1000.0,
        "km" | "kilometers" | "kilometer" => val * 1000.0,
        "ft" | "feet" | "foot" | "'" => val * 0.3048,
        "in" | "inch" | "inches" | "\"" => val * 0.0254,
        "yd" | "yards" | "yard" => val * 0.9144,
        "mi" | "miles" | "mile" => val * 1609.34,
        _ => val,
    };
    Some(meters)
}

/// Represents a parsed OSM building
#[derive(Debug, Clone)]
pub struct OsmBuilding {
    pub points: Vec<Vec2>,
    pub height: f32,
    #[allow(dead_code)]
    pub levels: Option<u32>,
    #[allow(dead_code)]
    pub tags: HashMap<String, String>,
}

/// Represents a parsed OSM road
#[derive(Debug, Clone)]
pub struct OsmRoad {
    pub points: Vec<Vec2>,
    pub width: f32,
    #[allow(dead_code)]
    pub road_type: String,
}

/// Resource holding parsed OSM data
#[derive(Resource, Default, Clone)]
pub struct OsmData {
    pub buildings: Vec<OsmBuilding>,
    pub roads: Vec<OsmRoad>,
}

/// Load OSM data from a JSON file (Overpass API format) or fall back to procedural
pub fn load_osm_data(path: &std::path::Path, geo: &GeoReference) -> anyhow::Result<OsmData> {
    if path.exists() && path.extension().map(|e| e == "json").unwrap_or(false) {
        match load_osm_json(path, geo) {
            Ok(data) if !data.buildings.is_empty() || !data.roads.is_empty() => {
                eprintln!("Loaded OSM data: {} buildings, {} roads", data.buildings.len(), data.roads.len());
                return Ok(data);
            }
            Ok(_) => eprintln!("OSM JSON file was empty, falling back to procedural city."),
            Err(e) => eprintln!("Failed to parse OSM JSON ({}), falling back to procedural city.", e),
        }
    }

    Ok(generate_procedural_city(geo))
}

/// Load OSM data from the given path, or fall back to procedural city generation.
pub fn load_osm_with_fallback(path: Option<&std::path::Path>, geo: &GeoReference) -> OsmData {
    match path {
        Some(p) => load_osm_data(p, geo).unwrap_or_else(|e| {
            eprintln!("Warning: Failed to load OSM data: {}. Using procedural city.", e);
            load_osm_data(std::path::Path::new(""), geo)
                .expect("procedural city generation should never fail")
        }),
        None => load_osm_data(std::path::Path::new(""), geo)
            .expect("procedural city generation should never fail"),
    }
}

#[derive(Debug, serde::Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassElement>,
}

#[derive(Debug, serde::Deserialize)]
struct OverpassElement {
    #[serde(rename = "type")]
    elem_type: String,
    id: i64,
    lat: Option<f64>,
    lon: Option<f64>,
    nodes: Option<Vec<i64>>,
    tags: Option<HashMap<String, String>>,
}

fn load_osm_json(path: &std::path::Path, geo: &GeoReference) -> anyhow::Result<OsmData> {
    let text = std::fs::read_to_string(path)?;
    let resp: OverpassResponse = serde_json::from_str(&text)?;

    // Index all nodes by id
    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    for elem in &resp.elements {
        if elem.elem_type == "node" {
            if let (Some(lat), Some(lon)) = (elem.lat, elem.lon) {
                nodes.insert(elem.id, (lat, lon));
            }
        }
    }

    let mut data = OsmData::default();

    for elem in &resp.elements {
        if elem.elem_type != "way" {
            continue;
        }
        let tags = elem.tags.as_ref().unwrap_or(&HashMap::new()).clone();
        let node_ids = elem.nodes.as_ref().cloned().unwrap_or_default();

        // Resolve node coordinates
        let mut points_3d: Vec<Vec3> = Vec::new();
        for nid in &node_ids {
            if let Some(&(lat, lon)) = nodes.get(nid) {
                let gps = GpsCoord {
                    latitude: lat,
                    longitude: lon,
                    altitude_msl: 0.0,
                };
                let world = geo.gps_to_world(&gps);
                points_3d.push(world);
            }
        }

        if points_3d.len() < 2 {
            continue;
        }

        // Convert to 2D points (x, z) for the mini-map
        let points_2d: Vec<Vec2> = points_3d.iter().map(|p| Vec2::new(p.x, p.z)).collect();

        if tags.contains_key("building") {
            let height = tags.get("height")
                .and_then(|h| parse_osm_length(h))
                .or_else(|| tags.get("building:levels").and_then(|l| l.parse::<f32>().ok()).map(|l| l * 3.0))
                .unwrap_or(10.0);

            data.buildings.push(OsmBuilding {
                points: points_2d,
                height,
                levels: tags.get("building:levels").and_then(|l| l.parse().ok()),
                tags,
            });
        } else if let Some(highway) = tags.get("highway") {
            let width = tags.get("width")
                .and_then(|w| parse_osm_length(w))
                .unwrap_or_else(|| match highway.as_str() {
                    "motorway" | "trunk" => 12.0,
                    "primary" => 10.0,
                    "secondary" => 8.0,
                    "tertiary" => 7.0,
                    _ => 5.0,
                });

            data.roads.push(OsmRoad {
                points: points_2d,
                width,
                road_type: highway.clone(),
            });
        }
    }

    Ok(data)
}

/// Generate a procedural city layout when no OSM data is available
fn generate_procedural_city(_geo: &GeoReference) -> OsmData {
    let mut data = OsmData::default();
    let mut rng = fastrand::Rng::new();

    // Grid of buildings
    for bx in -5..=5 {
        for bz in -5..=5 {
            if rng.f32() < 0.3 {
                continue; // Empty lot
            }

            let x = bx as f32 * 40.0 + rng.f32() * 10.0;
            let z = bz as f32 * 40.0 + rng.f32() * 10.0;
            let w = 15.0 + rng.f32() * 10.0;
            let d = 15.0 + rng.f32() * 10.0;
            let h = 5.0 + rng.f32() * 25.0;

            data.buildings.push(OsmBuilding {
                points: vec![
                    Vec2::new(x - w / 2.0, z - d / 2.0),
                    Vec2::new(x + w / 2.0, z - d / 2.0),
                    Vec2::new(x + w / 2.0, z + d / 2.0),
                    Vec2::new(x - w / 2.0, z + d / 2.0),
                ],
                height: h,
                levels: Some((h / 3.0) as u32),
                tags: HashMap::new(),
            });
        }
    }

    // Grid roads
    for i in -6..=6 {
        let pos = i as f32 * 40.0;
        data.roads.push(OsmRoad {
            points: vec![
                Vec2::new(pos, -250.0),
                Vec2::new(pos, 250.0),
            ],
            width: 6.0,
            road_type: "residential".into(),
        });
        data.roads.push(OsmRoad {
            points: vec![
                Vec2::new(-250.0, pos),
                Vec2::new(250.0, pos),
            ],
            width: 6.0,
            road_type: "residential".into(),
        });
    }

    data
}
