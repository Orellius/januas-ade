//! Image-texture primitive — sampled `RGBA8` textures with rounded-corner
//! clipping (S5.5d).
//!
//! Purpose: draw raster images (the brand logo, eventually screenshots and
//! file-tree thumbnails) through the same instanced-quad model the rect
//! pipeline uses. One texture per loaded image at v0.1, one bind group per
//! `(pipeline, texture)` pair; instanced draws within a texture share one
//! call.
//! Public surface: [`ImageId`], [`ImageInstance`], [`ImagePipeline`].
//! Why this file: glyphon's text path is hard-coded to its own atlas, and the
//! rect SDF pipeline can't sample textures without giving up its branchless
//! fragment shader. A separate pipeline keeps both clean and lets each grow
//! independently (S5.5d.5 will port DirectWrite contrast on the glyph side
//! without touching this).
//! Not responsibilities: PNG decoding (`png` crate; called from
//! [`crate::Renderer::load_image`]), layout (`crates/ui/`), hit-testing
//! (`crates/ui/src/pointer.rs`).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt as _;

const SHADER: &str = include_str!("shaders/image.wgsl");

/// Starting per-instance buffer capacity. Grows on demand in
/// [`ImagePipeline::set_instances`].
const INITIAL_CAPACITY: usize = 16;

/// Stable identifier for an image loaded into the renderer.
///
/// Newtyped over a small integer index so callers can hold it across frames
/// without taking a ref-count or worrying about validity beyond the
/// renderer's lifetime.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(pub(crate) u32);

/// One positioned image instance. Top-left anchor in surface pixels; `tint`
/// multiplies the sampled texel in linear space.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ImageInstance {
    /// Which loaded image to draw.
    pub id: ImageId,
    /// Top-left corner in surface pixels.
    pub pos: [f32; 2],
    /// Width and height in surface pixels.
    pub size: [f32; 2],
    /// Corner radius in surface pixels for the rounded-rect mask; `0.0`
    /// disables clipping (sharp rectangle).
    pub radius: f32,
    /// Linear-space RGBA tint multiplier. `[1.0; 4]` paints the texture
    /// verbatim.
    pub tint: [f32; 4],
}

/// GPU-side instance struct — host mirror of the WGSL vertex-input attributes
/// declared in `image.wgsl`. `repr(C)` so field offsets are stable.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct InstanceData {
    pos: [f32; 2],
    size: [f32; 2],
    radius: f32,
    _pad0: [f32; 3],
    tint: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

/// One loaded texture + its bind group. Owned by the pipeline so callers
/// never see GPU resources directly.
struct LoadedImage {
    bind_group: wgpu::BindGroup,
    /// Held so the GPU keeps the underlying allocation alive.
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
}

/// The image render pipeline. Owns its shader, both bind-group layouts, the
/// uniform globals buffer, the loaded-images table, and a growable
/// per-instance vertex buffer.
pub struct ImagePipeline {
    pipeline: wgpu::RenderPipeline,
    globals_bgl: wgpu::BindGroupLayout,
    globals_bg: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,

    image_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    images: Vec<LoadedImage>,

    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    /// `(image_index, first_instance, count)` — one entry per contiguous
    /// instance range using the same texture.
    batches: Vec<(usize, u32, u32)>,
}

impl ImagePipeline {
    /// Construct the pipeline for the given surface format and starting
    /// viewport.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "linear GPU resource construction; splitting hurts readability more than it helps"
    )]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, viewport: [f32; 2]) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image-globals-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let globals = Globals {
            viewport,
            _pad: [0.0, 0.0],
        };
        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("image-globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image-globals-bg"),
            layout: &globals_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image-tex-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image-layout"),
            bind_group_layouts: &[Some(&globals_bgl), Some(&image_bgl)],
            immediate_size: 0,
        });

        let instance_attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceData>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attrs,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[instance_layout],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image-instances"),
            size: (INITIAL_CAPACITY * std::mem::size_of::<InstanceData>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            globals_bgl,
            globals_bg,
            globals_buffer,
            image_bgl,
            sampler,
            images: Vec::new(),
            instance_buffer,
            instance_capacity: INITIAL_CAPACITY,
            batches: Vec::new(),
        }
    }

    /// Update the viewport uniform — call from the renderer's resize path.
    pub fn resize(&self, queue: &wgpu::Queue, viewport: [f32; 2]) {
        let globals = Globals {
            viewport,
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
    }

    /// Upload a decoded RGBA8 image and return its [`ImageId`]. `width` and
    /// `height` are in texels; `rgba` is a row-major `width * height * 4`
    /// byte buffer.
    ///
    /// The texture is created in `Rgba8UnormSrgb` so the GPU sample step
    /// converts sRGB → linear automatically, matching the rest of the
    /// pipeline's linear-space blend assumption.
    pub fn load_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> ImageId {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image-texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image-bg"),
            layout: &self.image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        #[allow(
            clippy::cast_possible_truncation,
            reason = "u32 image table covers any realistic image count"
        )]
        let id = ImageId(self.images.len() as u32);
        self.images.push(LoadedImage {
            bind_group,
            _texture: texture,
            _view: view,
        });
        id
    }

    /// Suppress the unused-field warning when no callers consume the layout
    /// directly — keep it around because [`Self::globals_bg`] is rebuilt
    /// against it on the (currently theoretical) hot-reload path.
    #[allow(
        dead_code,
        reason = "kept for future hot-reload of globals bind group; the value flows through pipeline_layout"
    )]
    pub(crate) const fn globals_bgl(&self) -> &wgpu::BindGroupLayout {
        &self.globals_bgl
    }

    /// Replace the per-instance image list. Coalesces consecutive instances
    /// that share the same [`ImageId`] into one batched draw; non-consecutive
    /// duplicates cost an extra draw call. Order is preserved so callers can
    /// rely on declaration-order painting.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "instance counts fit in u32 by design — wgpu's draw API takes u32"
    )]
    pub fn set_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[ImageInstance],
    ) {
        self.batches.clear();
        if instances.is_empty() {
            return;
        }

        let count = instances.len();
        if count > self.instance_capacity {
            let mut cap = self.instance_capacity.max(1);
            while cap < count {
                cap *= 2;
            }
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("image-instances"),
                size: (cap * std::mem::size_of::<InstanceData>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = cap;
        }

        let mut packed: Vec<InstanceData> = Vec::with_capacity(count);
        let mut current_image: Option<usize> = None;
        let mut batch_start: u32 = 0;
        for (i, inst) in instances.iter().enumerate() {
            packed.push(InstanceData {
                pos: inst.pos,
                size: inst.size,
                radius: inst.radius,
                _pad0: [0.0; 3],
                tint: inst.tint,
            });
            let image_idx = inst.id.0 as usize;
            match current_image {
                Some(idx) if idx == image_idx => {}
                Some(idx) => {
                    self.batches
                        .push((idx, batch_start, i as u32 - batch_start));
                    current_image = Some(image_idx);
                    batch_start = i as u32;
                }
                None => {
                    current_image = Some(image_idx);
                    batch_start = i as u32;
                }
            }
        }
        if let Some(idx) = current_image {
            self.batches
                .push((idx, batch_start, count as u32 - batch_start));
        }

        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&packed));
    }

    /// Issue the draw calls into the active render pass. No-op when no
    /// instances have been set (safe to call every frame unconditionally).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.batches.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals_bg, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        for (image_idx, first, count) in &self.batches {
            let Some(image) = self.images.get(*image_idx) else {
                continue;
            };
            pass.set_bind_group(1, &image.bind_group, &[]);
            pass.draw(0..4, *first..(*first + *count));
        }
    }
}
