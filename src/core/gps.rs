use bevy::prelude::*;
use crate::core::types::{GpsCoord, EnuCoord};
use geographiclib_rs::{Geodesic, DirectGeodesic, InverseGeodesic};

/// Reference frame for converting between GPS and local ENU coordinates
#[derive(Debug, Clone, Resource)]
pub struct GeoReference {
    pub origin_lat: f64,
    pub origin_lon: f64,
    pub origin_alt: f64,
    geod: Geodesic,
}

impl GeoReference {
    pub fn new(origin_lat: f64, origin_lon: f64, origin_alt: f64) -> Self {
        Self {
            origin_lat,
            origin_lon,
            origin_alt,
            geod: Geodesic::wgs84(),
        }
    }

    /// Convert GPS coordinate to local ENU (meters relative to origin)
    pub fn gps_to_enu(&self, gps: &GpsCoord) -> EnuCoord {
        // Use geographiclib for accurate geodesic calculations
        let (dist, azi1, _, _) = self.geod.inverse(
            self.origin_lat,
            self.origin_lon,
            gps.latitude,
            gps.longitude,
        );

        let azi_rad = azi1.to_radians();
        let east = dist * azi_rad.sin();
        let north = dist * azi_rad.cos();
        let up = gps.altitude_msl - self.origin_alt;

        EnuCoord { east, north, up }
    }

    /// Convert local ENU to GPS coordinate
    pub fn enu_to_gps(&self, enu: &EnuCoord) -> GpsCoord {
        let dist = (enu.east * enu.east + enu.north * enu.north).sqrt();
        let azi = if dist > 1e-9 {
            enu.east.atan2(enu.north).to_degrees()
        } else {
            0.0
        };

        let (lat, lon, _) = self.geod.direct(self.origin_lat, self.origin_lon, azi, dist);

        GpsCoord {
            latitude: lat,
            longitude: lon,
            altitude_msl: self.origin_alt + enu.up,
        }
    }

    /// Convert ENU to Bevy world coordinates (ENU: E=x+, N=z+, U=y+)
    pub fn enu_to_world(&self, enu: &EnuCoord) -> Vec3 {
        Vec3::new(
            enu.east as f32,
            enu.up as f32,
            enu.north as f32,
        )
    }

    /// Convert Bevy world coordinates to ENU
    pub fn world_to_enu(&self, world: Vec3) -> EnuCoord {
        EnuCoord {
            east: world.x as f64,
            north: world.z as f64,
            up: world.y as f64,
        }
    }

    /// Direct GPS to Bevy world
    pub fn gps_to_world(&self, gps: &GpsCoord) -> Vec3 {
        let enu = self.gps_to_enu(gps);
        self.enu_to_world(&enu)
    }

    /// Bevy world to GPS
    pub fn world_to_gps(&self, world: Vec3) -> GpsCoord {
        let enu = self.world_to_enu(world);
        self.enu_to_gps(&enu)
    }

    /// Calculate great-circle distance between two GPS points (meters)
    #[allow(dead_code)]
    pub fn distance_between(&self, a: &GpsCoord, b: &GpsCoord) -> f64 {
        let (dist, _, _, _) = self.geod.inverse(a.latitude, a.longitude, b.latitude, b.longitude);
        dist
    }

    /// Calculate bearing from a to b (degrees, clockwise from north)
    #[allow(dead_code)]
    pub fn bearing(&self, a: &GpsCoord, b: &GpsCoord) -> f64 {
        let (azi, _, _) = self.geod.inverse(a.latitude, a.longitude, b.latitude, b.longitude);
        azi
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mercator::{lat_lon_to_pixel, pixel_to_lat_lon};

    /// Tolerance for GPS roundtrip error in degrees
    const GPS_DEG_TOL: f64 = 1e-7;
    /// Tolerance for ENU roundtrip error in meters (1 cm)
    const ENU_M_TOL: f64 = 0.01;

    // ------------------------------------------------------------------
    // 1. ENU -> GPS -> ENU roundtrip accuracy (within 1 cm)
    // ------------------------------------------------------------------
    #[test]
    fn test_enu_gps_roundtrip() {
        let geo = GeoReference::new(37.7749, -122.4194, 10.0);
        let original = EnuCoord {
            east: 150.0,
            north: 200.0,
            up: 50.0,
        };

        let gps = geo.enu_to_gps(&original);
        let back = geo.gps_to_enu(&gps);

        assert!(
            (original.east - back.east).abs() < ENU_M_TOL,
            "east error: {} m",
            (original.east - back.east).abs()
        );
        assert!(
            (original.north - back.north).abs() < ENU_M_TOL,
            "north error: {} m",
            (original.north - back.north).abs()
        );
        assert!(
            (original.up - back.up).abs() < ENU_M_TOL,
            "up error: {} m",
            (original.up - back.up).abs()
        );
    }

    // ------------------------------------------------------------------
    // 2. GPS -> ENU -> GPS roundtrip accuracy (within 1e-7 degrees)
    // ------------------------------------------------------------------
    #[test]
    fn test_gps_enu_roundtrip() {
        let geo = GeoReference::new(37.7749, -122.4194, 10.0);
        let original = GpsCoord {
            latitude: 37.7750,
            longitude: -122.4190,
            altitude_msl: 15.0,
        };

        let enu = geo.gps_to_enu(&original);
        let back = geo.enu_to_gps(&enu);

        assert!(
            (original.latitude - back.latitude).abs() < GPS_DEG_TOL,
            "latitude error: {} deg",
            (original.latitude - back.latitude).abs()
        );
        assert!(
            (original.longitude - back.longitude).abs() < GPS_DEG_TOL,
            "longitude error: {} deg",
            (original.longitude - back.longitude).abs()
        );
        assert!(
            (original.altitude_msl - back.altitude_msl).abs() < ENU_M_TOL,
            "altitude error: {} m",
            (original.altitude_msl - back.altitude_msl).abs()
        );
    }

    // ------------------------------------------------------------------
    // 3. San Francisco coordinates conversion
    // ------------------------------------------------------------------
    #[test]
    fn test_sf_coordinates() {
        let geo = GeoReference::new(37.7749, -122.4194, 10.0);
        let sf_gps = GpsCoord {
            latitude: 37.7749,
            longitude: -122.4194,
            altitude_msl: 10.0,
        };

        let enu = geo.gps_to_enu(&sf_gps);
        // At the origin, ENU should be approximately zero
        assert!(
            enu.east.abs() < ENU_M_TOL,
            "SF origin east should be ~0, got {}",
            enu.east
        );
        assert!(
            enu.north.abs() < ENU_M_TOL,
            "SF origin north should be ~0, got {}",
            enu.north
        );
        assert!(
            enu.up.abs() < ENU_M_TOL,
            "SF origin up should be ~0, got {}",
            enu.up
        );
    }

    // ------------------------------------------------------------------
    // 4. New York coordinates conversion
    // ------------------------------------------------------------------
    #[test]
    fn test_nyc_coordinates() {
        let geo = GeoReference::new(40.7128, -74.0060, 0.0);
        // A point ~100 m north-east of origin
        let nyc_gps = GpsCoord {
            latitude: 40.7137,
            longitude: -74.0048,
            altitude_msl: 25.0,
        };

        let enu = geo.gps_to_enu(&nyc_gps);
        let back = geo.enu_to_gps(&enu);

        assert!(
            (nyc_gps.latitude - back.latitude).abs() < GPS_DEG_TOL,
            "NYC lat error: {} deg",
            (nyc_gps.latitude - back.latitude).abs()
        );
        assert!(
            (nyc_gps.longitude - back.longitude).abs() < GPS_DEG_TOL,
            "NYC lon error: {} deg",
            (nyc_gps.longitude - back.longitude).abs()
        );
        assert!(
            (nyc_gps.altitude_msl - back.altitude_msl).abs() < ENU_M_TOL,
            "NYC alt error: {} m",
            (nyc_gps.altitude_msl - back.altitude_msl).abs()
        );
    }

    // ------------------------------------------------------------------
    // 5. Zero origin handling
    // ------------------------------------------------------------------
    #[test]
    fn test_zero_origin() {
        let geo = GeoReference::new(0.0, 0.0, 0.0);
        let gps = GpsCoord {
            latitude: 0.0,
            longitude: 0.0,
            altitude_msl: 0.0,
        };

        let enu = geo.gps_to_enu(&gps);
        assert!(
            enu.east.abs() < ENU_M_TOL,
            "zero origin east should be ~0, got {}",
            enu.east
        );
        assert!(
            enu.north.abs() < ENU_M_TOL,
            "zero origin north should be ~0, got {}",
            enu.north
        );
        assert!(
            enu.up.abs() < ENU_M_TOL,
            "zero origin up should be ~0, got {}",
            enu.up
        );

        let back = geo.enu_to_gps(&enu);
        assert!(
            (gps.latitude - back.latitude).abs() < GPS_DEG_TOL,
            "zero origin lat error: {} deg",
            (gps.latitude - back.latitude).abs()
        );
        assert!(
            (gps.longitude - back.longitude).abs() < GPS_DEG_TOL,
            "zero origin lon error: {} deg",
            (gps.longitude - back.longitude).abs()
        );
    }

    // ------------------------------------------------------------------
    // 6. Negative coordinates (southern hemisphere)
    // ------------------------------------------------------------------
    #[test]
    fn test_southern_hemisphere() {
        let geo = GeoReference::new(-33.8688, 151.2093, 5.0); // Sydney
        let sydney_gps = GpsCoord {
            latitude: -33.8688,
            longitude: 151.2093,
            altitude_msl: 5.0,
        };

        let enu = geo.gps_to_enu(&sydney_gps);
        assert!(
            enu.east.abs() < ENU_M_TOL,
            "Sydney origin east should be ~0, got {}",
            enu.east
        );
        assert!(
            enu.north.abs() < ENU_M_TOL,
            "Sydney origin north should be ~0, got {}",
            enu.north
        );

        // A point ~200 m away
        let distant = GpsCoord {
            latitude: -33.8670,
            longitude: 151.2100,
            altitude_msl: 5.0,
        };
        let enu2 = geo.gps_to_enu(&distant);
        let back = geo.enu_to_gps(&enu2);

        assert!(
            (distant.latitude - back.latitude).abs() < GPS_DEG_TOL,
            "Sydney distant lat error: {} deg",
            (distant.latitude - back.latitude).abs()
        );
        assert!(
            (distant.longitude - back.longitude).abs() < GPS_DEG_TOL,
            "Sydney distant lon error: {} deg",
            (distant.longitude - back.longitude).abs()
        );
    }

    // ------------------------------------------------------------------
    // 7. Large distances (> 1 km)
    // ------------------------------------------------------------------
    #[test]
    fn test_large_distance() {
        let geo = GeoReference::new(37.7749, -122.4194, 0.0); // SF origin
        // Point ~5 km away (roughly toward Oakland)
        let distant = GpsCoord {
            latitude: 37.8044,
            longitude: -122.2712,
            altitude_msl: 0.0,
        };

        let enu = geo.gps_to_enu(&distant);
        // Distance should be > 5000 m
        let dist = (enu.east * enu.east + enu.north * enu.north).sqrt();
        assert!(
            dist > 5000.0,
            "Expected distance > 5000 m, got {} m",
            dist
        );

        let back = geo.enu_to_gps(&enu);
        assert!(
            (distant.latitude - back.latitude).abs() < GPS_DEG_TOL,
            "Large dist lat error: {} deg",
            (distant.latitude - back.latitude).abs()
        );
        assert!(
            (distant.longitude - back.longitude).abs() < GPS_DEG_TOL,
            "Large dist lon error: {} deg",
            (distant.longitude - back.longitude).abs()
        );
    }

    // ------------------------------------------------------------------
    // 8. Altitude handling
    // ------------------------------------------------------------------
    #[test]
    fn test_altitude_handling() {
        let geo = GeoReference::new(37.7749, -122.4194, 10.0);
        let high_gps = GpsCoord {
            latitude: 37.7749,
            longitude: -122.4194,
            altitude_msl: 500.0,
        };

        let enu = geo.gps_to_enu(&high_gps);
        assert!(
            (enu.up - 490.0).abs() < ENU_M_TOL,
            "Altitude up should be ~490 m, got {} m",
            enu.up
        );

        let back = geo.enu_to_gps(&enu);
        assert!(
            (high_gps.altitude_msl - back.altitude_msl).abs() < ENU_M_TOL,
            "Altitude roundtrip error: {} m",
            (high_gps.altitude_msl - back.altitude_msl).abs()
        );
    }

    // ------------------------------------------------------------------
    // 9. GeoReference::new constructor
    // ------------------------------------------------------------------
    #[test]
    fn test_georeference_new() {
        let geo = GeoReference::new(45.0, -90.0, 100.0);
        assert_eq!(geo.origin_lat, 45.0);
        assert_eq!(geo.origin_lon, -90.0);
        assert_eq!(geo.origin_alt, 100.0);
    }

    // ------------------------------------------------------------------
    // 10. Mercator projection at equator vs poles
    // ------------------------------------------------------------------
    #[test]
    fn test_mercator_equator() {
        let zoom = 10;
        // At the equator, longitude 0 -> pixel x should be at the center of the world
        let (px, py) = lat_lon_to_pixel(0.0, 0.0, zoom);
        let n = (1u32 << zoom) as f64 * 256.0;
        // x should be exactly half of total width
        assert!(
            (px - n / 2.0).abs() < 0.01,
            "Equator x should be at center, got {}",
            px
        );
        // y should also be at half height for equator
        assert!(
            (py - n / 2.0).abs() < 0.01,
            "Equator y should be at center, got {}",
            py
        );
    }

    #[test]
    fn test_mercator_near_pole() {
        let zoom = 10;
        // Near the north pole, y should be close to 0
        let (_, py_north) = lat_lon_to_pixel(85.0, 0.0, zoom);
        assert!(
            py_north > 0.0,
            "North pole y should be positive, got {}",
            py_north
        );

        // Near the south pole, y should be close to max
        let (_, py_south) = lat_lon_to_pixel(-85.0, 0.0, zoom);
        let n = (1u32 << zoom) as f64 * 256.0;
        assert!(
            py_south < n,
            "South pole y should be < {}, got {}",
            n,
            py_south
        );
        assert!(
            py_south > py_north,
            "South pole y ({}) should be > north pole y ({})",
            py_south,
            py_north
        );
    }

    // ------------------------------------------------------------------
    // 11. Mercator roundtrip
    // ------------------------------------------------------------------
    #[test]
    fn test_mercator_roundtrip() {
        let zoom = 15;
        let test_cases = [
            (37.7749, -122.4194), // SF
            (40.7128, -74.0060),  // NYC
            (-33.8688, 151.2093), // Sydney
            (0.0, 0.0),           // Equator/Prime Meridian
            (51.5074, -0.1278),   // London
        ];

        for (lat, lon) in test_cases {
            let (px, py) = lat_lon_to_pixel(lat, lon, zoom);
            let (back_lat, back_lon) = pixel_to_lat_lon(px, py, zoom);

            assert!(
                (lat - back_lat).abs() < 1e-10,
                "Mercator lat roundtrip error for ({}, {}): {} -> {} -> {}",
                lat,
                lon,
                lat,
                back_lat,
                (lat - back_lat).abs()
            );
            assert!(
                (lon - back_lon).abs() < 1e-10,
                "Mercator lon roundtrip error for ({}, {}): {} -> {} -> {}",
                lat,
                lon,
                lon,
                back_lon,
                (lon - back_lon).abs()
            );
        }
    }

    // ------------------------------------------------------------------
    // 12. World <-> ENU roundtrip
    // ------------------------------------------------------------------
    #[test]
    fn test_world_enu_roundtrip() {
        let geo = GeoReference::new(37.7749, -122.4194, 10.0);
        let world = Vec3::new(100.0, 50.0, 200.0);

        let enu = geo.world_to_enu(world);
        let back = geo.enu_to_world(&enu);

        assert!((world.x - back.x).abs() < 1e-4, "world x roundtrip error");
        assert!((world.y - back.y).abs() < 1e-4, "world y roundtrip error");
        assert!((world.z - back.z).abs() < 1e-4, "world z roundtrip error");
    }
}
