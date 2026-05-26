use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::render_resource::Extent3d;
use crate::drone::{DroneIdentity, Kinematics};
use crate::drone::types::DroneTypeRegistry;

/// Component marking a drone's camera
#[derive(Component)]
pub struct DroneCamera {
    pub drone_id: crate::core::types::DroneId,
    pub fov: f32,
    pub resolution: (u32, u32),
}

/// Resource mapping drone IDs to their camera entities and image handles
#[derive(Resource, Default)]
pub struct DroneCameraMap {
    pub map: std::collections::HashMap<crate::core::types::DroneId, Entity>,
    pub images: std::collections::HashMap<crate::core::types::DroneId, Handle<Image>>,
}

/// Update camera transforms to follow drones
pub fn update_camera_views(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    drone_query: Query<(Entity, &DroneIdentity, &Kinematics), Without<DroneCamera>>,
    mut camera_query: Query<&mut Transform, With<DroneCamera>>,
    mut camera_map: ResMut<DroneCameraMap>,
    registry: Res<DroneTypeRegistry>,
) {
    // Update existing cameras
    for (entity, identity, _kinematics) in drone_query.iter() {
        if let Some(&cam_entity) = camera_map.map.get(&identity.id) {
            if let Ok(mut cam_transform) = camera_query.get_mut(cam_entity) {
                // Camera follows drone (child of drone entity) — local offset only.
                // Parent Transform is synced to world position/orientation by sync_drone_transforms.
                let camera_pos = Vec3::new(0.0, 0.1, -0.2); // up * 0.1 - forward * 0.2 in local frame
                
                *cam_transform = Transform::from_translation(camera_pos)
                    .looking_at(camera_pos + Vec3::Z * 10.0, Vec3::Y);
            }
        } else {
            // Create new camera for this drone
            let spec = registry.specs.get(&identity.drone_type).unwrap();
            let cam = &spec.camera;

            let size = Extent3d {
                width: cam.resolution.0,
                height: cam.resolution.1,
                ..default()
            };

            let mut image = Image::new_fill(
                size,
                bevy::render::render_resource::TextureDimension::D2,
                &[0, 0, 0, 255],
                bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb,
                bevy::render::render_asset::RenderAssetUsages::default(),
            );
            image.texture_descriptor.usage =
                bevy::render::render_resource::TextureUsages::TEXTURE_BINDING
                    | bevy::render::render_resource::TextureUsages::COPY_DST
                    | bevy::render::render_resource::TextureUsages::COPY_SRC
                    | bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT;

            let image_handle = images.add(image);

            let cam_entity = commands.spawn((
                Camera3dBundle {
                    camera: Camera {
                        target: RenderTarget::Image(image_handle.clone()),
                        order: 1,
                        ..default()
                    },
                    projection: Projection::Perspective(PerspectiveProjection {
                        fov: cam.fov_degrees.to_radians(),
                        near: 0.1,
                        far: 1000.0,
                        ..default()
                    }),
                    ..default()
                },
                DroneCamera {
                    drone_id: identity.id,
                    fov: cam.fov_degrees,
                    resolution: cam.resolution,
                },
                Name::new(format!("DroneCamera {:?}", identity.id)),
            )).set_parent(entity).id();

            camera_map.map.insert(identity.id, cam_entity);
            camera_map.images.insert(identity.id, image_handle.clone());

            // Also spawn a small viewport display in the 3D world
            commands.spawn(PbrBundle {
                mesh: meshes.add(Plane3d::new(Vec3::Y, Vec2::new(0.3, 0.2))),
                material: materials.add(StandardMaterial {
                    base_color_texture: Some(image_handle.clone()),
                    unlit: true,
                    ..default()
                }),
                transform: Transform::from_xyz(0.0, 0.15, 0.3)
                    .with_rotation(Quat::from_rotation_x(-0.3)),
                ..default()
            }).set_parent(entity);
        }
    }
}

pub fn cleanup_camera_map(
    mut events: EventReader<crate::drone::DroneDestroyedEvent>,
    mut camera_map: ResMut<DroneCameraMap>,
) {
    for event in events.read() {
        camera_map.map.remove(&event.drone_id);
        camera_map.images.remove(&event.drone_id);
        debug!("Cleaned up DroneCameraMap for drone {:?}", event.drone_id);
    }
}
