use std::fs::File;
use std::io::{Write, BufWriter};
use bevy::prelude::*;
use crate::core::types::DroneId;
use crate::drone::{GpsPosition, DroneIdentity};

/// Plugin for exporting flight data to KML (Google Earth).
pub struct ExportPlugin;

impl Plugin for ExportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KmlExporter>()
            .add_systems(Update, write_kml_updates);
    }
}

/// Resource controlling real-time KML export.
#[derive(Resource, Debug)]
pub struct KmlExporter {
    pub is_exporting: bool,
    pub file_path: String,
    writer: Option<BufWriter<File>>,
}

impl Default for KmlExporter {
    fn default() -> Self {
        Self {
            is_exporting: false,
            file_path: String::new(),
            writer: None,
        }
    }
}

impl KmlExporter {
    pub fn start_export(&mut self, path: String) {
        let file = match File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to create KML file: {}", e);
                return;
            }
        };
        let mut writer = BufWriter::new(file);
        let header = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
<name>Drone Flight Log</name>
<Style id="dronePath">
<LineStyle><color>ff0000ff</color><width>2</width></LineStyle>
</Style>
"#;
        let _ = writer.write_all(header.as_bytes());
        self.writer = Some(writer);
        self.is_exporting = true;
        self.file_path = path;
    }

    pub fn stop_export(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.write_all(b"</Document>\n</kml>\n");
            let _ = writer.flush();
        }
        self.is_exporting = false;
    }

    pub fn write_placemark_start(&mut self, drone_id: &str) {
        if let Some(writer) = self.writer.as_mut() {
            let placemark = format!(
                "<Placemark><name>{}</name><styleUrl>#dronePath</styleUrl><LineString><coordinates>\n",
                drone_id
            );
            let _ = writer.write_all(placemark.as_bytes());
        }
    }

    pub fn write_coordinate(&mut self, lon: f64, lat: f64, alt: f64) {
        if let Some(writer) = self.writer.as_mut() {
            let coord = format!("{},{},{} ", lon, lat, alt);
            let _ = writer.write_all(coord.as_bytes());
        }
    }

    pub fn write_placemark_end(&mut self) {
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.write_all(b"</coordinates></LineString></Placemark>\n");
        }
    }
}

/// Per-drone KML tracking state.
#[derive(Component, Debug, Clone)]
pub struct KmlTrack {
    pub has_started: bool,
}

fn write_kml_updates(
    mut exporter: ResMut<KmlExporter>,
    drones: Query<(&DroneIdentity, &GpsPosition), Changed<GpsPosition>>,
    mut commands: Commands,
    existing_tracks: Query<(Entity, &KmlTrack)>,
) {
    if !exporter.is_exporting {
        for (entity, _) in existing_tracks.iter() {
            commands.entity(entity).remove::<KmlTrack>();
        }
        return;
    }

    for (identity, gps) in drones.iter() {
        let id_str = format!("{:?}", identity.id.0);
        let coord = gps.coord;
        exporter.write_coordinate(coord.longitude, coord.latitude, coord.altitude_msl);
    }
}
