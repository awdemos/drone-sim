use bevy::prelude::*;
use crate::core::config::WorldConfig;
use super::dem;

#[derive(Resource, Clone)]
pub struct TerrainData {
    pub heights: Vec<Vec<f32>>,
    pub size_m: f32,
    pub resolution: usize,
    pub uses_real_elevation: bool,
}

impl TerrainData {
    pub fn generate(config: &WorldConfig) -> Self {
        let res = (config.terrain_resolution as usize).max(2);
        let size = config.terrain_size_m;
        let (heights, uses_real) = dem::generate_heights(config);

        Self {
            heights,
            size_m: size,
            resolution: res,
            uses_real_elevation: uses_real,
        }
    }

    pub fn get_height_at_world(&self, x: f32, z: f32) -> f32 {
        let half_size = self.size_m / 2.0;
        let nx = ((x + half_size) / self.size_m).clamp(0.0, 0.9999);
        let nz = ((z + half_size) / self.size_m).clamp(0.0, 0.9999);
        
        let ix = (nx * self.resolution as f32) as usize;
        let iz = (nz * self.resolution as f32) as usize;
        
        self.heights[iz.min(self.resolution - 1)][ix.min(self.resolution - 1)]
    }
}

pub fn insert_terrain_data(
    mut commands: Commands,
    config: Res<WorldConfig>,
) {
    let terrain = TerrainData::generate(&config);
    commands.insert_resource(terrain);
}

#[allow(dead_code)]
pub fn spawn_terrain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<WorldConfig>,
) {
    let terrain = TerrainData::generate(&config);
    let res = terrain.resolution;
    let size = terrain.size_m;
    let half_size = size / 2.0;

    let mut positions = Vec::with_capacity(res * res);
    let mut uvs = Vec::with_capacity(res * res);
    let mut indices = Vec::with_capacity((res - 1) * (res - 1) * 6);

    for z in 0..res {
        for x in 0..res {
            let px = (x as f32 / (res - 1) as f32) * size - half_size;
            let pz = (z as f32 / (res - 1) as f32) * size - half_size;
            let py = terrain.heights[z][x];
            positions.push([px, py, pz]);
            uvs.push([x as f32 / (res - 1) as f32, z as f32 / (res - 1) as f32]);
        }
    }

    for z in 0..res - 1 {
        for x in 0..res - 1 {
            let i = z * res + x;
            indices.push(i as u32);
            indices.push((i + res) as u32);
            indices.push((i + 1) as u32);
            indices.push((i + 1) as u32);
            indices.push((i + res) as u32);
            indices.push((i + res + 1) as u32);
        }
    }

    let mut mesh = Mesh::new(bevy::render::mesh::PrimitiveTopology::TriangleList, bevy::render::render_asset::RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));
    mesh.compute_normals();

    commands.spawn(PbrBundle {
        mesh: meshes.add(mesh),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.6, 0.35),
            perceptual_roughness: 0.9,
            ..default()
        }),
        ..default()
    });

    commands.insert_resource(terrain);
}
