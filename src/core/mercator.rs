/// Tile key for identifying map tiles at a specific zoom level.
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct TileKey {
    pub z: u32,
    pub x: u32,
    pub y: u32,
}

/// Convert latitude/longitude to pixel coordinates at a given zoom level.
pub fn lat_lon_to_pixel(lat: f64, lon: f64, zoom: u32) -> (f64, f64) {
    let n = (1u32 << zoom) as f64;
    let px = n * ((lon + 180.0) / 360.0) * 256.0;
    let lat = lat.clamp(-89.9999, 89.9999);
    let lat_rad = lat.to_radians();
    let py = n * (1.0 - ((lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI)) / 2.0 * 256.0;
    (px, py)
}

/// Convert pixel coordinates to latitude/longitude at a given zoom level.
pub fn pixel_to_lat_lon(px: f64, py: f64, zoom: u32) -> (f64, f64) {
    let n = (1u32 << zoom) as f64;
    let lon = px / (n * 256.0) * 360.0 - 180.0;
    let lat_rad = (std::f64::consts::PI * (1.0 - 2.0 * py / (n * 256.0))).sinh().atan();
    (lat_rad.to_degrees(), lon)
}

/// Convert pixel coordinates to tile indices.
pub fn pixel_to_tile(px: f64, py: f64) -> (i32, i32) {
    ((px / 256.0).floor() as i32, (py / 256.0).floor() as i32)
}

/// Convert tile indices to pixel coordinates of the top-left corner.
pub fn tile_to_pixel(tx: i32, ty: i32) -> (f64, f64) {
    (tx as f64 * 256.0, ty as f64 * 256.0)
}

/// Compute the latitude/longitude bounds of a tile.
/// Returns (min_lat, min_lon, max_lat, max_lon).
pub fn tile_lat_lon_bounds(tx: i32, ty: i32, zoom: u32) -> (f64, f64, f64, f64) {
    let (min_px, min_py) = tile_to_pixel(tx, ty);
    let (max_px, max_py) = tile_to_pixel(tx + 1, ty + 1);
    let (max_lat, min_lon) = pixel_to_lat_lon(min_px, min_py, zoom);
    let (min_lat, max_lon) = pixel_to_lat_lon(max_px, max_py, zoom);
    (min_lat, min_lon, max_lat, max_lon)
}
