# Drone Module

## Overview
Drone simulation: physics, flight control, per-drone cameras, and visual frame capture.

## Structure
```
src/drone/
├── mod.rs          # Plugin, system sets, components, fleet resource
├── physics.rs      # Gravity, integration, terrain/building collision
├── camera.rs       # Render-to-texture cameras, camera transform updates
├── controller.rs   # Keyboard input, flight mode logic, drone selection
├── visual.rs       # Async frame capture via RenderApp sub-app
└── types.rs        # DroneTypeRegistry (10 hardcoded variants)
```

## Where to Look
| Task | File | Notes |
|------|------|-------|
| Add drone type | `types.rs` | Add variant + spec to `DroneTypeRegistry::default()` |
| Change physics | `physics.rs` | `apply_gravity`, `update_physics` in `DroneSystemSet::Physics` |
| Camera setup | `camera.rs` | Spawned as child entities, `DroneCameraMap` resource |
| Input handling | `controller.rs` | `DroneInputState.selected_drone` controls target |
| Visual capture | `visual.rs` | Crossbeam channels between Update and Render sets |
| Fleet query | `mod.rs` | `Fleet.drones: HashMap<DroneId, Entity>` |

## Critical Patterns

### System Ordering
```
Input → FlightControl → Physics → (Collision | Camera | Visual)
```
Configured in `DronePlugin.build()` via `DroneSystemSet`.

### RenderApp Sub-App
Frame capture runs in Bevy's `RenderApp` (not main `App`):
- `CaptureJobSender` / `CaptureResultReceiver` in main app
- `CaptureJobReceiver` / `CaptureResultSender` in render app
- `process_frame_captures` runs in `RenderSet::Cleanup`

### Drone Selection
- `DroneInputState.selected_drone: Option<DroneId>`
- `None` = ALL drones receive input (`unwrap_or(true)`)
- First drone auto-selected on spawn (`select_first_drone` system)

### Spawn Pipeline
`spawn_drone_bundle()` spawns components, then `spawn_drones_impl()` adds:
- PBR body mesh
- Arm meshes (children)
- VTOL wing (if applicable)

## Anti-Patterns
- **DO NOT** block in visual capture systems — use channels
- **DO NOT** query `DroneIdentity` directly for lookup — use `Fleet`
- **DO NOT** modify `DroneCameraMap` outside `camera.rs` cleanup systems
