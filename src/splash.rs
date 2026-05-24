use bevy::prelude::*;

pub struct SplashPlugin;

impl Plugin for SplashPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_splash)
            .add_systems(Update, (update_splash_timer, animate_splash_drone));
    }
}

#[derive(Component)]
struct SplashScreen;

#[derive(Component)]
struct SplashDrone;

#[derive(Component)]
struct SplashDroneRotor {
    arm_index: u32,
}

#[derive(Resource)]
struct SplashTimer {
    timer: Timer,
}

impl Default for SplashTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(4.0, TimerMode::Once),
        }
    }
}

fn setup_splash(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(SplashTimer::default());

    let body_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.2, 0.2),
        metallic: 0.6,
        perceptual_roughness: 0.3,
        ..default()
    });
    let arm_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.3, 0.3),
        metallic: 0.8,
        perceptual_roughness: 0.2,
        ..default()
    });
    let rotor_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.1, 0.1),
        metallic: 0.4,
        perceptual_roughness: 0.5,
        ..default()
    });
    let rotor_tip_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.9, 0.9),
        metallic: 0.2,
        perceptual_roughness: 0.8,
        ..default()
    });
    let led_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.0, 1.0, 0.5),
        emissive: Color::srgb(0.0, 0.5, 0.25).into(),
        ..default()
    });

    let drone_entity = commands.spawn((
        SplashDrone,
        PbrBundle {
            mesh: meshes.add(Cuboid::new(0.3, 0.08, 0.2)),
            material: body_color,
            transform: Transform::from_xyz(0.0, 0.0, 3.0),
            ..default()
        },
    )).id();

    commands.spawn(PbrBundle {
        mesh: meshes.add(Sphere::new(0.015)),
        material: led_color.clone(),
        transform: Transform::from_xyz(-0.08, 0.05, -0.08),
        ..default()
    }).set_parent(drone_entity);
    commands.spawn(PbrBundle {
        mesh: meshes.add(Sphere::new(0.015)),
        material: led_color,
        transform: Transform::from_xyz(0.08, 0.05, -0.08),
        ..default()
    }).set_parent(drone_entity);

    for arm in 0..4 {
        let arm_angle = (arm as f32 / 4.0) * std::f32::consts::TAU + std::f32::consts::FRAC_PI_4;
        let arm_x = arm_angle.cos() * 0.25;
        let arm_z = arm_angle.sin() * 0.25;

        commands.spawn(PbrBundle {
            mesh: meshes.add(Cuboid::new(0.04, 0.03, 0.35)),
            material: arm_color.clone(),
            transform: Transform::from_xyz(arm_x, 0.0, arm_z)
                .with_rotation(Quat::from_rotation_y(arm_angle)),
            ..default()
        }).set_parent(drone_entity);

        commands.spawn(PbrBundle {
            mesh: meshes.add(Cylinder::new(0.03, 0.04)),
            material: arm_color.clone(),
            transform: Transform::from_xyz(arm_x * 1.8, 0.03, arm_z * 1.8),
            ..default()
        }).set_parent(drone_entity);

        let rotor_entity = commands.spawn((
            SplashDroneRotor { arm_index: arm },
            PbrBundle {
                mesh: meshes.add(Cylinder::new(0.12, 0.005)),
                material: rotor_color.clone(),
                transform: Transform::from_xyz(arm_x * 1.8, 0.06, arm_z * 1.8)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ..default()
            },
        )).id();

        commands.spawn(PbrBundle {
            mesh: meshes.add(Sphere::new(0.02)),
            material: rotor_tip_color.clone(),
            transform: Transform::from_xyz(0.12, 0.0, 0.0),
            ..default()
        }).set_parent(rotor_entity);
        commands.spawn(PbrBundle {
            mesh: meshes.add(Sphere::new(0.02)),
            material: rotor_tip_color.clone(),
            transform: Transform::from_xyz(-0.12, 0.0, 0.0),
            ..default()
        }).set_parent(rotor_entity);

        commands.entity(rotor_entity).set_parent(drone_entity);
    }

    commands
        .spawn((
            SplashScreen,
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    position_type: PositionType::Absolute,
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    row_gap: Val::Px(20.0),
                    ..default()
                },
                background_color: Color::srgba(0.05, 0.07, 0.12, 0.85).into(),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn(
                TextBundle::from_section(
                    "DRONE SIMULATOR",
                    TextStyle {
                        font_size: 72.0,
                        color: Color::srgb(0.2, 0.8, 1.0),
                        ..default()
                    },
                )
                .with_text_justify(JustifyText::Center),
            );

            parent.spawn(
                TextBundle::from_section(
                    "Scanning the skies above real cities...",
                    TextStyle {
                        font_size: 24.0,
                        color: Color::srgb(0.7, 0.8, 0.9),
                        ..default()
                    },
                )
                .with_text_justify(JustifyText::Center),
            );

            parent.spawn(
                TextBundle::from_section(
                    "Press SPACE to skip",
                    TextStyle {
                        font_size: 16.0,
                        color: Color::srgb(0.4, 0.5, 0.6),
                        ..default()
                    },
                )
                .with_text_justify(JustifyText::Center),
            );
        });
}

#[derive(Resource)]
pub struct AppReady;

fn update_splash_timer(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<SplashTimer>,
    keyboard: Res<ButtonInput<KeyCode>>,
    splash_query: Query<Entity, With<SplashScreen>>,
    drone_query: Query<Entity, With<SplashDrone>>,
    app_ready: Option<Res<AppReady>>,
) {
    if app_ready.is_some() {
        return;
    }

    timer.timer.tick(time.delta());

    if timer.timer.finished() || keyboard.just_pressed(KeyCode::Space) {
        for entity in splash_query.iter() {
            commands.entity(entity).despawn_recursive();
        }
        for entity in drone_query.iter() {
            commands.entity(entity).despawn_recursive();
        }
        commands.insert_resource(AppReady);
    }
}

fn animate_splash_drone(
    time: Res<Time>,
    mut drone_query: Query<&mut Transform, With<SplashDrone>>,
    mut rotor_query: Query<(&mut Transform, &SplashDroneRotor), Without<SplashDrone>>,
) {
    let t = time.elapsed_seconds();

    for mut transform in drone_query.iter_mut() {
        transform.translation.y = (t * 1.5).sin() * 0.15;
        transform.rotation = Quat::from_rotation_y(t * 0.3);
    }

    for (mut transform, rotor) in rotor_query.iter_mut() {
        let spin_dir = if rotor.arm_index % 2 == 0 { 1.0 } else { -1.0 };
        let spin_angle = t * 25.0 * spin_dir;
        transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
            * Quat::from_rotation_y(spin_angle);
    }
}
