//! Scene plus ECS-extracted sprite/text UI composition, usable on any target.
use crate::{GpuRenderer, GpuRenderer3d, wgpu};
use titan::render::{
    ImageAssets, RenderFrame,
    three_d::{BaseColor, RenderFrame3d},
};

/// Render scene and UI into a caller-owned single-sample target, including an
/// offscreen COPY_SRC texture. No capture protocol, surface, or submission is owned.
/// The target must have the format passed to `new`; it is fully overwritten.
pub struct GpuSceneRenderer3d {
    device: wgpu::Device,
    scene: GpuRenderer3d,
    ui: GpuRenderer,
    overlay: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bindings: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    prepared: bool,
}
impl GpuSceneRenderer3d {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err("3D composition requires RGBA/BGRA8 unorm or sRGB output".into());
        }
        let scene = GpuRenderer3d::new(
            device.clone(),
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
        )
        .map_err(|e| e.to_string())?;
        let ui = GpuRenderer::new(device.clone(), queue, wgpu::TextureFormat::Rgba8Unorm);
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Titan composition inputs"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let overlay = overlay_target(&device, width, height);
        let bindings = bindings(&device, &layout, &sampler, scene.color_view(), &overlay);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Titan scene/UI color conversion"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composition.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Titan scene/UI composition"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
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
        Ok(Self {
            device,
            scene,
            ui,
            overlay,
            layout,
            sampler,
            bindings,
            pipeline,
            prepared: false,
        })
    }
    /// Invalidates preparation even on failure. Existing allocation is retained
    /// on invalid dimensions; zero sizes must be suspended by the host.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.prepared = false;
        let old = self.scene.size();
        self.scene
            .resize(width, height)
            .map_err(|e| e.to_string())?;
        if old != (width, height) {
            self.overlay = overlay_target(&self.device, width, height);
            self.bindings = bindings(
                &self.device,
                &self.layout,
                &self.sampler,
                self.scene.color_view(),
                &self.overlay,
            );
        }
        Ok(())
    }
    pub fn size(&self) -> (u32, u32) {
        self.scene.size()
    }
    /// Overlay clear alpha normally equals zero. Its logical size can differ
    /// from the scene and is scaled with nearest sampling, as in 2D players.
    /// Any failure invalidates the entire composition, preventing stale UI/scene.
    pub fn prepare(
        &mut self,
        scene: &RenderFrame3d,
        clear: BaseColor,
        overlay: &RenderFrame,
        assets: &ImageAssets,
    ) -> Result<(), String> {
        self.prepared = false;
        // Bound UI intermediates too: eight bytes per logical pixel.
        if u64::from(overlay.width()) * u64::from(overlay.height()) > crate::MAX_3D_TARGET_BYTES / 8
        {
            return Err("UI target exceeds composition budget".into());
        }
        self.scene
            .prepare(scene, clear)
            .map_err(|e| e.to_string())?;
        self.ui
            .prepare(overlay, assets)
            .map_err(|e| e.to_string())?;
        self.prepared = true;
        Ok(())
    }
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) -> Result<(), String> {
        if !self.prepared {
            return Err("prepare scene and UI before rendering".into());
        }
        self.scene.render(encoder).map_err(|e| e.to_string())?;
        self.ui
            .render(encoder, &self.overlay)
            .map_err(|e| e.to_string())?;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Titan scene/UI pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}
fn overlay_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("Titan byte-space UI layer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
fn bindings(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    scene: &wgpu::TextureView,
    overlay: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Titan scene/UI bindings"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(overlay),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
