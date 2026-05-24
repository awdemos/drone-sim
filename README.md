# Drone Flight Simulator

A complete 3D drone flight simulator built in Rust with LLM integration and an evaluation framework for collecting reasoning traces. The simulator features real-world coordinate systems, procedural/OpenStreetMap city generation, per-drone camera feeds, and a comprehensive telemetry/evaluation system.

## Features

### 3D Simulation
- **Bevy 3D Engine**: High-performance rendering with real-time shadows and PBR materials
- **Procedural Terrain**: Multi-octave noise-based terrain generation with configurable resolution
- **City Generation**: Procedural building and road grid (with optional OpenStreetMap PBF import)
- **Multiple Drones**: Spawn and control multiple drones simultaneously
- **Per-Drone Cameras**: Each drone has its own camera feed rendered to a texture

### GPS & Navigation
- **WGS84 Coordinate System**: Full GPS latitude/longitude/altitude support
- **ENU Local Frame**: Accurate East-North-Up coordinate conversion using geodesic calculations
- ** configurable Origin**: Set any real-world location as the simulation origin
- **Flight Modes**: Manual, Stabilize, AltHold, Loiter, Guided, Auto (waypoint), RTL, Land

### LLM Integration
- **Multi-Provider Support**: Ollama (local), OpenAI, Anthropic-compatible APIs
- **Visual LLM Support**: Send drone camera frames to multimodal models (LLaVA, GPT-4V, etc.)
- **Structured Decisions**: LLM outputs JSON actions (hover, move, rotate, land, etc.)
- **Reasoning Traces**: Full chain-of-thought capture with confidence scores

### Evaluation Framework
- **Event Tracing**: All simulation events recorded with timestamps
- **Metrics Collection**: Distance, speed, altitude, battery efficiency per drone
- **Frame Capture**: Periodic camera frame storage for visual analysis
- **Trace Export**: JSON export of complete simulation traces
- **Reasoning Export**: Separate export of LLM reasoning chains

## Quick Start

```bash
# Generate default configuration
cargo run -- --gen-config

# Run with 3 drones at San Francisco coordinates
cargo run -- --drones 3 --lat 37.7749 --lon -122.4194

# Run with LLM enabled (requires Ollama running locally)
cargo run -- --llm --provider ollama --drones 1

# Run with OpenAI
cargo run -- --llm --provider openai --drones 1
```

## Controls

### Drone Control (when selected)
- `W/S` - Forward/Backward
- `A/D` - Left/Right
- `Space/Shift` - Ascend/Descend
- `Q/E` - Yaw Left/Right

### Flight Modes
- `1` - Manual
- `2` - Stabilize
- `3` - AltHold
- `4` - Guided
- `5` - Auto
- `6` - RTL (Return to Launch)
- `7` - Land

### Camera
- `Arrow Keys` - Move viewport camera
- `Page Up/=` / `Page Down/-` - Zoom in/out

## Configuration

Create `config/sim.toml`:

```toml
[window]
title = "Drone Flight Simulator"
width = 1600
height = 900

[world]
origin_lat = 37.7749
origin_lon = -122.4194
terrain_size_m = 2000.0
terrain_resolution = 256
max_building_height = 30.0
procedural_vegetation = true
# osm_path = "path/to/map.osm.pbf"  # Optional real OSM data

[drone]
spawn_count = 2
spawn_altitude_agl = 10.0
max_speed_ms = 15.0
max_climb_rate_ms = 5.0
camera_fov_degrees = 90.0
camera_resolution = [640, 480]
battery_duration_secs = 600.0

[llm]
enabled = false
provider = "ollama"
model = "llava:13b"
api_url = "http://localhost:11434/api/generate"
max_tokens = 512
temperature = 0.7
decision_interval_secs = 2.0
visual_mode = true
system_prompt = """You are the AI pilot of a quadcopter drone..."""

[eval]
enabled = true
output_dir = "./traces"
capture_frames = true
capture_interval_secs = 1.0
capture_reasoning = true
max_trace_size_mb = 100
```

## Architecture

```
drone-sim/
├── src/
│   ├── core/          # Types, config, GPS coordinate conversion
│   ├── world/         # Terrain, OSM loading, buildings, environment
│   ├── drone/         # Drone physics, camera, controller, visual capture
│   ├── llm/           # LLM client (Ollama/OpenAI), reasoning traces
│   ├── eval/          # Trace collection, metrics, evaluation framework
│   ├── ui/            # EGUI panels (telemetry, map, LLM reasoning)
│   └── main.rs        # Application entry point
├── config/            # Configuration files
└── traces/            # Output trace directory
```

## Dependencies

- **Bevy 0.14**: 3D engine and ECS
- **bevy_egui**: Immediate-mode GUI
- **geographiclib-rs**: Accurate geodesic calculations
- **geo**: Geospatial types and algorithms
- **reqwest**: HTTP client for LLM APIs
- **serde/json/toml**: Serialization
- **chrono**: Timestamps
- **uuid**: Unique identifiers
- **image**: Image processing for visual LLM

## LLM Setup

The simulator reads API keys automatically from environment variables based on the selected provider. No config file changes needed for keys.

### Ollama (Recommended for local use)
```bash
# Install Ollama from https://ollama.ai
ollama pull llava:13b
ollama serve
# Then run simulator with --llm --provider ollama
```

### Moonshot / Kimi (月之暗面)
```bash
export MOONSHOT_API_KEY=sk-...
cargo run -- --llm --provider moonshot --drones 1
```

Or use the `kimi` alias (same backend):
```bash
export KIMI_API_KEY=sk-...
cargo run -- --llm --provider kimi --drones 1
```

Default model: `moonshot-v1-8k`  
Default endpoint: `https://api.moonshot.cn/v1/chat/completions`

**Quick start with the provided key:**
```bash
source .env
cargo run -- --llm --provider moonshot --drones 1
```

### OpenAI
```bash
export OPENAI_API_KEY=sk-...
cargo run -- --llm --provider openai --drones 1
```

## Evaluation & Traces

Traces are saved to `./traces/` by default:

- `trace-<timestamp>.json` - Complete event log with telemetry
- `trace-<timestamp>_frames/` - Captured camera frames (if enabled)
- `reasoning_export.json` - LLM reasoning chains (manual export via UI)

Each trace contains:
- Drone spawn/destroy events
- Flight mode changes
- Waypoint reaches
- LLM decisions with confidence scores
- Obstacle detections
- Collision events
- Full sensor snapshots

## License

MIT OR Apache-2.0
