use bevy::prelude::*;

/// Event to despawn all world entities (buildings, roads, terrain)
#[derive(Event)]
pub struct DespawnWorldEvent;

/// Event to reload OSM data at a new location
#[derive(Event)]
pub struct ReloadOsmEvent {
    pub lat: f64,
    pub lon: f64,
}

/// Event to respawn all drones
#[derive(Event)]
pub struct RespawnDronesEvent;

/// Event to clear all drone data
#[derive(Event)]
pub struct ClearDroneDataEvent;
