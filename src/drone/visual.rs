use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::render_asset::RenderAssets;
use bevy::render::texture::GpuImage;
use bevy::render::render_resource::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d,
    ImageCopyBuffer, ImageCopyTexture, TextureAspect,
};
use crossbeam_channel::{Sender, Receiver};
use crate::core::config::EvalConfig;
use crate::drone::{DroneIdentity, camera::DroneCameraMap};
use crate::drone::camera::DroneCamera;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

/// Resource holding the latest camera frames for each drone
#[derive(Resource, Default)]
pub struct VisualFrameBuffer {
    pub frames: std::collections::HashMap<crate::core::types::DroneId, Arc<Mutex<Vec<u8>>>>,
    pub last_capture: std::collections::HashMap<crate::core::types::DroneId, f32>,
}

/// Channel for sending capture jobs from main world to render world
#[derive(Resource, Clone)]
pub struct CaptureJobSender {
    pub sender: Sender<CaptureJob>,
}

/// Channel for receiving capture results from render world
#[derive(Resource, Clone)]
pub struct CaptureResultReceiver {
    pub receiver: Receiver<CaptureResult>,
}

/// A request to capture a frame from a drone camera
#[derive(Clone)]
pub struct CaptureJob {
    pub drone_id: crate::core::types::DroneId,
    pub handle: Handle<Image>,
}

/// Result of a frame capture
#[derive(Clone)]
pub struct CaptureResult {
    pub drone_id: crate::core::types::DroneId,
    pub data: Vec<u8>,
}

/// Resources for the render world side of capture
#[derive(Resource, Clone)]
pub struct CaptureJobReceiver {
    pub receiver: Receiver<CaptureJob>,
}

#[derive(Resource, Clone)]
pub struct CaptureResultSender {
    pub sender: Sender<CaptureResult>,
}

/// Main-world system: queue capture jobs and collect results
pub fn capture_visual_frames(
    time: Res<Time>,
    mut frame_buffer: ResMut<VisualFrameBuffer>,
    drone_query: Query<&DroneIdentity>,
    camera_map: Res<DroneCameraMap>,
    eval_config: Res<EvalConfig>,
    job_sender: Res<CaptureJobSender>,
    result_receiver: Res<CaptureResultReceiver>,
) {
    if !eval_config.capture_frames {
        return;
    }

    while let Ok(result) = result_receiver.receiver.try_recv() {
        frame_buffer.frames.insert(result.drone_id, Arc::new(Mutex::new(result.data)));
    }

    let now = time.elapsed_seconds();
    let interval = eval_config.capture_interval_secs;

    for identity in drone_query.iter() {
        let last = frame_buffer.last_capture.get(&identity.id).copied().unwrap_or(0.0);
        if now - last < interval {
            continue;
        }

        if let Some(handle) = camera_map.images.get(&identity.id) {
            let _ = job_sender.sender.send(CaptureJob {
                drone_id: identity.id,
                handle: handle.clone(),
            });
            frame_buffer.last_capture.insert(identity.id, now);
        }
    }
}

/// Render-world system: process pending capture jobs
pub fn process_frame_captures(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    job_receiver: Res<CaptureJobReceiver>,
    result_sender: Res<CaptureResultSender>,
) {
    while let Ok(job) = job_receiver.receiver.try_recv() {
        if let Some(gpu_image) = gpu_images.get(job.handle.id()) {
            let width = gpu_image.size.x;
            let height = gpu_image.size.y;
            // WGPU requires bytes_per_row to be a multiple of 256 for buffer copies.
            let bytes_per_row = ((width * 4 + 255) / 256) * 256;
            let buffer_size = (bytes_per_row * height) as u64;

            let buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some("frame_capture"),
                size: buffer_size,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("frame_capture"),
            });

            encoder.copy_texture_to_buffer(
                ImageCopyTexture {
                    texture: &gpu_image.texture,
                    mip_level: 0,
                    origin: bevy::render::render_resource::Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                ImageCopyBuffer {
                    buffer: &buffer,
                    layout: bevy::render::render_resource::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );

            render_queue.submit(std::iter::once(encoder.finish()));

            let (tx, rx) = std::sync::mpsc::channel();
            let slice = buffer.slice(..);
            slice.map_async(bevy::render::render_resource::MapMode::Read, move |result| {
                if tx.send(result).is_err() {
                    eprintln!("Frame capture callback channel closed");
                }
            });
            render_device.poll(bevy::render::render_resource::Maintain::Wait);

            if rx.recv().is_ok() {
                let data = slice.get_mapped_range().to_vec();
                buffer.unmap();

                if result_sender.sender.send(CaptureResult {
                    drone_id: job.drone_id,
                    data,
                }).is_err() {
                    eprintln!("Frame capture result channel closed");
                }
            }
        }
    }
}

pub fn cleanup_visual_buffer(
    mut events: EventReader<crate::drone::DroneDestroyedEvent>,
    mut frame_buffer: ResMut<VisualFrameBuffer>,
) {
    for event in events.read() {
        frame_buffer.frames.remove(&event.drone_id);
        frame_buffer.last_capture.remove(&event.drone_id);
        debug!("Cleaned up VisualFrameBuffer for drone {:?}", event.drone_id);
    }
}

/// Event requesting a screenshot of all drone cameras
#[derive(Event)]
pub struct ScreenshotEvent;

/// Keyboard system: trigger screenshot on F12
pub fn screenshot_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut events: EventWriter<ScreenshotEvent>,
) {
    if keyboard.just_pressed(KeyCode::F12) {
        events.send(ScreenshotEvent);
    }
}

/// Save buffered frames to PNG when screenshot is requested
pub fn save_screenshots(
    mut events: EventReader<ScreenshotEvent>,
    frame_buffer: Res<VisualFrameBuffer>,
    camera_query: Query<(&DroneCamera, &DroneIdentity)>,
) {
    let _ = std::fs::create_dir_all("data/screenshots");

    for _event in events.read() {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");

        for (drone_camera, identity) in camera_query.iter() {
            if let Some(frame) = frame_buffer.frames.get(&identity.id) {
                let data = frame.lock().unwrap();
                let (width, height) = drone_camera.resolution;
                let bytes_per_row = ((width * 4 + 255) / 256) * 256;

                let mut img_data = Vec::with_capacity((width * height * 4) as usize);
                for row in 0..height {
                    let row_start = (row * bytes_per_row) as usize;
                    let row_end = row_start + (width * 4) as usize;
                    if row_end <= data.len() {
                        img_data.extend_from_slice(&data[row_start..row_end]);
                    }
                }

                if let Some(img) = image::RgbaImage::from_raw(width, height, img_data) {
                    let path = PathBuf::from(format!(
                        "data/screenshots/{}_{}.png",
                        timestamp, identity.name.replace(' ', "_")
                    ));
                    if let Err(e) = img.save(&path) {
                        eprintln!("Failed to save screenshot: {}", e);
                    } else {
                        info!("Screenshot saved: {:?}", path);
                    }
                }
            }
        }
    }
}
