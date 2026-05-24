# Drone-Sim Agent Guide

## Quick Commands

```bash
# Run (dev build with dynamic linking)
cargo run

# Run with options
cargo run -- --drones 3 --lat 37.7749 --lon -122.4194
cargo run -- --llm --provider moonshot --drones 1

# Generate default config
cargo run -- --gen-config

# Release build (slower compile, faster runtime)
cargo run --release
```

## Architecture

- **Bevy 0.14** ECS engine with `dynamic_linking` feature enabled for fast dev builds
- **Entry**: `src/main.rs` — sets up plugins, spawns scene, runs event loop
- **Plugins** (load order matters):
  1. `SplashPlugin` → `WorldPlugin` → `DronePlugin` → `LlmPlugin` → `EvalPlugin` → `UiPlugin`
  2. `DronePlugin` sub-app: `RenderApp` for async frame capture (crossbeam channels)
- **Key modules**:
  - `core/` — Config (TOML), GPS/ENU conversion, Sim types
  - `world/` — Terrain generation, OSM loading, satellite tile downloads, buildings
  - `drone/` — Physics, camera (render-to-texture), controller input, visual capture
  - `llm/` — HTTP client for Ollama/OpenAI/Moonshot, reasoning trace collection
  - `ui/` — EGUI panels: drone control, telemetry, map tiles, mission planner, LLM reasoning
  - `eval/` — Metrics, trace collector, event buffering

## Critical Code Patterns

### Drone Selection
- `DroneInputState.selected_drone: Option<DroneId>` controls which drone receives keyboard input
- `None` means ALL drones receive input simultaneously (`unwrap_or(true)` in controller)
- UI panel allows selecting individual drones or "Select All"
- When spawning drones, first drone should be auto-selected

### Camera Controls (main 3D viewport)
- **Middle-click drag** — Orbit camera
- **Right-click drag** — Pan camera (disables drone follow)
- **Scroll** — Zoom
- **Arrow keys** — Keyboard pan
- **F** — Toggle follow-first-drone mode
- **Ctrl + mouse** — Alternative pan/zoom controls

### Map Panel (EGUI window)
- Drag to pan, scroll to zoom (zooms around cursor)
- Provider switcher: OpenStreetMap / Esri Satellite / Esri Street / Carto Dark / Carto Light
- Grid lines and labels toggle
- Drone positions overlaid as red dots with heading arrows

### Satellite Terrain (3D ground plane)
- Downloads ESRI satellite tiles at computed zoom level (~16)
- Builds texture atlas from tiles, applies to procedural terrain mesh
- Cached at `data/tiles/{z}/{x}/{y}.jpg` (old flat structure)
- Separate from map panel tile cache which uses `data/tiles/{provider}_{z}/{x}_{y}.{ext}`

## Configuration

- Config file: `config/sim.toml` (TOML format, optional — falls back to defaults)
- Env vars for LLM keys: `MOONSHOT_API_KEY`, `KIMI_API_KEY`, `OPENAI_API_KEY`
- CLI overrides: `--drones`, `--lat`, `--lon`, `--llm`, `--provider`

## Build Notes

- Requires `protoc` for OSM PBF support (currently disabled — `osmpbfreader` commented out)
- Uses `dynamic_linking` feature — first build links Bevy dylibs, subsequent builds are fast
- Tile download creates `data/tiles/` directory automatically
- Traces written to `./traces/` by default

## Common Issues

- **No OSM data**: Falls back to procedural city (empty path triggers default)
- **LLM without key**: Prints warning, continues without LLM decisions
- **Map tiles black/pixelated**: Check network, old cache may need clearing (`rm -rf data/tiles/`)
- **No camera feeds**: Requires render-to-texture setup in `DronePlugin` render sub-app
