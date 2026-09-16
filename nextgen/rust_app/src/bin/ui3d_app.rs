use std::f32::consts::{FRAC_PI_2, PI};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use egui::{Color32, RichText};
use glam::{DMat4, DVec3, Mat4, Vec3};
use observatory_core::{build_application, parse_campaign_config, CampaignTargetConfig};
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowBuilder};

const PLANET_SHADER_WGSL: &str = r#"
struct FrameUniform {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_pos: vec4<f32>,
    planet_color: vec4<f32>,
    atmosphere_color: vec4<f32>,
    atmosphere_strength: f32,
    specular_power: f32,
    specular_strength: f32,
    _padding: f32,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world = frame.model * vec4<f32>(input.position, 1.0);
    out.world_position = world.xyz;
    out.world_normal = normalize((frame.model * vec4<f32>(input.normal, 0.0)).xyz);
    out.clip_position = frame.view_proj * world;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(input.world_normal);
    let l = normalize(frame.light_dir.xyz);
    let v = normalize(frame.camera_pos.xyz - input.world_position);
    let h = normalize(l + v);

    let ndotl = max(dot(n, l), 0.0);
    let ndotv = max(dot(n, v), 0.0);
    let diffuse = 0.12 + 0.88 * ndotl;

    let spec_angle = max(dot(n, h), 0.0);
    let specular = pow(spec_angle, frame.specular_power) * frame.specular_strength;
    let fresnel = pow(1.0 - ndotv, 3.2) * (0.55 + 0.45 * frame.atmosphere_strength);
    let scattering = pow(ndotl, 5.0) * (0.18 + 0.82 * frame.atmosphere_strength);

    let base = frame.planet_color.rgb * (0.45 + 0.55 * diffuse);
    let atmosphere = frame.atmosphere_color.rgb * (fresnel + scattering);

    let color = base + atmosphere + vec3<f32>(specular, specular, specular);
    return vec4<f32>(color, 1.0);
}
"#;

const STAR_SHADER_WGSL: &str = r#"
struct ViewUniform {
    view_proj: mat4x4<f32>,
    time_s: f32,
    _pad0: vec3<f32>,
};

@group(0) @binding(0)
var<uniform> view_data: ViewUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) luminance: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) luminance: f32,
    @location(1) world_position: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = view_data.view_proj * vec4<f32>(input.position, 1.0);
    out.luminance = input.luminance;
    out.world_position = input.position;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let warm = vec3<f32>(1.0, 0.97, 0.89);
    let cold = vec3<f32>(0.72, 0.84, 1.0);
    let t = clamp(input.luminance, 0.0, 1.0);

    let twinkle = 0.78 + 0.22 * sin(view_data.time_s * 1.85 + dot(input.world_position, vec3<f32>(0.11, 0.17, 0.13)));
    let color = mix(cold, warm, t) * (0.24 + 0.76 * t) * twinkle;
    return vec4<f32>(color, 1.0);
}
"#;

const GUIDE_SHADER_WGSL: &str = r#"
struct ViewUniform {
    view_proj: mat4x4<f32>,
    time_s: f32,
    _pad0: vec3<f32>,
};

@group(0) @binding(0)
var<uniform> view_data: ViewUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = view_data.view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 0.52);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

impl MeshVertex {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct StarVertex {
    position: [f32; 3],
    luminance: f32,
}

impl StarVertex {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<StarVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GuideVertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl GuideVertex {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GuideVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    light_dir: [f32; 4],
    camera_pos: [f32; 4],
    planet_color: [f32; 4],
    atmosphere_color: [f32; 4],
    atmosphere_strength: f32,
    specular_power: f32,
    specular_strength: f32,
    _padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ViewUniform {
    view_proj: [[f32; 4]; 4],
    time_s: f32,
    _padding: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    config_path: Option<String>,
    filter: String,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<String>,
    width: u32,
    height: u32,
    fov_deg: f32,
    near_plane: f32,
    far_plane: f32,
    planet_radius: f32,
    star_shell_radius: f32,
    planet_rotation_deg_per_s: f32,
    atmosphere_strength: f32,
    show_guides: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            filter: "R".to_string(),
            exposure_s: 60.0,
            repeats: 2,
            output_dir: None,
            width: 1600,
            height: 900,
            fov_deg: 56.0,
            near_plane: 0.1,
            far_plane: 400.0,
            planet_radius: 1.0,
            star_shell_radius: 45.0,
            planet_rotation_deg_per_s: 7.5,
            atmosphere_strength: 0.65,
            show_guides: true,
        }
    }
}

#[derive(Clone, Debug)]
struct SceneTarget {
    name: String,
    position: [f32; 3],
    luminance: f32,
}

#[derive(Clone, Debug)]
struct SceneDefinition {
    campaign_name: String,
    targets: Vec<SceneTarget>,
}

#[derive(Clone, Debug)]
struct RuntimeSummary {
    app_name: String,
    config_name: String,
    ready: bool,
    phase: String,
}

#[derive(Debug)]
struct Camera {
    yaw: f64,
    pitch: f64,
    distance: f64,
    fov_deg: f32,
    near_plane: f32,
    far_plane: f32,
}

impl Camera {
    fn eye(&self) -> DVec3 {
        let x = self.distance * self.pitch.cos() * self.yaw.cos();
        let y = self.distance * self.pitch.sin();
        let z = self.distance * self.pitch.cos() * self.yaw.sin();
        DVec3::new(x, y, z)
    }

    fn view_proj_f64(&self, aspect: f64) -> DMat4 {
        let eye = self.eye();
        let center = DVec3::ZERO;
        let up = DVec3::Y;
        let view = DMat4::look_at_rh(eye, center, up);
        let proj = DMat4::perspective_rh(self.fov_deg as f64, aspect, self.near_plane as f64, self.far_plane as f64);
        proj * view
    }

    fn view_proj(&self, aspect: f32) -> Mat4 {
        let view_proj = self.view_proj_f64(aspect as f64).to_cols_array_2d();
        Mat4::from_cols_array_2d(&[
            [view_proj[0][0] as f32, view_proj[0][1] as f32, view_proj[0][2] as f32, view_proj[0][3] as f32],
            [view_proj[1][0] as f32, view_proj[1][1] as f32, view_proj[1][2] as f32, view_proj[1][3] as f32],
            [view_proj[2][0] as f32, view_proj[2][1] as f32, view_proj[2][2] as f32, view_proj[2][3] as f32],
            [view_proj[3][0] as f32, view_proj[3][1] as f32, view_proj[3][2] as f32, view_proj[3][3] as f32],
        ])
    }

    fn reset(&mut self) {
        self.yaw = 0.25 * PI as f64;
        self.pitch = 0.18 * PI as f64;
        self.distance = 5.5;
    }
}

#[derive(Debug, Default)]
struct InputState {
    yaw_left: bool,
    yaw_right: bool,
    pitch_up: bool,
    pitch_down: bool,
    zoom_in: bool,
    zoom_out: bool,
    mouse_drag_active: bool,
    last_cursor: Option<(f64, f64)>,
    pending_scroll: f32,
}

struct DepthBuffer {
    view: wgpu::TextureView,
}

impl DepthBuffer {
    fn new(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth_buffer"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view }
    }
}

struct UiFrameData {
    paint_jobs: Vec<egui::epaint::ClippedPrimitive>,
    textures_delta: egui::TexturesDelta,
    pixels_per_point: f32,
}

struct RenderState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth: DepthBuffer,

    planet_pipeline: wgpu::RenderPipeline,
    star_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    guide_pipeline: wgpu::RenderPipeline,

    frame_uniform: FrameUniform,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,

    view_uniform: ViewUniform,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,

    planet_vertex_buffer: wgpu::Buffer,
    planet_index_buffer: wgpu::Buffer,
    planet_index_count: u32,

    star_vertex_buffer: wgpu::Buffer,
    star_count: u32,
    sky_vertex_buffer: wgpu::Buffer,
    sky_index_buffer: wgpu::Buffer,
    sky_index_count: u32,

    guide_vertex_buffer: wgpu::Buffer,
    guide_vertex_count: u32,

    camera: Camera,
    input: InputState,

    target_names: Vec<String>,
    target_count: usize,
    show_guides: bool,
    paused: bool,
    atmosphere_strength: f32,
    planet_rotation_deg_per_s: f32,

    planet_rotation_rad: f32,
    light_orbit_rad: f32,
    elapsed_time_s: f32,

    runtime: RuntimeSummary,
    campaign_name: String,
    title_last_update: Instant,

    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

impl RenderState {
    async fn new(
        window: Arc<Window>,
        scene: SceneDefinition,
        runtime: RuntimeSummary,
        options: &CliOptions,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| format!("surface creation failed: {error}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| "no compatible GPU adapter found".to_string())?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ui3d_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|error| format!("request_device failed: {error}"))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = select_surface_format(&surface_caps)
            .ok_or_else(|| "no surface format available".to_string())?;
        let present_mode = select_present_mode(&surface_caps)
            .ok_or_else(|| "no surface present mode available".to_string())?;
        let alpha_mode = surface_caps
            .alpha_modes
            .first()
            .copied()
            .ok_or_else(|| "no surface alpha mode available".to_string())?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: options.width.max(1),
            height: options.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };

        surface.configure(&device, &config);
        let depth = DepthBuffer::new(&device, &config);

        let camera = Camera {
            yaw: 0.25 * PI as f64,
            pitch: 0.18 * PI as f64,
            distance: 5.5,
            fov_deg: options.fov_deg,
            near_plane: options.near_plane,
            far_plane: options.far_plane,
        };

        let aspect = config.width as f32 / config.height as f32;
        let view_proj = camera.view_proj(aspect);
        let sky_tint = tone_map_color(sky_background_color(0.75, 8.0));
        let solar_glow = solar_halo_color(0.75, 0.9);
        let (planet_color, atmosphere_color) = atmospheric_palette(
            options.atmosphere_strength,
            0.75,
        );

        let frame_uniform = FrameUniform {
            view_proj: view_proj.to_cols_array_2d(),
            model: Mat4::IDENTITY.to_cols_array_2d(),
            light_dir: [0.7, 0.35, 0.61, 0.0],
            camera_pos: [
                camera.eye().x as f32,
                camera.eye().y as f32,
                camera.eye().z as f32,
                0.0,
            ],
            planet_color: [
                (planet_color[0] * 0.85 + sky_tint[0] * 0.12 + solar_glow[0] * 0.08).clamp(0.0, 1.0),
                (planet_color[1] * 0.85 + sky_tint[1] * 0.12 + solar_glow[1] * 0.08).clamp(0.0, 1.0),
                (planet_color[2] * 0.85 + sky_tint[2] * 0.12 + solar_glow[2] * 0.08).clamp(0.0, 1.0),
                1.0,
            ],
            atmosphere_color: [
                (atmosphere_color[0] * 0.74 + sky_tint[0] * 0.18 + solar_glow[0] * 0.18).clamp(0.0, 1.0),
                (atmosphere_color[1] * 0.74 + sky_tint[1] * 0.18 + solar_glow[1] * 0.18).clamp(0.0, 1.0),
                (atmosphere_color[2] * 0.74 + sky_tint[2] * 0.18 + solar_glow[2] * 0.18).clamp(0.0, 1.0),
                1.0,
            ],
            atmosphere_strength: options.atmosphere_strength,
            specular_power: 48.0,
            specular_strength: 0.18,
            _padding: 0.0,
        };

        let frame_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frame_uniform_buffer"),
            contents: bytemuck::bytes_of(&frame_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let view_uniform = ViewUniform {
            view_proj: view_proj.to_cols_array_2d(),
            time_s: 0.0,
            _padding: [0.0; 3],
        };

        let view_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("view_uniform_buffer"),
            contents: bytemuck::bytes_of(&view_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let frame_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame_bind_group"),
            layout: &frame_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });

        let view_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view_bind_group"),
            layout: &view_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });

        let planet_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("planet_shader"),
            source: wgpu::ShaderSource::Wgsl(PLANET_SHADER_WGSL.into()),
        });

        let star_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("star_shader"),
            source: wgpu::ShaderSource::Wgsl(STAR_SHADER_WGSL.into()),
        });

        let guide_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("guide_shader"),
            source: wgpu::ShaderSource::Wgsl(GUIDE_SHADER_WGSL.into()),
        });

        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky_shader"),
            source: wgpu::ShaderSource::Wgsl(r#"
                struct ViewUniform {
                    view_proj: mat4x4<f32>,
                    time_s: f32,
                    _pad0: vec3<f32>,
                };

                @group(0) @binding(0)
                var<uniform> view_data: ViewUniform;

                struct VertexInput {
                    @location(0) position: vec3<f32>,
                    @location(1) color: vec3<f32>,
                };

                struct VertexOutput {
                    @builtin(position) clip_position: vec4<f32>,
                    @location(0) color: vec3<f32>,
                };

                @vertex
                fn vs_main(input: VertexInput) -> VertexOutput {
                    var out: VertexOutput;
                    out.clip_position = view_data.view_proj * vec4<f32>(input.position, 1.0);
                    out.color = input.color;
                    return out;
                }

                @fragment
                fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
                    return vec4<f32>(input.color, 1.0);
                }
            "#.into()),
        });

        let planet_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("planet_pipeline_layout"),
            bind_group_layouts: &[&frame_bind_group_layout],
            push_constant_ranges: &[],
        });

        let planet_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("planet_pipeline"),
            layout: Some(&planet_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &planet_shader,
                entry_point: "vs_main",
                buffers: &[MeshVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &planet_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let star_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("star_pipeline_layout"),
            bind_group_layouts: &[&view_bind_group_layout],
            push_constant_ranges: &[],
        });

        let star_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("star_pipeline"),
            layout: Some(&star_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &star_shader,
                entry_point: "vs_main",
                buffers: &[StarVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &star_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::PointList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky_pipeline_layout"),
            bind_group_layouts: &[&view_bind_group_layout],
            push_constant_ranges: &[],
        });

        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky_pipeline"),
            layout: Some(&sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: "vs_main",
                buffers: &[GuideVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let guide_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("guide_pipeline_layout"),
            bind_group_layouts: &[&view_bind_group_layout],
            push_constant_ranges: &[],
        });

        let guide_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("guide_pipeline"),
            layout: Some(&guide_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &guide_shader,
                entry_point: "vs_main",
                buffers: &[GuideVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &guide_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let (planet_vertices, planet_indices) = generate_uv_sphere(96, 64, options.planet_radius);
        let planet_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("planet_vertex_buffer"),
            contents: bytemuck::cast_slice(&planet_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let planet_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("planet_index_buffer"),
            contents: bytemuck::cast_slice(&planet_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let background_stars = generate_background_stars(600, 120.0);
        let star_vertices: Vec<StarVertex> = background_stars
            .into_iter()
            .chain(scene.targets.iter().map(|target| StarVertex {
                position: target.position,
                luminance: target.luminance,
            }))
            .collect();

        let star_vertex_buffer = create_vertex_buffer_with_fallback(
            &device,
            "star_vertex_buffer",
            &star_vertices,
            wgpu::BufferUsages::VERTEX,
        );

        let mut guide_vertices = Vec::<GuideVertex>::with_capacity(scene.targets.len().saturating_mul(2));
        for target in &scene.targets {
            guide_vertices.push(GuideVertex {
                position: [0.0, 0.0, 0.0],
                color: [0.30, 0.66, 1.0],
            });
            guide_vertices.push(GuideVertex {
                position: target.position,
                color: [0.30, 0.66, 1.0],
            });
        }

        let (sky_vertices, sky_indices) = generate_sky_dome(32, 24, 120.0);
        let sky_vertex_buffer = create_vertex_buffer_with_fallback(
            &device,
            "sky_vertex_buffer",
            &sky_vertices,
            wgpu::BufferUsages::VERTEX,
        );
        let sky_index_buffer = create_vertex_buffer_with_fallback(
            &device,
            "sky_index_buffer",
            &sky_indices,
            wgpu::BufferUsages::INDEX,
        );

        let guide_vertex_buffer = create_vertex_buffer_with_fallback(
            &device,
            "guide_vertex_buffer",
            &guide_vertices,
            wgpu::BufferUsages::VERTEX,
        );

        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format,
            Some(wgpu::TextureFormat::Depth32Float),
            1,
        );

        let target_names = scene.targets.iter().map(|target| target.name.clone()).collect();

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            depth,
            planet_pipeline,
            star_pipeline,
            sky_pipeline,
            guide_pipeline,
            frame_uniform,
            frame_buffer,
            frame_bind_group,
            view_uniform,
            view_buffer,
            view_bind_group,
            planet_vertex_buffer,
            planet_index_buffer,
            planet_index_count: planet_indices.len() as u32,
            star_vertex_buffer,
            star_count: star_vertices.len() as u32,
            sky_vertex_buffer,
            sky_index_buffer,
            sky_index_count: sky_indices.len() as u32,
            guide_vertex_buffer,
            guide_vertex_count: guide_vertices.len() as u32,
            camera,
            input: InputState::default(),
            target_names,
            target_count: scene.targets.len(),
            show_guides: options.show_guides,
            paused: false,
            atmosphere_strength: options.atmosphere_strength,
            planet_rotation_deg_per_s: options.planet_rotation_deg_per_s,
            planet_rotation_rad: 0.0,
            light_orbit_rad: 0.0,
            elapsed_time_s: 0.0,
            runtime,
            campaign_name: scene.campaign_name,
            title_last_update: Instant::now(),
            egui_ctx,
            egui_state,
            egui_renderer,
        })
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }

        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        self.depth = DepthBuffer::new(&self.device, &self.config);
    }

    fn process_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => {
                self.input.mouse_drag_active = *state == ElementState::Pressed;
                if !self.input.mouse_drag_active {
                    self.input.last_cursor = None;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.input.mouse_drag_active {
                    if let Some((last_x, last_y)) = self.input.last_cursor {
                        let dx = (position.x - last_x) as f64;
                        let dy = (position.y - last_y) as f64;
                        self.camera.yaw += dx * 0.0042;
                        self.camera.pitch -= dy * 0.0038;
                    }
                    self.input.last_cursor = Some((position.x, position.y));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(pixels) => pixels.y as f32 * 0.03,
                };
                self.input.pending_scroll += scroll;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    match code {
                        KeyCode::KeyA | KeyCode::ArrowLeft => self.input.yaw_left = pressed,
                        KeyCode::KeyD | KeyCode::ArrowRight => self.input.yaw_right = pressed,
                        KeyCode::KeyW | KeyCode::ArrowUp => self.input.pitch_up = pressed,
                        KeyCode::KeyS | KeyCode::ArrowDown => self.input.pitch_down = pressed,
                        KeyCode::KeyQ => self.input.zoom_in = pressed,
                        KeyCode::KeyE => self.input.zoom_out = pressed,
                        _ => {}
                    }

                    if pressed && !event.repeat {
                        match code {
                            KeyCode::Space => self.paused = !self.paused,
                            KeyCode::KeyG => self.show_guides = !self.show_guides,
                            KeyCode::Equal | KeyCode::NumpadAdd => {
                                self.atmosphere_strength = (self.atmosphere_strength + 0.05).min(1.5);
                            }
                            KeyCode::Minus | KeyCode::NumpadSubtract => {
                                self.atmosphere_strength = (self.atmosphere_strength - 0.05).max(0.0);
                            }
                            KeyCode::BracketRight => {
                                self.planet_rotation_deg_per_s =
                                    (self.planet_rotation_deg_per_s + 0.5).min(30.0);
                            }
                            KeyCode::BracketLeft => {
                                self.planet_rotation_deg_per_s =
                                    (self.planet_rotation_deg_per_s - 0.5).max(0.0);
                            }
                            KeyCode::KeyR => self.camera.reset(),
                            KeyCode::KeyH => print_controls(),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn update(&mut self, dt_s: f32) {
        let yaw_speed = 1.05_f64;
        let pitch_speed = 0.95_f64;
        let zoom_speed = 2.8_f64;
        let dt_s = dt_s as f64;

        if self.input.yaw_left {
            self.camera.yaw -= yaw_speed * dt_s;
        }
        if self.input.yaw_right {
            self.camera.yaw += yaw_speed * dt_s;
        }
        if self.input.pitch_up {
            self.camera.pitch += pitch_speed * dt_s;
        }
        if self.input.pitch_down {
            self.camera.pitch -= pitch_speed * dt_s;
        }
        if self.input.zoom_in {
            self.camera.distance -= zoom_speed * dt_s;
        }
        if self.input.zoom_out {
            self.camera.distance += zoom_speed * dt_s;
        }

        if self.input.pending_scroll.abs() > f32::EPSILON {
            self.camera.distance -= self.input.pending_scroll as f64 * 0.45;
            self.input.pending_scroll = 0.0;
        }

        self.camera.pitch = self
            .camera
            .pitch
            .clamp(-(FRAC_PI_2 as f64) + 0.02, (FRAC_PI_2 as f64) - 0.02);
        self.camera.distance = self.camera.distance.clamp(1.8, 80.0);

        if !self.paused {
            self.planet_rotation_rad += self.planet_rotation_deg_per_s.to_radians() * dt_s as f32;
            self.light_orbit_rad += 0.16 * dt_s as f32;
            self.elapsed_time_s += dt_s as f32;
        }

        let aspect = self.config.width as f32 / self.config.height as f32;
        let view_proj = self.camera.view_proj(aspect);
        self.view_uniform.view_proj = view_proj.to_cols_array_2d();
        self.view_uniform.time_s = self.elapsed_time_s;
        self.queue
            .write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&self.view_uniform));

        let planet_model = Mat4::from_rotation_y(self.planet_rotation_rad);
        let light_dir = Vec3::new(self.light_orbit_rad.cos(), 0.30, self.light_orbit_rad.sin()).normalize();
        let (planet_color, atmosphere_color) = atmospheric_palette(
            self.atmosphere_strength,
            0.7 + 0.3 * self.light_orbit_rad.cos().abs(),
        );
        let solar_glow = solar_halo_color(0.8 + 0.2 * self.light_orbit_rad.cos(), 0.7 + 0.3 * self.light_orbit_rad.cos().abs());

        self.frame_uniform.view_proj = view_proj.to_cols_array_2d();
        self.frame_uniform.model = planet_model.to_cols_array_2d();
        self.frame_uniform.light_dir = [light_dir.x, light_dir.y, light_dir.z, 0.0];
        let eye = self.camera.eye();
        self.frame_uniform.camera_pos = [
            eye.x as f32,
            eye.y as f32,
            eye.z as f32,
            0.0,
        ];
        self.frame_uniform.planet_color = [
            (planet_color[0] * 0.88 + solar_glow[0] * 0.12).clamp(0.0, 1.0),
            (planet_color[1] * 0.88 + solar_glow[1] * 0.12).clamp(0.0, 1.0),
            (planet_color[2] * 0.88 + solar_glow[2] * 0.12).clamp(0.0, 1.0),
            1.0,
        ];
        self.frame_uniform.atmosphere_color = [
            (atmosphere_color[0] * 0.78 + solar_glow[0] * 0.22).clamp(0.0, 1.0),
            (atmosphere_color[1] * 0.78 + solar_glow[1] * 0.22).clamp(0.0, 1.0),
            (atmosphere_color[2] * 0.78 + solar_glow[2] * 0.22).clamp(0.0, 1.0),
            1.0,
        ];
        self.frame_uniform.atmosphere_strength = self.atmosphere_strength;

        self.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&self.frame_uniform));

        if self.title_last_update.elapsed() >= Duration::from_millis(220) {
            self.window.set_title(&build_title(self));
            self.title_last_update = Instant::now();
        }
    }

    fn draw_egui(&mut self) -> UiFrameData {
        let raw_input = self.egui_state.take_egui_input(self.window.as_ref());
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading(RichText::new("Observatory UI + 3D").strong());
                    ui.separator();
                    ui.label(format!("campaign: {}", self.campaign_name));
                    ui.separator();
                    ui.label(format!("phase: {}", self.runtime.phase));
                    ui.separator();
                    let status = if self.runtime.ready { "READY" } else { "NOT_READY" };
                    let color = if self.runtime.ready {
                        Color32::from_rgb(120, 240, 160)
                    } else {
                        Color32::from_rgb(255, 180, 120)
                    };
                    ui.colored_label(color, status);
                });
            });

            egui::SidePanel::left("controls")
                .resizable(true)
                .default_width(300.0)
                .show(ctx, |ui| {
                    ui.heading("Rendu immersif");
                    ui.add_space(6.0);

                    ui.checkbox(&mut self.paused, "Pause animation");
                    ui.checkbox(&mut self.show_guides, "Afficher les guides 3D");

                    ui.add(
                        egui::Slider::new(&mut self.atmosphere_strength, 0.0..=1.5)
                            .text("Atmosphere"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.planet_rotation_deg_per_s, 0.0..=30.0)
                            .text("Rotation planet (deg/s)"),
                    );

                    ui.separator();
                    ui.label(format!("Camera distance: {:.2}", self.camera.distance));
                    ui.label(format!("Camera fov: {:.1} deg", self.camera.fov_deg));
                    if ui.button("Reset camera (R)").clicked() {
                        self.camera.reset();
                    }

                    ui.separator();
                    ui.heading("Cibles de campagne");
                    if self.target_names.is_empty() {
                        ui.label("Aucune cible dans la configuration");
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .show(ui, |ui| {
                                for (index, name) in self.target_names.iter().enumerate() {
                                    ui.label(format!("{:02} - {}", index + 1, name));
                                }
                            });
                    }

                    ui.separator();
                    ui.label("Controles souris/clavier:");
                    ui.label("- drag souris: orbite camera");
                    ui.label("- molette / Q / E: zoom");
                    ui.label("- WASD / fleches: orbite");
                    ui.label("- [ ] vitesse rotation");
                    ui.label("- - + atmosphere");
                    if ui.button("Afficher l'aide terminal (H)").clicked() {
                        print_controls();
                    }
                });
        });

        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output: _,
            ..
        } = full_output;

        self.egui_state
            .handle_platform_output(self.window.as_ref(), platform_output);

        let paint_jobs = self.egui_ctx.tessellate(shapes, pixels_per_point);

        UiFrameData {
            paint_jobs,
            textures_delta,
            pixels_per_point,
        }
    }

    fn render(&mut self, ui_frame: UiFrameData) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        for (id, image_delta) in &ui_frame.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ui_frame.pixels_per_point,
        };

        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("ui3d_encoder"),
                });

        let mut user_cmd_buffers = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &ui_frame.paint_jobs,
            &screen_descriptor,
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui3d_scene_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky_background_color(-0.9, self.elapsed_time_s)[0] as f64,
                            g: sky_background_color(-0.9, self.elapsed_time_s)[1] as f64,
                            b: sky_background_color(-0.9, self.elapsed_time_s)[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if self.star_count > 0 {
                pass.set_pipeline(&self.star_pipeline);
                pass.set_bind_group(0, &self.view_bind_group, &[]);
                pass.set_vertex_buffer(0, self.star_vertex_buffer.slice(..));
                pass.draw(0..self.star_count, 0..1);
            }

            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(0, &self.view_bind_group, &[]);
            pass.set_vertex_buffer(0, self.sky_vertex_buffer.slice(..));
            pass.set_index_buffer(self.sky_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.sky_index_count, 0, 0..1);

            if self.show_guides && self.guide_vertex_count > 1 {
                pass.set_pipeline(&self.guide_pipeline);
                pass.set_bind_group(0, &self.view_bind_group, &[]);
                pass.set_vertex_buffer(0, self.guide_vertex_buffer.slice(..));
                pass.draw(0..self.guide_vertex_count, 0..1);
            }

            pass.set_pipeline(&self.planet_pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_vertex_buffer(0, self.planet_vertex_buffer.slice(..));
            pass.set_index_buffer(self.planet_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.planet_index_count, 0, 0..1);
        }

        {
            let mut ui_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui3d_egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.egui_renderer
                .render(&mut ui_pass, &ui_frame.paint_jobs, &screen_descriptor);
        }

        user_cmd_buffers.push(encoder.finish());
        self.queue.submit(user_cmd_buffers);
        output.present();

        for id in &ui_frame.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        Ok(())
    }
}

fn create_vertex_buffer_with_fallback<T: Pod>(
    device: &wgpu::Device,
    label: &str,
    data: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    if data.is_empty() {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<T>() as u64,
            usage,
            mapped_at_creation: false,
        })
    } else {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(data),
            usage,
        })
    }
}

fn print_controls() {
    println!("--- ui3d_app controls ---");
    println!("mouse left drag: orbit camera");
    println!("wheel / Q / E   : zoom in/out");
    println!("W/A/S/D         : orbit camera");
    println!("space           : pause/resume animation");
    println!("G               : toggle target guide lines");
    println!("[ and ]         : decrease/increase planet rotation speed");
    println!("- and +         : decrease/increase atmosphere strength");
    println!("R               : reset camera");
    println!("H               : print controls");
}

fn build_title(state: &RenderState) -> String {
    format!(
        "{} | campaign={} phase={} ready={} | targets={} | dist={:.2} fov={:.1}deg | atmosphere={:.2} rot={:.1}deg/s | guides={} paused={}",
        state.runtime.app_name,
        state.runtime.config_name,
        state.runtime.phase,
        state.runtime.ready,
        state.target_count,
        state.camera.distance,
        state.camera.fov_deg,
        state.atmosphere_strength,
        state.planet_rotation_deg_per_s,
        state.show_guides,
        state.paused,
    )
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --filter");
                    std::process::exit(1);
                };
                options.filter = value.clone();
                i += 2;
            }
            "--exposure" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --exposure");
                    std::process::exit(1);
                };
                options.exposure_s = value.parse::<f64>().unwrap_or_else(|_| {
                    eprintln!("invalid --exposure value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--repeats" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --repeats");
                    std::process::exit(1);
                };
                options.repeats = value.parse::<usize>().unwrap_or_else(|_| {
                    eprintln!("invalid --repeats value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--output-dir" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --output-dir");
                    std::process::exit(1);
                };
                options.output_dir = Some(value.clone());
                i += 2;
            }
            "--width" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --width");
                    std::process::exit(1);
                };
                options.width = value.parse::<u32>().unwrap_or_else(|_| {
                    eprintln!("invalid --width value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--height" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --height");
                    std::process::exit(1);
                };
                options.height = value.parse::<u32>().unwrap_or_else(|_| {
                    eprintln!("invalid --height value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--fov" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --fov");
                    std::process::exit(1);
                };
                options.fov_deg = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --fov value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--near" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --near");
                    std::process::exit(1);
                };
                options.near_plane = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --near value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--far" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --far");
                    std::process::exit(1);
                };
                options.far_plane = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --far value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--planet-radius" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --planet-radius");
                    std::process::exit(1);
                };
                options.planet_radius = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --planet-radius value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--star-radius" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --star-radius");
                    std::process::exit(1);
                };
                options.star_shell_radius = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --star-radius value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--rotation-speed" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --rotation-speed");
                    std::process::exit(1);
                };
                options.planet_rotation_deg_per_s = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --rotation-speed value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--atmosphere" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --atmosphere");
                    std::process::exit(1);
                };
                options.atmosphere_strength = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --atmosphere value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--no-guides" => {
                options.show_guides = false;
                i += 1;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ if args[i].starts_with("--output-dir=") => {
                options.output_dir = Some(args[i].trim_start_matches("--output-dir=").to_string());
                i += 1;
            }
            _ => {
                if options.config_path.is_none() {
                    options.config_path = Some(args[i].clone());
                } else {
                    eprintln!("unexpected argument: '{}'", args[i]);
                    print_usage();
                    std::process::exit(1);
                }
                i += 1;
            }
        }
    }

    if options.width == 0 || options.height == 0 {
        eprintln!("--width and --height must be strictly positive");
        std::process::exit(1);
    }
    if options.fov_deg <= 1.0 || options.fov_deg >= 160.0 {
        eprintln!("--fov must be in range ]1, 160[");
        std::process::exit(1);
    }
    if options.near_plane <= 0.0 || options.far_plane <= options.near_plane {
        eprintln!("invalid near/far planes: near must be > 0 and far > near");
        std::process::exit(1);
    }
    if options.planet_radius <= 0.0 || options.star_shell_radius <= options.planet_radius {
        eprintln!("invalid scene scale: star radius must be larger than planet radius");
        std::process::exit(1);
    }

    options
}

fn print_usage() {
    println!("Usage:");
    println!("  ui3d_app <config-file> [options]");
    println!();
    println!("Core options:");
    println!("  --filter <name>            photometric filter for runtime validation (default: R)");
    println!("  --exposure <seconds>       exposure duration (default: 60)");
    println!("  --repeats <count>          repeats per target (default: 2)");
    println!("  --output-dir <path>        output directory for runtime artifacts");
    println!();
    println!("Window and camera:");
    println!("  --width <px>               window width (default: 1600)");
    println!("  --height <px>              window height (default: 900)");
    println!("  --fov <deg>                camera field of view in degrees (default: 56)");
    println!("  --near <value>             near clipping plane (default: 0.1)");
    println!("  --far <value>              far clipping plane (default: 400)");
    println!();
    println!("3D scene:");
    println!("  --planet-radius <value>    planet radius in scene units (default: 1)");
    println!("  --star-radius <value>      radius of target star shell (default: 45)");
    println!("  --rotation-speed <deg/s>   planet rotation speed (default: 7.5)");
    println!("  --atmosphere <value>       atmosphere rim strength (default: 0.65)");
    println!("  --no-guides                disable guide lines from origin to targets");
    println!();
    println!("Interactive controls:");
    println!("  mouse drag / WASD          orbit camera");
    println!("  wheel, Q, E                zoom");
    println!("  space                      pause/resume animation");
    println!("  [ and ]                    decrease/increase rotation speed");
    println!("  - and +                    decrease/increase atmosphere");
    println!("  G                          toggle guide lines");
    println!("  R                          reset camera");
    println!("  H                          print controls to terminal");
}

fn create_scene(config_text: &str, star_shell_radius: f32) -> Result<SceneDefinition, String> {
    let config = parse_campaign_config(config_text)?;

    let targets: Vec<SceneTarget> = config
        .targets
        .iter()
        .map(|target| {
            let position = ra_dec_to_cartesian_f64(target, star_shell_radius as f64);
            SceneTarget {
                name: target.name.clone(),
                position: [position[0] as f32, position[1] as f32, position[2] as f32],
                luminance: priority_to_luminance(target.priority),
            }
        })
        .collect();

    Ok(SceneDefinition {
        campaign_name: config.name,
        targets,
    })
}

fn ra_dec_to_cartesian_f64(target: &CampaignTargetConfig, radius: f64) -> [f64; 3] {
    let ra = target.ra_deg.to_radians() as f64;
    let dec = target.dec_deg.to_radians() as f64;

    let x = radius * dec.cos() * ra.cos();
    let y = radius * dec.sin();
    let z = radius * dec.cos() * ra.sin();

    [x, y, z]
}

#[allow(dead_code)]
fn ra_dec_to_cartesian(target: &CampaignTargetConfig, radius: f32) -> [f32; 3] {
    let position = ra_dec_to_cartesian_f64(target, radius as f64);
    [position[0] as f32, position[1] as f32, position[2] as f32]
}

fn priority_to_luminance(priority: u8) -> f32 {
    let normalized = (priority as f32 / 10.0).clamp(0.0, 1.0);
    0.30 + 0.70 * normalized
}

fn atmospheric_palette(atmosphere_strength: f32, light_tilt: f32) -> ([f32; 4], [f32; 4]) {
    let glow = atmosphere_strength.clamp(0.0, 1.5);
    let day_mix = (light_tilt * 0.5 + 0.5).clamp(0.0, 1.0);
    let dawn_glow = (1.0 - (light_tilt - 0.65).abs() / 0.65).clamp(0.0, 1.0);

    let planet = [
        0.09 + 0.12 * day_mix + 0.08 * dawn_glow,
        0.18 + 0.30 * day_mix + 0.12 * dawn_glow,
        0.36 + 0.42 * day_mix + 0.18 * dawn_glow,
        1.0,
    ];
    let atmosphere = [
        0.16 + 0.48 * glow + 0.20 * dawn_glow,
        0.30 + 0.50 * glow + 0.24 * dawn_glow,
        0.72 + 0.22 * glow + 0.12 * dawn_glow,
        1.0,
    ];

    (planet, atmosphere)
}

fn sky_background_color(elevation: f32, time_of_day: f32) -> [f32; 3] {
    let norm = elevation.clamp(-1.0, 1.0);
    let daylight = (0.5 + 0.5 * (time_of_day * 0.1).sin()).clamp(0.1, 1.0);
    let zenith_factor = (norm + 1.0) * 0.5;
    let horizon_factor = (1.0 - zenith_factor).max(0.0);
    let solar_altitude = (time_of_day * 0.12).sin().clamp(-1.0, 1.0);
    let twilight = (1.0 - solar_altitude.abs()).clamp(0.0, 1.0);

    let rayleigh = 0.09 + 0.36 * daylight + 0.18 * zenith_factor;
    let ozone = 0.04 + 0.28 * daylight + 0.10 * twilight;
    let haze = 0.06 + 0.22 * horizon_factor;

    [
        0.01 + rayleigh + 0.12 * zenith_factor - haze,
        0.02 + 0.95 * rayleigh + 0.18 * zenith_factor - 0.15 * horizon_factor,
        0.05 + 1.35 * rayleigh + ozone + 0.22 * zenith_factor + 0.18 * horizon_factor,
    ]
}

fn tone_map_color(value: [f32; 3]) -> [f32; 3] {
    let exposure = 1.42;
    let mapped = [
        value[0] * exposure,
        value[1] * exposure,
        value[2] * exposure,
    ];
    let gamma = [
        mapped[0].powf(0.82),
        mapped[1].powf(0.82),
        mapped[2].powf(0.82),
    ];

    [
        gamma[0].clamp(0.0, 1.0),
        gamma[1].clamp(0.0, 1.0),
        gamma[2].clamp(0.0, 1.0),
    ]
}

fn solar_halo_color(elevation: f32, daylight: f32) -> [f32; 3] {
    let norm = elevation.clamp(-1.0, 1.0);
    let glow = daylight.clamp(0.0, 1.0);
    let horizon = (1.0 - (norm + 1.0) * 0.5).clamp(0.0, 1.0);
    let solar = 0.2 + 0.8 * glow * (1.0 - horizon * 0.65);

    [
        0.08 + 0.42 * solar,
        0.14 + 0.46 * solar,
        0.30 + 0.70 * solar,
    ]
}

fn generate_sky_dome(lat_segments: u32, lon_segments: u32, radius: f32) -> (Vec<GuideVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for lat in 0..=lat_segments {
        let v = lat as f32 / lat_segments as f32;
        let theta = v * PI;
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();

        for lon in 0..=lon_segments {
            let u = lon as f32 / lon_segments as f32;
            let phi = u * 2.0 * PI;
            let sin_phi = phi.sin();
            let cos_phi = phi.cos();

            let x = radius * sin_theta * cos_phi;
            let y = radius * cos_theta;
            let z = radius * sin_theta * sin_phi;

            let elevation = (y / radius).clamp(-1.0, 1.0);
            let color = sky_background_color(elevation, 8.0);
            vertices.push(GuideVertex {
                position: [x, y, z],
                color,
            });
        }
    }

    let ring = lon_segments + 1;
    for lat in 0..lat_segments {
        for lon in 0..lon_segments {
            let idx0 = lat * ring + lon;
            let idx1 = idx0 + 1;
            let idx2 = idx0 + ring;
            let idx3 = idx2 + 1;
            indices.push(idx0);
            indices.push(idx2);
            indices.push(idx1);
            indices.push(idx1);
            indices.push(idx2);
            indices.push(idx3);
        }
    }

    (vertices, indices)
}

fn generate_background_stars(count: usize, radius: f32) -> Vec<StarVertex> {
    let mut stars = Vec::with_capacity(count);

    for index in 0..count {
        let seed = (index as u64 + 1)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let u0 = ((seed & 0xFFFF_FFFF) as f32) / 4294967295.0_f32;
        let u1 = (((seed >> 32) & 0xFFFF_FFFF) as f32) / 4294967295.0_f32;
        let u2 = (((seed >> 16) & 0xFFFF_FFFF) as f32) / 4294967295.0_f32;

        // Deterministic spherical distribution on a shell of constant radius.
        let theta = 2.0 * PI * u0;
        let cos_theta = 2.0 * u1 - 1.0;
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();

        let x = radius * sin_theta * theta.cos();
        let y = radius * cos_theta;
        let z = radius * sin_theta * theta.sin();

        let luminance = 0.15 + 0.85 * (0.45 + 0.55 * u2);
        stars.push(StarVertex {
            position: [x, y, z],
            luminance,
        });
    }

    stars
}

fn generate_uv_sphere(
    longitude_segments: u32,
    latitude_segments: u32,
    radius: f32,
) -> (Vec<MeshVertex>, Vec<u32>) {
    let mut vertices =
        Vec::with_capacity(((longitude_segments + 1) * (latitude_segments + 1)) as usize);
    let mut indices = Vec::with_capacity((longitude_segments * latitude_segments * 6) as usize);

    for lat in 0..=latitude_segments {
        let v = lat as f32 / latitude_segments as f32;
        let theta = v * PI;
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();

        for lon in 0..=longitude_segments {
            let u = lon as f32 / longitude_segments as f32;
            let phi = u * 2.0 * PI;
            let sin_phi = phi.sin();
            let cos_phi = phi.cos();

            let nx = cos_phi * sin_theta;
            let ny = cos_theta;
            let nz = sin_phi * sin_theta;

            vertices.push(MeshVertex {
                position: [radius * nx, radius * ny, radius * nz],
                normal: [nx, ny, nz],
            });
        }
    }

    let ring_vertices = longitude_segments + 1;
    for lat in 0..latitude_segments {
        for lon in 0..longitude_segments {
            let i0 = lat * ring_vertices + lon;
            let i1 = i0 + 1;
            let i2 = i0 + ring_vertices;
            let i3 = i2 + 1;

            indices.push(i0);
            indices.push(i2);
            indices.push(i1);

            indices.push(i1);
            indices.push(i2);
            indices.push(i3);
        }
    }

    (vertices, indices)
}

fn select_surface_format(caps: &wgpu::SurfaceCapabilities) -> Option<wgpu::TextureFormat> {
    caps.formats
        .iter()
        .copied()
        .find(|format| format.is_srgb())
        .or_else(|| caps.formats.first().copied())
}

fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> Option<wgpu::PresentMode> {
    if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        Some(wgpu::PresentMode::Mailbox)
    } else if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
        Some(wgpu::PresentMode::Fifo)
    } else {
        caps.present_modes.first().copied()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_cli_args(&args);

    let Some(config_path) = options.config_path.clone() else {
        print_usage();
        std::process::exit(1);
    };

    let config_text = std::fs::read_to_string(&config_path).unwrap_or_else(|error| {
        eprintln!("failed to read config '{}': {error}", config_path);
        std::process::exit(1);
    });

    if let Some(output_dir) = options.output_dir.as_ref() {
        let path = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&path) {
            eprintln!("failed to create output dir '{}': {error}", output_dir);
            std::process::exit(1);
        }
    }

    let app = build_application(
        &config_text,
        &options.filter,
        options.exposure_s,
        options.repeats,
        options.output_dir.as_deref(),
    )
    .unwrap_or_else(|error| {
        eprintln!("runtime validation failed before UI launch: {error}");
        std::process::exit(1);
    });

    let runtime = RuntimeSummary {
        app_name: app.name,
        config_name: app.config_name,
        ready: app.ready,
        phase: format!("{:?}", app.phase),
    };

    let scene = create_scene(&config_text, options.star_shell_radius).unwrap_or_else(|error| {
        eprintln!("scene creation failed: {error}");
        std::process::exit(1);
    });

    println!("launching UI/3D viewer with egui + wgpu");
    println!("campaign={} targets={}", scene.campaign_name, scene.targets.len());
    println!("runtime_ready={} runtime_phase={}", runtime.ready, runtime.phase);
    println!("window={}x{}", options.width, options.height);
    print_controls();

    let event_loop = EventLoop::new().unwrap_or_else(|error| {
        eprintln!("failed to create event loop: {error}");
        std::process::exit(1);
    });

    let window = WindowBuilder::new()
        .with_title(format!("{} | starting", runtime.app_name))
        .with_inner_size(PhysicalSize::new(options.width, options.height))
        .with_min_inner_size(PhysicalSize::new(960, 540))
        .build(&event_loop)
        .unwrap_or_else(|error| {
            eprintln!("failed to create window: {error}");
            std::process::exit(1);
        });

    let window = Arc::new(window);

    let mut state =
        pollster::block_on(RenderState::new(window.clone(), scene, runtime, &options))
            .unwrap_or_else(|error| {
                eprintln!("renderer initialization failed: {error}");
                std::process::exit(1);
            });

    state.window.set_title(&build_title(&state));

    let mut frame_time = Instant::now();
    let _ = event_loop.run(move |event, target| {
        target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { window_id, event } if window_id == state.window.id() => {
                match &event {
                    WindowEvent::CloseRequested => {
                        target.exit();
                        return;
                    }
                    WindowEvent::Resized(new_size) => {
                        state.resize(*new_size);
                    }
                    WindowEvent::RedrawRequested => {
                        let now = Instant::now();
                        let dt_s = (now - frame_time).as_secs_f32().clamp(0.0, 0.1);
                        frame_time = now;

                        state.update(dt_s);
                        let ui_frame = state.draw_egui();

                        match state.render(ui_frame) {
                            Ok(()) => {}
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                                state.resize(state.window.inner_size());
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => {
                                eprintln!("GPU memory exhausted, exiting viewer");
                                target.exit();
                            }
                            Err(wgpu::SurfaceError::Timeout) => {}
                        }
                    }
                    _ => {}
                }

                let egui_response = state
                    .egui_state
                    .on_window_event(state.window.as_ref(), &event);

                if !egui_response.consumed {
                    state.process_window_event(&event);
                }

                if egui_response.repaint {
                    state.window.request_redraw();
                }
            }
            Event::AboutToWait => {
                state.window.request_redraw();
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        atmospheric_palette, generate_background_stars, generate_sky_dome, parse_cli_args,
        priority_to_luminance, ra_dec_to_cartesian, ra_dec_to_cartesian_f64, sky_background_color,
        solar_halo_color, tone_map_color,
    };
    use observatory_core::CampaignTargetConfig;

    #[test]
    fn parse_cli_args_reads_graphics_options() {
        let args = vec![
            "sample.cfg".to_string(),
            "--width".to_string(),
            "1920".to_string(),
            "--height".to_string(),
            "1080".to_string(),
            "--fov".to_string(),
            "62".to_string(),
            "--planet-radius".to_string(),
            "1.5".to_string(),
            "--star-radius".to_string(),
            "60".to_string(),
            "--rotation-speed".to_string(),
            "4.5".to_string(),
            "--atmosphere".to_string(),
            "0.8".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.config_path, Some("sample.cfg".to_string()));
        assert_eq!(parsed.width, 1920);
        assert_eq!(parsed.height, 1080);
        assert!((parsed.fov_deg - 62.0).abs() < 1e-6);
        assert!((parsed.planet_radius - 1.5).abs() < 1e-6);
        assert!((parsed.star_shell_radius - 60.0).abs() < 1e-6);
        assert!((parsed.planet_rotation_deg_per_s - 4.5).abs() < 1e-6);
        assert!((parsed.atmosphere_strength - 0.8).abs() < 1e-6);
    }

    #[test]
    fn priority_to_luminance_is_monotonic() {
        let low = priority_to_luminance(1);
        let high = priority_to_luminance(9);
        assert!(high > low);
    }

    #[test]
    fn ra_dec_to_cartesian_maps_equatorial_axes() {
        let target = CampaignTargetConfig {
            name: "eq".to_string(),
            ra_deg: 0.0,
            dec_deg: 0.0,
            priority: 5,
        };
        let position = ra_dec_to_cartesian(&target, 10.0);
        assert!((position[0] - 10.0).abs() < 1e-6);
        assert!(position[1].abs() < 1e-6);
        assert!(position[2].abs() < 1e-6);
    }

    #[test]
    fn ra_dec_to_cartesian_f64_matches_equatorial_axis_precision() {
        let target = CampaignTargetConfig {
            name: "eq64".to_string(),
            ra_deg: 90.0,
            dec_deg: 30.0,
            priority: 6,
        };
        let position = ra_dec_to_cartesian_f64(&target, 10.0);
        assert!((position[0].abs() - 0.0).abs() < 1e-12);
        assert!((position[1] - 5.0).abs() < 1e-12);
        assert!((position[2] - 8.660254037844386).abs() < 1e-12);
    }

    #[test]
    fn generate_background_stars_has_expected_density_and_radius() {
        let field = generate_background_stars(256, 120.0);
        assert_eq!(field.len(), 256);
        for star in field {
            let radius_sq = star.position[0] * star.position[0]
                + star.position[1] * star.position[1]
                + star.position[2] * star.position[2];
            assert!((radius_sq - 120.0 * 120.0).abs() < 1.0);
            assert!((star.luminance >= 0.15) && (star.luminance <= 1.0));
        }
    }

    #[test]
    fn atmospheric_palette_remains_in_range_and_alpha_is_opaque() {
        let (planet, atmosphere) = atmospheric_palette(0.75, 1.2);
        assert!(planet[0].is_finite() && planet[1].is_finite() && planet[2].is_finite());
        assert!(atmosphere[0].is_finite() && atmosphere[1].is_finite() && atmosphere[2].is_finite());
        assert!((planet[3] - 1.0).abs() < 1e-6);
        assert!((atmosphere[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sky_background_color_makes_zenith_brighter_than_horizon() {
        let horizon = sky_background_color(-0.9, 8.0);
        let zenith = sky_background_color(0.9, 8.0);
        assert!(zenith[0] > horizon[0]);
        assert!(zenith[1] > horizon[1]);
        assert!(zenith[2] > horizon[2]);
    }

    #[test]
    fn generate_sky_dome_creates_expected_geometry() {
        let (vertices, indices) = generate_sky_dome(8, 12, 120.0);
        assert_eq!(vertices.len(), (8 + 1) * (12 + 1));
        assert_eq!(indices.len(), 8 * 12 * 6);
        assert!(vertices.iter().all(|vertex| vertex.position.iter().all(|value| value.is_finite())));
    }

    #[test]
    fn tone_map_color_keeps_values_in_unit_range() {
        let tone = tone_map_color([3.0, 0.5, 2.0]);
        assert!(tone.iter().all(|value| (*value >= 0.0) && (*value <= 1.0)));
    }

    #[test]
    fn solar_halo_color_is_more_intense_in_daylight() {
        let night = solar_halo_color(-0.8, 0.1);
        let day = solar_halo_color(0.8, 0.9);
        assert!(day[0] > night[0]);
        assert!(day[1] > night[1]);
        assert!(day[2] > night[2]);
    }
}