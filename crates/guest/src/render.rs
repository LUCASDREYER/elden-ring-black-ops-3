//! wgpu rendering for Elden Ring: the world layer (colour + depth, depth-tested against
//! Elden Ring's scene) and the hand layer (the first-person weapon, never occluded), both
//! read back each frame. Also a preview of the same frame in the guest's own window.
//!
//! Depth convention: a standard left-handed perspective (glam's `lh::proj::directx`, near -> 0,
//! far -> 1). With the same near and far planes that equals OpenGL window depth, which is
//! what the bridge's compositor linearises, so no conversion pass is needed.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::viewmodel::{SkinVertex, Viewmodel};

/// Matches the bridge's frame layout: BGRA in memory (`DXGI_FORMAT_B8G8R8A8_UNORM`).
pub const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HandGlobals {
    view_proj: [[f32; 4]; 4],
    to_view: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialParams {
    tint: [f32; 4],
    flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FlashUniform {
    proj: [[f32; 4]; 4],
    center: [f32; 4],
    params: [f32; 4],
}

/// A muzzle flash this frame.
#[derive(Clone, Copy, Debug)]
pub struct MuzzleFlash {
    /// The muzzle in camera space, metres.
    pub center: glam::Vec3,
    pub half_size: f32,
    pub intensity: f32,
    pub rotation: f32,
}

/// What to draw in the hand layer this frame.
#[derive(Clone, Copy)]
pub struct HandFrame<'a> {
    /// Eye space (inches) to clip space.
    pub view_proj: Mat4,
    /// Eye space to camera space, for lighting.
    pub to_view: Mat4,
    /// Camera space to clip space (the flash is built in camera space).
    pub proj: Mat4,
    /// Per part skinning matrices (see [`Viewmodel::skin`]).
    pub skins: &'a [Vec<Mat4>],
    pub flash: Option<MuzzleFlash>,
}

/// The layers read back for one frame, rows top-down `stride` bytes apart.
pub struct Layers<'a> {
    pub world: &'a [u8],
    pub depth: &'a [u8],
    pub hand: Option<&'a [u8]>,
    pub stride: usize,
}

struct Readback {
    width: u32,
    height: u32,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    hand: wgpu::Texture,
    hand_view: wgpu::TextureView,
    hand_depth_view: wgpu::TextureView,
    color_buf: wgpu::Buffer,
    depth_buf: wgpu::Buffer,
    hand_buf: wgpu::Buffer,
    stride: u32,
}

struct Preview {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    hand_pipeline: wgpu::RenderPipeline,
    depth_view: wgpu::TextureView,
}

struct GpuPart {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    bones: wgpu::Buffer,
    bone_bind: wgpu::BindGroup,
    draws: Vec<(u32, u32, usize)>,
    materials: Vec<wgpu::BindGroup>,
}

struct FlashGpu {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

struct Hand {
    flash: FlashGpu,
    layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    preview_globals: wgpu::Buffer,
    preview_globals_bind: wgpu::BindGroup,
    parts: Vec<GpuPart>,
}

pub struct Renderer {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    module: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    frame_pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    frame_globals: wgpu::Buffer,
    frame_bind: wgpu::BindGroup,
    preview_globals: wgpu::Buffer,
    preview_bind: wgpu::BindGroup,
    readback: Option<Readback>,
    preview: Option<Preview>,
    hand: Option<Hand>,
}

impl Renderer {
    /// With a window, the frame is also previewed there; without one the renderer is headless.
    pub fn new(window: Option<Arc<winit::window::Window>>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = window.as_ref().map(|w| instance.create_surface(w.clone()).expect("surface"));
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface.as_ref(),
            ..Default::default()
        }))
        .expect("adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX), texture_entry(1), sampler_entry(2)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&bind_layout)],
            ..Default::default()
        });
        let frame_pipeline = scene_pipeline(&device, &layout, &module, FRAME_FORMAT);

        let (verts, idx) = cube();
        let vertices = buffer_init(&device, bytemuck::cast_slice(&verts), wgpu::BufferUsages::VERTEX);
        let indices = buffer_init(&device, bytemuck::cast_slice(&idx), wgpu::BufferUsages::INDEX);

        let texture_view = checker_texture(&device, &queue);
        let sampler = linear_sampler(&device);
        let bind = |buf: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene"),
                layout: &bind_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&texture_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            })
        };
        let frame_globals = uniform_buffer(&device, std::mem::size_of::<Globals>());
        let preview_globals = uniform_buffer(&device, std::mem::size_of::<Globals>());
        let frame_bind = bind(&frame_globals);
        let preview_bind = bind(&preview_globals);

        let mut r = Renderer {
            adapter,
            device,
            queue,
            module,
            layout,
            frame_pipeline,
            vertices,
            indices,
            index_count: idx.len() as u32,
            frame_globals,
            frame_bind,
            preview_globals,
            preview_bind,
            readback: None,
            preview: None,
            hand: None,
        };
        if let (Some(surface), Some(window)) = (surface, window) {
            let size = window.inner_size();
            r.attach_preview(surface, size.width, size.height);
        }
        r
    }

    fn attach_preview(&mut self, surface: wgpu::Surface<'static>, width: u32, height: u32) {
        let caps = surface.get_capabilities(&self.adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let mut config = surface
            .get_default_config(&self.adapter, width.max(1), height.max(1))
            .expect("surface config");
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoNoVsync;
        surface.configure(&self.device, &config);
        let pipeline = scene_pipeline(&self.device, &self.layout, &self.module, format);
        let hand_pipeline = match &self.hand {
            Some(h) => hand_pipeline(&self.device, &h.layout, &h.module, format),
            None => pipeline.clone(),
        };
        let depth_view = depth_texture(&self.device, config.width, config.height).create_view(&Default::default());
        self.preview = Some(Preview { surface, config, pipeline, hand_pipeline, depth_view });
    }

    pub fn resize_preview(&mut self, width: u32, height: u32) {
        if let Some(p) = &mut self.preview {
            if width == 0 || height == 0 {
                return;
            }
            p.config.width = width;
            p.config.height = height;
            p.surface.configure(&self.device, &p.config);
            p.depth_view = depth_texture(&self.device, width, height).create_view(&Default::default());
        }
    }

    pub fn preview_aspect(&self) -> f32 {
        self.preview.as_ref().map(|p| p.config.width as f32 / p.config.height as f32).unwrap_or(1.6)
    }

    /// Uploads the first-person weapon's meshes and textures.
    pub fn load_viewmodel(&mut self, vm: &Viewmodel) {
        let d = &self.device;
        let globals_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hand globals"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let bones_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hand bones"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let material_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hand material"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::FRAGMENT), texture_entry(1), sampler_entry(2)],
        });
        let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hand"),
            bind_group_layouts: &[Some(&globals_layout), Some(&bones_layout), Some(&material_layout)],
            ..Default::default()
        });
        let module = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hand"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hand.wgsl").into()),
        });
        let pipeline = hand_pipeline(d, &layout, &module, FRAME_FORMAT);
        let globals_for = |buf: &wgpu::Buffer| {
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hand globals"),
                layout: &globals_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
            })
        };
        let globals = uniform_buffer(d, std::mem::size_of::<HandGlobals>());
        let preview_globals = uniform_buffer(d, std::mem::size_of::<HandGlobals>());
        let globals_bind = globals_for(&globals);
        let preview_globals_bind = globals_for(&preview_globals);

        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white = rgba_texture(d, &self.queue, 1, 1, &[255, 255, 255, 255]);

        let mut parts = Vec::new();
        for part in &vm.parts {
            let vertices = buffer_init(d, bytemuck::cast_slice(&part.vertices), wgpu::BufferUsages::VERTEX);
            let indices = buffer_init(d, bytemuck::cast_slice(&part.indices), wgpu::BufferUsages::INDEX);
            let bones = d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bones"),
                size: (part.bone_count().max(1) * 64) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bone_bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bones"),
                layout: &bones_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: bones.as_entire_binding() }],
            });
            let materials = part
                .materials
                .iter()
                .map(|m| {
                    let view = match &m.texture {
                        Some(t) => rgba_mipmapped(d, &self.queue, t.width, t.height, &t.rgba),
                        None => white.clone(),
                    };
                    let params = MaterialParams { tint: m.tint, flags: [if m.cutout { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0] };
                    let ubuf = buffer_init(d, bytemuck::bytes_of(&params), wgpu::BufferUsages::UNIFORM);
                    d.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&m.name),
                        layout: &material_layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                        ],
                    })
                })
                .collect();
            let draws = part.draws.iter().map(|dr| (dr.first, dr.count, dr.material)).collect();
            parts.push(GpuPart { vertices, indices, bones, bone_bind, draws, materials });
        }
        if let Some(p) = &mut self.preview {
            p.hand_pipeline = hand_pipeline(d, &layout, &module, p.config.format);
        }
        let flash = flash_gpu(d);
        self.hand = Some(Hand { flash, layout, module, pipeline, globals, globals_bind, preview_globals, preview_globals_bind, parts });
    }

    fn draw_scene(
        &self,
        enc: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        clear: wgpu::Color,
    ) {
        let mut pass = begin(enc, "scene", color, depth, wgpu::LoadOp::Clear(clear));
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    fn write_hand(&self, hand: &HandFrame, globals: &wgpu::Buffer) {
        let h = self.hand.as_ref().unwrap();
        let g = HandGlobals { view_proj: hand.view_proj.to_cols_array_2d(), to_view: hand.to_view.to_cols_array_2d() };
        self.queue.write_buffer(globals, 0, bytemuck::bytes_of(&g));
        if let Some(fl) = hand.flash {
            let u = FlashUniform {
                proj: hand.proj.to_cols_array_2d(),
                center: [fl.center.x, fl.center.y, fl.center.z, fl.half_size],
                params: [fl.intensity, fl.rotation, 0.0, 0.0],
            };
            self.queue.write_buffer(&h.flash.uniform, 0, bytemuck::bytes_of(&u));
        }
        for (part, skin) in h.parts.iter().zip(hand.skins) {
            let mats: Vec<[[f32; 4]; 4]> = skin.iter().map(|m| m.to_cols_array_2d()).collect();
            if !mats.is_empty() {
                self.queue.write_buffer(&part.bones, 0, bytemuck::cast_slice(&mats));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_hand(
        &self,
        enc: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        globals_bind: &wgpu::BindGroup,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        flash: bool,
    ) {
        let h = self.hand.as_ref().unwrap();
        let mut pass = begin(enc, "hand", color, depth, load);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, globals_bind, &[]);
        for part in &h.parts {
            pass.set_bind_group(1, &part.bone_bind, &[]);
            pass.set_vertex_buffer(0, part.vertices.slice(..));
            pass.set_index_buffer(part.indices.slice(..), wgpu::IndexFormat::Uint32);
            for &(first, count, mat) in &part.draws {
                pass.set_bind_group(2, &part.materials[mat], &[]);
                pass.draw_indexed(first..first + count, 0, 0..1);
            }
        }
        if flash {
            pass.set_pipeline(&h.flash.pipeline);
            pass.set_bind_group(0, &h.flash.bind, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    /// Renders one frame for Elden Ring at `width` x `height` and reads its layers back,
    /// handing them to `publish` while they're mapped.
    pub fn render_frame(
        &mut self,
        width: u32,
        height: u32,
        view_proj: Mat4,
        model: Mat4,
        hand: Option<&HandFrame>,
        publish: impl FnOnce(&Layers),
    ) {
        if self.readback.as_ref().is_none_or(|r| r.width != width || r.height != height) {
            self.readback = Some(self.make_readback(width, height));
        }
        let globals = Globals { view_proj: view_proj.to_cols_array_2d(), model: model.to_cols_array_2d() };
        self.queue.write_buffer(&self.frame_globals, 0, bytemuck::bytes_of(&globals));
        let hand = hand.filter(|_| self.hand.is_some());
        if let Some(hf) = hand {
            self.write_hand(hf, &self.hand.as_ref().unwrap().globals);
        }

        let rb = self.readback.as_ref().unwrap();
        let mut enc = self.device.create_command_encoder(&Default::default());
        self.draw_scene(&mut enc, &self.frame_pipeline, &self.frame_bind, &rb.color_view, &rb.depth_view, wgpu::Color::TRANSPARENT);
        if hand.is_some() {
            let h = self.hand.as_ref().unwrap();
            let flash = hand.is_some_and(|hf| hf.flash.is_some());
            self.draw_hand(&mut enc, &h.pipeline, &h.globals_bind, &rb.hand_view, &rb.hand_depth_view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), flash);
        }
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let mut copies = vec![
            (&rb.color, wgpu::TextureAspect::All, &rb.color_buf),
            (&rb.depth, wgpu::TextureAspect::DepthOnly, &rb.depth_buf),
        ];
        if hand.is_some() {
            copies.push((&rb.hand, wgpu::TextureAspect::All, &rb.hand_buf));
        }
        for (tex, aspect, buf) in &copies {
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: *aspect },
                wgpu::TexelCopyBufferInfo {
                    buffer: buf,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(rb.stride), rows_per_image: Some(height) },
                },
                extent,
            );
        }
        let sub = self.queue.submit([enc.finish()]);

        let mapped = Arc::new(AtomicUsize::new(0));
        for (_, _, buf) in &copies {
            let m = mapped.clone();
            buf.map_async(wgpu::MapMode::Read, .., move |r| {
                r.expect("map readback");
                m.fetch_add(1, Ordering::Release);
            });
        }
        self.device
            .poll(wgpu::PollType::Wait { submission_index: Some(sub), timeout: None })
            .expect("poll");
        assert_eq!(mapped.load(Ordering::Acquire), copies.len());
        {
            let world = rb.color_buf.get_mapped_range(..).expect("colour range");
            let depth = rb.depth_buf.get_mapped_range(..).expect("depth range");
            let hand_view = hand.map(|_| rb.hand_buf.get_mapped_range(..).expect("hand range"));
            publish(&Layers { world: &world, depth: &depth, hand: hand_view.as_deref(), stride: rb.stride as usize });
        }
        for (_, _, buf) in &copies {
            buf.unmap();
        }
    }

    fn make_readback(&self, width: u32, height: u32) -> Readback {
        let stride = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let color_tex = |label| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FRAME_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let buf = |label| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: stride as u64 * height as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let color = color_tex("world colour");
        let hand = color_tex("hand colour");
        let depth = depth_texture(&self.device, width, height);
        Readback {
            width,
            height,
            color_view: color.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            hand_view: hand.create_view(&Default::default()),
            hand_depth_view: depth_texture(&self.device, width, height).create_view(&Default::default()),
            color,
            depth,
            hand,
            color_buf: buf("world readback"),
            depth_buf: buf("depth readback"),
            hand_buf: buf("hand readback"),
            stride,
        }
    }

    /// Draws the frame into the guest's own window: the world layer over a dark sky, then the
    /// hand layer on top.
    pub fn render_preview(&mut self, view_proj: Mat4, model: Mat4, hand: Option<&HandFrame>) {
        let Some(p) = &self.preview else { return };
        let frame = match p.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                p.surface.configure(&self.device, &p.config);
                return;
            }
            _ => return,
        };
        let globals = Globals { view_proj: view_proj.to_cols_array_2d(), model: model.to_cols_array_2d() };
        self.queue.write_buffer(&self.preview_globals, 0, bytemuck::bytes_of(&globals));
        let hand = hand.filter(|_| self.hand.is_some());
        if let Some(hf) = hand {
            self.write_hand(hf, &self.hand.as_ref().unwrap().preview_globals);
        }
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        let sky = wgpu::Color { r: 0.05, g: 0.06, b: 0.09, a: 1.0 };
        self.draw_scene(&mut enc, &p.pipeline, &self.preview_bind, &view, &p.depth_view, sky);
        if hand.is_some() {
            // The hand is never occluded: clear depth, keep the colour.
            let h = self.hand.as_ref().unwrap();
            // The flash pipeline targets the frame format; the preview skips it.
            self.draw_hand(&mut enc, &p.hand_pipeline, &h.preview_globals_bind, &view, &p.depth_view, wgpu::LoadOp::Load, false);
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
    }
}

fn begin<'a>(
    enc: &'a mut wgpu::CommandEncoder,
    label: &str,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn uniform_buffer(device: &wgpu::Device, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn linear_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

fn depth_stencil() -> Option<wgpu::DepthStencilState> {
    Some(wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: Default::default(),
        bias: Default::default(),
    })
}

fn scene_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("scene"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
            })],
        },
        primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
        depth_stencil: depth_stencil(),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn hand_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("hand"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SkinVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Uint32x4, 4 => Float32x4
                ],
            })],
        },
        // Double-sided: the meshes are closed, and it spares a winding convention.
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: depth_stencil(),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn flash_gpu(device: &wgpu::Device) -> FlashGpu {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("flash"),
        source: wgpu::ShaderSource::Wgsl(include_str!("flash.wgsl").into()),
    });
    let layout_bg = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("flash"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("flash"),
        bind_group_layouts: &[Some(&layout_bg)],
        ..Default::default()
    });
    let uniform = uniform_buffer(device, std::mem::size_of::<FlashUniform>());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("flash"),
        layout: &layout_bg,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("flash"),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState::default(),
        // Over the weapon regardless of depth, never writing it.
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FRAME_FORMAT,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    FlashGpu { pipeline, uniform, bind }
}

fn depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn buffer_init(device: &wgpu::Device, data: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: data.len().next_multiple_of(4).max(4) as u64,
        usage,
        mapped_at_creation: true,
    });
    buf.slice(..).get_mapped_range_mut().expect("init range").slice(..data.len()).copy_from_slice(data);
    buf.unmap();
    buf
}

fn rgba_texture(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, px: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        px,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
        size,
    );
    tex.create_view(&Default::default())
}

/// An RGBA texture with a CPU-built mip chain (box filter), so small on-screen parts don't
/// shimmer.
fn rgba_mipmapped(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, px: &[u8]) -> wgpu::TextureView {
    let levels = 32 - w.max(h).leading_zeros();
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (mut cw, mut ch, mut cur) = (w, h, px.to_vec());
    for level in 0..levels {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &cur,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(cw * 4), rows_per_image: Some(ch) },
            wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        );
        if level + 1 == levels {
            break;
        }
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut s = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(cw - 1);
                        let sy = (y * 2 + dy).min(ch - 1);
                        s += cur[((sy * cw + sx) * 4 + c) as usize] as u32;
                    }
                    next[((y * nw + x) * 4 + c) as usize] = (s / 4) as u8;
                }
            }
        }
        (cw, ch, cur) = (nw, nh, next);
    }
    tex.create_view(&Default::default())
}

/// A 1 m cube centred on the origin. Each face's `u x v` points inward, which with the
/// left-handed view and projection puts its corners counter-clockwise on screen when seen
/// from outside: front faces for wgpu's default `FrontFace::Ccw`.
fn cube() -> (Vec<Vertex>, Vec<u16>) {
    // (normal, u axis, v axis) per face
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    let mut v = Vec::new();
    let mut i = Vec::new();
    for (n, u, w) in faces {
        let base = v.len() as u16;
        for (su, sv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = std::array::from_fn(|k| 0.5 * n[k] + 0.5 * su * u[k] + 0.5 * sv * w[k]);
            v.push(Vertex { pos: p, normal: n, uv: [(su + 1.0) * 0.5, 1.0 - (sv + 1.0) * 0.5] });
        }
        i.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (v, i)
}

/// Orange and charcoal checks with a pale border, easy to spot in the Lands Between.
fn checker_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    const N: u32 = 64;
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let border = x < 2 || y < 2 || x >= N - 2 || y >= N - 2;
            let c = if border {
                [235, 230, 215, 255]
            } else if ((x / 8) + (y / 8)) % 2 == 0 {
                [240, 130, 20, 255]
            } else {
                [40, 40, 44, 255]
            };
            px.extend_from_slice(&c);
        }
    }
    rgba_texture(device, queue, N, N, &px)
}
