# World Module

## Overview
Procedural world generation: terrain, OSM city data, buildings, satellite textures.

## Structure
```
src/world/
├── mod.rs              # Plugin, spatial grid, world reload
├── terrain.rs          # Heightmap generation, TerrainData resource
├── osm_loader.rs       # OSM XML parsing, procedural city fallback
├── buildings.rs        # Building/road mesh spawning, colliders
├── satellite_terrain.rs # ESRI tile download, texture atlas
└── earth_backdrop.rs   # Distant Earth sphere for atmosphere
```

## Where to Look
| Task | File | Notes |
|------|------|-------|
| Change terrain | `terrain.rs` | `TerrainData::generate()` from `WorldConfig` |
| Load OSM data | `osm_loader.rs` | `load_osm_data(path, geo)` — empty path = procedural |
| Spawn buildings | `buildings.rs` | `spawn_buildings_and_roads_impl()` |
| Satellite tiles | `satellite_terrain.rs` | Downloads to `data/tiles/{z}/{x}/{y}.jpg` |
| World reload | `mod.rs` | `reload_osm_data` system handles `ReloadOsmEvent` |
| Collision grid | `mod.rs` | `SpatialGrid` resource, 50m cell size |

## Critical Patterns

### World Reload Flow
```
ReloadOsmEvent → despawn_world_entities → reload_osm_data
```
- Clears `SpatialGrid`
- Generates new `TerrainData`
- Spawns buildings + roads
- Starts satellite tile downloads

### OSM Fallback
Empty path or failed load → procedural city generation:
```rust
load_osm_data(Path::new(""), geo) // never fails
```

### Spatial Grid
- `SpatialGrid::new(50.0)` — 50m cells for building collision
- `insert()` adds building to all overlapping cells
- `query_near(position, radius)` for O(1) lookups

### Satellite Tile Cache
- Old flat structure: `data/tiles/{z}/{x}/{y}.jpg`
- Map panel uses separate: `data/tiles/{provider}_{z}/{x}_{y}.{ext}`
- Downloaded at computed zoom (~16)

## Anti-Patterns
- **DO NOT** forget to tag spawned entities with `WorldEntity` — required for cleanup
- **DO NOT** query buildings directly — use `SpatialGrid.query_near()`
- **DO NOT** call `load_osm_with_fallback` from here — it's in `main.rs` (tech debt)
