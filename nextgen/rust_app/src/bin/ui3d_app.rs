use std::f32::consts::{FRAC_PI_2, PI};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytemuck::{Pod, Zeroable};
use egui::{Color32, RichText};
use glam::{DMat4, DVec3, Mat4, Vec3};
use observatory_core::{
    analyze_frame, apply_pointing_correction, build_application, build_sky_chart,
    compute_local_sidereal_time_rad, config_to_targets, deg_to_rad, ecliptic_vector_to_equatorial,
    initialize_mount, parse_campaign_config, rad_to_deg, read_fits_file, sample_diurnal_track,
    scan_frame_directory, schedule_observation_queue, stack_frame_files, start_capture,
    AtmosphericConditions, CalibrationFrame, CampaignTarget, CampaignTargetConfig, CaptureResult,
    CaptureSession, DetectionParams, FrameAnalysis, FrameFileEntry, GeographicCoord, MountState,
    PointingModelTerms, QcThresholds, SchedulePlan, SiteLimits, SkyChart, SkyObjectClass,
    SkyObjectRequest, StackedResult, StackingMethod, StackingParams,
};
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowBuilder};

const PLANET_TEXTURE_LAYER_COUNT: u32 = 3;
const J2000_JULIAN_DAY: f64 = 2451545.0;
const UNIX_EPOCH_JULIAN_DAY: f64 = 2440587.5;
const KM_PER_AU: f64 = 149_597_870.7;
const AU_TO_SCENE_UNITS: f64 = 1.0;

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
    body_time_s: f32,
    interior_motion: f32,
    atmosphere_motion: f32,
    surface_texture_weight: f32,
    _padding: f32,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

@group(0) @binding(1)
var mars_layer_texture: texture_2d_array<f32>;

@group(0) @binding(2)
var mars_layer_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world = frame.model * vec4<f32>(input.position, 1.0);
    out.world_position = world.xyz;
    out.world_normal = normalize((frame.model * vec4<f32>(input.normal, 0.0)).xyz);
    out.uv = input.uv;
    out.clip_position = frame.view_proj * world;
    return out;
}

fn hash31(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (vec3<f32>(3.0, 3.0, 3.0) - 2.0 * f);

    let x00 = mix(hash31(i + vec3<f32>(0.0, 0.0, 0.0)), hash31(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash31(i + vec3<f32>(0.0, 1.0, 0.0)), hash31(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash31(i + vec3<f32>(0.0, 0.0, 1.0)), hash31(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash31(i + vec3<f32>(0.0, 1.0, 1.0)), hash31(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    let y0 = mix(x00, x10, u.y);
    let y1 = mix(x01, x11, u.y);
    return mix(y0, y1, u.z);
}

fn fbm(p: vec3<f32>) -> f32 {
    var q = p;
    var amplitude = 0.50;
    var sum = 0.0;
    var normalization = 0.0;

    for (var octave = 0; octave < 5; octave = octave + 1) {
        sum = sum + value_noise(q) * amplitude;
        normalization = normalization + amplitude;
        q = q * 2.03 + vec3<f32>(11.7, 4.8, 8.3);
        amplitude = amplitude * 0.52;
    }

    return sum / normalization;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(input.world_normal);
    let l = normalize(frame.light_dir.xyz);
    let v = normalize(frame.camera_pos.xyz - input.world_position);
    let h = normalize(l + v);

    let light_alignment = dot(n, l);
    let ndotl = max(light_alignment, 0.0);
    let ndotv = max(dot(n, v), 0.0);
    let daylight = smoothstep(-0.08, 0.42, light_alignment);
    let terminator = smoothstep(-0.22, 0.08, light_alignment) * (1.0 - daylight);

    let envelope_phase = frame.body_time_s * frame.interior_motion;
    let atmosphere_phase = frame.body_time_s * frame.atmosphere_motion;
    let continental = fbm(n * 2.15 + vec3<f32>(1.8, 4.1, 2.6));
    let uplands = fbm(n * 7.20 + vec3<f32>(9.2, 1.7, 5.4));
    let interior_flow = fbm(n * 4.20 + vec3<f32>(0.06 * envelope_phase, 0.025 * envelope_phase, -0.04 * envelope_phase));
    let weather = fbm(n * vec3<f32>(9.0, 3.4, 9.0) + vec3<f32>(2.7 + 0.055 * atmosphere_phase, 6.1, 0.8 - 0.035 * atmosphere_phase));
    let latitude = abs(n.y);
    let land_mask = smoothstep(0.55, 0.66, continental + 0.11 * uplands - 0.06 * latitude);
    let coast_mask = smoothstep(0.46, 0.58, continental) * (1.0 - land_mask);
    let cloud_mask = smoothstep(0.62, 0.82, weather + 0.10 * (1.0 - latitude)) * smoothstep(-0.70, 0.16, light_alignment);

    let spec_angle = max(dot(n, h), 0.0);
    let dust_sheen = 0.18 + 0.24 * (1.0 - land_mask);
    let specular = pow(spec_angle, frame.specular_power) * frame.specular_strength * dust_sheen * daylight;
    let fresnel = pow(1.0 - ndotv, 3.6) * (0.40 + 0.60 * frame.atmosphere_strength);
    let scattering = pow(ndotl, 4.0) * (0.10 + 0.90 * frame.atmosphere_strength);

    let interior_texel = textureSample(mars_layer_texture, mars_layer_sampler, input.uv, 0).rgb;
    let mars_texel = textureSample(mars_layer_texture, mars_layer_sampler, input.uv, 1).rgb;
    let atmosphere_uv = fract(input.uv + vec2<f32>(0.012 * atmosphere_phase, 0.004 * sin(0.27 * atmosphere_phase)));
    let atmosphere_texel = textureSample(mars_layer_texture, mars_layer_sampler, atmosphere_uv, 2);
    let observed_albedo = mars_texel * vec3<f32>(1.08, 0.88, 0.72);
    let basalt_plain = frame.planet_color.rgb * vec3<f32>(0.48, 0.40, 0.34) + vec3<f32>(0.018, 0.012, 0.008);
    let dust_plain = frame.planet_color.rgb * vec3<f32>(0.92, 0.66, 0.43) + vec3<f32>(0.040, 0.022, 0.012);
    let lowland = frame.planet_color.rgb * vec3<f32>(0.74, 0.50, 0.34) + vec3<f32>(0.030, 0.016, 0.010);
    let highland = frame.planet_color.rgb * vec3<f32>(1.08, 0.76, 0.52) + vec3<f32>(0.050, 0.030, 0.018);
    let land = mix(lowland, highland, smoothstep(0.48, 0.78, uplands));
    let polar = smoothstep(0.70, 0.93, latitude);
    var surface = mix(basalt_plain, dust_plain, coast_mask * 0.78);
    surface = mix(surface, land, land_mask);
    surface = mix(surface, vec3<f32>(0.78, 0.72, 0.64), polar * 0.42);
    surface = mix(surface, vec3<f32>(0.70, 0.56, 0.44), cloud_mask * 0.10 * daylight);
    surface = mix(surface, observed_albedo, frame.surface_texture_weight);

    let direct_light = 0.018 + 0.982 * daylight * (0.24 + 0.76 * ndotl);
    let nightside = interior_texel * (0.016 + 0.020 * interior_flow) * (1.0 - daylight);
    let atmosphere_density = 0.68 + 0.44 * atmosphere_texel.a;
    let dust_scatter = atmosphere_texel.rgb * 0.030 * daylight * frame.atmosphere_strength;
    let atmosphere = frame.atmosphere_color.rgb * (0.48 * fresnel + 0.34 * scattering + 0.22 * terminator) * atmosphere_density + dust_scatter;

    let color = surface * direct_light + nightside + atmosphere + vec3<f32>(specular, specular, specular);
    return vec4<f32>(color, 1.0);
}
"#;

const ATMOSPHERE_SHADER_WGSL: &str = r#"
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
    body_time_s: f32,
    interior_motion: f32,
    atmosphere_motion: f32,
    surface_texture_weight: f32,
    _padding: f32,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

@group(0) @binding(1)
var mars_layer_texture: texture_2d_array<f32>;

@group(0) @binding(2)
var mars_layer_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

fn hash31(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(71.3, 183.1, 421.7))) * 9182.731);
}

fn soft_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (vec3<f32>(3.0, 3.0, 3.0) - 2.0 * f);
    let x00 = mix(hash31(i + vec3<f32>(0.0, 0.0, 0.0)), hash31(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash31(i + vec3<f32>(0.0, 1.0, 0.0)), hash31(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash31(i + vec3<f32>(0.0, 0.0, 1.0)), hash31(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash31(i + vec3<f32>(0.0, 1.0, 1.0)), hash31(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let shell_position = input.position * 1.035;
    let world = frame.model * vec4<f32>(shell_position, 1.0);
    out.world_position = world.xyz;
    out.world_normal = normalize((frame.model * vec4<f32>(input.normal, 0.0)).xyz);
    out.uv = input.uv;
    out.clip_position = frame.view_proj * world;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(input.world_normal);
    let l = normalize(frame.light_dir.xyz);
    let v = normalize(frame.camera_pos.xyz - input.world_position);
    let ndotl = max(dot(n, l), 0.0);
    let ndotv = max(dot(n, v), 0.0);
    let phase = frame.body_time_s * frame.atmosphere_motion;
    let flow_uv = fract(input.uv + vec2<f32>(0.018 * phase, 0.006 * sin(0.31 * phase)));
    let dust = textureSample(mars_layer_texture, mars_layer_sampler, flow_uv, 2);
    let turbulence = soft_noise(n * 12.0 + vec3<f32>(0.09 * phase, 0.02 * phase, -0.06 * phase));
    let limb = pow(1.0 - ndotv, 2.2);
    let forward_scatter = pow(ndotl, 3.0);
    let density = frame.atmosphere_strength * (0.16 + 0.84 * dust.a) * (0.65 + 0.35 * turbulence);
    let alpha = clamp((0.030 + 0.22 * limb + 0.050 * forward_scatter) * density, 0.0, 0.32);
    let color = frame.atmosphere_color.rgb * (0.45 + 0.55 * forward_scatter) + dust.rgb * 0.045;
    return vec4<f32>(color, alpha);
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
    uv: [f32; 2],
}

impl MeshVertex {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
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
    body_time_s: f32,
    interior_motion: f32,
    atmosphere_motion: f32,
    surface_texture_weight: f32,
    _padding: f32,
}

#[derive(Clone, Copy, Debug)]
struct ViewUniform {
    bytes: [u8; 96],
}

impl ViewUniform {
    fn new(view_proj: Mat4, time_s: f32) -> Self {
        let mut bytes = [0u8; 96];
        for (index, value) in view_proj.to_cols_array().iter().enumerate() {
            let offset = index * std::mem::size_of::<f32>();
            bytes[offset..offset + std::mem::size_of::<f32>()]
                .copy_from_slice(&value.to_ne_bytes());
        }
        bytes[64..68].copy_from_slice(&time_s.to_ne_bytes());
        Self { bytes }
    }
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
            near_plane: 0.000001,
            far_plane: 400.0,
            planet_radius: 1.0,
            star_shell_radius: 45.0,
            planet_rotation_deg_per_s: 7.5,
            atmosphere_strength: 0.18,
            show_guides: false,
        }
    }
}

#[derive(Clone, Debug)]
struct SceneTarget {
    position: [f32; 3],
    luminance: f32,
}

#[derive(Clone, Debug)]
struct SceneDefinition {
    campaign_name: String,
    targets: Vec<SceneTarget>,
    campaign_targets: Vec<CampaignTarget>,
    calibration: CalibrationFrame,
    detection_threshold: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Workspace {
    Home,
    SkyOperations,
    Science,
    Simulator,
}

impl Workspace {
    const ALL: [Workspace; 4] = [
        Workspace::Home,
        Workspace::SkyOperations,
        Workspace::Science,
        Workspace::Simulator,
    ];

    fn title(self) -> &'static str {
        match self {
            Workspace::Home => "Accueil",
            Workspace::SkyOperations => "Cartographie & pilotage",
            Workspace::Science => "Exploitation scientifique",
            Workspace::Simulator => "Simulateur spatial",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Workspace::Home => "\u{25C6}",
            Workspace::SkyOperations => "\u{25CE}",
            Workspace::Science => "\u{25A6}",
            Workspace::Simulator => "\u{25C9}",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Workspace::Home => "Presentation du logiciel, etat du systeme et acces aux trois volets",
            Workspace::SkyOperations => {
                "Carte du ciel locale temps reel, pilotage de la monture et acquisition"
            }
            Workspace::Science => {
                "Reduction, photometrie, PSF, empilement et controle qualite des cliches"
            }
            Workspace::Simulator => {
                "Moteur 3D temps reel du systeme solaire couple aux cibles observees"
            }
        }
    }
}

const FILTER_NAMES: [&str; 8] = ["L", "R", "G", "B", "Ha", "OIII", "SII", "V"];
const SKY_CHART_REFRESH_S: f64 = 1.0;
const MAX_OPERATION_LOG_LINES: usize = 200;

struct OperationsState {
    site: GeographicCoord,
    limits: SiteLimits,
    conditions: AtmosphericConditions,
    pointing_model: PointingModelTerms,
    mount: MountState,
    time_offset_hours: f64,
    follow_clock: bool,
    show_solar_system: bool,
    show_campaign_targets: bool,
    show_track: bool,
    selected_object: Option<String>,
    chart: Option<SkyChart>,
    chart_refresh_timer_s: f64,
    schedule: Option<SchedulePlan>,
    exposure_s: f64,
    frame_count: usize,
    filter_index: usize,
    incoming_dir: String,
    last_capture: Option<CaptureResult>,
    log: Vec<String>,
}

impl OperationsState {
    fn new(exposure_s: f64, repeats: usize, filter: &str, incoming_dir: String) -> Self {
        let filter_index = FILTER_NAMES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(filter))
            .unwrap_or(0);

        Self {
            site: GeographicCoord {
                latitude_deg: 43.9346,
                longitude_deg: 5.7133,
                elevation_m: 650.0,
            },
            limits: SiteLimits::default(),
            conditions: AtmosphericConditions::default(),
            pointing_model: PointingModelTerms::default(),
            mount: initialize_mount(),
            time_offset_hours: 0.0,
            follow_clock: true,
            show_solar_system: true,
            show_campaign_targets: true,
            show_track: true,
            selected_object: None,
            chart: None,
            chart_refresh_timer_s: SKY_CHART_REFRESH_S,
            schedule: None,
            exposure_s,
            frame_count: repeats.max(1),
            filter_index,
            incoming_dir,
            last_capture: None,
            log: Vec::new(),
        }
    }

    fn filter_name(&self) -> &'static str {
        FILTER_NAMES[self.filter_index.min(FILTER_NAMES.len() - 1)]
    }

    fn push_log(&mut self, message: String) {
        self.log.push(message);
        if self.log.len() > MAX_OPERATION_LOG_LINES {
            let excess = self.log.len() - MAX_OPERATION_LOG_LINES;
            self.log.drain(0..excess);
        }
    }

    fn chart_julian_day(&self) -> f64 {
        current_julian_day() + self.time_offset_hours / 24.0
    }
}

struct LoadedFrame {
    path: String,
    file_name: String,
    width: usize,
    height: usize,
    object: Option<String>,
    filter: Option<String>,
    exposure_s: Option<f64>,
    julian_day: Option<f64>,
    raw_pixels: Vec<f64>,
}

struct ScienceState {
    frames_dir: String,
    entries: Vec<FrameFileEntry>,
    selected_entry: Option<usize>,
    stack_selection: Vec<String>,
    loaded: Option<LoadedFrame>,
    calibration: CalibrationFrame,
    detection: DetectionParams,
    thresholds: QcThresholds,
    stacking: StackingParams,
    analysis: Option<FrameAnalysis>,
    stack_result: Option<StackedResult>,
    texture: Option<egui::TextureHandle>,
    texture_source: Option<String>,
    black_percentile: f32,
    white_percentile: f32,
    show_detections: bool,
    plate_scale_arcsec_per_px: f64,
    status: String,
    auto_refresh: bool,
    refresh_timer_s: f64,
}

impl ScienceState {
    fn new(frames_dir: String, calibration: CalibrationFrame, detection_threshold: f64) -> Self {
        let mut detection = DetectionParams::default();
        if detection_threshold > 0.0 {
            detection.detection_sigma = (detection_threshold / 5.0).clamp(1.5, 20.0);
        }

        Self {
            frames_dir,
            entries: Vec::new(),
            selected_entry: None,
            stack_selection: Vec::new(),
            loaded: None,
            calibration,
            detection,
            thresholds: QcThresholds::default(),
            stacking: StackingParams::default(),
            analysis: None,
            stack_result: None,
            texture: None,
            texture_source: None,
            black_percentile: 1.0,
            white_percentile: 99.5,
            show_detections: true,
            plate_scale_arcsec_per_px: 1.0,
            status: "Aucun cliche charge".to_string(),
            auto_refresh: false,
            refresh_timer_s: 0.0,
        }
    }

    fn invalidate_texture(&mut self) {
        self.texture = None;
        self.texture_source = None;
    }
}

#[derive(Clone, Copy, Debug)]
struct OrbitalElements {
    semi_major_axis_au: f64,
    eccentricity: f64,
    inclination_deg: f64,
    longitude_ascending_node_deg: f64,
    longitude_perihelion_deg: f64,
    mean_longitude_deg: f64,
    orbital_period_days: f64,
}

#[derive(Clone, Debug)]
enum BodyOrbit {
    FixedSun,
    Heliocentric(OrbitalElements),
    Planetocentric {
        parent_index: usize,
        semi_major_axis_au: f64,
        orbital_period_days: f64,
        inclination_deg: f64,
        mean_longitude_deg: f64,
    },
    BeltObject {
        semi_major_axis_au: f64,
        eccentricity: f64,
        inclination_deg: f64,
        longitude_ascending_node_deg: f64,
        longitude_perihelion_deg: f64,
        mean_longitude_deg: f64,
        orbital_period_days: f64,
    },
}

#[derive(Clone, Debug)]
struct CelestialBody {
    name: &'static str,
    texture_asset: &'static str,
    orbit: BodyOrbit,
    radius_scene: f32,
    rotation_multiplier: f32,
    surface_color: [f32; 4],
    atmosphere_color: [f32; 4],
    atmosphere_strength: f32,
    specular_strength: f32,
    surface_texture_weight: f32,
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
    target: DVec3,
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
        self.target + DVec3::new(x, y, z)
    }

    fn view_proj_f64(&self, aspect: f64) -> DMat4 {
        let eye = self.eye();
        let center = self.target;
        let up = DVec3::Y;
        let view = DMat4::look_at_rh(eye, center, up);
        let proj = DMat4::perspective_rh(self.fov_deg.to_radians() as f64, aspect, self.near_plane as f64, self.far_plane as f64);
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
        self.target = DVec3::ZERO;
        self.yaw = 0.25 * PI as f64;
        self.pitch = 0.18 * PI as f64;
        self.distance = 34.0;
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

#[derive(Debug)]
struct TextureLodImage {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

struct PlanetLayerTextures {
    layer_view: wgpu::TextureView,
    layer_sampler: wgpu::Sampler,
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
    atmosphere_pipeline: wgpu::RenderPipeline,
    star_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    guide_pipeline: wgpu::RenderPipeline,

    frame_uniform: FrameUniform,
    _frame_buffer: wgpu::Buffer,
    _frame_bind_group: wgpu::BindGroup,
    body_frame_buffers: Vec<wgpu::Buffer>,
    planet_bind_groups: Vec<wgpu::BindGroup>,
    _planet_layers: Vec<PlanetLayerTextures>,

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

    target_count: usize,
    show_guides: bool,
    paused: bool,
    atmosphere_strength: f32,
    planet_rotation_deg_per_s: f32,

    planet_rotation_rad: f32,
    light_orbit_rad: f32,
    elapsed_time_s: f32,
    solar_system_bodies: Vec<CelestialBody>,
    solar_system_positions: Vec<Vec3>,
    selected_body_index: Option<usize>,

    runtime: RuntimeSummary,
    campaign_name: String,
    campaign_targets: Vec<CampaignTarget>,
    workspace: Workspace,
    nav_expanded: bool,
    operations: OperationsState,
    science: ScienceState,
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
            target: DVec3::ZERO,
            yaw: 0.25 * PI as f64,
            pitch: 0.18 * PI as f64,
            distance: 34.0,
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
                (planet_color[0] * 0.94 + sky_tint[0] * 0.03 + solar_glow[0] * 0.03).clamp(0.0, 1.0),
                (planet_color[1] * 0.94 + sky_tint[1] * 0.03 + solar_glow[1] * 0.03).clamp(0.0, 1.0),
                (planet_color[2] * 0.94 + sky_tint[2] * 0.03 + solar_glow[2] * 0.03).clamp(0.0, 1.0),
                1.0,
            ],
            atmosphere_color: [
                (atmosphere_color[0] * 0.88 + sky_tint[0] * 0.04 + solar_glow[0] * 0.08).clamp(0.0, 1.0),
                (atmosphere_color[1] * 0.88 + sky_tint[1] * 0.04 + solar_glow[1] * 0.08).clamp(0.0, 1.0),
                (atmosphere_color[2] * 0.88 + sky_tint[2] * 0.04 + solar_glow[2] * 0.08).clamp(0.0, 1.0),
                1.0,
            ],
            atmosphere_strength: options.atmosphere_strength,
            specular_power: 48.0,
            specular_strength: 0.045,
            body_time_s: 0.0,
            interior_motion: 0.35,
            atmosphere_motion: 1.0,
            surface_texture_weight: 0.92,
            _padding: 0.0,
        };

        let frame_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frame_uniform_buffer"),
            contents: bytemuck::bytes_of(&frame_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let planet_layers = load_planet_layer_textures(&device, &queue, "mars")?;

        let view_uniform = ViewUniform::new(view_proj, 0.0);

        let view_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("view_uniform_buffer"),
            contents: &view_uniform.bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let frame_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame_bind_group"),
            layout: &frame_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&planet_layers.layer_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&planet_layers.layer_sampler),
                },
            ],
        });

        let solar_system_bodies = build_solar_system_bodies();
        let mut body_frame_buffers = Vec::with_capacity(solar_system_bodies.len());
        let mut planet_bind_groups = Vec::with_capacity(solar_system_bodies.len());
        let mut planet_layer_resources = Vec::with_capacity(solar_system_bodies.len() + 1);
        planet_layer_resources.push(planet_layers);

        let mut asset_layer_indices = HashMap::<&'static str, usize>::new();
        asset_layer_indices.insert("mars", 0);

        for body in &solar_system_bodies {
            let layer_index = if let Some(index) = asset_layer_indices.get(body.texture_asset) {
                *index
            } else {
                let layer_index = planet_layer_resources.len();
                planet_layer_resources.push(load_planet_layer_textures(&device, &queue, body.texture_asset)?);
                asset_layer_indices.insert(body.texture_asset, layer_index);
                layer_index
            };
            let body_frame_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("solar_system_body_frame_uniform_buffer"),
                contents: bytemuck::bytes_of(&frame_uniform),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let layers = &planet_layer_resources[layer_index];
            planet_bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("solar_system_body_bind_group"),
                layout: &frame_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: body_frame_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&layers.layer_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&layers.layer_sampler),
                    },
                ],
            }));
            body_frame_buffers.push(body_frame_buffer);
        }
        let solar_system_positions = vec![Vec3::ZERO; solar_system_bodies.len()];

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

        let atmosphere_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("atmosphere_shader"),
            source: wgpu::ShaderSource::Wgsl(ATMOSPHERE_SHADER_WGSL.into()),
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

        let atmosphere_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("atmosphere_pipeline"),
            layout: Some(&planet_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &atmosphere_shader,
                entry_point: "vs_main",
                buffers: &[MeshVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &atmosphere_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

        let background_stars = generate_background_stars(2400, 120.0);
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
            None,
            1,
        );

        let frames_dir = options
            .output_dir
            .clone()
            .unwrap_or_else(|| ".".to_string());

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            depth,
            planet_pipeline,
            atmosphere_pipeline,
            star_pipeline,
            sky_pipeline,
            guide_pipeline,
            frame_uniform,
            _frame_buffer: frame_buffer,
            _frame_bind_group: frame_bind_group,
            body_frame_buffers,
            planet_bind_groups,
            _planet_layers: planet_layer_resources,
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
            target_count: scene.targets.len(),
            show_guides: options.show_guides,
            paused: false,
            atmosphere_strength: options.atmosphere_strength,
            planet_rotation_deg_per_s: options.planet_rotation_deg_per_s,
            planet_rotation_rad: 0.0,
            light_orbit_rad: 0.0,
            elapsed_time_s: 0.0,
            solar_system_bodies,
            solar_system_positions,
            selected_body_index: None,
            runtime,
            campaign_name: scene.campaign_name,
            campaign_targets: scene.campaign_targets,
            workspace: Workspace::Home,
            nav_expanded: true,
            operations: OperationsState::new(
                options.exposure_s,
                options.repeats,
                &options.filter,
                frames_dir.clone(),
            ),
            science: ScienceState::new(frames_dir, scene.calibration, scene.detection_threshold),
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
        self.camera.distance = self.camera.distance.clamp(0.000001, 120.0);

        if !self.paused {
            self.planet_rotation_rad += self.planet_rotation_deg_per_s.to_radians() * dt_s as f32;
            self.light_orbit_rad += 0.16 * dt_s as f32;
            self.elapsed_time_s += dt_s as f32;
        }

        let aspect = self.config.width as f32 / self.config.height as f32;
        let view_proj = self.camera.view_proj(aspect);
        self.view_uniform = ViewUniform::new(view_proj, self.elapsed_time_s);
        self.queue
            .write_buffer(&self.view_buffer, 0, &self.view_uniform.bytes);

        self.frame_uniform.view_proj = view_proj.to_cols_array_2d();
        let eye = self.camera.eye();
        self.frame_uniform.camera_pos = [
            eye.x as f32,
            eye.y as f32,
            eye.z as f32,
            0.0,
        ];
        self.frame_uniform.body_time_s = self.elapsed_time_s;

        let julian_day = current_julian_day();
        self.solar_system_positions = compute_solar_system_positions(&self.solar_system_bodies, julian_day);
        if let Some(index) = self.selected_body_index {
            if let Some(position) = self.solar_system_positions.get(index) {
                self.camera.target = DVec3::new(position.x as f64, position.y as f64, position.z as f64);
            }
        }

        if self.title_last_update.elapsed() >= Duration::from_millis(220) {
            self.window.set_title(&build_title(self));
            self.title_last_update = Instant::now();
        }

        self.operations.chart_refresh_timer_s -= dt_s as f64;
        if self.operations.chart.is_none() || self.operations.chart_refresh_timer_s <= 0.0 {
            self.operations.chart_refresh_timer_s = SKY_CHART_REFRESH_S;
            if self.operations.follow_clock || self.operations.chart.is_none() {
                self.rebuild_sky_chart();
            }
        }

        if self.science.auto_refresh {
            self.science.refresh_timer_s -= dt_s as f64;
            if self.science.refresh_timer_s <= 0.0 {
                self.science.refresh_timer_s = 2.0;
                self.refresh_frame_directory();
            }
        }
    }

    fn sky_chart_requests(&self) -> Vec<SkyObjectRequest> {
        let mut requests = Vec::new();

        if self.operations.show_campaign_targets {
            for target in &self.campaign_targets {
                requests.push(SkyObjectRequest {
                    name: target.name.to_string(),
                    ra_deg: target.ra_deg,
                    dec_deg: target.dec_deg,
                    class: SkyObjectClass::CampaignTarget,
                    priority: target.priority,
                });
            }
        }

        if self.operations.show_solar_system {
            let julian_day = self.operations.chart_julian_day();
            let positions = compute_solar_system_positions(&self.solar_system_bodies, julian_day);
            let earth_index = self
                .solar_system_bodies
                .iter()
                .position(|body| body.name == "Earth");

            if let Some(earth_index) = earth_index {
                let earth = positions[earth_index];
                for (index, body) in self.solar_system_bodies.iter().enumerate() {
                    if index == earth_index {
                        continue;
                    }
                    let geocentric = positions[index] - earth;
                    // Scene axes are ecliptic with Y normal to the ecliptic plane.
                    let (ra_deg, dec_deg, range) = ecliptic_vector_to_equatorial(
                        geocentric.x as f64,
                        geocentric.z as f64,
                        geocentric.y as f64,
                    );
                    if range <= 0.0 {
                        continue;
                    }
                    requests.push(SkyObjectRequest {
                        name: body.name.to_string(),
                        ra_deg,
                        dec_deg,
                        class: SkyObjectClass::SolarSystemBody,
                        priority: 5,
                    });
                }
            }
        }

        requests.push(SkyObjectRequest {
            name: "Monture".to_string(),
            ra_deg: self.operations.mount.ra_deg,
            dec_deg: self.operations.mount.dec_deg,
            class: SkyObjectClass::MountPointing,
            priority: 0,
        });

        requests
    }

    fn rebuild_sky_chart(&mut self) {
        let requests = self.sky_chart_requests();
        let julian_day = self.operations.chart_julian_day();
        self.operations.chart = Some(build_sky_chart(
            &requests,
            &self.operations.site,
            &self.operations.limits,
            julian_day,
        ));
    }

    fn rebuild_schedule(&mut self) {
        let plan = schedule_observation_queue(
            &self.campaign_targets,
            &self.operations.site,
            &self.operations.limits,
            self.operations.chart_julian_day(),
            self.operations.exposure_s,
            self.operations.frame_count,
        );
        self.operations.push_log(format!(
            "planification: {} cibles observables sur {} ({:.0} s de pose cumulee)",
            plan.observable_targets, plan.total_targets, plan.total_estimated_duration_s
        ));
        self.operations.schedule = Some(plan);
    }

    fn slew_to(&mut self, name: &str) {
        let Some(chart) = self.operations.chart.as_ref() else {
            return;
        };
        let Some(object) = chart.find(name) else {
            return;
        };

        let ra_deg = object.ra_deg;
        let dec_deg = object.dec_deg;
        let altitude_deg = object.altitude_deg;
        let azimuth_deg = object.azimuth_deg;
        let airmass = object.airmass;
        let lst_rad = compute_local_sidereal_time_rad(
            self.operations.chart_julian_day(),
            self.operations.site.longitude_deg,
        );

        let (corrected_ha_rad, corrected_dec_rad) = apply_pointing_correction(
            deg_to_rad(ra_deg),
            deg_to_rad(dec_deg),
            lst_rad,
            deg_to_rad(self.operations.site.latitude_deg),
            &self.operations.pointing_model,
            &self.operations.conditions,
        );

        let corrected_ra_deg = rad_to_deg(lst_rad - corrected_ha_rad).rem_euclid(360.0);
        self.operations.mount.ra_deg = corrected_ra_deg;
        self.operations.mount.dec_deg = rad_to_deg(corrected_dec_rad);
        self.operations.mount.tracking = true;
        self.operations.selected_object = Some(name.to_string());

        self.operations.push_log(format!(
            "pointage {name}: RA {ra_deg:.4} deg DEC {dec_deg:.4} deg | alt {altitude_deg:.2} deg az {azimuth_deg:.2} deg | masse d'air {}",
            format_airmass(airmass)
        ));
        self.operations.push_log(format!(
            "consigne monture corrigee (modele + refraction): RA {:.4} deg DEC {:.4} deg",
            self.operations.mount.ra_deg, self.operations.mount.dec_deg
        ));

        self.rebuild_sky_chart();
    }

    fn start_selected_capture(&mut self) {
        let Some(selected) = self.operations.selected_object.clone() else {
            self.operations
                .push_log("acquisition refusee: aucune cible selectionnee".to_string());
            return;
        };

        let target_name: &'static str = self
            .campaign_targets
            .iter()
            .find(|target| target.name == selected)
            .map(|target| target.name)
            .unwrap_or("UNLISTED_TARGET");

        let session = CaptureSession {
            target: target_name,
            exposure_s: self.operations.exposure_s,
            filter: self.operations.filter_name(),
            count: self.operations.frame_count,
            enabled: self.operations.mount.tracking,
        };

        let result = start_capture(session);
        if result.sync_ok {
            self.operations.push_log(format!(
                "acquisition lancee: {} x {:.1} s filtre {} sur {}",
                result.frames_acquired, session.exposure_s, session.filter, selected
            ));
        } else {
            self.operations.push_log(format!(
                "acquisition rejetee par le coeur: pose {:.1} s, {} images, suivi {}",
                session.exposure_s,
                session.count,
                if session.enabled { "actif" } else { "arrete" }
            ));
        }
        self.operations.last_capture = Some(result);
    }

    fn refresh_frame_directory(&mut self) {
        match scan_frame_directory(&self.science.frames_dir) {
            Ok(entries) => {
                let previous = self.science.entries.len();
                if entries.len() != previous {
                    self.science.status =
                        format!("{} cliche(s) FITS detecte(s) dans le repertoire", entries.len());
                }
                self.science.entries = entries;
                if let Some(index) = self.science.selected_entry {
                    if index >= self.science.entries.len() {
                        self.science.selected_entry = None;
                    }
                }
            }
            Err(error) => {
                self.science.entries.clear();
                self.science.selected_entry = None;
                self.science.status = error;
            }
        }
    }

    fn load_frame(&mut self, path: &str) {
        match read_fits_file(path) {
            Ok(image) => {
                let file_name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(path)
                    .to_string();

                self.science.status = format!(
                    "{} charge: {}x{} pixels, BITPIX {:?}",
                    file_name, image.width, image.height, image.bitpix
                );
                self.science.loaded = Some(LoadedFrame {
                    path: path.to_string(),
                    file_name,
                    width: image.width,
                    height: image.height,
                    object: image.header.get_str("OBJECT"),
                    filter: image.header.get_str("FILTER"),
                    exposure_s: image.header.get_float("EXPTIME"),
                    julian_day: image.header.get_float("JD"),
                    raw_pixels: image.data,
                });
                self.science.analysis = None;
                self.science.invalidate_texture();
            }
            Err(error) => {
                self.science.status = error;
            }
        }
    }

    fn run_frame_analysis(&mut self) {
        let Some(frame) = self.science.loaded.as_ref() else {
            self.science.status = "aucun cliche charge a analyser".to_string();
            return;
        };

        match analyze_frame(
            &frame.raw_pixels,
            frame.width,
            frame.height,
            self.science.calibration,
            &self.science.detection,
            &self.science.thresholds,
        ) {
            Ok(analysis) => {
                self.science.status = format!(
                    "{}: {} source(s), FWHM median {:.2} px, qualite {:?}",
                    frame.file_name,
                    analysis.stars.len(),
                    analysis.quality.median_fwhm_pixels,
                    analysis.quality.quality_flag
                );
                self.science.analysis = Some(analysis);
                self.science.invalidate_texture();
            }
            Err(error) => {
                self.science.status = error;
            }
        }
    }

    fn run_stacking(&mut self) {
        if self.science.stack_selection.len() < 2 {
            self.science.status =
                "empilement impossible: selectionner au moins deux cliches".to_string();
            return;
        }

        match stack_frame_files(&self.science.stack_selection, &self.science.stacking) {
            Ok(result) => {
                self.science.status = format!(
                    "empilement de {} cliches: bruit {:.3}, gain SNR x{:.2}",
                    result.frame_count, result.noise_std_dev, result.snr_improvement_factor
                );
                self.science.stack_result = Some(result);
            }
            Err(error) => {
                self.science.status = error;
            }
        }
    }

    fn draw_egui(&mut self) -> UiFrameData {
        let raw_input = self.egui_state.take_egui_input(self.window.as_ref());
        let context = self.egui_ctx.clone();
        let full_output = context.run(raw_input, |ctx| {
            self.build_workspace_ui(ctx);
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

    fn build_workspace_ui(&mut self, ctx: &egui::Context) {
        self.ui_top_bar(ctx);
        self.ui_navigation(ctx);

        match self.workspace {
            Workspace::Home => self.ui_home(ctx),
            Workspace::SkyOperations => self.ui_sky_operations(ctx),
            Workspace::Science => self.ui_science(ctx),
            Workspace::Simulator => self.ui_simulator(ctx),
        }
    }

    fn ui_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let toggle = if self.nav_expanded { "\u{25C0}" } else { "\u{25B6}" };
                if ui
                    .button(toggle)
                    .on_hover_text("Rabattre ou deployer le menu lateral")
                    .clicked()
                {
                    self.nav_expanded = !self.nav_expanded;
                }
                ui.heading(RichText::new("Celestia Observatory Suite").strong());
                ui.separator();
                ui.label(self.workspace.title());
                ui.separator();
                ui.label(format!("campagne: {}", self.campaign_name));
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
    }

    fn ui_navigation(&mut self, ctx: &egui::Context) {
        let width = if self.nav_expanded { 244.0 } else { 54.0 };
        egui::SidePanel::left("primary_navigation")
            .resizable(false)
            .exact_width(width)
            .show(ctx, |ui| {
                ui.add_space(8.0);

                for workspace in Workspace::ALL {
                    let selected = self.workspace == workspace;
                    let label = if self.nav_expanded {
                        format!("{}  {}", workspace.icon(), workspace.title())
                    } else {
                        workspace.icon().to_string()
                    };

                    let response = ui.add_sized(
                        [ui.available_width(), 34.0],
                        egui::SelectableLabel::new(selected, RichText::new(label).size(15.0)),
                    );
                    if response.on_hover_text(workspace.summary()).clicked() {
                        self.workspace = workspace;
                    }
                    ui.add_space(4.0);
                }

                if !self.nav_expanded {
                    return;
                }

                ui.separator();
                ui.label(RichText::new("Etat systeme").strong());
                ui.label(format!("cibles: {}", self.target_count));
                ui.label(format!("corps 3D: {}", self.solar_system_bodies.len()));
                if let Some(chart) = self.operations.chart.as_ref() {
                    ui.label(format!(
                        "au-dessus horizon: {}",
                        chart.above_horizon_count
                    ));
                    ui.label(format!("observables: {}", chart.observable_count));
                    ui.label(format!("TSL: {}", format_hours_hms(chart.local_sidereal_time_hours())));
                }
                ui.label(format!("cliches indexes: {}", self.science.entries.len()));

                ui.separator();
                ui.label(RichText::new("Repertoire de travail").strong());
                ui.label(RichText::new(&self.science.frames_dir).monospace().size(11.0));
            });
    }

    fn ui_home(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_source("home_scroll")
                .show(ui, |ui| {
                    ui.add_space(10.0);
                    ui.heading(RichText::new("Celestia Observatory Suite").size(30.0).strong());
                    ui.label(
                        RichText::new(
                            "Chaine complete d'observation astronomique: preparation et pilotage \
                             d'observation, reduction scientifique des cliches et simulation \
                             spatiale temps reel, sur un unique coeur de calcul natif.",
                        )
                        .size(15.0),
                    );

                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(10.0);

                    ui.label(RichText::new("Les trois volets").size(19.0).strong());
                    ui.add_space(6.0);

                    for workspace in Workspace::ALL {
                        if workspace == Workspace::Home {
                            continue;
                        }

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(workspace.icon()).size(22.0));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(workspace.title()).size(16.0).strong());
                                    ui.label(workspace.summary());
                                    for capability in workspace_capabilities(workspace) {
                                        ui.label(format!("  \u{2022} {capability}"));
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            if ui
                                .button(format!("Ouvrir {}", workspace.title()))
                                .clicked()
                            {
                                self.workspace = workspace;
                            }
                        });
                        ui.add_space(8.0);
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(RichText::new("Session courante").size(17.0).strong());
                    ui.add_space(4.0);

                    egui::Grid::new("home_session_grid")
                        .num_columns(2)
                        .spacing([24.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Application");
                            ui.label(&self.runtime.app_name);
                            ui.end_row();

                            ui.label("Configuration");
                            ui.label(&self.runtime.config_name);
                            ui.end_row();

                            ui.label("Cibles de campagne");
                            ui.label(self.target_count.to_string());
                            ui.end_row();

                            ui.label("Corps celestes simules");
                            ui.label(self.solar_system_bodies.len().to_string());
                            ui.end_row();

                            ui.label("Site d'observation");
                            ui.label(format!(
                                "lat {:.4} deg, lon {:.4} deg, alt {:.0} m",
                                self.operations.site.latitude_deg,
                                self.operations.site.longitude_deg,
                                self.operations.site.elevation_m
                            ));
                            ui.end_row();

                            ui.label("Jour julien courant");
                            ui.label(format!("{:.5}", current_julian_day()));
                            ui.end_row();

                            ui.label("Repertoire des cliches");
                            ui.label(RichText::new(&self.science.frames_dir).monospace());
                            ui.end_row();
                        });

                    ui.add_space(12.0);
                });
        });
    }

    fn ui_sky_operations(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("sky_operations_controls")
            .resizable(true)
            .default_width(340.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("sky_ops_scroll")
                    .show(ui, |ui| {
                        let mut chart_dirty = false;

                        ui.label(RichText::new("Site d'observation").strong());
                        chart_dirty |= ui
                            .add(
                                egui::DragValue::new(&mut self.operations.site.latitude_deg)
                                    .speed(0.01)
                                    .clamp_range(-90.0..=90.0)
                                    .prefix("lat "),
                            )
                            .changed();
                        chart_dirty |= ui
                            .add(
                                egui::DragValue::new(&mut self.operations.site.longitude_deg)
                                    .speed(0.01)
                                    .clamp_range(-180.0..=180.0)
                                    .prefix("lon "),
                            )
                            .changed();
                        chart_dirty |= ui
                            .add(
                                egui::DragValue::new(&mut self.operations.site.elevation_m)
                                    .speed(1.0)
                                    .clamp_range(-420.0..=6000.0)
                                    .suffix(" m"),
                            )
                            .changed();

                        ui.separator();
                        ui.label(RichText::new("Contraintes d'observation").strong());
                        chart_dirty |= ui
                            .add(
                                egui::Slider::new(
                                    &mut self.operations.limits.min_altitude_deg,
                                    0.0..=60.0,
                                )
                                .text("altitude min (deg)"),
                            )
                            .changed();
                        chart_dirty |= ui
                            .add(
                                egui::Slider::new(&mut self.operations.limits.max_airmass, 1.0..=6.0)
                                    .text("masse d'air max"),
                            )
                            .changed();

                        ui.separator();
                        ui.label(RichText::new("Horloge").strong());
                        chart_dirty |= ui
                            .checkbox(&mut self.operations.follow_clock, "suivre l'heure reelle")
                            .changed();
                        chart_dirty |= ui
                            .add(
                                egui::Slider::new(
                                    &mut self.operations.time_offset_hours,
                                    -12.0..=12.0,
                                )
                                .text("decalage (h)"),
                            )
                            .changed();

                        ui.separator();
                        ui.label(RichText::new("Couches affichees").strong());
                        chart_dirty |= ui
                            .checkbox(
                                &mut self.operations.show_campaign_targets,
                                "cibles de campagne",
                            )
                            .changed();
                        chart_dirty |= ui
                            .checkbox(&mut self.operations.show_solar_system, "systeme solaire")
                            .changed();
                        ui.checkbox(&mut self.operations.show_track, "trace diurne de la selection");

                        ui.separator();
                        ui.label(RichText::new("Conditions atmospheriques").strong());
                        ui.add(
                            egui::DragValue::new(&mut self.operations.conditions.temperature_c)
                                .speed(0.1)
                                .clamp_range(-60.0..=55.0)
                                .suffix(" degC"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.operations.conditions.pressure_hpa)
                                .speed(0.5)
                                .clamp_range(500.0..=1100.0)
                                .suffix(" hPa"),
                        );
                        ui.add(
                            egui::DragValue::new(
                                &mut self.operations.conditions.relative_humidity_pct,
                            )
                            .speed(0.5)
                            .clamp_range(0.0..=100.0)
                            .suffix(" %HR"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.operations.conditions.wavelength_nm)
                                .speed(1.0)
                                .clamp_range(300.0..=1100.0)
                                .suffix(" nm"),
                        );

                        ui.separator();
                        ui.label(RichText::new("Modele de pointage (arcsec)").strong());
                        ui.add(
                            egui::DragValue::new(
                                &mut self.operations.pointing_model.polar_elevation_error_arcsec,
                            )
                            .speed(1.0)
                            .prefix("ME "),
                        );
                        ui.add(
                            egui::DragValue::new(
                                &mut self.operations.pointing_model.polar_azimuth_error_arcsec,
                            )
                            .speed(1.0)
                            .prefix("MA "),
                        );
                        ui.add(
                            egui::DragValue::new(
                                &mut self.operations.pointing_model.collimation_cone_error_arcsec,
                            )
                            .speed(1.0)
                            .prefix("CH "),
                        );
                        ui.add(
                            egui::DragValue::new(
                                &mut self.operations.pointing_model.tube_flexure_arcsec,
                            )
                            .speed(1.0)
                            .prefix("TF "),
                        );

                        ui.separator();
                        ui.label(RichText::new("Monture").strong());
                        ui.label(format!(
                            "RA {} / DEC {}",
                            format_right_ascension(self.operations.mount.ra_deg),
                            format_declination(self.operations.mount.dec_deg)
                        ));
                        ui.checkbox(&mut self.operations.mount.tracking, "suivi sideral actif");

                        let selected = self.operations.selected_object.clone();
                        ui.horizontal(|ui| {
                            let can_slew = selected.is_some();
                            if ui
                                .add_enabled(can_slew, egui::Button::new("Pointer la selection"))
                                .clicked()
                            {
                                if let Some(name) = selected.clone() {
                                    self.slew_to(&name);
                                }
                            }
                            if ui.button("Parking").clicked() {
                                self.operations.mount = initialize_mount();
                                self.operations.mount.tracking = false;
                                self.operations
                                    .push_log("monture repliee en position de parking".to_string());
                                chart_dirty = true;
                            }
                        });

                        ui.separator();
                        ui.label(RichText::new("Acquisition").strong());
                        ui.add(
                            egui::Slider::new(&mut self.operations.exposure_s, 0.1..=1800.0)
                                .logarithmic(true)
                                .text("pose (s)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.operations.frame_count, 1..=200)
                                .text("images"),
                        );
                        egui::ComboBox::from_label("filtre")
                            .selected_text(self.operations.filter_name())
                            .show_ui(ui, |ui| {
                                for (index, name) in FILTER_NAMES.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.operations.filter_index,
                                        index,
                                        *name,
                                    );
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label("reception:");
                            ui.text_edit_singleline(&mut self.operations.incoming_dir);
                        });

                        ui.horizontal(|ui| {
                            if ui.button("Lancer l'acquisition").clicked() {
                                self.start_selected_capture();
                            }
                            if ui.button("Planifier la nuit").clicked() {
                                self.rebuild_schedule();
                            }
                        });

                        if ui.button("Analyser les cliches recus").clicked() {
                            self.science.frames_dir = self.operations.incoming_dir.clone();
                            self.refresh_frame_directory();
                            self.workspace = Workspace::Science;
                        }

                        if let Some(capture) = self.operations.last_capture.as_ref() {
                            ui.label(format!(
                                "derniere commande: {} image(s), synchro {}",
                                capture.frames_acquired,
                                if capture.sync_ok { "ok" } else { "refusee" }
                            ));
                        }

                        if let Some(plan) = self.operations.schedule.as_ref() {
                            ui.separator();
                            ui.label(RichText::new("File d'observation").strong());
                            egui::ScrollArea::vertical()
                                .id_source("schedule_scroll")
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    for observation in &plan.queue {
                                        ui.label(format!(
                                            "{:02}. {} alt {:.1} deg X {:.2}",
                                            observation.rank,
                                            observation.target_name,
                                            observation.altitude_deg,
                                            observation.airmass
                                        ));
                                    }
                                });
                        }

                        ui.separator();
                        ui.label(RichText::new("Journal").strong());
                        egui::ScrollArea::vertical()
                            .id_source("operations_log_scroll")
                            .max_height(180.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for line in &self.operations.log {
                                    ui.label(RichText::new(line).size(11.5).monospace());
                                }
                            });

                        if chart_dirty {
                            self.rebuild_sky_chart();
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let clicked = self.draw_sky_chart(ui);
            if let Some(name) = clicked {
                self.operations.selected_object = Some(name);
            }
        });
    }

    fn draw_sky_chart(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let Some(chart) = self.operations.chart.clone() else {
            ui.label("carte du ciel en cours de calcul");
            return None;
        };

        let available = ui.available_size();
        let (response, painter) =
            ui.allocate_painter(available, egui::Sense::click_and_drag());
        let rect = response.rect;
        let center = rect.center();
        let radius = rect.width().min(rect.height()) * 0.46;
        if radius <= 1.0 {
            return None;
        }

        let to_screen = |x: f64, y: f64| {
            egui::pos2(
                center.x + (x as f32) * radius,
                center.y - (y as f32) * radius,
            )
        };

        painter.rect_filled(rect, 0.0, Color32::from_rgb(8, 10, 18));
        painter.circle_filled(center, radius, Color32::from_rgb(14, 18, 32));

        for altitude_deg in [0.0_f64, 30.0, 60.0] {
            let ring = (1.0 - altitude_deg / 90.0) as f32 * radius;
            let stroke_color = if altitude_deg == 0.0 {
                Color32::from_rgb(120, 150, 190)
            } else {
                Color32::from_rgb(45, 58, 80)
            };
            painter.circle_stroke(center, ring, egui::Stroke::new(1.0_f32, stroke_color));
        }

        let limit_ring =
            (1.0 - self.operations.limits.min_altitude_deg / 90.0).clamp(0.0, 1.0) as f32 * radius;
        painter.circle_stroke(
            center,
            limit_ring,
            egui::Stroke::new(1.4_f32, Color32::from_rgb(190, 130, 60)),
        );

        for azimuth_deg in (0..360).step_by(30) {
            let azimuth_rad = (azimuth_deg as f64).to_radians();
            let outer = to_screen(-azimuth_rad.sin(), azimuth_rad.cos());
            painter.line_segment(
                [center, outer],
                egui::Stroke::new(0.6_f32, Color32::from_rgb(34, 44, 62)),
            );
        }

        for (azimuth_deg, label) in [(0.0_f64, "N"), (90.0, "E"), (180.0, "S"), (270.0, "O")] {
            let azimuth_rad = azimuth_deg.to_radians();
            let position = to_screen(
                -azimuth_rad.sin() * 1.06,
                azimuth_rad.cos() * 1.06,
            );
            painter.text(
                position,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(15.0),
                Color32::from_rgb(180, 200, 230),
            );
        }

        if self.operations.show_track {
            if let Some(selected) = self.operations.selected_object.as_ref() {
                if let Some(object) = chart.find(selected) {
                    let track = sample_diurnal_track(
                        object.ra_deg,
                        object.dec_deg,
                        &self.operations.site,
                        chart.julian_day,
                        10.0,
                        121,
                    );
                    let mut segment: Vec<egui::Pos2> = Vec::new();
                    for (x, y, altitude_deg) in track {
                        if altitude_deg <= 0.0 {
                            if segment.len() > 1 {
                                painter.add(egui::Shape::line(
                                    std::mem::take(&mut segment),
                                    egui::Stroke::new(1.2_f32, Color32::from_rgb(90, 150, 220)),
                                ));
                            } else {
                                segment.clear();
                            }
                            continue;
                        }
                        segment.push(to_screen(x, y));
                    }
                    if segment.len() > 1 {
                        painter.add(egui::Shape::line(
                            segment,
                            egui::Stroke::new(1.2_f32, Color32::from_rgb(90, 150, 220)),
                        ));
                    }
                }
            }
        }

        let selected_name = self.operations.selected_object.clone();
        let mut closest_click: Option<(f32, String)> = None;
        let pointer = response.interact_pointer_pos();

        for object in &chart.objects {
            if !object.is_above_horizon {
                continue;
            }

            let position = to_screen(object.chart_x, object.chart_y);
            let is_selected = selected_name.as_deref() == Some(object.name.as_str());

            match object.class {
                SkyObjectClass::MountPointing => {
                    let arm = 9.0;
                    let stroke = egui::Stroke::new(1.6_f32, Color32::from_rgb(255, 210, 90));
                    painter.line_segment(
                        [
                            egui::pos2(position.x - arm, position.y),
                            egui::pos2(position.x + arm, position.y),
                        ],
                        stroke,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(position.x, position.y - arm),
                            egui::pos2(position.x, position.y + arm),
                        ],
                        stroke,
                    );
                    painter.circle_stroke(position, 5.0, stroke);
                }
                SkyObjectClass::SolarSystemBody => {
                    painter.circle_filled(position, 5.0, Color32::from_rgb(255, 176, 96));
                    painter.text(
                        egui::pos2(position.x + 8.0, position.y),
                        egui::Align2::LEFT_CENTER,
                        &object.name,
                        egui::FontId::proportional(11.5),
                        Color32::from_rgb(235, 200, 160),
                    );
                }
                SkyObjectClass::CampaignTarget => {
                    let size = 3.0 + object.priority as f32 * 0.55;
                    let color = if object.is_observable {
                        Color32::from_rgb(120, 240, 160)
                    } else {
                        Color32::from_rgb(200, 120, 110)
                    };
                    painter.circle_filled(position, size, color);
                    painter.text(
                        egui::pos2(position.x + size + 4.0, position.y),
                        egui::Align2::LEFT_CENTER,
                        &object.name,
                        egui::FontId::proportional(11.5),
                        Color32::from_rgb(210, 225, 240),
                    );
                }
            }

            if is_selected {
                painter.circle_stroke(
                    position,
                    11.0,
                    egui::Stroke::new(1.6_f32, Color32::from_rgb(255, 255, 255)),
                );
            }

            if let Some(pointer) = pointer {
                let distance = pointer.distance(position);
                if distance < 16.0
                    && closest_click
                        .as_ref()
                        .map(|(best, _)| distance < *best)
                        .unwrap_or(true)
                {
                    closest_click = Some((distance, object.name.clone()));
                }
            }
        }

        let info_origin = egui::pos2(rect.left() + 12.0, rect.top() + 12.0);
        let mut lines = vec![
            format!("JJ {:.5}", chart.julian_day),
            format!("TSL {}", format_hours_hms(chart.local_sidereal_time_hours())),
            format!(
                "site {:.4} deg / {:.4} deg",
                chart.site_latitude_deg, chart.site_longitude_deg
            ),
            format!(
                "{} au-dessus horizon, {} observables",
                chart.above_horizon_count, chart.observable_count
            ),
        ];

        if let Some(selected) = selected_name.as_ref() {
            if let Some(object) = chart.find(selected) {
                lines.push(String::new());
                lines.push(format!("selection: {}", object.name));
                lines.push(format!(
                    "RA {} DEC {}",
                    format_right_ascension(object.ra_deg),
                    format_declination(object.dec_deg)
                ));
                lines.push(format!(
                    "alt {:.2} deg  az {:.2} deg",
                    object.altitude_deg, object.azimuth_deg
                ));
                lines.push(format!(
                    "angle horaire {:.3} h  masse d'air {}",
                    object.hour_angle_deg / 15.0,
                    format_airmass(object.airmass)
                ));
                lines.push(format!(
                    "statut: {}",
                    if object.is_observable {
                        "observable"
                    } else {
                        "hors contraintes"
                    }
                ));
            }
        }

        for (index, line) in lines.iter().enumerate() {
            painter.text(
                egui::pos2(info_origin.x, info_origin.y + index as f32 * 15.0),
                egui::Align2::LEFT_TOP,
                line,
                egui::FontId::monospace(12.0),
                Color32::from_rgb(190, 210, 235),
            );
        }

        if response.clicked() {
            return closest_click.map(|(_, name)| name);
        }

        None
    }

    fn ui_science(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("science_controls")
            .resizable(true)
            .default_width(350.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("science_controls_scroll")
                    .show(ui, |ui| {
                        ui.label(RichText::new("Source des cliches").strong());
                        ui.text_edit_singleline(&mut self.science.frames_dir);
                        ui.horizontal(|ui| {
                            if ui.button("Indexer").clicked() {
                                self.refresh_frame_directory();
                            }
                            ui.checkbox(&mut self.science.auto_refresh, "suivi direct");
                        });

                        ui.separator();
                        ui.label(RichText::new("Calibration").strong());
                        ui.add(
                            egui::DragValue::new(&mut self.science.calibration.bias)
                                .speed(0.5)
                                .prefix("offset "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.science.calibration.dark_current)
                                .speed(0.5)
                                .prefix("courant noir "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.science.calibration.flat_field)
                                .speed(0.01)
                                .clamp_range(0.001..=100.0)
                                .prefix("plage plate "),
                        );

                        ui.separator();
                        ui.label(RichText::new("Detection et photometrie").strong());
                        ui.add(
                            egui::Slider::new(&mut self.science.detection.detection_sigma, 1.0..=20.0)
                                .text("seuil (sigma)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.detection.aperture_radius_px, 1.0..=20.0)
                                .text("ouverture (px)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.detection.annulus_inner_px, 2.0..=40.0)
                                .text("anneau interne (px)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.detection.annulus_outer_px, 3.0..=60.0)
                                .text("anneau externe (px)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.detection.min_separation_px, 1..=30)
                                .text("separation min (px)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.detection.max_sources, 1..=500)
                                .text("sources max"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.plate_scale_arcsec_per_px, 0.05..=10.0)
                                .logarithmic(true)
                                .text("echelle (arcsec/px)"),
                        );

                        ui.separator();
                        ui.label(RichText::new("Controle qualite").strong());
                        ui.add(
                            egui::Slider::new(&mut self.science.thresholds.max_fwhm_pixels, 1.0..=20.0)
                                .text("FWHM max (px)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.thresholds.min_roundness, 0.1..=1.0)
                                .text("rondeur min"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.thresholds.min_snr, 1.0..=100.0)
                                .text("SNR min"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.science.thresholds.saturation_limit_adu)
                                .speed(100.0)
                                .prefix("saturation "),
                        );

                        ui.separator();
                        ui.label(RichText::new("Visualisation").strong());
                        let mut stretch_changed = false;
                        stretch_changed |= ui
                            .add(
                                egui::Slider::new(&mut self.science.black_percentile, 0.0..=49.0)
                                    .text("point noir (%)"),
                            )
                            .changed();
                        stretch_changed |= ui
                            .add(
                                egui::Slider::new(&mut self.science.white_percentile, 50.0..=100.0)
                                    .text("point blanc (%)"),
                            )
                            .changed();
                        ui.checkbox(&mut self.science.show_detections, "marquer les sources");
                        if stretch_changed {
                            self.science.invalidate_texture();
                        }

                        ui.separator();
                        if ui.button("Analyser le cliche charge").clicked() {
                            self.run_frame_analysis();
                        }

                        ui.separator();
                        ui.label(RichText::new("Empilement").strong());
                        egui::ComboBox::from_label("methode")
                            .selected_text(stacking_method_name(self.science.stacking.method))
                            .show_ui(ui, |ui| {
                                for method in [
                                    StackingMethod::Average,
                                    StackingMethod::Median,
                                    StackingMethod::SigmaClipping,
                                ] {
                                    ui.selectable_value(
                                        &mut self.science.stacking.method,
                                        method,
                                        stacking_method_name(method),
                                    );
                                }
                            });
                        ui.add(
                            egui::Slider::new(&mut self.science.stacking.sigma_clip_low, 0.5..=8.0)
                                .text("rejet bas (sigma)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.science.stacking.sigma_clip_high, 0.5..=8.0)
                                .text("rejet haut (sigma)"),
                        );
                        ui.label(format!(
                            "{} cliche(s) dans la pile",
                            self.science.stack_selection.len()
                        ));
                        ui.horizontal(|ui| {
                            if ui.button("Empiler").clicked() {
                                self.run_stacking();
                            }
                            if ui.button("Vider la pile").clicked() {
                                self.science.stack_selection.clear();
                            }
                        });

                        if let Some(result) = self.science.stack_result.as_ref() {
                            ui.label(format!(
                                "resultat: {}x{}, {} images, bruit {:.4}, gain SNR x{:.2}",
                                result.width,
                                result.height,
                                result.frame_count,
                                result.noise_std_dev,
                                result.snr_improvement_factor
                            ));
                        }
                    });
            });

        egui::SidePanel::left("science_frame_list")
            .resizable(true)
            .default_width(270.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Cliches disponibles").strong());
                ui.label(RichText::new(&self.science.status).size(11.5));
                ui.separator();

                if self.science.entries.is_empty() {
                    ui.label("aucun fichier FITS indexe");
                    return;
                }

                let mut to_load: Option<String> = None;
                let mut to_toggle: Option<String> = None;

                egui::ScrollArea::vertical()
                    .id_source("science_entries_scroll")
                    .show(ui, |ui| {
                        for (index, entry) in self.science.entries.iter().enumerate() {
                            let selected = self.science.selected_entry == Some(index);
                            let in_stack = self.science.stack_selection.contains(&entry.path);

                            ui.horizontal(|ui| {
                                let label = if in_stack {
                                    format!("\u{2713} {}", entry.file_name)
                                } else {
                                    entry.file_name.clone()
                                };
                                if ui.selectable_label(selected, label).clicked() {
                                    to_load = Some(entry.path.clone());
                                }
                                if ui.small_button("pile").clicked() {
                                    to_toggle = Some(entry.path.clone());
                                }
                            });
                            ui.label(
                                RichText::new(format!("{:.1} kio", entry.size_bytes as f64 / 1024.0))
                                    .size(10.5)
                                    .weak(),
                            );
                        }
                    });

                if let Some(path) = to_load {
                    self.science.selected_entry = self
                        .science
                        .entries
                        .iter()
                        .position(|entry| entry.path == path);
                    self.load_frame(&path);
                }

                if let Some(path) = to_toggle {
                    if let Some(position) = self
                        .science
                        .stack_selection
                        .iter()
                        .position(|item| item == &path)
                    {
                        self.science.stack_selection.remove(position);
                    } else {
                        self.science.stack_selection.push(path);
                    }
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_frame_view(ui);
        });
    }

    fn draw_frame_view(&mut self, ui: &mut egui::Ui) {
        let Some(frame) = self.science.loaded.as_ref() else {
            ui.centered_and_justified(|ui| {
                ui.label("Selectionner un cliche FITS pour demarrer l'exploitation scientifique");
            });
            return;
        };

        let width = frame.width;
        let height = frame.height;
        let file_name = frame.file_name.clone();
        let source_key = format!(
            "{}|{}|{:.3}|{:.3}",
            frame.path,
            self.science.analysis.is_some(),
            self.science.black_percentile,
            self.science.white_percentile
        );

        if self.science.texture_source.as_deref() != Some(source_key.as_str()) {
            let pixels = match self.science.analysis.as_ref() {
                Some(analysis) => analysis.calibrated_pixels.as_slice(),
                None => frame.raw_pixels.as_slice(),
            };
            let image = build_frame_color_image(
                pixels,
                width,
                height,
                self.science.black_percentile,
                self.science.white_percentile,
            );
            let texture =
                ui.ctx()
                    .load_texture(format!("frame::{file_name}"), image, egui::TextureOptions::LINEAR);
            self.science.texture = Some(texture);
            self.science.texture_source = Some(source_key);
        }

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&file_name).strong());
            ui.separator();
            ui.label(format!("{width} x {height} px"));
            if let Some(object) = self.science.loaded.as_ref().and_then(|f| f.object.clone()) {
                ui.separator();
                ui.label(format!("objet {object}"));
            }
            if let Some(filter) = self.science.loaded.as_ref().and_then(|f| f.filter.clone()) {
                ui.separator();
                ui.label(format!("filtre {filter}"));
            }
            if let Some(exposure_s) = self.science.loaded.as_ref().and_then(|f| f.exposure_s) {
                ui.separator();
                ui.label(format!("pose {exposure_s:.2} s"));
            }
            if let Some(julian_day) = self.science.loaded.as_ref().and_then(|f| f.julian_day) {
                ui.separator();
                ui.label(format!("JJ {julian_day:.5}"));
            }
        });
        ui.separator();

        let image_height = (ui.available_height() * 0.62).max(180.0);
        let available_width = ui.available_width();
        let scale = (available_width / width as f32).min(image_height / height as f32);
        let display_size = egui::vec2(width as f32 * scale, height as f32 * scale);

        if let Some(texture) = self.science.texture.as_ref() {
            let (response, painter) =
                ui.allocate_painter(display_size, egui::Sense::hover());
            let rect = response.rect;
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );

            if self.science.show_detections {
                if let Some(analysis) = self.science.analysis.as_ref() {
                    let marker_radius = (self.science.detection.aperture_radius_px as f32 * scale)
                        .clamp(3.0, 40.0);
                    for star in &analysis.stars {
                        let position = egui::pos2(
                            rect.left() + (star.measurement.x as f32 + 0.5) * scale,
                            rect.top() + (star.measurement.y as f32 + 0.5) * scale,
                        );
                        let color = if star.measurement.snr >= self.science.thresholds.min_snr {
                            Color32::from_rgb(110, 240, 160)
                        } else {
                            Color32::from_rgb(240, 180, 90)
                        };
                        painter.circle_stroke(
                            position,
                            marker_radius,
                            egui::Stroke::new(1.2_f32, color),
                        );
                        painter.text(
                            egui::pos2(position.x + marker_radius + 2.0, position.y),
                            egui::Align2::LEFT_CENTER,
                            star.measurement.star_id.to_string(),
                            egui::FontId::monospace(10.0),
                            color,
                        );
                    }
                }
            }
        }

        ui.separator();

        let Some(analysis) = self.science.analysis.as_ref() else {
            ui.label("Lancer l'analyse pour obtenir photometrie, PSF et controle qualite");
            return;
        };

        ui.horizontal_wrapped(|ui| {
            ui.label(format!("fond median {:.2}", analysis.statistics.median));
            ui.separator();
            ui.label(format!("bruit (MAD) {:.3}", analysis.statistics.background_sigma));
            ui.separator();
            ui.label(format!("seuil detection {:.2}", analysis.detection_threshold));
            ui.separator();
            ui.label(format!(
                "dynamique {:.1} .. {:.1}",
                analysis.statistics.minimum, analysis.statistics.maximum
            ));
            ui.separator();
            ui.label(format!(
                "pixels satures {}",
                analysis.statistics.saturated_pixel_count
            ));
        });

        let quality = &analysis.quality;
        let quality_color = if quality.is_accepted {
            Color32::from_rgb(120, 240, 160)
        } else {
            Color32::from_rgb(240, 150, 120)
        };
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                quality_color,
                format!("qualite {:?}", quality.quality_flag),
            );
            ui.separator();
            ui.label(format!("{} sources", quality.total_detected_stars));
            ui.separator();
            ui.label(format!(
                "FWHM median {:.2} px ({:.2} arcsec)",
                quality.median_fwhm_pixels,
                quality.median_fwhm_pixels * self.science.plate_scale_arcsec_per_px
            ));
            ui.separator();
            ui.label(format!("rondeur mediane {:.3}", quality.median_roundness));
            ui.separator();
            ui.label(format!("SNR median {:.1}", quality.median_snr));
        });

        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_source("science_sources_scroll")
            .show(ui, |ui| {
                egui::Grid::new("science_sources_grid")
                    .num_columns(8)
                    .striped(true)
                    .spacing([14.0, 4.0])
                    .show(ui, |ui| {
                        for header in [
                            "id", "x", "y", "flux net", "SNR", "FWHM x", "FWHM y", "mag inst.",
                        ] {
                            ui.label(RichText::new(header).strong());
                        }
                        ui.end_row();

                        for star in &analysis.stars {
                            ui.label(star.measurement.star_id.to_string());
                            ui.label(format!("{:.2}", star.measurement.x));
                            ui.label(format!("{:.2}", star.measurement.y));
                            ui.label(format!("{:.1}", star.photometry.net_flux));
                            ui.label(format!("{:.1}", star.measurement.snr));
                            ui.label(format!("{:.2}", star.fwhm_x_pixels));
                            ui.label(format!("{:.2}", star.fwhm_y_pixels));
                            if star.instrumental_magnitude.is_finite() {
                                ui.label(format!("{:.3}", star.instrumental_magnitude));
                            } else {
                                ui.label("-");
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn ui_simulator(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("simulator_controls")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("simulator_scroll")
                    .show(ui, |ui| {
                        ui.label(RichText::new("Moteur temps reel").strong());
                        ui.checkbox(&mut self.paused, "pause de l'animation");
                        ui.checkbox(&mut self.show_guides, "guides 3D");
                        ui.add(
                            egui::Slider::new(&mut self.atmosphere_strength, 0.0..=1.5)
                                .text("enveloppes atmospheriques"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.planet_rotation_deg_per_s, 0.0..=30.0)
                                .text("rotation propre (deg/s)"),
                        );

                        ui.separator();
                        ui.label(RichText::new("Camera").strong());
                        ui.label(format!("distance {:.6} ua", self.camera.distance));
                        ui.label(format!("champ {:.1} deg", self.camera.fov_deg));
                        if ui.button("Reinitialiser (R)").clicked() {
                            self.camera.reset();
                        }

                        ui.separator();
                        ui.label(RichText::new("Corps celestes").strong());
                        ui.label(format!("{} corps simules", self.solar_system_bodies.len()));
                        if let Some(index) = self.selected_body_index {
                            let body = &self.solar_system_bodies[index];
                            let position = self.solar_system_positions[index];
                            ui.label(format!("selection: {}", body.name));
                            ui.label(format!(
                                "rayon {:.0} km",
                                body.radius_scene as f64 * KM_PER_AU / AU_TO_SCENE_UNITS
                            ));
                            ui.label(format!(
                                "distance heliocentrique {:.4} ua",
                                (position.length() as f64) / AU_TO_SCENE_UNITS
                            ));
                        }

                        let mut focus_index: Option<usize> = None;
                        egui::ScrollArea::vertical()
                            .id_source("simulator_bodies_scroll")
                            .max_height(240.0)
                            .show(ui, |ui| {
                                for index in 0..self.solar_system_bodies.len() {
                                    let selected = self.selected_body_index == Some(index);
                                    if ui
                                        .selectable_label(
                                            selected,
                                            self.solar_system_bodies[index].name,
                                        )
                                        .clicked()
                                    {
                                        focus_index = Some(index);
                                    }
                                }
                            });
                        if let Some(index) = focus_index {
                            self.focus_body(index);
                        }

                        ui.separator();
                        ui.label(RichText::new("Couplage observation").strong());
                        if let Some(selected) = self.operations.selected_object.clone() {
                            ui.label(format!("cible active: {selected}"));
                        } else {
                            ui.label("aucune cible active dans l'espace d'observation");
                        }

                        let mut aim: Option<(f64, f64, String)> = None;
                        if ui.button("Viser la consigne de monture").clicked() {
                            aim = Some((
                                self.operations.mount.ra_deg,
                                self.operations.mount.dec_deg,
                                "consigne monture".to_string(),
                            ));
                        }

                        egui::ScrollArea::vertical()
                            .id_source("simulator_targets_scroll")
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for target in &self.campaign_targets {
                                    let selected = self.operations.selected_object.as_deref()
                                        == Some(target.name);
                                    if ui.selectable_label(selected, target.name).clicked() {
                                        aim = Some((
                                            target.ra_deg,
                                            target.dec_deg,
                                            target.name.to_string(),
                                        ));
                                    }
                                }
                            });

                        if let Some((ra_deg, dec_deg, name)) = aim {
                            self.aim_camera_from_earth(ra_deg, dec_deg);
                            self.operations.selected_object = Some(name);
                        }

                        ui.separator();
                        ui.label(RichText::new("Controles").strong());
                        ui.label("drag souris: orbite camera");
                        ui.label("molette / Q / E: zoom");
                        ui.label("WASD ou fleches: orbite");
                        ui.label("[ et ]: vitesse de rotation propre");
                        ui.label("- et +: intensite des enveloppes");
                        if ui.button("Afficher l'aide terminal (H)").clicked() {
                            print_controls();
                        }
                    });
            });
    }

    fn focus_body(&mut self, index: usize) {
        let Some(body) = self.solar_system_bodies.get(index) else {
            return;
        };
        self.selected_body_index = Some(index);
        if let Some(position) = self.solar_system_positions.get(index) {
            self.camera.target =
                DVec3::new(position.x as f64, position.y as f64, position.z as f64);
        }
        self.camera.distance = selected_body_camera_distance(body);
        self.camera.near_plane = selected_body_near_plane(body);
    }

    fn aim_camera_from_earth(&mut self, ra_deg: f64, dec_deg: f64) {
        let earth_index = self
            .solar_system_bodies
            .iter()
            .position(|body| body.name == "Earth");
        let Some(earth_index) = earth_index else {
            return;
        };

        let earth = self.solar_system_positions[earth_index];
        self.camera.target = DVec3::new(earth.x as f64, earth.y as f64, earth.z as f64);

        let direction = equatorial_to_scene_direction(ra_deg, dec_deg);
        // The camera orbits its target, so the eye must sit opposite to the aim direction.
        let eye_direction = -direction;
        self.camera.yaw = eye_direction.z.atan2(eye_direction.x);
        self.camera.pitch = eye_direction
            .y
            .clamp(-1.0, 1.0)
            .asin()
            .clamp(-(FRAC_PI_2 as f64) + 0.02, (FRAC_PI_2 as f64) - 0.02);

        let earth_radius = self.solar_system_bodies[earth_index].radius_scene as f64;
        self.camera.distance = (earth_radius * 6.0).max(0.00005);
        self.camera.near_plane = (earth_radius * 0.02).max(1e-8) as f32;
        self.selected_body_index = Some(earth_index);
    }

    fn body_frame_uniform(&self, body: &CelestialBody, position: Vec3) -> FrameUniform {
        let rotation = Mat4::from_rotation_y(self.planet_rotation_rad * body.rotation_multiplier);
        let model = Mat4::from_translation(position) * rotation * Mat4::from_scale(Vec3::splat(body.radius_scene));
        let light_dir = if position.length_squared() > f32::EPSILON {
            (-position).normalize()
        } else {
            Vec3::new(0.7, 0.35, 0.61).normalize()
        };

        let mut frame_uniform = self.frame_uniform;
        frame_uniform.model = model.to_cols_array_2d();
        frame_uniform.light_dir = [light_dir.x, light_dir.y, light_dir.z, 0.0];
        frame_uniform.planet_color = body.surface_color;
        frame_uniform.atmosphere_color = body.atmosphere_color;
        frame_uniform.atmosphere_strength = body.atmosphere_strength * self.atmosphere_strength.max(0.05);
        frame_uniform.specular_strength = body.specular_strength;
        frame_uniform.interior_motion = 0.12 + body.rotation_multiplier.abs() * 0.05;
        frame_uniform.atmosphere_motion = 0.35 + body.atmosphere_strength * 2.2;
        frame_uniform.surface_texture_weight = body.surface_texture_weight;
        frame_uniform
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

        let render_scene = self.workspace == Workspace::Simulator;
        let clear_color = if render_scene {
            let sky = sky_background_color(-0.9, self.elapsed_time_s);
            wgpu::Color {
                r: sky[0] as f64,
                g: sky[1] as f64,
                b: sky[2] as f64,
                a: 1.0,
            }
        } else {
            wgpu::Color {
                r: 0.043,
                g: 0.047,
                b: 0.060,
                a: 1.0,
            }
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui3d_scene_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
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

            if render_scene {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_bind_group(0, &self.view_bind_group, &[]);
                pass.set_vertex_buffer(0, self.sky_vertex_buffer.slice(..));
                pass.set_index_buffer(self.sky_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.sky_index_count, 0, 0..1);

                if self.star_count > 0 {
                    pass.set_pipeline(&self.star_pipeline);
                    pass.set_bind_group(0, &self.view_bind_group, &[]);
                    pass.set_vertex_buffer(0, self.star_vertex_buffer.slice(..));
                    pass.draw(0..self.star_count, 0..1);
                }

                if self.show_guides && self.guide_vertex_count > 1 {
                    pass.set_pipeline(&self.guide_pipeline);
                    pass.set_bind_group(0, &self.view_bind_group, &[]);
                    pass.set_vertex_buffer(0, self.guide_vertex_buffer.slice(..));
                    pass.draw(0..self.guide_vertex_count, 0..1);
                }

                for body_index in 0..self.solar_system_bodies.len() {
                    let body = self.solar_system_bodies[body_index].clone();
                    let position = self.solar_system_positions[body_index];
                    let frame_uniform = self.body_frame_uniform(&body, position);
                    self.queue.write_buffer(
                        &self.body_frame_buffers[body_index],
                        0,
                        bytemuck::bytes_of(&frame_uniform),
                    );

                    pass.set_pipeline(&self.planet_pipeline);
                    pass.set_bind_group(0, &self.planet_bind_groups[body_index], &[]);
                    pass.set_vertex_buffer(0, self.planet_vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        self.planet_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..self.planet_index_count, 0, 0..1);

                    if body.atmosphere_strength > 0.001 {
                        pass.set_pipeline(&self.atmosphere_pipeline);
                        pass.set_bind_group(0, &self.planet_bind_groups[body_index], &[]);
                        pass.set_vertex_buffer(0, self.planet_vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            self.planet_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..self.planet_index_count, 0, 0..1);
                    }
                }
            }
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

fn load_planet_layer_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    asset_name: &str,
) -> Result<PlanetLayerTextures, String> {
    let base_dir = Path::new("assets").join("textures").join(asset_name);
    let lod_names = ["lod0.ppm", "lod1.ppm", "lod2.ppm", "lod3.ppm"];
    let lods: Vec<TextureLodImage> = lod_names
        .iter()
        .map(|name| load_ppm_rgb(&base_dir.join(name)))
        .collect::<Result<_, _>>()?;

    let layer_texture = create_mipmapped_layer_texture(
        device,
        queue,
        "mars_body_multilayer_lod_texture",
        &lods,
    )?;
    let layer_view = layer_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("mars_body_layer_view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layer_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("mars_body_lod_sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    Ok(PlanetLayerTextures {
        layer_view,
        layer_sampler,
    })
}

fn create_mipmapped_layer_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    lods: &[TextureLodImage],
) -> Result<wgpu::Texture, String> {
    let Some(base_lod) = lods.first() else {
        return Err("missing Mars texture LOD images".to_string());
    };

    for (index, lod) in lods.iter().enumerate() {
        if lod.rgb.len() != (lod.width as usize * lod.height as usize * 3) {
            return Err(format!("invalid RGB data length for Mars LOD {index}"));
        }
        if index > 0 {
            let previous = &lods[index - 1];
            if lod.width != previous.width / 2 || lod.height != previous.height / 2 {
                return Err(format!("Mars LOD {index} is not half of previous level"));
            }
        }
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: base_lod.width,
            height: base_lod.height,
            depth_or_array_layers: PLANET_TEXTURE_LAYER_COUNT,
        },
        mip_level_count: lods.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (mip_level, lod) in lods.iter().enumerate() {
        for layer in 0..PLANET_TEXTURE_LAYER_COUNT {
            let rgba = build_mars_layer_rgba(lod, layer);
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &texture,
                    mip_level: mip_level as u32,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * lod.width),
                    rows_per_image: Some(lod.height),
                },
                wgpu::Extent3d {
                    width: lod.width,
                    height: lod.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    Ok(texture)
}

fn build_mars_layer_rgba(lod: &TextureLodImage, layer: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(lod.width as usize * lod.height as usize * 4);
    for pixel in lod.rgb.chunks_exact(3) {
        let red = pixel[0] as f32;
        let green = pixel[1] as f32;
        let blue = pixel[2] as f32;
        let luminance = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255.0;

        match layer {
            0 => {
                rgba.push((34.0 + 82.0 * luminance).round() as u8);
                rgba.push((12.0 + 34.0 * luminance).round() as u8);
                rgba.push((8.0 + 24.0 * luminance).round() as u8);
                rgba.push(255);
            }
            1 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            2 => {
                let dust_alpha = (32.0 + 116.0 * luminance).round() as u8;
                rgba.push((180.0 + 42.0 * luminance).round() as u8);
                rgba.push((112.0 + 38.0 * luminance).round() as u8);
                rgba.push((72.0 + 24.0 * luminance).round() as u8);
                rgba.push(dust_alpha);
            }
            _ => unreachable!("invalid Mars texture layer"),
        }
    }
    rgba
}

fn load_ppm_rgb(path: &Path) -> Result<TextureLodImage, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read Mars texture '{}': {error}", path.display()))?;
    parse_ppm_rgb(&bytes).map_err(|error| format!("invalid Mars texture '{}': {error}", path.display()))
}

fn parse_ppm_rgb(bytes: &[u8]) -> Result<TextureLodImage, String> {
    let mut cursor = 0usize;
    let magic = next_ppm_token(bytes, &mut cursor).ok_or_else(|| "missing PPM magic".to_string())?;
    if magic != "P6" {
        return Err(format!("unsupported PPM magic '{magic}'"));
    }

    let width = next_ppm_token(bytes, &mut cursor)
        .ok_or_else(|| "missing PPM width".to_string())?
        .parse::<u32>()
        .map_err(|error| format!("invalid PPM width: {error}"))?;
    let height = next_ppm_token(bytes, &mut cursor)
        .ok_or_else(|| "missing PPM height".to_string())?
        .parse::<u32>()
        .map_err(|error| format!("invalid PPM height: {error}"))?;
    let max_value = next_ppm_token(bytes, &mut cursor)
        .ok_or_else(|| "missing PPM max value".to_string())?
        .parse::<u32>()
        .map_err(|error| format!("invalid PPM max value: {error}"))?;
    if width == 0 || height == 0 {
        return Err("PPM dimensions must be non-zero".to_string());
    }
    if max_value != 255 {
        return Err(format!("unsupported PPM max value {max_value}"));
    }

    if cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    let expected = width as usize * height as usize * 3;
    let remaining = bytes.len().saturating_sub(cursor);
    if remaining != expected {
        return Err(format!("expected {expected} RGB bytes, found {remaining}"));
    }

    Ok(TextureLodImage {
        width,
        height,
        rgb: bytes[cursor..].to_vec(),
    })
}

fn next_ppm_token(bytes: &[u8], cursor: &mut usize) -> Option<String> {
    loop {
        while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
            *cursor += 1;
        }
        if *cursor >= bytes.len() || bytes[*cursor] != b'#' {
            break;
        }
        while *cursor < bytes.len() && bytes[*cursor] != b'\n' {
            *cursor += 1;
        }
    }

    let start = *cursor;
    while *cursor < bytes.len() && !bytes[*cursor].is_ascii_whitespace() {
        *cursor += 1;
    }
    if start == *cursor {
        return None;
    }

    std::str::from_utf8(&bytes[start..*cursor])
        .ok()
        .map(str::to_string)
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

fn workspace_capabilities(workspace: Workspace) -> &'static [&'static str] {
    match workspace {
        Workspace::Home => &[],
        Workspace::SkyOperations => &[
            "carte azimutale temps reel du ciel local (cibles, planetes, lunes, asteroides)",
            "temps sideral local, angle horaire, altitude, azimut et masse d'air",
            "consigne de monture corrigee du modele de pointage et de la refraction",
            "planification de la file d'observation par merite et acquisition pilotee",
        ],
        Workspace::Science => &[
            "lecture FITS native (8, 16, 32 bits entiers et 32/64 bits flottants)",
            "calibration offset / courant d'obscurite / plage plate",
            "detection multi-sources robuste et photometrie d'ouverture",
            "metrologie PSF (FWHM, rondeur) et controle qualite d'image",
            "empilement moyenne, mediane ou rejet sigma sur une pile de cliches",
        ],
        Workspace::Simulator => &[
            "rendu 3D temps reel du systeme solaire en unites astronomiques",
            "positions kepleriennes des planetes, satellites et asteroides",
            "rotation propre, enveloppes internes et atmospheres animees",
            "visee de la camera depuis la Terre vers la cible observee",
        ],
    }
}

fn format_airmass(airmass: f64) -> String {
    if airmass.is_finite() {
        format!("{airmass:.3}")
    } else {
        "hors horizon".to_string()
    }
}

fn format_hours_hms(hours: f64) -> String {
    let normalized = hours.rem_euclid(24.0);
    let h = normalized.floor();
    let minutes_total = (normalized - h) * 60.0;
    let m = minutes_total.floor();
    let s = (minutes_total - m) * 60.0;
    format!("{:02}h{:02}m{:05.2}s", h as u32, m as u32, s)
}

fn format_right_ascension(ra_deg: f64) -> String {
    format_hours_hms(ra_deg.rem_euclid(360.0) / 15.0)
}

fn format_declination(dec_deg: f64) -> String {
    let sign = if dec_deg < 0.0 { '-' } else { '+' };
    let absolute = dec_deg.abs();
    let degrees = absolute.floor();
    let minutes_total = (absolute - degrees) * 60.0;
    let minutes = minutes_total.floor();
    let seconds = (minutes_total - minutes) * 60.0;
    format!("{sign}{:02}d{:02}m{:04.1}s", degrees as u32, minutes as u32, seconds)
}

fn stacking_method_name(method: StackingMethod) -> &'static str {
    match method {
        StackingMethod::Average => "moyenne",
        StackingMethod::Median => "mediane",
        StackingMethod::SigmaClipping => "rejet sigma",
    }
}

fn percentile_value(sorted: &[f64], percentile: f32) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let ratio = (percentile as f64 / 100.0).clamp(0.0, 1.0);
    let index = (ratio * (sorted.len() - 1) as f64).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn build_frame_color_image(
    pixels: &[f64],
    width: usize,
    height: usize,
    black_percentile: f32,
    white_percentile: f32,
) -> egui::ColorImage {
    if width == 0 || height == 0 || pixels.len() != width * height {
        return egui::ColorImage::new([1, 1], Color32::BLACK);
    }

    let mut sorted: Vec<f64> = pixels.iter().copied().filter(|value| value.is_finite()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let black = percentile_value(&sorted, black_percentile);
    let white = percentile_value(&sorted, white_percentile);
    let span = if (white - black).abs() > f64::EPSILON {
        white - black
    } else {
        1.0
    };

    let mut image = egui::ColorImage::new([width, height], Color32::BLACK);
    for (index, value) in pixels.iter().enumerate() {
        let normalized = if value.is_finite() {
            ((value - black) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let level = (normalized * 255.0).round() as u8;
        image.pixels[index] = Color32::from_gray(level);
    }

    image
}

fn equatorial_to_scene_direction(ra_deg: f64, dec_deg: f64) -> DVec3 {
    let ra_rad = ra_deg.to_radians();
    let dec_rad = dec_deg.to_radians();
    let x_eq = dec_rad.cos() * ra_rad.cos();
    let y_eq = dec_rad.cos() * ra_rad.sin();
    let z_eq = dec_rad.sin();

    let obliquity = observatory_core::J2000_MEAN_OBLIQUITY_DEG.to_radians();
    let cos_eps = obliquity.cos();
    let sin_eps = obliquity.sin();

    let x_ecl = x_eq;
    let y_ecl = y_eq * cos_eps + z_eq * sin_eps;
    let z_ecl = -y_eq * sin_eps + z_eq * cos_eps;

    // Scene axes: X and Z span the ecliptic plane, Y is the ecliptic normal.
    DVec3::new(x_ecl, z_ecl, y_ecl).normalize_or_zero()
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

fn current_julian_day() -> f64 {
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);
    UNIX_EPOCH_JULIAN_DAY + unix_seconds / 86_400.0
}

fn scene_radius_from_km(radius_km: f64) -> f32 {
    (radius_km / KM_PER_AU * AU_TO_SCENE_UNITS) as f32
}

fn selected_body_camera_distance(body: &CelestialBody) -> f64 {
    (body.radius_scene as f64 * 8.0).max(0.00005)
}

fn selected_body_near_plane(body: &CelestialBody) -> f32 {
    (body.radius_scene * 0.02).max(0.00000001)
}

fn solve_kepler(mean_anomaly_rad: f64, eccentricity: f64) -> f64 {
    let mut eccentric_anomaly = mean_anomaly_rad;
    for _ in 0..8 {
        let delta = (eccentric_anomaly - eccentricity * eccentric_anomaly.sin() - mean_anomaly_rad)
            / (1.0 - eccentricity * eccentric_anomaly.cos());
        eccentric_anomaly -= delta;
        if delta.abs() < 1e-12 {
            break;
        }
    }
    eccentric_anomaly
}

fn heliocentric_position(elements: OrbitalElements, julian_day: f64) -> Vec3 {
    let days_since_j2000 = julian_day - J2000_JULIAN_DAY;
    let mean_anomaly_deg = elements.mean_longitude_deg
        - elements.longitude_perihelion_deg
        + 360.0 * days_since_j2000 / elements.orbital_period_days;
    let mean_anomaly_rad = mean_anomaly_deg.to_radians().rem_euclid(std::f64::consts::TAU);
    let eccentric_anomaly = solve_kepler(mean_anomaly_rad, elements.eccentricity);
    let xv = eccentric_anomaly.cos() - elements.eccentricity;
    let yv = (1.0 - elements.eccentricity * elements.eccentricity).sqrt() * eccentric_anomaly.sin();
    let true_anomaly = yv.atan2(xv);
    let radius_au = elements.semi_major_axis_au * (1.0 - elements.eccentricity * eccentric_anomaly.cos());

    let longitude_node = elements.longitude_ascending_node_deg.to_radians();
    let inclination = elements.inclination_deg.to_radians();
    let argument_perihelion = (elements.longitude_perihelion_deg - elements.longitude_ascending_node_deg).to_radians();
    let argument = true_anomaly + argument_perihelion;

    let x = radius_au * (longitude_node.cos() * argument.cos() - longitude_node.sin() * argument.sin() * inclination.cos());
    let y = radius_au * (argument.sin() * inclination.sin());
    let z = radius_au * (longitude_node.sin() * argument.cos() + longitude_node.cos() * argument.sin() * inclination.cos());
    Vec3::new((x * AU_TO_SCENE_UNITS) as f32, (y * AU_TO_SCENE_UNITS) as f32, (z * AU_TO_SCENE_UNITS) as f32)
}

fn circular_relative_position(
    semi_major_axis_au: f64,
    orbital_period_days: f64,
    inclination_deg: f64,
    mean_longitude_deg: f64,
    julian_day: f64,
) -> Vec3 {
    let days_since_j2000 = julian_day - J2000_JULIAN_DAY;
    let phase = (mean_longitude_deg + 360.0 * days_since_j2000 / orbital_period_days).to_radians();
    let inclination = inclination_deg.to_radians();
    let x = semi_major_axis_au * phase.cos();
    let y = semi_major_axis_au * phase.sin() * inclination.sin();
    let z = semi_major_axis_au * phase.sin() * inclination.cos();
    Vec3::new((x * AU_TO_SCENE_UNITS) as f32, (y * AU_TO_SCENE_UNITS) as f32, (z * AU_TO_SCENE_UNITS) as f32)
}

fn compute_solar_system_positions(bodies: &[CelestialBody], julian_day: f64) -> Vec<Vec3> {
    let mut positions = Vec::with_capacity(bodies.len());
    for body in bodies {
        let position = match body.orbit {
            BodyOrbit::FixedSun => Vec3::ZERO,
            BodyOrbit::Heliocentric(elements) => heliocentric_position(elements, julian_day),
            BodyOrbit::Planetocentric {
                parent_index,
                semi_major_axis_au,
                orbital_period_days,
                inclination_deg,
                mean_longitude_deg,
            } => positions[parent_index]
                + circular_relative_position(
                    semi_major_axis_au,
                    orbital_period_days,
                    inclination_deg,
                    mean_longitude_deg,
                    julian_day,
                ),
            BodyOrbit::BeltObject {
                semi_major_axis_au,
                eccentricity,
                inclination_deg,
                longitude_ascending_node_deg,
                longitude_perihelion_deg,
                mean_longitude_deg,
                orbital_period_days,
            } => heliocentric_position(
                OrbitalElements {
                    semi_major_axis_au,
                    eccentricity,
                    inclination_deg,
                    longitude_ascending_node_deg,
                    longitude_perihelion_deg,
                    mean_longitude_deg,
                    orbital_period_days,
                },
                julian_day,
            ),
        };
        positions.push(position);
    }
    positions
}

fn build_solar_system_bodies() -> Vec<CelestialBody> {
    let mut bodies = Vec::new();
    bodies.push(CelestialBody {
        name: "Sun",
        texture_asset: "jupiter",
        orbit: BodyOrbit::FixedSun,
        radius_scene: scene_radius_from_km(696_340.0),
        rotation_multiplier: 0.15,
        surface_color: [1.0, 0.78, 0.35, 1.0],
        atmosphere_color: [1.0, 0.55, 0.18, 1.0],
        atmosphere_strength: 0.32,
        specular_strength: 0.0,
        surface_texture_weight: 0.02,
    });

    let planets = [
        ("Mercury", "mercury", scene_radius_from_km(2_439.7), 1.8, [0.55, 0.50, 0.44, 1.0], [0.20, 0.18, 0.15, 1.0], 0.0, 0.02, 0.92, OrbitalElements { semi_major_axis_au: 0.387098, eccentricity: 0.205630, inclination_deg: 7.00487, longitude_ascending_node_deg: 48.33167, longitude_perihelion_deg: 77.45645, mean_longitude_deg: 252.25084, orbital_period_days: 87.9691 }),
        ("Venus", "venus", scene_radius_from_km(6_051.8), 0.9, [0.86, 0.68, 0.43, 1.0], [0.94, 0.75, 0.46, 1.0], 0.48, 0.01, 0.94, OrbitalElements { semi_major_axis_au: 0.723332, eccentricity: 0.006772, inclination_deg: 3.39471, longitude_ascending_node_deg: 76.68069, longitude_perihelion_deg: 131.53298, mean_longitude_deg: 181.97973, orbital_period_days: 224.701 }),
        ("Earth", "earth", scene_radius_from_km(6_371.0), 1.0, [0.48, 0.62, 0.84, 1.0], [0.42, 0.64, 1.0, 1.0], 0.38, 0.12, 0.96, OrbitalElements { semi_major_axis_au: 1.000000, eccentricity: 0.016710, inclination_deg: 0.00005, longitude_ascending_node_deg: -11.26064, longitude_perihelion_deg: 102.94719, mean_longitude_deg: 100.46435, orbital_period_days: 365.256 }),
        ("Mars", "mars", scene_radius_from_km(3_389.5), 0.98, [0.62, 0.25, 0.12, 1.0], [0.48, 0.22, 0.12, 1.0], 0.18, 0.045, 0.97, OrbitalElements { semi_major_axis_au: 1.523662, eccentricity: 0.093412, inclination_deg: 1.85061, longitude_ascending_node_deg: 49.57854, longitude_perihelion_deg: 336.04084, mean_longitude_deg: 355.45332, orbital_period_days: 686.980 }),
        ("Jupiter", "jupiter", scene_radius_from_km(69_911.0), 2.4, [0.78, 0.62, 0.46, 1.0], [0.72, 0.58, 0.44, 1.0], 0.16, 0.08, 0.96, OrbitalElements { semi_major_axis_au: 5.203363, eccentricity: 0.048393, inclination_deg: 1.30530, longitude_ascending_node_deg: 100.55615, longitude_perihelion_deg: 14.75385, mean_longitude_deg: 34.40438, orbital_period_days: 4332.589 }),
        ("Saturn", "saturn", scene_radius_from_km(58_232.0), 2.2, [0.82, 0.72, 0.50, 1.0], [0.74, 0.66, 0.48, 1.0], 0.12, 0.06, 0.96, OrbitalElements { semi_major_axis_au: 9.537070, eccentricity: 0.054151, inclination_deg: 2.48446, longitude_ascending_node_deg: 113.71504, longitude_perihelion_deg: 92.43194, mean_longitude_deg: 49.94432, orbital_period_days: 10759.22 }),
        ("Uranus", "uranus", scene_radius_from_km(25_362.0), 1.7, [0.52, 0.78, 0.82, 1.0], [0.42, 0.72, 0.86, 1.0], 0.18, 0.04, 0.96, OrbitalElements { semi_major_axis_au: 19.191264, eccentricity: 0.047168, inclination_deg: 0.76986, longitude_ascending_node_deg: 74.22988, longitude_perihelion_deg: 170.96424, mean_longitude_deg: 313.23218, orbital_period_days: 30685.4 }),
        ("Neptune", "neptune", scene_radius_from_km(24_622.0), 1.8, [0.36, 0.48, 0.86, 1.0], [0.24, 0.38, 0.82, 1.0], 0.20, 0.05, 0.96, OrbitalElements { semi_major_axis_au: 30.068963, eccentricity: 0.008586, inclination_deg: 1.76917, longitude_ascending_node_deg: 131.72169, longitude_perihelion_deg: 44.97135, mean_longitude_deg: 304.88003, orbital_period_days: 60190.0 }),
    ];
    for (name, texture_asset, radius_scene, rotation_multiplier, surface_color, atmosphere_color, atmosphere_strength, specular_strength, surface_texture_weight, orbit) in planets {
        bodies.push(CelestialBody { name, texture_asset, orbit: BodyOrbit::Heliocentric(orbit), radius_scene, rotation_multiplier, surface_color, atmosphere_color, atmosphere_strength, specular_strength, surface_texture_weight });
    }

    let moons = [
        ("Moon", 3usize, 0.00257, 27.3217, 5.14, 125.1, scene_radius_from_km(1_737.4)),
        ("Phobos", 4usize, 0.000063, 0.3189, 1.1, 20.0, scene_radius_from_km(11.1)),
        ("Deimos", 4usize, 0.000157, 1.263, 1.8, 260.0, scene_radius_from_km(6.2)),
        ("Io", 5usize, 0.00282, 1.769, 0.05, 40.0, scene_radius_from_km(1_821.6)),
        ("Europa", 5usize, 0.00449, 3.551, 0.47, 90.0, scene_radius_from_km(1_560.8)),
        ("Ganymede", 5usize, 0.00716, 7.155, 0.20, 160.0, scene_radius_from_km(2_634.1)),
        ("Callisto", 5usize, 0.01258, 16.689, 0.28, 240.0, scene_radius_from_km(2_410.3)),
        ("Titan", 6usize, 0.00817, 15.945, 0.35, 70.0, scene_radius_from_km(2_574.7)),
        ("Triton", 8usize, 0.00237, 5.877, 23.0, 180.0, scene_radius_from_km(1_353.4)),
    ];
    for (name, parent_index, semi_major_axis_au, orbital_period_days, inclination_deg, mean_longitude_deg, radius_scene) in moons {
        bodies.push(CelestialBody {
            name,
            texture_asset: "mercury",
            orbit: BodyOrbit::Planetocentric { parent_index, semi_major_axis_au, orbital_period_days, inclination_deg, mean_longitude_deg },
            radius_scene,
            rotation_multiplier: 0.7,
            surface_color: [0.55, 0.52, 0.48, 1.0],
            atmosphere_color: [0.42, 0.38, 0.32, 1.0],
            atmosphere_strength: if name == "Titan" { 0.26 } else { 0.0 },
            specular_strength: 0.01,
            surface_texture_weight: 0.45,
        });
    }

    let asteroids = [
        ("Ceres", 2.7675, 0.0758, 10.59, 80.30, 73.60, 95.99, 1_680.0, 469.7),
        ("Vesta", 2.3618, 0.0887, 7.14, 103.85, 150.73, 151.20, 1_325.8, 262.7),
        ("Pallas", 2.7730, 0.2310, 34.84, 173.10, 310.17, 33.22, 1_686.0, 256.0),
        ("Hygiea", 3.1415, 0.1125, 3.83, 283.20, 312.32, 60.90, 2_034.0, 217.0),
        ("Interamnia", 3.0620, 0.1550, 17.31, 280.36, 95.76, 280.0, 1_956.0, 166.0),
        ("Davida", 3.1640, 0.1860, 15.94, 107.60, 337.60, 310.0, 2_055.0, 149.0),
        ("Psyche", 2.9230, 0.1340, 3.10, 150.04, 229.33, 35.0, 1_827.0, 113.0),
        ("Eros", 1.4580, 0.2230, 10.83, 304.30, 178.80, 178.0, 643.0, 8.4),
    ];
    for (name, semi_major_axis_au, eccentricity, inclination_deg, longitude_ascending_node_deg, longitude_perihelion_deg, mean_longitude_deg, orbital_period_days, radius_km) in asteroids {
        bodies.push(CelestialBody {
            name,
            texture_asset: "mercury",
            orbit: BodyOrbit::BeltObject {
                semi_major_axis_au,
                eccentricity,
                inclination_deg,
                longitude_ascending_node_deg,
                longitude_perihelion_deg,
                mean_longitude_deg,
                orbital_period_days,
            },
            radius_scene: scene_radius_from_km(radius_km),
            rotation_multiplier: 1.8,
            surface_color: [0.34, 0.31, 0.27, 1.0],
            atmosphere_color: [0.0, 0.0, 0.0, 1.0],
            atmosphere_strength: 0.0,
            specular_strength: 0.0,
            surface_texture_weight: 0.18,
        });
    }

    bodies
}

fn create_scene(config_text: &str, star_shell_radius: f32) -> Result<SceneDefinition, String> {
    let config = parse_campaign_config(config_text)?;

    let targets: Vec<SceneTarget> = config
        .targets
        .iter()
        .map(|target| {
            let position = ra_dec_to_cartesian_f64(target, star_shell_radius as f64);
            SceneTarget {
                position: [position[0] as f32, position[1] as f32, position[2] as f32],
                luminance: priority_to_luminance(target.priority),
            }
        })
        .collect();

    Ok(SceneDefinition {
        campaign_name: config.name.clone(),
        targets,
        campaign_targets: config_to_targets(&config),
        calibration: CalibrationFrame {
            bias: config.bias,
            dark_current: config.dark_current,
            flat_field: config.flat_field,
        },
        detection_threshold: config.threshold,
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
        0.025 + 0.070 * day_mix + 0.040 * dawn_glow,
        0.060 + 0.155 * day_mix + 0.055 * dawn_glow,
        0.120 + 0.260 * day_mix + 0.075 * dawn_glow,
        1.0,
    ];
    let atmosphere = [
        0.035 + 0.170 * glow + 0.090 * dawn_glow,
        0.080 + 0.240 * glow + 0.110 * dawn_glow,
        0.180 + 0.320 * glow + 0.150 * dawn_glow,
        1.0,
    ];

    (planet, atmosphere)
}

fn sky_background_color(elevation: f32, time_of_day: f32) -> [f32; 3] {
    let norm = elevation.clamp(-1.0, 1.0);
    let zenith_factor = (norm + 1.0) * 0.5;
    let horizon_factor = (1.0 - zenith_factor).max(0.0);
    let solar_altitude = (time_of_day * 0.12).sin().clamp(-1.0, 1.0);
    let zodiacal = 0.014 * (1.0 - solar_altitude.max(0.0)).clamp(0.0, 1.0);
    let airglow = 0.003 + 0.003 * horizon_factor;
    let deep_space = 0.005 + 0.018 * zenith_factor;

    [
        deep_space * 0.62 + airglow * 0.32 + zodiacal * 0.18,
        deep_space * 0.92 + airglow * 0.42 + zodiacal * 0.15,
        deep_space * 1.85 + airglow * 0.58 + zodiacal * 0.12,
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
                uv: [1.0 - u, v],
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
        atmospheric_palette, build_frame_color_image, build_solar_system_bodies,
        compute_solar_system_positions, equatorial_to_scene_direction, format_airmass,
        format_declination, format_hours_hms, format_right_ascension, generate_background_stars,
        generate_sky_dome, generate_uv_sphere, parse_cli_args, parse_ppm_rgb, percentile_value,
        priority_to_luminance, ra_dec_to_cartesian, ra_dec_to_cartesian_f64, sky_background_color,
        solar_halo_color, stacking_method_name, tone_map_color, workspace_capabilities, Workspace,
        J2000_JULIAN_DAY, KM_PER_AU,
    };
    use observatory_core::{ecliptic_vector_to_equatorial, CampaignTargetConfig, StackingMethod};

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
    fn generate_uv_sphere_assigns_texture_coordinates() {
        let (vertices, indices) = generate_uv_sphere(8, 4, 1.0);
        assert_eq!(vertices.len(), (8 + 1) * (4 + 1));
        assert_eq!(indices.len(), 8 * 4 * 6);
        assert!(vertices.iter().all(|vertex| {
            vertex.uv[0].is_finite()
                && vertex.uv[1].is_finite()
                && vertex.uv[0] >= 0.0
                && vertex.uv[0] <= 1.0
                && vertex.uv[1] >= 0.0
                && vertex.uv[1] <= 1.0
        }));
    }

    #[test]
    fn parse_ppm_rgb_reads_binary_lod_image() {
        let ppm = b"P6\n# generated test texture\n2 1\n255\n\x0a\x20\x30\x40\x50\x60";
        let image = parse_ppm_rgb(ppm).expect("PPM should parse");
        assert_eq!(image.width, 2);
        assert_eq!(image.height, 1);
        assert_eq!(image.rgb, vec![0x0a, 0x20, 0x30, 0x40, 0x50, 0x60]);
    }

    #[test]
    fn solar_system_catalog_includes_planets_moons_and_asteroids() {
        let bodies = build_solar_system_bodies();
        assert!(bodies.iter().any(|body| body.name == "Earth"));
        assert!(bodies.iter().any(|body| body.name == "Moon"));
        assert!(bodies.iter().any(|body| body.name == "Titan"));
        assert!(bodies.iter().any(|body| body.name == "Ceres"));
        assert!(bodies.iter().any(|body| body.name == "Vesta"));
        let earth = bodies.iter().find(|body| body.name == "Earth").expect("Earth exists");
        assert!(((earth.radius_scene as f64 * KM_PER_AU) - 6_371.0).abs() < 0.5);
        let positions = compute_solar_system_positions(&bodies, J2000_JULIAN_DAY);
        assert_eq!(positions.len(), bodies.len());
        assert!(positions.iter().all(|position| position.is_finite()));
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

    #[test]
    fn every_workspace_exposes_navigation_metadata() {
        for workspace in Workspace::ALL {
            assert!(!workspace.title().is_empty());
            assert!(!workspace.icon().is_empty());
            assert!(!workspace.summary().is_empty());
        }
        assert!(workspace_capabilities(Workspace::Home).is_empty());
        for workspace in [
            Workspace::SkyOperations,
            Workspace::Science,
            Workspace::Simulator,
        ] {
            assert!(!workspace_capabilities(workspace).is_empty());
        }
    }

    #[test]
    fn formats_equatorial_coordinates_in_sexagesimal() {
        assert_eq!(format_right_ascension(0.0), "00h00m00.00s");
        assert_eq!(format_right_ascension(180.0), "12h00m00.00s");
        assert_eq!(format_right_ascension(-15.0), "23h00m00.00s");
        assert_eq!(format_declination(0.0), "+00d00m00.0s");
        assert_eq!(format_declination(-41.5), "-41d30m00.0s");
        assert_eq!(format_hours_hms(1.5), "01h30m00.00s");
    }

    #[test]
    fn airmass_formatting_marks_objects_below_the_horizon() {
        assert_eq!(format_airmass(1.25), "1.250");
        assert_eq!(format_airmass(f64::INFINITY), "hors horizon");
    }

    #[test]
    fn stacking_method_names_are_distinct() {
        let names = [
            stacking_method_name(StackingMethod::Average),
            stacking_method_name(StackingMethod::Median),
            stacking_method_name(StackingMethod::SigmaClipping),
        ];
        assert_ne!(names[0], names[1]);
        assert_ne!(names[1], names[2]);
        assert_ne!(names[0], names[2]);
    }

    #[test]
    fn scene_direction_round_trips_through_equatorial_conversion() {
        for (ra_deg, dec_deg) in [(0.0, 0.0), (83.633, 22.014), (201.3, -43.1), (359.9, 67.5)] {
            let direction = equatorial_to_scene_direction(ra_deg, dec_deg);
            assert!((direction.length() - 1.0).abs() < 1e-12);

            let (recovered_ra, recovered_dec, range) =
                ecliptic_vector_to_equatorial(direction.x, direction.z, direction.y);
            assert!((range - 1.0).abs() < 1e-12);
            assert!((recovered_dec - dec_deg).abs() < 1e-9);
            let delta_ra = (recovered_ra - ra_deg).rem_euclid(360.0);
            assert!(delta_ra < 1e-9 || (360.0 - delta_ra) < 1e-9);
        }
    }

    #[test]
    fn percentile_value_selects_ordered_samples() {
        let sorted = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile_value(&sorted, 0.0), 0.0);
        assert_eq!(percentile_value(&sorted, 100.0), 4.0);
        assert_eq!(percentile_value(&sorted, 50.0), 2.0);
        assert_eq!(percentile_value(&[], 50.0), 0.0);
    }

    #[test]
    fn frame_color_image_applies_percentile_stretch() {
        let pixels: Vec<f64> = (0..16).map(|value| value as f64).collect();
        let image = build_frame_color_image(&pixels, 4, 4, 0.0, 100.0);
        assert_eq!(image.size, [4, 4]);
        assert_eq!(image.pixels[0].r(), 0);
        assert_eq!(image.pixels[15].r(), 255);
        assert!(image.pixels[8].r() > image.pixels[4].r());
    }

    #[test]
    fn frame_color_image_rejects_inconsistent_geometry() {
        let image = build_frame_color_image(&[1.0, 2.0, 3.0], 4, 4, 1.0, 99.0);
        assert_eq!(image.size, [1, 1]);
    }
}