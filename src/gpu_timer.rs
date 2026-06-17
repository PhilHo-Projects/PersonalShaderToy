//! GPU pass timing via wgpu timestamp queries with a non-blocking readback ring.
//!
//! Begin/end timestamps bracket each shader render pass (egui and clear passes
//! are not timed). Results arrive a few frames late through async buffer maps,
//! which is fine for stats and avoids pipeline stalls.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const MAX_PASSES: u32 = 16;
const RING_SIZE: usize = 4;

/// GPU timings for one frame, in pass-encoding order. The frame total is
/// reported separately through `begin_frame`'s return value.
#[derive(Debug, Clone)]
pub struct FrameGpuTiming {
    pub pass_ms: Vec<f64>,
}

pub struct GpuTimer {
    query_set: wgpu::QuerySet,
    resolve_buf: wgpu::Buffer,
    readback: Vec<wgpu::Buffer>,
    /// Slots resolved this or earlier frames, awaiting map completion: (slot, pass_count).
    in_flight: VecDeque<(usize, u32)>,
    free_slots: Vec<usize>,
    completed: Arc<Mutex<Vec<(usize, Vec<u64>)>>>,
    period_ns: f32,
    passes_this_frame: u32,
    latest: Option<FrameGpuTiming>,
}

impl GpuTimer {
    /// Returns None when the device lacks TIMESTAMP_QUERY (e.g. most GL).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu-timer-queries"),
            ty: wgpu::QueryType::Timestamp,
            count: MAX_PASSES * 2,
        });
        let resolve_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-timer-resolve"),
            size: (MAX_PASSES as u64 * 2) * 8,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = (0..RING_SIZE)
            .map(|i| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("gpu-timer-readback-{i}")),
                    size: (MAX_PASSES as u64 * 2) * 8,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Some(Self {
            query_set,
            resolve_buf,
            readback,
            in_flight: VecDeque::new(),
            free_slots: (0..RING_SIZE).collect(),
            completed: Arc::new(Mutex::new(Vec::new())),
            period_ns: queue.get_timestamp_period(),
            passes_this_frame: 0,
            latest: None,
        })
    }

    /// Harvest finished readbacks and reset the per-frame pass counter.
    /// Returns the total GPU ms of each newly completed frame (usually 0 or 1
    /// entries; more if several readbacks landed at once) so callers can feed
    /// per-frame histories without double-counting stale values.
    pub fn begin_frame(&mut self) -> Vec<f64> {
        self.passes_this_frame = 0;
        let done: Vec<(usize, Vec<u64>)> = {
            let mut guard = self.completed.lock().unwrap();
            guard.drain(..).collect()
        };
        let mut new_totals = Vec::new();
        for (slot, raw) in done {
            if let Some(pos) = self.in_flight.iter().position(|(s, _)| *s == slot) {
                let (_, pass_count) = self.in_flight.remove(pos).unwrap();
                let mut pass_ms = Vec::with_capacity(pass_count as usize);
                for p in 0..pass_count as usize {
                    let begin = raw[p * 2];
                    let end = raw[p * 2 + 1];
                    let delta_ns = end.saturating_sub(begin) as f64 * self.period_ns as f64;
                    pass_ms.push(delta_ns / 1_000_000.0);
                }
                new_totals.push(pass_ms.iter().sum());
                self.latest = Some(FrameGpuTiming { pass_ms });
                self.free_slots.push(slot);
            }
        }
        new_totals
    }

    /// Allocate begin/end query indices for the next shader pass this frame.
    pub fn pass_timestamp_writes(&mut self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if self.passes_this_frame >= MAX_PASSES {
            return None;
        }
        let base = self.passes_this_frame * 2;
        self.passes_this_frame += 1;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(base),
            end_of_pass_write_index: Some(base + 1),
        })
    }

    /// Encode resolve + copy into a free readback slot. Call after the shader
    /// passes, before submit. Skips silently when the ring is saturated (that
    /// frame simply has no GPU sample).
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<usize> {
        if self.passes_this_frame == 0 {
            return None;
        }
        let slot = self.free_slots.pop()?;
        let count = self.passes_this_frame * 2;
        encoder.resolve_query_set(&self.query_set, 0..count, &self.resolve_buf, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve_buf,
            0,
            &self.readback[slot],
            0,
            count as u64 * 8,
        );
        self.in_flight.push_back((slot, self.passes_this_frame));
        Some(slot)
    }

    /// Kick off the async map for the slot returned by `resolve`. Call after
    /// `queue.submit`.
    pub fn after_submit(&mut self, slot: usize) {
        let Some(&(_, pass_count)) = self.in_flight.iter().find(|(s, _)| *s == slot) else {
            return;
        };
        let byte_len = (pass_count as u64 * 2) * 8;
        let buffer = self.readback[slot].clone();
        let completed = self.completed.clone();
        self.readback[slot]
            .slice(0..byte_len)
            .map_async(wgpu::MapMode::Read, move |result| {
                if result.is_ok() {
                    let raw: Vec<u64> = {
                        let view = buffer.slice(0..byte_len).get_mapped_range();
                        bytemuck::cast_slice::<u8, u64>(&view).to_vec()
                    };
                    buffer.unmap();
                    completed.lock().unwrap().push((slot, raw));
                }
            });
    }

    pub fn latest(&self) -> Option<&FrameGpuTiming> {
        self.latest.as_ref()
    }
}
