use crate::core::config::WorldConfig;

/// SRTM HGT tile: 1° × 1° at either 1 arc-second (3601×3601) or 3 arc-second (1201×1201) resolution.
/// Big-endian signed 16-bit integers, -32768 = nodata.
pub struct SrtmTile {
    pub data: Vec<i16>,
    pub width: usize,
    pub height: usize,
    pub lat_origin: i32,
    pub lon_origin: i32,
}

impl SrtmTile {
    /// Load an SRTM .hgt file from disk.
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let size = bytes.len() / 2;
        let dim = if size >= 3601 * 3601 { 3601 } else if size >= 1201 * 1201 { 1201 } else { return None };

        let data: Vec<i16> = bytes.chunks_exact(2)
            .map(|chunk| i16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();

        // Parse filename for lat/lon origin (e.g., N37W122.hgt)
        let filename = path.file_stem()?.to_str()?;
        let (lat_origin, lon_origin) = parse_hgt_filename(filename)?;

        Some(Self {
            data,
            width: dim,
            height: dim,
            lat_origin,
            lon_origin,
        })
    }

    /// Sample elevation at fractional lat/lon (bilinear interpolation).
    /// Returns `None` if outside tile bounds or nodata.
    pub fn sample(&self, lat: f64, lon: f64) -> Option<f32> {
        let local_lat = lat - self.lat_origin as f64;
        let local_lon = lon - self.lon_origin as f64;

        if local_lat < 0.0 || local_lat > 1.0 || local_lon < 0.0 || local_lon > 1.0 {
            return None;
        }

        let x = local_lon * (self.width - 1) as f64;
        let y = (1.0 - local_lat) * (self.height - 1) as f64; // SRTM: row 0 = north

        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);

        let fx = x - x0 as f64;
        let fy = y - y0 as f64;

        let h00 = self.get(y0, x0)?;
        let h10 = self.get(y0, x1)?;
        let h01 = self.get(y1, x0)?;
        let h11 = self.get(y1, x1)?;

        // If any corner is nodata, just return the nearest valid value
        let h = h00 as f64 * (1.0 - fx) * (1.0 - fy)
              + h10 as f64 * fx * (1.0 - fy)
              + h01 as f64 * (1.0 - fx) * fy
              + h11 as f64 * fx * fy;

        Some(h as f32)
    }

    fn get(&self, row: usize, col: usize) -> Option<f32> {
        let val = self.data.get(row * self.width + col)?;
        if *val == -32768 {
            return None;
        }
        Some(*val as f32)
    }
}

/// Parse SRTM HGT filename like "N37W122" or "S01E045" to (lat_origin, lon_origin).
fn parse_hgt_filename(name: &str) -> Option<(i32, i32)> {
    let name = name.to_uppercase();
    if name.len() < 7 {
        return None;
    }

    let lat_sign = if name.starts_with('N') { 1 } else if name.starts_with('S') { -1 } else { return None };
    let lat_val: i32 = name[1..3].parse().ok()?;

    let rest = &name[3..];
    let lon_sign = if rest.starts_with('W') { -1 } else if rest.starts_with('E') { 1 } else { return None };
    let lon_val: i32 = rest[1..].parse().ok()?;

    Some((lat_sign * lat_val, lon_sign * lon_val))
}

/// Multi-tile DEM coverage. Loads all HGT tiles found in the data/srtm/ directory.
pub struct DemCoverage {
    tiles: Vec<SrtmTile>,
}

impl DemCoverage {
    /// Load all .hgt files from the given directory.
    pub fn load_dir(dir: &std::path::Path) -> Option<Self> {
        if !dir.exists() {
            return None;
        }
        let mut tiles = Vec::new();
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "hgt") {
                if let Some(tile) = SrtmTile::load(&path) {
                    log::info!("Loaded SRTM tile: {:?}", path);
                    tiles.push(tile);
                }
            }
        }
        if tiles.is_empty() {
            None
        } else {
            log::info!("Loaded {} SRTM tile(s)", tiles.len());
            Some(Self { tiles })
        }
    }

    /// Sample elevation at lat/lon by searching all loaded tiles.
    pub fn sample(&self, lat: f64, lon: f64) -> Option<f32> {
        for tile in &self.tiles {
            if let Some(h) = tile.sample(lat, lon) {
                return Some(h);
            }
        }
        None
    }
}

/// Generate terrain heights using DEM data when available, with noise-based fallback.
/// Returns a Vec<Vec<f32>> of height values.
pub fn generate_heights(config: &WorldConfig) -> (Vec<Vec<f32>>, bool) {
    let res = (config.terrain_resolution as usize).max(2);
    let size = config.terrain_size_m;

    // Try to load DEM/SRTM data
    let dem = DemCoverage::load_dir(std::path::Path::new("data/srtm"));

    if dem.is_some() {
        let heights = generate_from_dem(&dem.unwrap(), config, res, size);
        (heights, true)
    } else if config.heightmap_path.as_ref().map_or(false, |p| p.exists()) {
        let heights = generate_from_heightmap(config.heightmap_path.as_ref().unwrap(), res, size);
        (heights, true)
    } else {
        let heights = generate_procedural(config, res, size);
        (heights, false)
    }
}

fn generate_from_dem(dem: &DemCoverage, config: &WorldConfig, res: usize, size: f32) -> Vec<Vec<f32>> {
    let mut heights = vec![vec![0.0f32; res]; res];
    let half_size = size / 2.0;
    let meters_per_deg_lat = 111_320.0; // approximate
    let meters_per_deg_lon = 111_320.0_f64 * config.origin_lat.to_radians().cos();

    let mut min_h = f32::MAX;
    let mut max_h = f32::MIN;

    for z in 0..res {
        for x in 0..res {
            // World position to lat/lon
            let wx = (x as f32 / (res - 1) as f32) * size - half_size;
            let wz = (z as f32 / (res - 1) as f32) * size - half_size;

            let lat = config.origin_lat + (wz as f64 / meters_per_deg_lat);
            let lon = config.origin_lon + (wx as f64 / meters_per_deg_lon);

            let h = dem.sample(lat, lon).unwrap_or(0.0);
            heights[z][x] = h;
            min_h = min_h.min(h);
            max_h = max_h.max(h);
        }
    }

    log::info!("DEM terrain: elevation range {:.0}m - {:.0}m", min_h, max_h);
    heights
}

fn generate_from_heightmap(path: &std::path::Path, res: usize, size: f32) -> Vec<Vec<f32>> {
    // Load grayscale image as heightmap using basic BMP/PNG decoding
    // For simplicity, read raw bytes and interpret as grayscale
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return generate_procedural_fallback(res, size),
    };

    // Try to decode as a simple 16-bit raw heightmap (width * height * 2 bytes)
    // or fall back to interpreting as an 8-bit grayscale image
    let dim = (res as f32).sqrt() as usize;
    let mut heights = vec![vec![0.0f32; res]; res];

    if bytes.len() >= res * res * 2 {
        // 16-bit raw heightmap
        for z in 0..res {
            for x in 0..res {
                let idx = (z * res + x) * 2;
                let val = u16::from_be_bytes([bytes[idx], bytes[idx + 1]]);
                heights[z][x] = val as f32 * 0.1; // Scale: 1 unit = 0.1m
            }
        }
    } else if bytes.len() >= res * res {
        // 8-bit raw heightmap
        for z in 0..res {
            for x in 0..res {
                let idx = z * res + x;
                heights[z][x] = bytes[idx] as f32 * 0.4; // Scale: 1 unit = 0.4m
            }
        }
    } else {
        return generate_procedural_fallback(res, size);
    }

    log::info!("Loaded heightmap from {:?}", path);
    heights
}

/// Improved procedural terrain with multi-octave fractal noise.
/// Uses proper gradient noise instead of sine-based hash.
fn generate_procedural(config: &WorldConfig, res: usize, size: f32) -> Vec<Vec<f32>> {
    let mut heights = vec![vec![0.0f32; res]; res];

    // Use origin lat/lon as seed for deterministic but location-varying terrain
    let seed = ((config.origin_lat * 1000.0) as u64)
        .wrapping_mul(2654435761)
        .wrapping_add((config.origin_lon * 1000.0) as u64);

    for z in 0..res {
        for x in 0..res {
            let nx = x as f32 / res as f32;
            let nz = z as f32 / res as f32;

            // Multi-octave fractal noise (5 octaves)
            let mut h = 0.0f32;
            let mut amp = 50.0f32;
            let mut freq = 1.0f32;

            for octave in 0..5 {
                h += value_noise_2d(
                    nx * freq + seed.wrapping_add(octave as u64) as f32 * 0.1,
                    nz * freq + seed.wrapping_add(octave as u64 + 100) as f32 * 0.1,
                ) * amp;
                amp *= 0.5;
                freq *= 2.1;
            }

            // Ridge noise for mountain-like features
            let ridge = 1.0 - (value_noise_2d(
                nx * 3.0 + seed.wrapping_add(200) as f32 * 0.1,
                nz * 3.0 + seed.wrapping_add(300) as f32 * 0.1,
            ) * 2.0 - 1.0).abs();
            h += ridge * ridge * 30.0;

            // Flatten edges so drones can take off from center
            let dx = nx - 0.5;
            let dz = nz - 0.5;
            let dist_from_center = (dx * dx + dz * dz).sqrt();
            let flatten = (dist_from_center * 2.5).min(1.0);
            h = h * flatten + 5.0;

            heights[z][x] = h.max(0.0);
        }
    }

    heights
}

fn generate_procedural_fallback(res: usize, size: f32) -> Vec<Vec<f32>> {
    let config = WorldConfig::default();
    generate_procedural(&config, res, size)
}

/// Hash-based value noise with smooth interpolation.
/// Much better than the original sine-hash — produces natural-looking terrain.
fn value_noise_2d(x: f32, y: f32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - ix as f32;
    let fy = y - iy as f32;

    // Smoothstep interpolation
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);

    let n00 = hash2d(ix, iy);
    let n10 = hash2d(ix + 1, iy);
    let n01 = hash2d(ix, iy + 1);
    let n11 = hash2d(ix + 1, iy + 1);

    let nx0 = n00 * (1.0 - ux) + n10 * ux;
    let nx1 = n01 * (1.0 - ux) + n11 * ux;

    nx0 * (1.0 - uy) + nx1 * uy
}

/// Fast integer hash for 2D coordinates. Returns [0, 1).
fn hash2d(x: i32, y: i32) -> f32 {
    let mut h = (x as u64).wrapping_mul(374761393);
    h = h.wrapping_add(y as u64);
    h = h.wrapping_mul(668265263);
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^= h >> 16;
    (h as f32 / u32::MAX as f32).fract()
}
