use bevy::prelude::*;

use crate::world::WorldEntity;

pub struct EarthBackdropPlugin;

impl Plugin for EarthBackdropPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_earth_backdrop);
    }
}

fn spawn_earth_backdrop(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let earth_radius = 6_371_000.0;
    let texture_handle = asset_server.load("blue_marble.jpg");

    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(earth_radius).mesh().uv(64, 32)),
            material: materials.add(StandardMaterial {
                base_color_texture: Some(texture_handle),
                perceptual_roughness: 0.8,
                reflectance: 0.1,
                ..default()
            }),
            transform: Transform::from_xyz(0.0, -earth_radius + 100.0, 0.0),
            ..default()
        },
        WorldEntity,
    ));

    println!("Earth backdrop spawned with NASA Blue Marble texture");
}
