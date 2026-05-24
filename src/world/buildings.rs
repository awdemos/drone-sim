use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use crate::world::osm_loader::OsmData;
use crate::world::{WorldEntity, BuildingCollider, SpatialGrid};

fn create_window_texture() -> Image {
    let size = 64u32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    
    for y in 0..size {
        for x in 0..size {
            let window_x = (x % 16) < 2 || (x % 16) > 13;
            let window_y = (y % 16) < 2 || (y % 16) > 13;
            let is_window = !window_x && !window_y;
            
            if is_window {
                data.push(80);
                data.push(90);
                data.push(100);
                data.push(255);
            } else {
                data.push(180);
                data.push(170);
                data.push(160);
                data.push(255);
            }
        }
    }
    
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = bevy::render::texture::ImageSampler::nearest();
    image
}

/// Core spawn logic callable from both Startup and reload handlers
pub fn spawn_buildings_and_roads_impl(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    osm_data: &OsmData,
    spatial_grid: &mut SpatialGrid,
) {
    // Spawn buildings
    for (idx, building) in osm_data.buildings.iter().enumerate() {
        if building.points.len() < 3 {
            continue;
        }

        let mut mesh = Mesh::new(bevy::render::mesh::PrimitiveTopology::TriangleList, bevy::render::render_asset::RenderAssetUsages::default());
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        let height = building.height;
        let ground_y = 0.0f32;
        let top_y = height;
        let window_repeat = 3.0f32;

        let n = building.points.len();
        for i in 0..n {
            let p0 = building.points[i];
            let p1 = building.points[(i + 1) % n];

            let dx = p1.x - p0.x;
            let dz = p1.y - p0.y;
            let len = (dx * dx + dz * dz).sqrt();
            if len < 0.001 {
                continue;
            }

            let nx = dz / len;
            let nz = -dx / len;

            let base_idx = positions.len() as u32;
            let u_repeat = len / window_repeat;
            let v_repeat = height / window_repeat;

            positions.push([p0.x, ground_y, p0.y]);
            normals.push([nx, 0.0, nz]);
            uvs.push([0.0, 0.0]);
            positions.push([p1.x, ground_y, p1.y]);
            normals.push([nx, 0.0, nz]);
            uvs.push([u_repeat, 0.0]);
            positions.push([p1.x, top_y, p1.y]);
            normals.push([nx, 0.0, nz]);
            uvs.push([u_repeat, v_repeat]);
            positions.push([p0.x, top_y, p0.y]);
            normals.push([nx, 0.0, nz]);
            uvs.push([0.0, v_repeat]);

            indices.push(base_idx);
            indices.push(base_idx + 1);
            indices.push(base_idx + 2);
            indices.push(base_idx);
            indices.push(base_idx + 2);
            indices.push(base_idx + 3);
        }

        if n >= 3 {
            let roof_base = positions.len() as u32;
            let center = building.points.iter().fold(Vec2::ZERO, |a, b| a + *b) / n as f32;
            positions.push([center.x, top_y, center.y]);
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([0.5, 0.5]);

            for i in 0..n {
                let p = building.points[i];
                positions.push([p.x, top_y, p.y]);
                normals.push([0.0, 1.0, 0.0]);
                uvs.push([0.5 + (p.x - center.x) / window_repeat * 0.1, 0.5 + (p.y - center.y) / window_repeat * 0.1]);
            }

            for i in 0..n {
                let next = (i + 1) % n;
                indices.push(roof_base);
                indices.push(roof_base + 1 + i as u32);
                indices.push(roof_base + 1 + next as u32);
            }
        }

        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));

        let hue = 0.05 + (idx as f32 * 0.03).fract() * 0.1;
        let color = Color::hsl(hue * 360.0, 0.3, 0.65 + (idx as f32 * 0.01).fract() * 0.15);

        let min_x = building.points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let max_x = building.points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        let min_z = building.points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_z = building.points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);

        let collider = BuildingCollider {
            min: Vec3::new(min_x, 0.0, min_z),
            max: Vec3::new(max_x, height, max_z),
        };
        let entity = commands.spawn((
            PbrBundle {
                mesh: meshes.add(mesh),
                material: materials.add(StandardMaterial {
                    base_color: color,
                    unlit: true,
                    ..default()
                }),
                ..default()
            },
            WorldEntity,
            collider.clone(),
        )).id();
        spatial_grid.insert(entity, collider);
    }

    // Spawn roads
    let road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.35, 0.38),
        unlit: true,
        ..default()
    });

    for road in osm_data.roads.iter() {
        if road.points.len() < 2 {
            continue;
        }

        let half_width = road.width / 2.0;
        let mut positions = Vec::new();
        let mut indices = Vec::new();

        for i in 0..road.points.len() - 1 {
            let p0 = road.points[i];
            let p1 = road.points[i + 1];
            let dx = p1.x - p0.x;
            let dz = p1.y - p0.y;
            let len = (dx * dx + dz * dz).sqrt();
            if len < 0.001 {
                continue;
            }

            let perp_x = -dz / len * half_width;
            let perp_z = dx / len * half_width;

            let base = positions.len() as u32;
            positions.push([p0.x + perp_x, 0.02, p0.y + perp_z]);
            positions.push([p0.x - perp_x, 0.02, p0.y - perp_z]);
            positions.push([p1.x - perp_x, 0.02, p1.y - perp_z]);
            positions.push([p1.x + perp_x, 0.02, p1.y + perp_z]);

            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
            indices.push(base);
            indices.push(base + 2);
            indices.push(base + 3);
        }

        if positions.len() >= 4 {
            let mut mesh = Mesh::new(bevy::render::mesh::PrimitiveTopology::TriangleList, bevy::render::render_asset::RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            mesh.compute_normals();
            commands.spawn((
                PbrBundle {
                    mesh: meshes.add(mesh),
                    material: road_material.clone(),
                    ..default()
                },
                WorldEntity,
            ));
        }
    }
}

/// Startup system wrapper
pub fn spawn_buildings_and_roads(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    osm_data: Res<OsmData>,
    mut spatial_grid: ResMut<SpatialGrid>,
) {
    spawn_buildings_and_roads_impl(&mut commands, &mut meshes, &mut materials, &osm_data, &mut spatial_grid);
}
