//! Core simulation primitives: configuration, coordinate systems, and shared types.
//!
//! This module contains data structures and utilities used by every other
//! plugin in the simulator. It intentionally has no Bevy dependencies so
//! that config parsing and coordinate math can be unit-tested in isolation.

/// TOML-based configuration structs with serde support.
pub mod config;
/// WGS84 / ENU coordinate conversions and `GeoReference` origin management.
pub mod gps;
/// Shared domain types: drone IDs, flight modes, missions, sensor snapshots, etc.
pub mod types;
/// Web-Mercator tile coordinate conversions shared by the minimap and terrain systems.
pub mod mercator;
