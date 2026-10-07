//! Frame readback spike for PLAN.md "to check" item 3.
//!
//! Renders a scene into offscreen world colour + depth, clears GUI and hand layers, copies
//! all layers GPU -> CPU every frame and writes them into a file laid out like the bridge's
//! `/tmp/ermc/frames.shm` (bridge_protocol.h, ERMC_FRAMES_VERSION 2): rows flipped
//! bottom-up, three slots, seq odd while writing. Measures the sustained frame rate with
//! the readback pipelined over `in_flight` frames, and fully synchronous for comparison.
//!
//! Usage: frame-readback <out_file>

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering, fence};
use std::time::{Duration, Instant};

const MAX_W: u64 = 1920;
const MAX_H: u64 = 1200;
const SLOTS: u64 = 3;
const FRAME_HDR: u64 = 0x100;
const SLOT_SIZE: u64 = FRAME_HDR + MAX_W * MAX_H * 16;
const FILE_SIZE: u64 = 0x1000 + SLOTS * SLOT_SIZE;
const MAGIC: u32 = 0x524D_484D; // "MHMR"

const SHADER: &str = r#"
struct Out { @builtin(position) pos: vec4<f32>, @location(0) col: vec4<f32> };

fn hash(n: u32) -> f32 {
    var x = n * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    return f32((x >> 22u) ^ x) / 4294967295.0;
}

@vertex
fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    let c = vec2(hash(i * 3u) * 2.2 - 1.1, hash(i * 3u + 1u) * 2.2 - 1.1);
    let p = c + corners[v] * 0.12;
    var o: Out;
    o.pos = vec4(p, hash(i * 3u + 2u), 1.0);
    o.col = vec4(hash(i), hash(i + 7u), hash(i + 13u), 1.0);
    return o;
}

@fragment
fn fs(o: Out) -> @location(0) vec4<f32> { return o.col; }
"#;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

struct Case {
    w: u32,
    h: u32,
    /// world colour + world depth + GUI + hand when true; GUI + hand only when false
    world: bool,
    in_flight: usize,
}

#[derive(Default)]
struct Stats {
    fps: f64,
    submit: Duration,
    wait: Duration,
    copy: Duration,
    worst: Duration,
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
}

fn main() {
    let out = std::env::args().nth(1).expect("usage: frame-readback <out_file>");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&out)
        .expect("open out file");
    file.set_len(FILE_SIZE).expect("size out file");
    let mut map = unsafe { memmap2::MmapMut::map_mut(&file) }.expect("mmap");
    map[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    map[4..8].copy_from_slice(&2u32.to_le_bytes());

    let gpu = pollster::block_on(init());

    let cases = [
        Case { w: 1920, h: 1200, world: true, in_flight: 3 },
        Case { w: 1920, h: 1200, world: true, in_flight: 1 },
        Case { w: 1280, h: 800, world: true, in_flight: 3 },
        Case { w: 1280, h: 800, world: true, in_flight: 1 },
        Case { w: 1920, h: 1200, world: false, in_flight: 3 },
        Case { w: 1280, h: 800, world: false, in_flight: 3 },
    ];

    println!(
        "{:>9} {:>6} {:>9} {:>8} {:>8} {:>10} {:>10} {:>10} {:>10}",
        "size", "layers", "in_flight", "MB/frame", "fps", "submit_ms", "wait_ms", "copy_ms", "worst_ms"
    );
    for c in &cases {
        let layers = if c.world { 4 } else { 2 };
        let mb = (c.w as f64 * c.h as f64 * 4.0 * layers as f64) / 1e6;
        let s = run(&gpu, c, &mut map, 60, 600);
        println!(
            "{:>9} {:>6} {:>9} {:>8.1} {:>8.1} {:>10.2} {:>10.2} {:>10.2} {:>10.2}",
            format!("{}x{}", c.w, c.h),
            layers,
            c.in_flight,
            mb,
            s.fps,
            ms(s.submit),
            ms(s.wait),
            ms(s.copy),
            ms(s.worst)
        );
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

async fn init() -> Gpu {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await
        .expect("adapter");
    let info = adapter.get_info();
    println!("adapter: {} ({:?})", info.name, info.backend);
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .expect("device");

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scene"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("scene"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    Gpu { device, queue, pipeline }
}

fn texture(device: &wgpu::Device, w: u32, h: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn color_attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
    }
}

fn run(gpu: &Gpu, c: &Case, map: &mut memmap2::MmapMut, warmup: usize, frames: usize) -> Stats {
    let (w, h) = (c.w, c.h);
    let row = w as usize * 4;
    assert!(row % 256 == 0, "bytes_per_row must be a multiple of 256");
    let layer_bytes = row * h as usize;

    let world = texture(&gpu.device, w, h, COLOR_FORMAT);
    let depth = texture(&gpu.device, w, h, DEPTH_FORMAT);
    let gui = texture(&gpu.device, w, h, COLOR_FORMAT);
    let hand = texture(&gpu.device, w, h, COLOR_FORMAT);
    let views: Vec<_> = [&world, &depth, &gui, &hand]
        .iter()
        .map(|t| t.create_view(&Default::default()))
        .collect();

    // Slot layer order from bridge_protocol.h: world RGBA, world depth, GUI RGBA, hand RGBA.
    let layers: Vec<(usize, &wgpu::Texture, wgpu::TextureAspect)> = if c.world {
        vec![
            (0, &world, wgpu::TextureAspect::All),
            (1, &depth, wgpu::TextureAspect::DepthOnly),
            (2, &gui, wgpu::TextureAspect::All),
            (3, &hand, wgpu::TextureAspect::All),
        ]
    } else {
        vec![(2, &gui, wgpu::TextureAspect::All), (3, &hand, wgpu::TextureAspect::All)]
    };

    let sets: Vec<Vec<wgpu::Buffer>> = (0..c.in_flight)
        .map(|_| {
            layers
                .iter()
                .map(|_| {
                    gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: layer_bytes as u64,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                })
                .collect()
        })
        .collect();
    let ready: Vec<Arc<AtomicUsize>> = (0..c.in_flight).map(|_| Arc::new(AtomicUsize::new(0))).collect();
    let mut subs: Vec<Option<wgpu::SubmissionIndex>> = vec![None; c.in_flight];

    let mut stats = Stats::default();
    let mut t0 = Instant::now();
    let total = warmup + frames;
    let flags: u32 = if c.world { 0b111 } else { 0b110 };

    for f in 0..total + c.in_flight - 1 {
        if f == warmup {
            t0 = Instant::now();
            stats = Stats::default();
        }
        let frame_start = Instant::now();

        if f < total {
            let s = f % c.in_flight;
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            if c.world {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("world"),
                    color_attachments: &[Some(color_attachment(&views[0]))],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &views[1],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&gpu.pipeline);
                pass.draw(0..6, 0..2000);
            }
            for v in [&views[2], &views[3]] {
                enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("overlay"),
                    color_attachments: &[Some(color_attachment(v))],
                    ..Default::default()
                });
            }
            for ((_, tex, aspect), buf) in layers.iter().zip(&sets[s]) {
                enc.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: *aspect,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: buf,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(row as u32),
                            rows_per_image: Some(h),
                        },
                    },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            }
            subs[s] = Some(gpu.queue.submit([enc.finish()]));
            for buf in &sets[s] {
                let r = ready[s].clone();
                buf.map_async(wgpu::MapMode::Read, .., move |res| {
                    res.expect("map");
                    r.fetch_add(1, Ordering::Release);
                });
            }
        }
        let after_submit = Instant::now();

        // Consume the oldest frame in flight.
        if f + 1 >= c.in_flight {
            let done = f + 1 - c.in_flight;
            let o = done % c.in_flight;
            gpu.device
                .poll(wgpu::PollType::Wait { submission_index: subs[o].take(), timeout: None })
                .expect("poll");
            assert_eq!(ready[o].swap(0, Ordering::Acquire), layers.len());
            let after_wait = Instant::now();

            let slot = (done as u64 % SLOTS) as usize;
            let base = 0x1000 + slot * SLOT_SIZE as usize;
            write_u32(map, base, 1); // seq odd: writing
            write_u32(map, base + 4, w);
            write_u32(map, base + 8, h);
            write_u32(map, base + 12, flags);
            map[base + 0x10..base + 0x18].copy_from_slice(&(done as u64).to_le_bytes());
            fence(Ordering::Release);
            for ((idx, _, _), buf) in layers.iter().zip(&sets[o]) {
                let src = buf.get_mapped_range(..).expect("mapped range");
                let dst = base + FRAME_HDR as usize + idx * layer_bytes;
                // GPU rows are top-down; the bridge wants bottom-up (OpenGL order).
                for y in 0..h as usize {
                    let s = y * row;
                    let d = dst + (h as usize - 1 - y) * row;
                    map[d..d + row].copy_from_slice(&src[s..s + row]);
                }
                drop(src);
                buf.unmap();
            }
            fence(Ordering::Release);
            write_u32(map, base, 2);
            write_u32(map, 8, slot as u32);
            map[0x10..0x18].copy_from_slice(&(done as u64).to_le_bytes());
            let after_copy = Instant::now();

            if f >= warmup {
                stats.wait += after_wait - after_submit;
                stats.copy += after_copy - after_wait;
            }
        }
        if f >= warmup {
            stats.submit += after_submit - frame_start;
            stats.worst = stats.worst.max(frame_start.elapsed());
        }
    }

    let elapsed = t0.elapsed().as_secs_f64();
    let n = frames as u32;
    stats.fps = frames as f64 / elapsed;
    stats.submit /= n;
    stats.wait /= n;
    stats.copy /= n;
    stats
}

fn write_u32(map: &mut memmap2::MmapMut, at: usize, v: u32) {
    map[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
