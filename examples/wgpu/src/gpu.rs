//! The device, pipeline and swapchain behind the plasma, shared with the
//! `audiocapture` example (`#[path]`), so there is one copy of it. Android
//! only: it builds against `libmobile::android::SurfaceTarget`.

use crate::uniforms::Uniforms;
use libmobile::android::{AndroidApp, SurfaceTarget};

/// The live swapchain, absent between `WindowDestroyed` and the next
/// `WindowCreated`.
pub(crate) struct Swapchain {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// Device-lifetime state: built on the first window, kept across
/// background/resume.
pub(crate) struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pub(crate) swapchain: Option<Swapchain>,
}

impl Gpu {
    pub(crate) fn new(target: SurfaceTarget) -> Result<Gpu, String> {
        let (w, h) = target.size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(target)
            .map_err(|e| format!("create_surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| format!("no compatible adapter: {e}"))?;
        let info = adapter.get_info();
        log::info!("wgpu: adapter {:?} {}", info.backend, info.name);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| format!("request_device: {e}"))?;
        device.on_uncaptured_error(std::sync::Arc::new(|err: wgpu::Error| {
            log::error!("wgpu: {err}");
        }));

        let format = pick_format(&surface, &adapter);
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: Uniforms::SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("plasma"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("plasma"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let mut gpu = Gpu {
            instance,
            adapter,
            device,
            queue,
            format,
            pipeline,
            uniforms,
            bind_group,
            swapchain: None,
        };
        gpu.swapchain = Some(gpu.configure(surface, w, h)?);
        log::info!("wgpu: up {w}x{h} {format:?}");
        Ok(gpu)
    }

    /// A swapchain for a new window, reusing device and pipeline.
    pub(crate) fn attach(&mut self, target: SurfaceTarget) -> Result<(), String> {
        let (w, h) = target.size();
        let surface = self
            .instance
            .create_surface(target)
            .map_err(|e| format!("create_surface: {e}"))?;
        if !surface
            .get_capabilities(&self.adapter)
            .formats
            .contains(&self.format)
        {
            return Err("new surface lost the pipeline's format".into());
        }
        self.swapchain = Some(self.configure(surface, w, h)?);
        Ok(())
    }

    fn configure(
        &self,
        surface: wgpu::Surface<'static>,
        w: u32,
        h: u32,
    ) -> Result<Swapchain, String> {
        let mut config = surface
            .get_default_config(&self.adapter, w, h)
            .ok_or("surface incompatible with adapter")?;
        config.format = self.format;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&self.device, &config);
        Ok(Swapchain { surface, config })
    }

    pub(crate) fn resize(&mut self, w: u32, h: u32) {
        let Some(sc) = self.swapchain.as_mut() else {
            return;
        };
        if (sc.config.width, sc.config.height) != (w, h) {
            sc.config.width = w;
            sc.config.height = h;
            sc.surface.configure(&self.device, &sc.config);
        }
    }

    pub(crate) fn render(&mut self, uniforms: &Uniforms) {
        use wgpu::CurrentSurfaceTexture as Cst;
        let Some(sc) = self.swapchain.as_mut() else {
            return;
        };
        let frame = match sc.surface.get_current_texture() {
            Cst::Success(frame) | Cst::Suboptimal(frame) => frame,
            Cst::Timeout | Cst::Occluded => return,
            other => {
                // Outdated / Lost / ...: reconfigure, try next frame.
                log::warn!("wgpu: acquire failed ({other:?}), reconfiguring");
                sc.surface.configure(&self.device, &sc.config);
                return;
            }
        };
        self.queue
            .write_buffer(&self.uniforms, 0, &uniforms.to_bytes());
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("plasma"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
    }
}

/// Prefer non-sRGB 8-bit so the shader's colours are written as-is.
fn pick_format(surface: &wgpu::Surface<'_>, adapter: &wgpu::Adapter) -> wgpu::TextureFormat {
    let caps = surface.get_capabilities(adapter);
    caps.formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0])
}

/// `WindowCreated` / `WindowResized`: resize or re-attach the swapchain,
/// building the device on the first window (or after a failed re-attach).
pub(crate) fn window_changed(gpu: &mut Option<Gpu>, uniforms: &mut Uniforms, app: &AndroidApp) {
    let Some(target) = SurfaceTarget::current(app) else {
        return;
    };
    let (w, h) = target.size();
    uniforms.resolution = [w as f32, h as f32];
    if let Some(gpu) = gpu.as_mut() {
        if gpu.swapchain.is_some() {
            gpu.resize(w, h);
            return;
        }
        match gpu.attach(target) {
            Ok(()) => return,
            // Usually a device lost while backgrounded: start over.
            Err(err) => log::warn!("wgpu: re-attach failed ({err}); rebuilding"),
        }
    }
    // First window, or a failed re-attach (which consumed `target`).
    let Some(target) = SurfaceTarget::current(app) else {
        return;
    };
    *gpu = Gpu::new(target)
        .map_err(|e| log::error!("wgpu: bring-up failed: {e}"))
        .ok();
}
