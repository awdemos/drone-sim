# Core Module

## Overview
Simulation primitives: config, coordinate math, shared types. No Bevy deps — unit-testable in isolation.

## Structure
```
src/core/
├── mod.rs      # Module docs, re-exports
├── config.rs   # TOML config structs with serde
├── gps.rs      # WGS84/ENU conversions, GeoReference origin
├── types.rs    # Domain types: DroneId, FlightMode, Mission, etc.
└── mercator.rs # Web-Mercator tile math
```

## Where to Look
| Task | File | Notes |
|------|------|-------|
| Add config field | `config.rs` | Add to struct + `Default` impl |
| GPS conversion | `gps.rs` | `GeoReference::world_to_gps()` / `gps_to_world()` |
| New domain type | `types.rs` | `SimTimestamp`, `SensorSnapshot`, `Pose` |
| Tile coordinates | `mercator.rs` | Lat/lon ↔ tile X/Y at zoom level |

## Critical Patterns

### Config Loading
```rust
let config = load_config(Some(Path::new("config/sim.toml")));
// Falls back to Default if file missing or field absent
```
- `DroneConfig.spawn_count` controls fleet size
- `WorldConfig.origin_lat/lon` sets simulation origin
- Env vars: `MOONSHOT_API_KEY`, `KIMI_API_KEY`, `OPENAI_API_KEY`

### Coordinate Systems
- **WGS84**: Lat/lon/alt (GPS coordinates)
- **ENU**: East-North-Up local Cartesian (simulation units)
- **GeoReference**: Origin anchor, converts between systems

### Type Design
- `DroneId`: UUID v4 wrapper
- `SimTimestamp`: Monotonic frame counter
- `FlightMode`: Stabilize / AltHold / Loiter / Auto / RTL / Land
- All config types derive `serde::Deserialize` + `Clone`

## Anti-Patterns
- **DO NOT** add Bevy deps here — keep it pure for testing
- **DO NOT** use `CameraMode` outside UI/camera context (currently in types.rs)
- **DO NOT** hardcode defaults in multiple places — `Default` impl is source of truth
