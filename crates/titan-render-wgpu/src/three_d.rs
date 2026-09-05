//! Bounded opaque 3D rendering into owned, single-sample offscreen targets.
use bytemuck::{Pod, Zeroable};
use std::{fmt, num::NonZeroU64, ops::Range};
use titan::render::three_d::{BaseColor, MathError, RenderFrame3d};
use wgpu::util::DeviceExt;

/// Color plus depth storage is conservatively charged at eight bytes per pixel.
pub const MAX_3D_TARGET_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gpu3dError {
    InvalidDimensions,
    UnsupportedFormat,
    UnsupportedLimits,
    TooMuchGeometry,
    Math(MathError),
    NotPrepared,
}
impl fmt::Display for Gpu3dError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions => f.write_str("3D target must be nonzero and fit GPU dimensions and the 64 MiB color/depth budget"),
            Self::UnsupportedFormat => f.write_str("3D target requires RGBA8/BGRA8 unorm or sRGB"),
            Self::UnsupportedLimits => f.write_str("GPU limits cannot support the 3D vertex/uniform pipeline"),
            Self::TooMuchGeometry => f.write_str("3D upload exceeds GPU buffer or draw addressing limits"),
            Self::Math(e) => write!(f, "unrepresentable 3D GPU input: {e}"),
            Self::NotPrepared => f.write_str("prepare a valid 3D frame before rendering"),
        }
    }
}
impl std::error::Error for Gpu3dError {}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex3d {
    position: [f32; 3],
    normal: [f32; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniform3d {
    mvp: [[f32; 4]; 4],
    color_diffuse: [f32; 4],
    light_ambient: [f32; 4],
}
struct Prepared3d {
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    uniforms: Option<wgpu::BindGroup>,
    draws: Vec<(Range<u32>, u32)>,
    clear: wgpu::Color,
}
struct Targets {
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
}
/// Low-level renderer with no App, surface, clock, submission, or readback ownership.
/// The caller acquires a device and handles device loss and submission errors.
pub struct GpuRenderer3d {
    device: wgpu::Device,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    targets: Targets,
    prepared: Option<Prepared3d>,
    uniform_stride: u64,
}
impl GpuRenderer3d {
    /// All four allowed formats store sRGB-encoded RGB bytes. sRGB attachments
    /// encode automatically; unorm attachments use shader encoding exactly once.
    pub fn new(
        device: wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Gpu3dError> {
        validate_format(format)?;
        let limits = device.limits();
        validate_dimensions(width, height, &limits)?;
        let uniform_stride = validate_limits(&limits)?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Titan 3D uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<Uniform3d>() as u64),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Titan opaque 3D layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Titan opaque 3D shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("three_d.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Titan opaque 3D pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_mesh"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex3d>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if format.is_srgb() {
                    "fs_linear"
                } else {
                    "fs_encoded"
                }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let targets = create_targets(&device, width, height, format);
        Ok(Self {
            device,
            pipeline,
            layout,
            format,
            size: (width, height),
            targets,
            prepared: None,
            uniform_stride,
        })
    }
    /// Resize invalidates prepared draws, even on failure. Zero dimensions return
    /// an error; callers should suspend rendering until a nonzero resize succeeds.
    /// On failure the previous target allocation and dimensions remain available.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Gpu3dError> {
        self.prepared = None;
        validate_dimensions(width, height, &self.device.limits())?;
        if self.size != (width, height) {
            self.targets = create_targets(&self.device, width, height, self.format);
            self.size = (width, height);
        }
        Ok(())
    }
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    /// Single-sample texture usable as COPY_SRC, TEXTURE_BINDING or RENDER_ATTACHMENT.
    pub fn color_texture(&self) -> &wgpu::Texture {
        &self.targets.color
    }
    pub fn color_view(&self) -> &wgpu::TextureView {
        &self.targets.color_view
    }

    /// Rebuild frame-local uploads from the immutable snapshot. No asset ID cache
    /// survives across frames. Failure leaves render() in NotPrepared state.
    pub fn prepare(&mut self, frame: &RenderFrame3d, clear: BaseColor) -> Result<(), Gpu3dError> {
        self.prepared = None;
        let limits = self.device.limits();
        let mut vertex_count = 0usize;
        let mut index_count = 0usize;
        for resolved in frame.draws() {
            vertex_count = vertex_count
                .checked_add(resolved.mesh().positions().len())
                .ok_or(Gpu3dError::TooMuchGeometry)?;
            index_count = index_count
                .checked_add(resolved.mesh().indices().len())
                .ok_or(Gpu3dError::TooMuchGeometry)?;
        }
        let vertex_bytes = upload_size(
            vertex_count,
            std::mem::size_of::<Vertex3d>() as u64,
            &limits,
        )?;
        let index_bytes = upload_size(index_count, 4, &limits)?;
        let uniform_bytes = upload_size(frame.draws().len(), self.uniform_stride, &limits)?;
        if vertex_count > i32::MAX as usize
            || index_count > u32::MAX as usize
            || uniform_bytes > u64::from(u32::MAX)
        {
            return Err(Gpu3dError::TooMuchGeometry);
        }
        let mut vertices =
            Vec::with_capacity(vertex_bytes as usize / std::mem::size_of::<Vertex3d>());
        let mut indices = Vec::with_capacity(index_bytes as usize / 4);
        let mut uniforms = vec![0u8; uniform_bytes as usize];
        let mut draws = Vec::with_capacity(frame.draws().len());
        let vp = frame
            .camera()
            .projection_matrix()
            .checked_mul(frame.camera().view_matrix())
            .map_err(Gpu3dError::Math)?;
        for (draw_index, resolved) in frame.draws().iter().enumerate() {
            let draw = resolved.draw();
            let mvp = vp
                .checked_mul(draw.transform.matrix())
                .map_err(Gpu3dError::Math)?
                .columns();
            let base_vertex = vertices.len() as u32;
            for (&p, &n) in resolved
                .mesh()
                .positions()
                .iter()
                .zip(resolved.mesh().normals())
            {
                // Reject overflowing f32 shader intermediates, even when f64
                // cancellation would produce a finite final clip coordinate.
                for (row, _) in mvp[0].iter().enumerate() {
                    let values = [p.x, p.y, p.z, 1.0];
                    let magnitude = (0..4)
                        .map(|col| (f64::from(mvp[col][row]) * f64::from(values[col])).abs())
                        .sum::<f64>();
                    if magnitude > f64::from(f32::MAX) {
                        return Err(Gpu3dError::Math(MathError::Unrepresentable));
                    }
                }
                let n = draw
                    .transform
                    .transform_normal(n)
                    .map_err(Gpu3dError::Math)?;
                vertices.push(Vertex3d {
                    position: [p.x, p.y, p.z],
                    normal: [n.x, n.y, n.z],
                });
            }
            let first_index = indices.len() as u32;
            indices.extend(
                resolved
                    .mesh()
                    .indices()
                    .iter()
                    .map(|index| index + base_vertex),
            );
            let color = draw.color.linear();
            let light = frame.lighting();
            let direction = light.to_light();
            let uniform = Uniform3d {
                mvp,
                color_diffuse: [color[0], color[1], color[2], light.diffuse()],
                light_ambient: [direction.x, direction.y, direction.z, light.ambient()],
            };
            let offset = draw_index * self.uniform_stride as usize;
            uniforms[offset..offset + std::mem::size_of::<Uniform3d>()]
                .copy_from_slice(bytemuck::bytes_of(&uniform));
            draws.push((first_index..indices.len() as u32, offset as u32));
        }
        let (vertices, indices, uniforms) = if draws.is_empty() {
            (None, None, None)
        } else {
            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Titan 3D vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Titan 3D indices"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
            let uniform_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Titan 3D draw uniforms"),
                        contents: &uniforms,
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Titan 3D draw bindings"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform_buffer,
                        offset: 0,
                        size: NonZeroU64::new(std::mem::size_of::<Uniform3d>() as u64),
                    }),
                }],
            });
            (Some(vertex_buffer), Some(index_buffer), Some(group))
        };
        let rgb = if self.format.is_srgb() {
            clear.linear()
        } else {
            [clear.red, clear.green, clear.blue].map(|v| f32::from(v) / 255.0)
        };
        self.prepared = Some(Prepared3d {
            vertices,
            indices,
            uniforms,
            draws,
            clear: wgpu::Color {
                r: f64::from(rgb[0]),
                g: f64::from(rgb[1]),
                b: f64::from(rgb[2]),
                a: 1.0,
            },
        });
        Ok(())
    }
    /// Encode one opaque pass. Every render clears color and depth; callers submit.
    pub fn render(&self, encoder: &mut wgpu::CommandEncoder) -> Result<(), Gpu3dError> {
        let prepared = self.prepared.as_ref().ok_or(Gpu3dError::NotPrepared)?;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Titan opaque 3D pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(prepared.clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        if let (Some(vertices), Some(indices), Some(uniforms)) =
            (&prepared.vertices, &prepared.indices, &prepared.uniforms)
        {
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            for (range, offset) in &prepared.draws {
                pass.set_bind_group(0, uniforms, &[*offset]);
                pass.draw_indexed(range.clone(), 0, 0..1);
            }
        }
        Ok(())
    }
}
fn validate_format(format: wgpu::TextureFormat) -> Result<(), Gpu3dError> {
    match format {
        wgpu::TextureFormat::Rgba8Unorm
        | wgpu::TextureFormat::Rgba8UnormSrgb
        | wgpu::TextureFormat::Bgra8Unorm
        | wgpu::TextureFormat::Bgra8UnormSrgb => Ok(()),
        _ => Err(Gpu3dError::UnsupportedFormat),
    }
}
fn validate_dimensions(width: u32, height: u32, limits: &wgpu::Limits) -> Result<(), Gpu3dError> {
    if width == 0
        || height == 0
        || width > limits.max_texture_dimension_2d
        || height > limits.max_texture_dimension_2d
        || u64::from(width) * u64::from(height) > MAX_3D_TARGET_BYTES / 8
    {
        Err(Gpu3dError::InvalidDimensions)
    } else {
        Ok(())
    }
}
fn validate_limits(limits: &wgpu::Limits) -> Result<u64, Gpu3dError> {
    let bytes = std::mem::size_of::<Uniform3d>() as u64;
    let alignment = u64::from(limits.min_uniform_buffer_offset_alignment).max(1);
    let stride = bytes.div_ceil(alignment) * alignment;
    if limits.max_color_attachments < 1
        || limits.max_color_attachment_bytes_per_sample < 4
        || limits.max_inter_stage_shader_variables < 1
        || limits.max_bind_groups < 1
        || limits.max_bindings_per_bind_group < 1
        || limits.max_uniform_buffers_per_shader_stage < 1
        || limits.max_dynamic_uniform_buffers_per_pipeline_layout < 1
        || limits.max_uniform_buffer_binding_size < bytes
        || limits.max_vertex_buffers < 1
        || limits.max_vertex_attributes < 2
        || limits.max_vertex_buffer_array_stride < std::mem::size_of::<Vertex3d>() as u32
        || limits.max_buffer_size < stride
    {
        Err(Gpu3dError::UnsupportedLimits)
    } else {
        Ok(stride)
    }
}
fn upload_size(count: usize, stride: u64, limits: &wgpu::Limits) -> Result<u64, Gpu3dError> {
    (count as u64)
        .checked_mul(stride)
        .filter(|&bytes| bytes <= limits.max_buffer_size && bytes <= usize::MAX as u64)
        .ok_or(Gpu3dError::TooMuchGeometry)
}
fn create_targets(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Targets {
    let descriptor = |label, format, usage| wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    };
    let color = device.create_texture(&descriptor(
        "Titan 3D color",
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
    ));
    let depth = device.create_texture(&descriptor(
        "Titan 3D depth",
        wgpu::TextureFormat::Depth24Plus,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    ));
    let color_view = color.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    Targets {
        color,
        color_view,
        depth_view,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_budget_and_formats_are_explicit() {
        let limits = wgpu::Limits::downlevel_webgl2_defaults();
        assert!(validate_dimensions(1024, 1024, &limits).is_ok());
        for (w, h) in [(0, 1), (1, 0), (u32::MAX, 1), (4096, 4096)] {
            assert_eq!(
                validate_dimensions(w, h, &limits),
                Err(Gpu3dError::InvalidDimensions)
            );
        }
        for format in [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ] {
            assert_eq!(validate_format(format), Ok(()));
        }
        for format in [
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureFormat::Depth24Plus,
            wgpu::TextureFormat::Rgba8Uint,
        ] {
            assert_eq!(validate_format(format), Err(Gpu3dError::UnsupportedFormat));
        }
    }

    #[test]
    fn uniform_layout_fits_portable_limits_and_uploads_are_checked() {
        assert_eq!(std::mem::size_of::<Uniform3d>(), 96);
        let limits = wgpu::Limits::downlevel_webgl2_defaults();
        let stride = validate_limits(&limits).unwrap();
        assert!(stride >= 96);
        assert_eq!(
            stride % u64::from(limits.min_uniform_buffer_offset_alignment),
            0
        );
        assert_eq!(upload_size(0, stride, &limits), Ok(0));
        assert_eq!(
            upload_size(usize::MAX, u64::MAX, &limits),
            Err(Gpu3dError::TooMuchGeometry)
        );
        assert_eq!(
            upload_size(1, limits.max_buffer_size + 1, &limits),
            Err(Gpu3dError::TooMuchGeometry)
        );
        assert_eq!(
            validate_limits(&wgpu::Limits {
                max_uniform_buffer_binding_size: 95,
                ..limits.clone()
            }),
            Err(Gpu3dError::UnsupportedLimits)
        );
        assert_eq!(
            validate_limits(&wgpu::Limits {
                max_dynamic_uniform_buffers_per_pipeline_layout: 0,
                ..limits
            }),
            Err(Gpu3dError::UnsupportedLimits)
        );
    }
}
