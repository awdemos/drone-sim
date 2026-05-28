use bevy::prelude::*;
use crate::core::types::{AirspaceGeometry, AirspaceRestrictionType, AirspaceZone};
use crate::faa::loader::AirspaceData;

#[derive(Resource, Default)]
pub struct TfrFetchState {
    pub last_fetch: Option<std::time::Instant>,
    pub active_tfrs: Vec<AirspaceZone>,
    pub fetch_error: Option<String>,
}

/// Periodically fetch TFRs from FAA and update the airspace data.
/// Runs every 5 minutes.
pub fn update_tfrs(
    mut tfr_state: ResMut<TfrFetchState>,
    mut airspace: ResMut<AirspaceData>,
    _time: Res<Time>,
) {
    let now = std::time::Instant::now();
    let should_fetch = match tfr_state.last_fetch {
        None => true,
        Some(last) => now.duration_since(last).as_secs() > 300,
    };

    if !should_fetch {
        return;
    }

    tfr_state.last_fetch = Some(now);

    match fetch_tfrs_blocking() {
        Ok(tfrs) => {
            tfr_state.active_tfrs = tfrs.clone();
            tfr_state.fetch_error = None;
            airspace.zones.retain(|z| !z.source.contains("TFR"));
            airspace.zones.extend(tfrs);
        }
        Err(e) => {
            tfr_state.fetch_error = Some(e.to_string());
        }
    }
}

fn fetch_tfrs_blocking() -> anyhow::Result<Vec<AirspaceZone>> {
    let url = "https://tfr.faa.gov/tfr2/list.html";
    let response = reqwest::blocking::get(url)?;
    let text = response.text()?;

    let mut zones = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.contains("TFR") && line.contains("radius") {
            if let Some(zone) = parse_tfr_line(line) {
                zones.push(zone);
            }
        }
    }

    Ok(zones)
}

fn parse_tfr_line(line: &str) -> Option<AirspaceZone> {
    let lat = extract_f64_after(line, "lat=")
        .or_else(|| extract_f64_after(line, "latitude="))
        .or_else(|| extract_f64_after(line, "Center:"))?;
    let lon = extract_f64_after(line, "lon=")
        .or_else(|| extract_f64_after(line, "longitude="))?;
    let radius_nm = extract_f64_after(line, "radius=")
        .or_else(|| extract_f64_after(line, "Radius:"))
        .unwrap_or(5.0);

    Some(AirspaceZone {
        name: "TFR (Temporary Flight Restriction)".to_string(),
        zone_type: AirspaceRestrictionType::Warning,
        geometry: AirspaceGeometry::Circle {
            center_lat: lat,
            center_lon: lon,
            radius_meters: radius_nm * 1852.0,
        },
        min_altitude_ft: Some(0.0),
        max_altitude_ft: None,
        source: "FAA TFR (fetched)".to_string(),
    })
}

fn extract_f64_after(text: &str, prefix: &str) -> Option<f64> {
    if let Some(pos) = text.find(prefix) {
        let start = pos + prefix.len();
        let rest = &text[start..];
        let numeric_start = rest.find(|c: char| c.is_digit(10) || c == '-' || c == '.')?;
        let numeric_part = &rest[numeric_start..];
        let end = numeric_part
            .find(|c: char| !c.is_digit(10) && c != '.' && c != '-')
            .unwrap_or(numeric_part.len());
        numeric_part[..end].parse().ok()
    } else {
        None
    }
}
