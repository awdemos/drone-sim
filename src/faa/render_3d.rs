use bevy::prelude::*;
use crate::core::types::{AirspaceGeometry, AirspaceRestrictionType};
use crate::faa::loader::AirspaceData;
use crate::core::gps::GeoReference;
use crate::core::types::GpsCoord;

#[derive(Component)]
pub struct Airspace3DMarker;

pub fn spawn_airspace_3d(
    mut commands: Commands,
    airspace: Res<AirspaceData>,
    geo: Res<GeoReference>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for zone in airspace.all_zones() {
        let (color, alpha) = match zone.zone_type {
            AirspaceRestrictionType::NoFly => (Color::srgba(1.0, 0.0, 0.0, 0.15), 0.15),
            AirspaceRestrictionType::HeightRestricted => (Color::srgba(1.0, 0.78, 0.0, 0.12), 0.12),
            AirspaceRestrictionType::Warning => (Color::srgba(1.0, 0.55, 0.0, 0.12), 0.12),
        };

        match &zone.geometry {
            AirspaceGeometry::Circle { center_lat, center_lon, radius_meters } => {
                let world_pos = geo.gps_to_world(&GpsCoord { latitude: *center_lat, longitude: *center_lon, altitude_msl: 0.0 });
                let radius = *radius_meters as f32;
                let height_m = zone.max_altitude_ft.map(|f| f as f32 / 3.281).unwrap_or(120.0);

                let mesh = Cylinder {
                    radius,
                    half_height: height_m / 2.0,
                };
                let material = StandardMaterial {
                    base_color: color,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                };

                commands.spawn((
                    PbrBundle {
                        mesh: meshes.add(mesh),
                        material: materials.add(material),
                        transform: Transform::from_xyz(world_pos.x, height_m / 2.0, world_pos.z),
                        visibility: Visibility::Visible,
                        ..default()
                    },
                    Airspace3DMarker,
                    Name::new(format!("Airspace3D: {}", zone.name)),
                ));

                let ring_mesh = Torus {
                    major_radius: radius,
                    minor_radius: 2.0,
                    ..default()
                };
                let ring_material = StandardMaterial {
                    base_color: Color::srgba(color.to_srgba().red, color.to_srgba().green, color.to_srgba().blue, 0.4),
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                };

                commands.spawn((
                    PbrBundle {
                        mesh: meshes.add(ring_mesh),
                        material: materials.add(ring_material),
                        transform: Transform::from_xyz(world_pos.x, 0.5, world_pos.z)
                            .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                        visibility: Visibility::Visible,
                        ..default()
                    },
                    Airspace3DMarker,
                    Name::new(format!("Airspace3D Ring: {}", zone.name)),
                ));
            }
            AirspaceGeometry::Polygon { vertices } => {
                if vertices.is_empty() {
                    continue;
                }
                let sum_lat: f64 = vertices.iter().map(|(lat, _)| lat).sum();
                let sum_lon: f64 = vertices.iter().map(|(_, lon)| lon).sum();
                let centroid_lat = sum_lat / vertices.len() as f64;
                let centroid_lon = sum_lon / vertices.len() as f64;
                let world_pos = geo.gps_to_world(&GpsCoord { latitude: centroid_lat, longitude: centroid_lon, altitude_msl: 0.0 });
                let height_m = zone.max_altitude_ft.map(|f| f as f32 / 3.281).unwrap_or(120.0);

                let mesh = Cylinder {
                    radius: 50.0,
                    half_height: height_m / 2.0,
                };
                let material = StandardMaterial {
                    base_color: color,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                };

                commands.spawn((
                    PbrBundle {
                        mesh: meshes.add(mesh),
                        material: materials.add(material),
                        transform: Transform::from_xyz(world_pos.x, height_m / 2.0, world_pos.z),
                        visibility: Visibility::Visible,
                        ..default()
                    },
                    Airspace3DMarker,
                    Name::new(format!("Airspace3D Polygon: {}", zone.name)),
                ));
            }
        }
    }
}

pub fn despawn_airspace_3d(
    mut commands: Commands,
    query: Query<Entity, With<Airspace3DMarker>>,
    mut events: EventReader<crate::events::DespawnWorldEvent>,
) {
    for _ in events.read() {
        for entity in query.iter() {
            commands.entity(entity).despawn_recursive();
        }
    }
}
