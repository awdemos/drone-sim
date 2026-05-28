use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Camera mode for the main 3D viewport
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Resource)]
pub enum CameraMode {
    Overhead,
    StreetView,
}

/// Orbit camera controller for the main viewport.
/// Middle-click or Ctrl+Left-drag to orbit (swivel), right-click to pan.
#[derive(Component)]
pub struct OrbitCamera {
    pub radius: f32,
    pub theta: f32,
    pub phi: f32,
    pub target: Vec3,
    pub follow_drone: bool,
    pub mode: CameraMode,
    pub eye_height: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            radius: 50.0,
            theta: 45f32.to_radians(),
            phi: 60f32.to_radians(),
            target: Vec3::ZERO,
            follow_drone: true,
            mode: CameraMode::Overhead,
            eye_height: 2.0,
        }
    }
}

/// System that handles camera input and updates the main viewport camera.
pub fn camera_controller(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: EventReader<bevy::input::mouse::MouseMotion>,
    mut mouse_wheel: EventReader<bevy::input::mouse::MouseWheel>,
    mut query: Query<(&mut Transform, &mut OrbitCamera), With<Camera3d>>,
    drone_query: Query<&crate::drone::Kinematics>,
    mut camera_mode: ResMut<CameraMode>,
) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);

    let orbit_speed = 0.005;
    let pan_speed = 0.5;
    let zoom_speed = 2.0;
    let key_speed = 20.0 * time.delta_seconds();

    let motions: Vec<_> = mouse_motion.read().collect();
    let wheels: Vec<_> = mouse_wheel.read().collect();

    for (mut transform, mut orbit) in query.iter_mut() {
        if keyboard.just_pressed(KeyCode::KeyF) {
            orbit.follow_drone = !orbit.follow_drone;
        }
        if keyboard.just_pressed(KeyCode::KeyV) {
            orbit.mode = match orbit.mode {
                CameraMode::Overhead => CameraMode::StreetView,
                CameraMode::StreetView => CameraMode::Overhead,
            };
            *camera_mode = orbit.mode;
            println!("Camera mode switched to: {:?}", orbit.mode);
        }
        if keyboard.just_pressed(KeyCode::KeyR) {
            *orbit = OrbitCamera::default();
            println!("Camera reset to default");
        }

        let drone_pos = drone_query.iter().next().map(|k| k.position);

        let frame_count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if frame_count % 300 == 0 {
            println!("Camera: mode={:?}, pos={:.1},{:.1},{:.1}, target={:.1},{:.1},{:.1}",
                orbit.mode,
                transform.translation.x, transform.translation.y, transform.translation.z,
                orbit.target.x, orbit.target.y, orbit.target.z);
        }

        match orbit.mode {
            CameraMode::Overhead => {
                if orbit.follow_drone {
                    if let Some(target_pos) = drone_pos {
                        if target_pos.is_finite() {
                            let lerp_factor = 5.0 * time.delta_seconds();
                            orbit.target = orbit.target.lerp(target_pos, lerp_factor.min(1.0));
                        }
                    }
                }

                let is_orbiting = mouse_button.pressed(MouseButton::Middle)
                    || (keyboard.pressed(KeyCode::ControlLeft) && mouse_button.pressed(MouseButton::Left));
                if is_orbiting {
                    for motion in &motions {
                        orbit.theta -= motion.delta.x * orbit_speed;
                        orbit.phi = (orbit.phi + motion.delta.y * orbit_speed)
                            .clamp(5f32.to_radians(), 175f32.to_radians());
                    }
                }

                let is_panning = mouse_button.pressed(MouseButton::Right);
                if is_panning {
                    orbit.follow_drone = false;
                    for motion in &motions {
                        let right = transform.right();
                        let up = transform.up();
                        orbit.target -= right * motion.delta.x * pan_speed;
                        orbit.target += up * motion.delta.y * pan_speed;
                    }
                }

                let zoom_input: f32 = wheels.iter().map(|w| w.y).sum();
                if zoom_input != 0.0 {
                    orbit.radius -= zoom_input * zoom_speed;
                    orbit.radius = orbit.radius.clamp(5.0, 500.0);
                }

                if keyboard.pressed(KeyCode::ArrowUp) {
                    orbit.follow_drone = false;
                    orbit.target += Vec3::Y * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowDown) {
                    orbit.follow_drone = false;
                    orbit.target -= Vec3::Y * key_speed;
                    if orbit.target.y < 0.5 {
                        orbit.target.y = 0.5;
                    }
                }
                if keyboard.pressed(KeyCode::ArrowLeft) {
                    orbit.follow_drone = false;
                    orbit.target -= Vec3::X * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowRight) {
                    orbit.follow_drone = false;
                    orbit.target += Vec3::X * key_speed;
                }
                if keyboard.pressed(KeyCode::PageUp) || keyboard.pressed(KeyCode::Equal) {
                    orbit.radius -= key_speed;
                    orbit.radius = orbit.radius.max(5.0);
                }
                if keyboard.pressed(KeyCode::PageDown) || keyboard.pressed(KeyCode::Minus) {
                    orbit.radius += key_speed;
                    orbit.radius = orbit.radius.min(500.0);
                }

                let x = orbit.radius * orbit.phi.sin() * orbit.theta.cos();
                let y = orbit.radius * orbit.phi.cos();
                let z = orbit.radius * orbit.phi.sin() * orbit.theta.sin();
                transform.translation = orbit.target + Vec3::new(x, y, z);
                transform.look_at(orbit.target, Vec3::Y);
            }
            CameraMode::StreetView => {
                if orbit.follow_drone {
                    if let Some(target_pos) = drone_pos {
                        if target_pos.is_finite() {
                            let lerp_factor = 5.0 * time.delta_seconds();
                            orbit.target.x = orbit.target.x.lerp(target_pos.x, lerp_factor.min(1.0));
                            orbit.target.z = orbit.target.z.lerp(target_pos.z, lerp_factor.min(1.0));
                        }
                    }
                }

                if mouse_button.pressed(MouseButton::Middle) || mouse_button.pressed(MouseButton::Right) {
                    for motion in &motions {
                        orbit.theta -= motion.delta.x * orbit_speed;
                        orbit.phi = (orbit.phi + motion.delta.y * orbit_speed)
                            .clamp(10f32.to_radians(), 170f32.to_radians());
                    }
                }

                let zoom_input: f32 = wheels.iter().map(|w| w.y).sum();
                if zoom_input != 0.0 {
                    orbit.eye_height += zoom_input * 0.5;
                    orbit.eye_height = orbit.eye_height.clamp(1.5, 50.0);
                }

                if keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW) {
                    orbit.follow_drone = false;
                    let forward = Vec3::new(orbit.theta.sin(), 0.0, orbit.theta.cos());
                    orbit.target += forward * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS) {
                    orbit.follow_drone = false;
                    let forward = Vec3::new(orbit.theta.sin(), 0.0, orbit.theta.cos());
                    orbit.target -= forward * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA) {
                    orbit.follow_drone = false;
                    let right = Vec3::new(orbit.theta.cos(), 0.0, -orbit.theta.sin());
                    orbit.target -= right * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD) {
                    orbit.follow_drone = false;
                    let right = Vec3::new(orbit.theta.cos(), 0.0, -orbit.theta.sin());
                    orbit.target += right * key_speed;
                }
                if keyboard.pressed(KeyCode::PageUp) || keyboard.pressed(KeyCode::Equal) {
                    orbit.eye_height += key_speed * 0.5;
                    orbit.eye_height = orbit.eye_height.min(50.0);
                }
                if keyboard.pressed(KeyCode::PageDown) || keyboard.pressed(KeyCode::Minus) {
                    orbit.eye_height -= key_speed * 0.5;
                    orbit.eye_height = orbit.eye_height.max(1.5);
                }

                let cam_pos = Vec3::new(orbit.target.x, orbit.eye_height.max(1.5), orbit.target.z);
                let look_dir = Vec3::new(
                    orbit.phi.sin() * orbit.theta.sin(),
                    orbit.phi.cos(),
                    orbit.phi.sin() * orbit.theta.cos(),
                );
                transform.translation = cam_pos;
                transform.look_at(cam_pos + look_dir, Vec3::Y);
            }
        }
    }
}

/// Spawn the main viewport camera with orbit controller
pub fn spawn_main_camera(mut commands: Commands) {
    use bevy::pbr::FogFalloff;

    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(20.0, 30.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        OrbitCamera::default(),
        FogSettings {
            color: Color::srgb(0.35, 0.55, 0.8),
            falloff: FogFalloff::Linear {
                start: 300.0,
                end: 2000.0,
            },
            ..default()
        },
    ));
}
