//! App-independent sprite and opaque 3D rendering for native and browser wgpu.
//! Sprites use a logical framebuffer and nearest presentation; 3D owns bounded
//! offscreen color/depth targets.
mod surface;
mod three_d;
pub use surface::SurfaceRenderer;
pub use three_d::{Gpu3dError, GpuRenderer3d, MAX_3D_TARGET_BYTES};

use bytemuck::{Pod, Zeroable};
use std::{collections::BTreeMap, fmt};
use titan::render::{Color, ImageAssets, ImageId, RenderFrame};
pub use wgpu;
use wgpu::util::DeviceExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuError {
    MissingImage(ImageId),
    InvalidDimensions,
    TooManySprites,
    NotPrepared,
}
impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingImage(id) => write!(f, "image asset {} does not exist", id.value()),
            Self::InvalidDimensions => write!(f, "frame or image exceeds GPU texture limits"),
            Self::TooManySprites => write!(f, "sprite vertices exceed GPU buffer limits"),
            Self::NotPrepared => write!(f, "prepare a frame before rendering"),
        }
    }
}
impl std::error::Error for GpuError {}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    tint: [f32; 4],
}
struct Prepared {
    target: wgpu::TextureView,
    presentation: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
    images: Vec<wgpu::BindGroup>,
    draws: Vec<usize>,
    clear: wgpu::Color,
}
/// Owns GPU resources only; it never owns an App, World, clock, window, or surface.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    sprites: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    prepared: Option<Prepared>,
}
impl GpuRenderer {
    /// The target format must support color rendering. Surface/adapter acquisition,
    /// device loss, resize, and submission remain the caller's responsibility.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Titan image layout"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Titan sprite pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Titan sprite shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprites.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x4];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        };
        let sprites = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Titan textured sprites"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sprite"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout)],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sprite"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let present = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Titan nearest presentation"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_present"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if target_format.is_srgb() {
                    "fs_present_srgb"
                } else {
                    "fs_present"
                }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Titan nearest sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Self {
            device,
            queue,
            sprites,
            present,
            layout,
            sampler,
            prepared: None,
        }
    }
    /// Upload image assets and construct sorted sprite quads. Image IDs are scoped
    /// to the supplied collection; per-frame uploads avoid stale cross-world IDs.
    /// An invalid frame leaves the previous prepared frame intact.
    pub fn prepare(&mut self, frame: &RenderFrame, assets: &ImageAssets) -> Result<(), GpuError> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if frame.width() == 0
            || frame.height() == 0
            || frame.width() > limit
            || frame.height() > limit
        {
            return Err(GpuError::InvalidDimensions);
        }
        let vertex_count = frame
            .sprites()
            .len()
            .checked_mul(6)
            .ok_or(GpuError::TooManySprites)?;
        let byte_len = vertex_count
            .checked_mul(std::mem::size_of::<Vertex>())
            .ok_or(GpuError::TooManySprites)?;
        if vertex_count > u32::MAX as usize
            || byte_len as u64 > self.device.limits().max_buffer_size
        {
            return Err(GpuError::TooManySprites);
        }
        let mut sorted: Vec<_> = frame.sprites().iter().enumerate().collect();
        sorted.sort_by_key(|(index, s)| (s.layer, s.order, *index));
        for (_, sprite) in &sorted {
            let image = assets
                .get(sprite.image)
                .ok_or(GpuError::MissingImage(sprite.image))?;
            if image.width() > limit || image.height() > limit {
                return Err(GpuError::InvalidDimensions);
            }
        }
        let target = self.texture_with_format(
            frame.width(),
            frame.height(),
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            "Titan logical framebuffer",
        );
        let target = target.create_view(&Default::default());
        let presentation = self.bind_image(&target);
        let mut images = Vec::new();
        let mut image_indices = BTreeMap::new();
        let mut draws = Vec::new();
        let mut vertices = Vec::with_capacity(vertex_count);
        for (_, sprite) in sorted {
            let image = assets.get(sprite.image).expect("assets validated");
            if image.width() == 0 || image.height() == 0 || sprite.pixel_scale == 0 {
                continue;
            }
            let index = *image_indices.entry(sprite.image).or_insert_with(|| {
                let texture = self.texture(
                    image.width(),
                    image.height(),
                    wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                    "Titan sprite asset",
                );
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    image.pixels(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(image.width() * 4),
                        rows_per_image: Some(image.height()),
                    },
                    wgpu::Extent3d {
                        width: image.width(),
                        height: image.height(),
                        depth_or_array_layers: 1,
                    },
                );
                let index = images.len();
                images.push(self.bind_image(&texture.create_view(&Default::default())));
                index
            });
            let x = sprite.x as f64;
            let y = sprite.y as f64;
            let right = x + f64::from(image.width()) * f64::from(sprite.pixel_scale);
            let bottom = y + f64::from(image.height()) * f64::from(sprite.pixel_scale);
            let tint = channels(sprite.tint);
            for (px, py, u, v) in [
                (x, y, 0., 0.),
                (x, bottom, 0., 1.),
                (right, y, 1., 0.),
                (right, y, 1., 0.),
                (x, bottom, 0., 1.),
                (right, bottom, 1., 1.),
            ] {
                vertices.push(Vertex {
                    position: [
                        (px / f64::from(frame.width()) * 2. - 1.) as f32,
                        (1. - py / f64::from(frame.height()) * 2.) as f32,
                    ],
                    uv: [u, v],
                    tint,
                });
            }
            draws.push(index);
        }
        let vertices = (!vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Titan sprite vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let clear = channels(frame.clear_color());
        let alpha = f64::from(clear[3]);
        self.prepared = Some(Prepared {
            target,
            presentation,
            vertices,
            images,
            draws,
            clear: wgpu::Color {
                r: f64::from(clear[0]) * alpha,
                g: f64::from(clear[1]) * alpha,
                b: f64::from(clear[2]) * alpha,
                a: alpha,
            },
        });
        Ok(())
    }
    /// Encode the prepared sprites and nearest-neighbor presentation into any
    /// single-sample target view with the format passed to `new`.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) -> Result<(), GpuError> {
        let frame = self.prepared.as_ref().ok_or(GpuError::NotPrepared)?;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Titan sprite pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(frame.clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.sprites);
            if let Some(vertices) = &frame.vertices {
                pass.set_vertex_buffer(0, vertices.slice(..));
                for (draw, &image) in frame.draws.iter().enumerate() {
                    pass.set_bind_group(0, &frame.images[image], &[]);
                    let first = draw as u32 * 6;
                    pass.draw(first..first + 6, 0..1);
                }
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Titan presentation pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.present);
            pass.set_bind_group(0, &frame.presentation, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }
    fn texture(
        &self,
        width: u32,
        height: u32,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        self.texture_with_format(width, height, wgpu::TextureFormat::Rgba8Unorm, usage, label)
    }
    fn texture_with_format(
        &self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
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
        })
    }
    fn bind_image(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Titan image"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}
fn channels(color: Color) -> [f32; 4] {
    [color.red, color.green, color.blue, color.alpha].map(|c| f32::from(c) / 255.)
}
