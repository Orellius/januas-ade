//! Rounded-rectangle primitive — SDF-based, instanced, sub-pixel anti-aliased.
//!
//! Purpose: own the `wgpu` pipeline that draws every UI surface that is not a
//! glyph. Cards, buttons, sidebar panels, the home-screen project tiles all
//! flow through this primitive.
//! Public surface: [`Rect`], [`RadialGradient`], [`GradientStop`],
//! [`RectPipeline`].
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

/// Starting gradient-slot capacity. Most scenes use zero or a handful of
/// gradients (currently only the home colonnade's three portal halos); the
/// buffer grows on demand in [`RectPipeline::set_gradients`].
const INITIAL_GRADIENT_CAPACITY: usize = 8;

/// Sentinel for "this rect has no gradient." Mirrors the WGSL fragment-stage
/// branch `gradient_index < 0`. Public so callers can construct a [`Rect`]
/// without going through a builder.
pub const NO_GRADIENT: i32 = -1;

/// One stop in a [`RadialGradient`].
///
/// `offset` is normalized 0..1 along the gradient's radial axis; `color` is
/// linear-space RGBA. Five stops per gradient (locked at S5.5h — see slice
/// notes for the tradeoff).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GradientStop {
    /// Normalized position along the gradient, `[0.0, 1.0]`.
    pub offset: f32,
    /// Linear-space RGBA color.
    pub color: [f32; 4],
}

/// One radial gradient — five color stops at parameterized offsets,
/// CSS-style ellipse-farthest-corner formulation.
///
/// `center` is in normalized (0..1) coordinates within the rect's local box
/// (mirrors CSS `radial-gradient(... at <x>% <y>%, ...)`). The gradient axis
/// extends along whichever rect side is longer, so a tall portal and a wide
/// banner both bound their stops at the rect's bigger half-extent.
///
/// Stops are stored in ascending-offset order; the shader doesn't sort.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RadialGradient {
    /// Gradient center in normalized 0..1 rect-local coordinates.
    pub center: [f32; 2],
    /// Five stops in ascending-offset order.
    pub stops: [GradientStop; 5],
}

/// One rounded rectangle in pixel-space coordinates with linear-space colors.
///
/// `pos` is the top-left corner; `size` is the rectangle's width and height,
/// both in surface pixels. `radii` are the four corner radii, ordered
/// `[TL, TR, BR, BL]` (clockwise from top-left), in the same units.
/// `border_width` is the inset border thickness; `0.0` disables the border.
/// `gradient_index` is an index into the slice passed to
/// [`RectPipeline::set_gradients`], or [`NO_GRADIENT`] for no gradient.
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
    /// Per-corner radii in surface pixels, ordered `[TL, TR, BR, BL]`.
    pub radii: [f32; 4],
    /// Inset border thickness; `0.0` for a borderless rect.
    pub border_width: f32,
    /// Linear-space RGBA fill color.
    pub fill: [f32; 4],
    /// Linear-space RGBA border color; ignored when `border_width == 0.0`.
    pub border: [f32; 4],
    /// Linear-space RGBA shadow color. Alpha `0.0` disables the shadow.
    pub shadow_color: [f32; 4],
    /// Shadow offset in surface pixels; positive y casts the shadow downward.
    pub shadow_offset: [f32; 2],
    /// Gaussian blur sigma in surface pixels. The shader expands the drawn
    /// quad outward by `3 × blur` so the shadow halo fits.
    pub shadow_blur: f32,
    /// Index into [`RectPipeline::set_gradients`]; [`NO_GRADIENT`] disables.
    pub gradient_index: i32,
}

/// GPU-side instance layout — host mirror of the WGSL vertex-input attributes
/// declared in `rounded_rect.wgsl`. `repr(C)` so field offsets are stable.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct RectInstance {
    pos: [f32; 2],
    size: [f32; 2],
    radii: [f32; 4],
    border_width: f32,
    fill: [f32; 4],
    border: [f32; 4],
    shadow_color: [f32; 4],
    shadow_offset: [f32; 2],
    shadow_blur: f32,
    gradient_index: i32,
}

impl From<Rect> for RectInstance {
    fn from(r: Rect) -> Self {
        Self {
            pos: r.pos,
            size: r.size,
            radii: r.radii,
            border_width: r.border_width,
            fill: r.fill,
            border: r.border,
            shadow_color: r.shadow_color,
            shadow_offset: r.shadow_offset,
            shadow_blur: r.shadow_blur,
            gradient_index: r.gradient_index,
        }
    }
}

/// GPU storage-buffer layout for one radial gradient — std430-compatible.
/// 112 bytes: 16 (center + pad) + 16 (offsets) + 80 (five RGBA color stops).
/// The fifth stop's offset is implicit at 1.0 (saves 16 bytes vs storing
/// all five and matches the locked-mockup [0, 28, 58, 85, 100]% scheme).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Default)]
struct GradientSlot {
    center: [f32; 2],
    _pad0: [f32; 2],
    /// First four stop offsets; the fifth is implicit at 1.0.
    offsets: [f32; 4],
    color0: [f32; 4],
    color1: [f32; 4],
    color2: [f32; 4],
    color3: [f32; 4],
    color4: [f32; 4],
}

impl From<RadialGradient> for GradientSlot {
    fn from(g: RadialGradient) -> Self {
        Self {
            center: g.center,
            _pad0: [0.0, 0.0],
            offsets: [
                g.stops[0].offset,
                g.stops[1].offset,
                g.stops[2].offset,
                g.stops[3].offset,
            ],
            color0: g.stops[0].color,
            color1: g.stops[1].color,
            color2: g.stops[2].color,
            color3: g.stops[3].color,
            color4: g.stops[4].color,
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

/// The rounded-rect render pipeline.
///
/// Owns its shader module, bind group, viewport uniform, a growable
/// per-instance vertex buffer, and a growable storage buffer of
/// [`GradientSlot`]s indexed by `Rect.gradient_index`.
pub struct RectPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,
    gradient_buffer: wgpu::Buffer,
    gradient_capacity: usize,
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
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
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

        // Storage buffer starts populated with one zero-filled slot so the
        // binding is always valid even before the caller invokes
        // `set_gradients`. The fragment shader skips the array when a rect's
        // `gradient_index < 0`, so the dummy slot is never sampled.
        let initial_gradients = vec![GradientSlot::default(); INITIAL_GRADIENT_CAPACITY];
        let gradient_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rect-gradients"),
            contents: bytemuck::cast_slice(&initial_gradients),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect-bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: gradient_buffer.as_entire_binding(),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect-layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        // Vertex-attribute offsets mirror `RectInstance`'s field order under
        // `repr(C)`. Locations are dense from 0; keep them in sync with the
        // WGSL `@location(...)` declarations in `rounded_rect.wgsl`.
        let instance_attrs = [
            // pos: [f32; 2] @ 0
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            // size: [f32; 2] @ 8
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            // radii: [f32; 4] @ 16 (per-corner TL, TR, BR, BL)
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            // border_width: f32 @ 32
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32,
            },
            // fill: [f32; 4] @ 36
            wgpu::VertexAttribute {
                offset: 36,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            // border: [f32; 4] @ 52
            wgpu::VertexAttribute {
                offset: 52,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            // shadow_color: [f32; 4] @ 68
            wgpu::VertexAttribute {
                offset: 68,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
            // shadow_offset: [f32; 2] @ 84
            wgpu::VertexAttribute {
                offset: 84,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x2,
            },
            // shadow_blur: f32 @ 92
            wgpu::VertexAttribute {
                offset: 92,
                shader_location: 8,
                format: wgpu::VertexFormat::Float32,
            },
            // gradient_index: i32 @ 96 (-1 = no gradient)
            wgpu::VertexAttribute {
                offset: 96,
                shader_location: 9,
                format: wgpu::VertexFormat::Sint32,
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
            bind_group_layout: bgl,
            bind_group,
            globals_buffer,
            instance_buffer,
            instance_capacity: INITIAL_CAPACITY,
            instance_count: 0,
            gradient_buffer,
            gradient_capacity: INITIAL_GRADIENT_CAPACITY,
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

    /// Replace the radial-gradient slot table. `Rect.gradient_index` indexes
    /// into this slice (or [`NO_GRADIENT`] to skip). Empty input zero-fills
    /// the dummy slot at index 0 so the storage binding stays valid; the
    /// fragment shader's `gradient_index < 0` branch keeps the dummy slot
    /// from ever being sampled.
    ///
    /// Grows the storage buffer to the next power of two when the new count
    /// exceeds capacity. The buffer is re-bound to the bind group whenever it
    /// is recreated.
    pub fn set_gradients(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gradients: &[RadialGradient],
    ) {
        let needed = gradients.len().max(1);
        if needed > self.gradient_capacity {
            let mut cap = self.gradient_capacity.max(1);
            while cap < needed {
                cap *= 2;
            }
            self.gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rect-gradients"),
                size: (cap * std::mem::size_of::<GradientSlot>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.gradient_capacity = cap;
            self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rect-bg"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.globals_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.gradient_buffer.as_entire_binding(),
                    },
                ],
            });
        }
        if gradients.is_empty() {
            // Re-zero the dummy slot — release a previously-uploaded gradient
            // so a stale slot can't get sampled if a future shader change
            // ever accesses index 0 unguarded.
            let zero = GradientSlot::default();
            queue.write_buffer(&self.gradient_buffer, 0, bytemuck::bytes_of(&zero));
            return;
        }
        let slots: Vec<GradientSlot> = gradients.iter().copied().map(GradientSlot::from).collect();
        queue.write_buffer(&self.gradient_buffer, 0, bytemuck::cast_slice(&slots));
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
