//! Rounded-rectangle primitive — SDF-based, instanced, sub-pixel anti-aliased.
//!
//! Purpose: own the `wgpu` pipeline that draws every UI surface that is not a
//! glyph. Cards, buttons, sidebar panels, the home-screen project tiles all
//! flow through this primitive.
//! Public surface: [`Rect`], [`RectPipeline`].
//! Why this file (vs inlining in `lib.rs`): the pipeline pulls in a shader, a
//! bind-group layout, a vertex-buffer layout, and per-frame upload logic that
//! together exceed the 80-LOC inlining threshold; isolating it keeps `lib.rs`
//! readable as the frame-loop choreographer.
//! Not responsibilities: layout (a later `ui` crate at S5.5b), input (`ui`
//! crate at S5.5c), glyph rendering (`glyphon`, in `lib.rs`).
//! Test strategy: the standalone `bench_rects` example exercises this module
//! at the slice's perf gate (100 rects ≥ 1000 fps on `--release`). Visual
//! correctness is gated by the read-aloud pass and by S5.5d's pixel-match.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt as _;

const SHADER: &str = include_str!("shaders/rounded_rect.wgsl");

/// Starting per-instance buffer capacity. Grows on demand in [`RectPipeline::set_rects`].
const INITIAL_CAPACITY: usize = 64;

/// One rounded rectangle in pixel-space coordinates with linear-space colors.
///
/// `pos` is the top-left corner; `size` is the rectangle's width and height,
/// both in surface pixels. `radius` is the corner radius in the same units.
/// `border_width` is the inset border thickness; `0.0` disables the border.
///
/// Colors are linear-space RGBA in `[0.0, 1.0]`. The pipeline blends with
/// pre-multiplied alpha — callers pass straight colors, the shader does the
/// pre-multiplication on output.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    /// Top-left corner in surface pixels.
    pub pos: [f32; 2],
    /// Width and height in surface pixels.
    pub size: [f32; 2],
    /// Corner radius in surface pixels.
    pub radius: f32,
    /// Inset border thickness; `0.0` for a borderless rect.
    pub border_width: f32,
    /// Linear-space RGBA fill color.
    pub fill: [f32; 4],
    /// Linear-space RGBA border color; ignored when `border_width == 0.0`.
    pub border: [f32; 4],
}

/// GPU-side instance layout — host mirror of the WGSL vertex-input attributes
/// declared in `rounded_rect.wgsl`. `repr(C)` so field offsets are stable.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct RectInstance {
    pos: [f32; 2],
    size: [f32; 2],
    radius: f32,
    border_width: f32,
    fill: [f32; 4],
    border: [f32; 4],
}

impl From<Rect> for RectInstance {
    fn from(r: Rect) -> Self {
        Self {
            pos: r.pos,
            size: r.size,
            radius: r.radius,
            border_width: r.border_width,
            fill: r.fill,
            border: r.border,
        }
    }
}

/// Uniform block — must mirror the WGSL `Globals` struct. Padded to 16 bytes
/// because uniform-buffer bindings round up to the minimum alignment.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

/// The rounded-rect render pipeline. Owns its shader module, bind group,
/// viewport uniform, and a growable per-instance vertex buffer.
pub struct RectPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,
}

impl RectPipeline {
    /// Construct the pipeline for the given surface format and starting viewport.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "linear GPU resource construction; splitting hurts readability more than it helps"
    )]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, viewport: [f32; 2]) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rounded_rect.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect-bgl"),
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
            label: Some("rect-globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect-bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect-layout"),
            bind_group_layouts: &[Some(&bgl)],
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
                offset: 20,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 40,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attrs,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect-pipeline"),
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
            label: Some("rect-instances"),
            size: (INITIAL_CAPACITY * std::mem::size_of::<RectInstance>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            globals_buffer,
            instance_buffer,
            instance_capacity: INITIAL_CAPACITY,
            instance_count: 0,
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

    /// Replace the per-instance rect list. Grows the GPU buffer to the next
    /// power of two when the new count exceeds the current capacity; reuses
    /// the existing buffer otherwise.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "instance counts fit in u32 by design — wgpu's draw API takes u32"
    )]
    pub fn set_rects(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rects: &[Rect]) {
        let count = rects.len();
        self.instance_count = count as u32;
        if count == 0 {
            return;
        }
        if count > self.instance_capacity {
            let mut cap = self.instance_capacity.max(1);
            while cap < count {
                cap *= 2;
            }
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rect-instances"),
                size: (cap * std::mem::size_of::<RectInstance>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = cap;
        }
        let instances: Vec<RectInstance> = rects.iter().copied().map(RectInstance::from).collect();
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
    }

    /// Issue the draw call into the active render pass. No-op when no rects
    /// have been set (so it is safe to call every frame unconditionally).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..4, 0..self.instance_count);
    }
}
