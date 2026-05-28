pub mod csv_loader;
pub mod geojson_loader;
pub mod loader;
pub mod render_3d;
pub mod tfr_fetcher;

use bevy::prelude::*;

pub struct FaaPlugin;

impl Plugin for FaaPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(loader::AirspaceData::load())
            .init_resource::<tfr_fetcher::TfrFetchState>()
            .add_systems(Startup, render_3d::spawn_airspace_3d.after(crate::world::terrain::insert_terrain_data))
            .add_systems(Update, render_3d::despawn_airspace_3d)
            .add_systems(Update, tfr_fetcher::update_tfrs);
    }
}
