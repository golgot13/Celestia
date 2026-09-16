use std::f32::consts::{FRAC_PI_2, PI};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytemuck::{Pod, Zeroable};
use egui::{Color32, RichText};
use glam::{DMat4, DVec3, DVec4, Mat4, Vec3};
use observatory_core::{
    analyze_frame, apply_pointing_correction, build_application, build_sky_chart,
    compute_catalogue_positions_au, compute_local_sidereal_time_rad, config_to_targets, deg_to_rad,
    ecliptic_vector_to_equatorial, evaluate_body_orientation, initialize_mount,
    parse_campaign_config, pole_direction_ecliptic, rad_to_deg, read_fits_file, read_ppm_file,
    sample_diurnal_track, scan_frame_directory, schedule_observation_queue, solar_system_catalogue,
    stack_frame_files, start_capture, AtmosphericConditions, BodyClass, BodyOrientation,
    CalibrationFrame, CampaignTarget, CampaignTargetConfig, CaptureResult, CaptureSession,
    DetectionParams, FrameAnalysis, FrameFileEntry, GeographicCoord, MountState, OrbitModel,
    PointingModelTerms, PpmImage, QcThresholds, ReferencePlane, RingGeometry, SchedulePlan,
    SiteLimits, SkyChart, SkyObjectClass, SkyObjectRequest, SolarSystemBody, StackedResult,
    StackingMethod, StackingParams,
};
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowBuilder};

const J2000_JULIAN_DAY: f64 = 2451545.0;
const UNIX_EPOCH_JULIAN_DAY: f64 = 2440587.5;
const KM_PER_AU: f64 = 149_597_870.7;
const AU_TO_SCENE_UNITS: f64 = 1.0;
/// Linear limb-darkening coefficient of the solar photosphere in the visible band.
const SOLAR_LIMB_DARKENING_U: f32 = 0.6;
/// Bond albedo of the icy particles that dominate the Saturnian ring system.
const RING_PARTICLE_ALBEDO: f32 = 0.5;

const PLANET_SHADER_WGSL: &str = r#"
struct FrameUniform {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_pos: vec4<f32>,
    base_color: vec4<f32>,
    atmosphere_color: vec4<f32>,
    surface_params: vec4<f32>,
    shape_params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

@group(0) @binding(1)
var surface_map: texture_2d<f32>;

@group(0) @binding(2)
var surface_sampler: sampler;

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
    @location(3) body_normal: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world = frame.model * vec4<f32>(input.position, 1.0);
    out.world_position = world.xyz;
    out.world_normal = normalize((frame.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    out.body_normal = normalize(input.normal);
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

fn reinhard_tone_map(color: vec3<f32>) -> vec3<f32> {
    return color / (vec3<f32>(1.0, 1.0, 1.0) + color);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(input.world_normal);
    let v = normalize(frame.camera_pos.xyz - input.world_position);
    let has_texture = frame.surface_params.x;
    let is_emissive = frame.surface_params.y;
    let class_id = frame.surface_params.z;
    let cloud_phase = frame.surface_params.w;
    let limb_u = frame.shape_params.y;
    let exposure = frame.shape_params.z;
    let specular_strength = frame.shape_params.w;

    if (is_emissive > 0.5) {
        // Photosphere: linear limb darkening I(mu)/I(1) = 1 - u (1 - mu), modulated by
        // a granulation pattern whose contrast matches the observed few-percent level.
        let mu = max(dot(n, v), 0.0);
        let limb = 1.0 - limb_u * (1.0 - mu);
        let granulation = fbm(input.body_normal * 46.0 + vec3<f32>(cloud_phase, 0.0, -cloud_phase));
        let intensity = limb * (0.97 + 0.06 * granulation);
        let radiance = frame.base_color.rgb * intensity * exposure;
        return vec4<f32>(reinhard_tone_map(radiance), 1.0);
    }

    let l = normalize(frame.light_dir.xyz);
    let irradiance = frame.light_dir.w;
    let ndotl = max(dot(n, l), 0.0);
    let ndotv = max(dot(n, v), 0.0);
    let albedo = frame.base_color.w;

    var surface = frame.base_color.rgb;
    if (has_texture > 0.5) {
        surface = textureSample(surface_map, surface_sampler, input.uv).rgb;
    } else {
        // No published surface map: procedural relief, with contrast bounded so that the
        // disc-integrated brightness stays driven by the measured geometric albedo.
        let relief = fbm(input.body_normal * 6.4);
        let craters = fbm(input.body_normal * 21.0);
        surface = surface * (0.80 + 0.40 * relief) * (0.88 + 0.24 * craters);
    }

    if (class_id > 1.5) {
        // Gas and ice giants: zonal banding advected by the equatorial wind field.
        let latitude = clamp(input.body_normal.y, -1.0, 1.0);
        let bands = sin(latitude * 18.0 + 0.6 * sin(latitude * 7.0 + cloud_phase));
        surface = surface * (0.92 + 0.10 * bands);
    }

    // Lambertian reflectance scaled by the measured geometric albedo and by the solar
    // irradiance at the body distance (inverse-square law).
    let diffuse = surface * albedo * ndotl * irradiance;

    let h = normalize(l + v);
    let specular = pow(max(dot(n, h), 0.0), 48.0) * specular_strength * ndotl * irradiance;

    let optical_thickness = frame.atmosphere_color.w;
    let rim = pow(1.0 - ndotv, 3.2);
    let forward = pow(ndotl, 2.0);
    let atmosphere = frame.atmosphere_color.rgb
        * optical_thickness
        * irradiance
        * (0.55 * rim * ndotl + 0.25 * forward);

    let radiance = (diffuse + atmosphere + vec3<f32>(specular, specular, specular)) * exposure;
    return vec4<f32>(reinhard_tone_map(radiance), 1.0);
}
"#;

const ATMOSPHERE_SHADER_WGSL: &str = r#"
struct FrameUniform {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_pos: vec4<f32>,
    base_color: vec4<f32>,
    atmosphere_color: vec4<f32>,
    surface_params: vec4<f32>,
    shape_params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

@group(0) @binding(1)
var surface_map: texture_2d<f32>;

@group(0) @binding(2)
var surface_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) shell_altitude: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // The shell radius is the body radius plus the number of scale heights carried in
    // shape_params.x, so the limb thickness follows the real atmospheric extent.
    let shell_scale = 1.0 + frame.shape_params.x;
    let shell_position = input.position * shell_scale;
    let world = frame.model * vec4<f32>(shell_position, 1.0);
    out.world_position = world.xyz;
    out.world_normal = normalize((frame.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    out.shell_altitude = frame.shape_params.x;
    out.clip_position = frame.view_proj * world;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(input.world_normal);
    let l = normalize(frame.light_dir.xyz);
    let v = normalize(frame.camera_pos.xyz - input.world_position);
    let irradiance = frame.light_dir.w;
    let ndotl = max(dot(n, l), 0.0);
    let ndotv = max(dot(n, v), 0.0);

    // Slant optical depth through a plane-parallel shell grows as 1 / cos(view angle),
    // which concentrates the scattering signal on the limb.
    let slant = 1.0 / max(ndotv, 0.04);
    let optical_depth = frame.atmosphere_color.w * slant;
    let transmittance = exp(-optical_depth);
    let scattered = (1.0 - transmittance) * ndotl * irradiance;

    // Rayleigh phase function for unpolarised single scattering.
    let cos_theta = clamp(dot(-v, l), -1.0, 1.0);
    let rayleigh_phase = 0.75 * (1.0 + cos_theta * cos_theta) / 3.0;

    let alpha = clamp(scattered * (0.35 + 0.65 * rayleigh_phase), 0.0, 0.85);
    let color = frame.atmosphere_color.rgb * (0.6 + 0.4 * rayleigh_phase);
    return vec4<f32>(color, alpha);
}
"#;

const RING_SHADER_WGSL: &str = r#"
struct FrameUniform {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_pos: vec4<f32>,
    base_color: vec4<f32>,
    atmosphere_color: vec4<f32>,
    surface_params: vec4<f32>,
    shape_params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

@group(0) @binding(1)
var surface_map: texture_2d<f32>;

@group(0) @binding(2)
var surface_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) plane_normal: vec3<f32>,
    @location(2) radial_fraction: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Vertices carry a unit direction in the ring plane; the radial fraction in uv.x is
    // mapped between the inner radius ratio (shape_params.x) and the outer radius.
    let inner_ratio = frame.shape_params.x;
    let radius = mix(inner_ratio, 1.0, input.uv.x);
    let local = vec3<f32>(input.position.x * radius, 0.0, input.position.z * radius);
    let world = frame.model * vec4<f32>(local, 1.0);
    out.world_position = world.xyz;
    out.plane_normal = normalize((frame.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    out.radial_fraction = input.uv.x;
    out.clip_position = frame.view_proj * world;
    return out;
}

fn ring_optical_depth(radial_fraction: f32, base_depth: f32) -> f32 {
    // Radial structure of the Saturnian ring system: the Cassini division near the
    // outer third of the span is nearly transparent, the B ring is the densest part.
    let cassini = 1.0 - 0.92 * exp(-pow((radial_fraction - 0.62) / 0.035, 2.0));
    let b_ring = 1.0 + 0.85 * exp(-pow((radial_fraction - 0.45) / 0.14, 2.0));
    let inner_fade = smoothstep(0.0, 0.10, radial_fraction);
    let outer_fade = 1.0 - smoothstep(0.90, 1.0, radial_fraction);
    return base_depth * cassini * b_ring * inner_fade * outer_fade;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let l = normalize(frame.light_dir.xyz);
    let v = normalize(frame.camera_pos.xyz - input.world_position);
    let irradiance = frame.light_dir.w;
    let exposure = frame.shape_params.z;

    let normal = normalize(input.plane_normal);
    let mu_view = abs(dot(normal, v));
    let mu_sun = abs(dot(normal, l));
    if (mu_view < 1.0e-4 || mu_sun < 1.0e-4) {
        discard;
    }

    let tau = ring_optical_depth(input.radial_fraction, frame.atmosphere_color.w);
    // Single-scattering slab: reflected fraction of the incident flux.
    let reflected = frame.base_color.w * mu_sun * (1.0 - exp(-tau * (1.0 / mu_view + 1.0 / mu_sun)));
    let opacity = clamp(1.0 - exp(-tau / mu_view), 0.0, 1.0);

    let radiance = frame.base_color.rgb * reflected * irradiance * exposure;
    let color = radiance / (vec3<f32>(1.0, 1.0, 1.0) + radiance);
    return vec4<f32>(color, opacity);
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
    @location(1) color: vec3<f32>,
    @location(2) irradiance: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) irradiance: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = view_data.view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    out.irradiance = input.irradiance;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // The star colour comes from its effective temperature and its brightness from the
    // catalogue apparent magnitude, already converted to a relative irradiance.
    let radiance = input.color * input.irradiance;
    return vec4<f32>(radiance / (vec3<f32>(1.0, 1.0, 1.0) + radiance), 1.0);
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
    color: [f32; 3],
    irradiance: f32,
}

impl StarVertex {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32];
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
    normal_matrix: [[f32; 4]; 4],
    /// xyz: unit vector toward the Sun. w: solar irradiance relative to 1 au.
    light_dir: [f32; 4],
    camera_pos: [f32; 4],
    /// rgb: linear base colour. w: geometric albedo.
    base_color: [f32; 4],
    /// rgb: scattering colour. w: normal optical depth.
    atmosphere_color: [f32; 4],
    /// x: has_texture, y: is_emissive, z: class id, w: cloud drift phase (radians).
    surface_params: [f32; 4],
    /// x: shell thickness ratio, y: limb darkening u, z: exposure, w: specular strength.
    shape_params: [f32; 4],
}

impl FrameUniform {
    fn identity() -> Self {
        let identity = Mat4::IDENTITY.to_cols_array_2d();
        Self {
            view_proj: identity,
            model: identity,
            normal_matrix: identity,
            light_dir: [0.0, 0.0, 1.0, 1.0],
            camera_pos: [0.0, 0.0, 0.0, 0.0],
            base_color: [1.0, 1.0, 1.0, 0.3],
            atmosphere_color: [0.0, 0.0, 0.0, 0.0],
            surface_params: [0.0, 0.0, 0.0, 0.0],
            shape_params: [0.0, SOLAR_LIMB_DARKENING_U, 1.0, 0.0],
        }
    }
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
    catalog_path: Option<String>,
    catalog_limit: f64,
    filter: String,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<String>,
    width: u32,
    height: u32,
    fov_deg: f32,
    near_plane: f32,
    far_plane: f32,
    star_shell_radius: f32,
    exposure: f32,
    time_scale: f64,
    show_guides: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            catalog_path: None,
            catalog_limit: 12.0,
            filter: "R".to_string(),
            exposure_s: 60.0,
            repeats: 2,
            output_dir: None,
            width: 1600,
            height: 900,
            fov_deg: 56.0,
            near_plane: 0.000001,
            far_plane: 400.0,
            star_shell_radius: 45.0,
            exposure: 1.6,
            time_scale: 1.0,
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
        let proj = reverse_z_perspective_rh(
            self.fov_deg.to_radians() as f64,
            aspect,
            self.near_plane as f64,
            self.far_plane as f64,
        );
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

struct SurfaceTexture {
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
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

struct RingDraw {
    body_index: usize,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    outer_radius_scene: f32,
    inner_radius_ratio: f32,
    geometry: RingGeometry,
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
    ring_pipeline: wgpu::RenderPipeline,
    star_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    guide_pipeline: wgpu::RenderPipeline,

    frame_uniform: FrameUniform,
    body_frame_buffers: Vec<wgpu::Buffer>,
    planet_bind_groups: Vec<wgpu::BindGroup>,
    ring_draws: Vec<RingDraw>,
    _surface_textures: Vec<SurfaceTexture>,

    view_uniform: ViewUniform,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,

    planet_vertex_buffer: wgpu::Buffer,
    planet_index_buffer: wgpu::Buffer,
    planet_index_count: u32,

    ring_vertex_buffer: wgpu::Buffer,
    ring_index_buffer: wgpu::Buffer,
    ring_index_count: u32,

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
    show_rings: bool,
    show_atmospheres: bool,
    paused: bool,
    exposure: f32,
    time_scale: f64,
    simulated_julian_day: f64,
    elapsed_time_s: f32,
    solar_system_bodies: Vec<SolarSystemBody>,
    solar_system_positions: Vec<Vec3>,
    body_orientations: Vec<BodyOrientation>,
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

        let mut frame_uniform = FrameUniform::identity();
        frame_uniform.view_proj = view_proj.to_cols_array_2d();
        frame_uniform.camera_pos = [
            camera.eye().x as f32,
            camera.eye().y as f32,
            camera.eye().z as f32,
            0.0,
        ];
        frame_uniform.shape_params[2] = options.exposure;

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
                        view_dimension: wgpu::TextureViewDimension::D2,
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

        let solar_system_bodies = solar_system_catalogue();
        let mut body_frame_buffers = Vec::with_capacity(solar_system_bodies.len());
        let mut planet_bind_groups = Vec::with_capacity(solar_system_bodies.len());
        let mut ring_draws = Vec::new();
        let mut surface_textures = vec![create_neutral_surface_texture(&device, &queue)];
        let mut asset_texture_indices = HashMap::<&'static str, usize>::new();

        for (body_index, body) in solar_system_bodies.iter().enumerate() {
            let texture_index = match body.texture_asset {
                None => 0,
                Some(asset) => match asset_texture_indices.get(asset) {
                    Some(index) => *index,
                    None => {
                        let index = surface_textures.len();
                        surface_textures.push(load_surface_texture(&device, &queue, asset)?);
                        asset_texture_indices.insert(asset, index);
                        index
                    }
                },
            };

            let body_frame_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("solar_system_body_frame_uniform_buffer"),
                contents: bytemuck::bytes_of(&frame_uniform),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let surface = &surface_textures[texture_index];
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
                        resource: wgpu::BindingResource::TextureView(&surface.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&surface.sampler),
                    },
                ],
            }));
            body_frame_buffers.push(body_frame_buffer);

            if let Some(geometry) = body.ring {
                let ring_uniform_buffer =
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("ring_frame_uniform_buffer"),
                        contents: bytemuck::bytes_of(&frame_uniform),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    });
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ring_bind_group"),
                    layout: &frame_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: ring_uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&surface.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&surface.sampler),
                        },
                    ],
                });
                ring_draws.push(RingDraw {
                    body_index,
                    uniform_buffer: ring_uniform_buffer,
                    bind_group,
                    outer_radius_scene: (geometry.outer_radius_km / KM_PER_AU * AU_TO_SCENE_UNITS)
                        as f32,
                    inner_radius_ratio: (geometry.inner_radius_km / geometry.outer_radius_km) as f32,
                    geometry,
                });
            }
        }
        let solar_system_positions = vec![Vec3::ZERO; solar_system_bodies.len()];
        let body_orientations: Vec<BodyOrientation> = solar_system_bodies
            .iter()
            .map(|body| evaluate_body_orientation(&body.rotation, J2000_JULIAN_DAY))
            .collect();

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

        let ring_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ring_shader"),
            source: wgpu::ShaderSource::Wgsl(RING_SHADER_WGSL.into()),
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
                depth_compare: wgpu::CompareFunction::Greater,
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
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ring_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ring_pipeline"),
            layout: Some(&planet_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ring_shader,
                entry_point: "vs_main",
                buffers: &[MeshVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ring_shader,
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
                // Ring particles are visible from both faces of the ring plane.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Greater,
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
                depth_compare: wgpu::CompareFunction::GreaterEqual,
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
                depth_compare: wgpu::CompareFunction::GreaterEqual,
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
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let (planet_vertices, planet_indices) = generate_uv_sphere(96, 64, 1.0);
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

        let (ring_vertices, ring_indices) = generate_ring_annulus(256, 48);
        let ring_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ring_vertex_buffer"),
            contents: bytemuck::cast_slice(&ring_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ring_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ring_index_buffer"),
            contents: bytemuck::cast_slice(&ring_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let background_stars = match options.catalog_path.as_deref() {
            Some(path) => load_catalog_stars(path, options.catalog_limit, options.star_shell_radius)?,
            None => generate_background_stars(2400, 120.0),
        };
        let star_vertices: Vec<StarVertex> = background_stars
            .into_iter()
            .chain(scene.targets.iter().map(|target| StarVertex {
                position: target.position,
                color: [1.0; 3],
                irradiance: target.luminance,
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
            ring_pipeline,
            star_pipeline,
            sky_pipeline,
            guide_pipeline,
            frame_uniform,
            body_frame_buffers,
            planet_bind_groups,
            ring_draws,
            _surface_textures: surface_textures,
            view_uniform,
            view_buffer,
            view_bind_group,
            planet_vertex_buffer,
            planet_index_buffer,
            planet_index_count: planet_indices.len() as u32,
            ring_vertex_buffer,
            ring_index_buffer,
            ring_index_count: ring_indices.len() as u32,
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
            show_rings: true,
            show_atmospheres: true,
            paused: false,
            exposure: options.exposure,
            time_scale: options.time_scale,
            simulated_julian_day: current_julian_day(),
            elapsed_time_s: 0.0,
            solar_system_bodies,
            solar_system_positions,
            body_orientations,
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
                        let dx = position.x - last_x;
                        let dy = position.y - last_y;
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
                                self.exposure = (self.exposure * 1.25).min(64.0);
                            }
                            KeyCode::Minus | KeyCode::NumpadSubtract => {
                                self.exposure = (self.exposure / 1.25).max(0.02);
                            }
                            KeyCode::BracketRight => {
                                self.time_scale = (self.time_scale * 4.0).min(4.0e7);
                            }
                            KeyCode::BracketLeft => {
                                self.time_scale = (self.time_scale / 4.0).max(1.0);
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
            self.elapsed_time_s += dt_s as f32;
            self.simulated_julian_day += dt_s * self.time_scale / 86_400.0;
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
        self.frame_uniform.shape_params[2] = self.exposure;

        let julian_day = self.simulated_julian_day;
        self.solar_system_positions =
            compute_scene_positions(&self.solar_system_bodies, julian_day);
        self.body_orientations = self
            .solar_system_bodies
            .iter()
            .map(|body| evaluate_body_orientation(&body.rotation, julian_day))
            .collect();

        if let Some(index) = self.selected_body_index {
            if let Some(position) = self.solar_system_positions.get(index) {
                self.camera.target = DVec3::new(position.x as f64, position.y as f64, position.z as f64);
            }
        }

        if self.title_last_update.elapsed() >= Duration::from_millis(220) {
            self.window.set_title(&build_title(self));
            self.title_last_update = Instant::now();
        }

        self.operations.chart_refresh_timer_s -= dt_s;
        if self.operations.chart.is_none() || self.operations.chart_refresh_timer_s <= 0.0 {
            self.operations.chart_refresh_timer_s = SKY_CHART_REFRESH_S;
            if self.operations.follow_clock || self.operations.chart.is_none() {
                self.rebuild_sky_chart();
            }
        }

        if self.science.auto_refresh {
            self.science.refresh_timer_s -= dt_s;
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
            let positions = compute_scene_positions(&self.solar_system_bodies, julian_day);
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
                        ui.checkbox(&mut self.paused, "figer le temps simule");
                        ui.checkbox(&mut self.show_guides, "guides 3D");
                        ui.checkbox(&mut self.show_atmospheres, "enveloppes atmospheriques");
                        ui.checkbox(&mut self.show_rings, "systemes d'anneaux");
                        ui.add(
                            egui::Slider::new(&mut self.exposure, 0.02..=64.0)
                                .logarithmic(true)
                                .text("exposition d'affichage"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.time_scale, 1.0..=4.0e7)
                                .logarithmic(true)
                                .text("echelle de temps (x)"),
                        );
                        ui.label(format!(
                            "date simulee: JJ {:.5}",
                            self.simulated_julian_day
                        ));
                        if ui.button("Resynchroniser sur l'heure reelle").clicked() {
                            self.simulated_julian_day = current_julian_day();
                        }

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
                            let body = self.solar_system_bodies[index];
                            let orientation = self.body_orientations[index];
                            let position = self.solar_system_positions[index];
                            ui.label(format!("selection: {} ({})", body.name, body.class.label()));
                            ui.label(format!(
                                "rayon equatorial {:.1} km, polaire {:.1} km",
                                body.equatorial_radius_km,
                                body.polar_radius_km()
                            ));
                            ui.label(format!("aplatissement {:.5}", body.flattening));
                            ui.label(format!("albedo geometrique {:.3}", body.geometric_albedo));
                            ui.label(format!(
                                "periode siderale {}",
                                format_rotation_period(body.rotation.sidereal_period_days())
                            ));
                            ui.label(format!(
                                "pole IAU: AD {:.3} deg, DEC {:.3} deg",
                                orientation.pole_ra_deg, orientation.pole_dec_deg
                            ));
                            ui.label(format!(
                                "meridien origine W = {:.3} deg",
                                orientation.prime_meridian_deg
                            ));
                            if !orientation.pole_constrained {
                                ui.colored_label(
                                    Color32::from_rgb(240, 180, 90),
                                    "axe de rotation non contraint par l'IAU",
                                );
                            }
                            if body.texture_asset.is_none() {
                                ui.colored_label(
                                    Color32::from_rgb(240, 180, 90),
                                    "aucune carte de surface publiee: relief procedural",
                                );
                            }
                            if let Some(ring) = body.ring {
                                ui.label(format!(
                                    "anneaux {:.0} - {:.0} km, tau {:.3}",
                                    ring.inner_radius_km,
                                    ring.outer_radius_km,
                                    ring.normal_optical_depth
                                ));
                            }
                            let distance_au = position.length() as f64 / AU_TO_SCENE_UNITS;
                            ui.label(format!("distance au Soleil {distance_au:.5} ua"));

                            match body.orbit {
                                OrbitModel::Fixed => {
                                    ui.label("origine du repere heliocentrique");
                                }
                                OrbitModel::Heliocentric(orbit) => {
                                    ui.label(format!(
                                        "orbite: a = {:.6} ua, e = {:.5}, i = {:.3} deg",
                                        orbit.semi_major_axis_au,
                                        orbit.eccentricity,
                                        orbit.inclination_deg
                                    ));
                                    ui.label(format!(
                                        "periode orbitale {:.3} jours",
                                        orbit.orbital_period_days
                                    ));
                                }
                                OrbitModel::Satellite(orbit) => {
                                    ui.label(format!(
                                        "satellite de {}: a = {:.0} km, e = {:.5}, i = {:.3} deg",
                                        orbit.parent,
                                        orbit.semi_major_axis_km,
                                        orbit.eccentricity,
                                        orbit.inclination_deg
                                    ));
                                    ui.label(format!(
                                        "periode {:.6} jours, plan de reference {}",
                                        orbit.sidereal_period_days.abs(),
                                        match orbit.reference_plane {
                                            ReferencePlane::Ecliptic => "ecliptique",
                                            ReferencePlane::ParentEquator => "equateur du parent",
                                        }
                                    ));
                                    if !orbit.epoch_phase_constrained {
                                        ui.colored_label(
                                            Color32::from_rgb(240, 180, 90),
                                            "phase orbitale a l'epoque non sourcee",
                                        );
                                    }
                                }
                            }
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

        let earth_radius = self.solar_system_bodies[earth_index].equatorial_radius_au();
        self.camera.distance = (earth_radius * 6.0).max(0.00005);
        self.camera.near_plane = (earth_radius * 0.02).max(1e-8) as f32;
        self.selected_body_index = Some(earth_index);
    }

    /// Body-fixed frame of a catalogued body, built from its IAU pole and prime meridian.
    /// Returns the model matrix and the matching normal matrix, the latter accounting for
    /// the polar flattening applied along the spin axis.
    fn body_model_matrices(
        body: &SolarSystemBody,
        orientation: &BodyOrientation,
        position: Vec3,
        radius_scale: f32,
        apply_flattening: bool,
    ) -> (Mat4, Mat4) {
        let pole_ecliptic = pole_direction_ecliptic(orientation);
        // Scene axes: X and Z span the ecliptic plane, Y is the ecliptic normal.
        let pole = Vec3::new(
            pole_ecliptic[0] as f32,
            pole_ecliptic[2] as f32,
            pole_ecliptic[1] as f32,
        )
        .normalize_or_zero();

        // The IAU node of the body equator lies at right ascension alpha0 + 90 degrees on
        // the ICRF equator; the prime meridian angle W is measured from it.
        let node_direction = equatorial_to_scene_direction(orientation.pole_ra_deg + 90.0, 0.0);
        let node = Vec3::new(
            node_direction.x as f32,
            node_direction.y as f32,
            node_direction.z as f32,
        );
        let node = (node - pole * node.dot(pole)).normalize_or_zero();

        let prime_meridian = (orientation.prime_meridian_deg as f32).to_radians();
        let x_axis = node * prime_meridian.cos() + pole.cross(node) * prime_meridian.sin();
        let y_axis = pole;
        let z_axis = x_axis.cross(y_axis);

        let basis = Mat4::from_cols(
            x_axis.extend(0.0),
            y_axis.extend(0.0),
            z_axis.extend(0.0),
            glam::Vec4::W,
        );

        let equatorial = (body.equatorial_radius_au() * AU_TO_SCENE_UNITS) as f32 * radius_scale;
        let polar = if apply_flattening {
            equatorial * (1.0 - body.flattening as f32)
        } else {
            equatorial
        };

        let model = Mat4::from_translation(position)
            * basis
            * Mat4::from_scale(Vec3::new(equatorial, polar, equatorial));
        // Inverse transpose of an orthonormal basis times a diagonal scale.
        let normal_matrix = basis
            * Mat4::from_scale(Vec3::new(
                1.0 / equatorial,
                1.0 / polar.max(f32::MIN_POSITIVE),
                1.0 / equatorial,
            ));

        (model, normal_matrix)
    }

    fn body_frame_uniform(
        &self,
        body: &SolarSystemBody,
        orientation: &BodyOrientation,
        position: Vec3,
    ) -> FrameUniform {
        let (model, normal_matrix) =
            Self::body_model_matrices(body, orientation, position, 1.0, true);

        let distance_au = (position.length() as f64 / AU_TO_SCENE_UNITS).max(1.0e-6);
        let is_emissive = matches!(body.class, BodyClass::Star);
        let irradiance = if is_emissive {
            1.0
        } else {
            (1.0 / (distance_au * distance_au)) as f32
        };
        let light_dir = if position.length_squared() > f32::EPSILON {
            (-position).normalize()
        } else {
            Vec3::Y
        };

        // Cloud advection phase: zonal wind drift accumulated since the epoch.
        let cloud_phase = ((body.zonal_wind_drift_deg_per_day()
            * (self.simulated_julian_day - J2000_JULIAN_DAY))
            .rem_euclid(360.0)) as f32;

        let shell_thickness = if self.show_atmospheres && body.has_atmosphere() {
            // Five scale heights capture the bulk of the scattering column.
            (5.0 * body.atmosphere_scale_height_km / body.equatorial_radius_km).min(0.5) as f32
        } else {
            0.0
        };

        let mut frame_uniform = self.frame_uniform;
        frame_uniform.model = model.to_cols_array_2d();
        frame_uniform.normal_matrix = normal_matrix.to_cols_array_2d();
        frame_uniform.light_dir = [light_dir.x, light_dir.y, light_dir.z, irradiance];
        frame_uniform.base_color = [
            body.base_color[0],
            body.base_color[1],
            body.base_color[2],
            body.geometric_albedo as f32,
        ];
        frame_uniform.atmosphere_color = [
            body.atmosphere_color[0],
            body.atmosphere_color[1],
            body.atmosphere_color[2],
            shell_thickness * 2.0,
        ];
        frame_uniform.surface_params = [
            if body.texture_asset.is_some() { 1.0 } else { 0.0 },
            if is_emissive { 1.0 } else { 0.0 },
            body_class_id(body.class),
            cloud_phase.to_radians(),
        ];
        frame_uniform.shape_params = [
            shell_thickness,
            SOLAR_LIMB_DARKENING_U,
            self.exposure,
            body_specular_strength(body.class),
        ];
        frame_uniform
    }

    fn ring_frame_uniform(&self, ring: &RingDraw) -> FrameUniform {
        let body = &self.solar_system_bodies[ring.body_index];
        let orientation = &self.body_orientations[ring.body_index];
        let position = self.solar_system_positions[ring.body_index];

        // Rings share the equatorial plane of their planet; the mesh is scaled to the
        // outer ring radius instead of the planet radius.
        let radius_scale =
            ring.outer_radius_scene / (body.equatorial_radius_au() * AU_TO_SCENE_UNITS) as f32;
        let (model, normal_matrix) =
            Self::body_model_matrices(body, orientation, position, radius_scale, false);

        let distance_au = (position.length() as f64 / AU_TO_SCENE_UNITS).max(1.0e-6);
        let irradiance = (1.0 / (distance_au * distance_au)) as f32;
        let light_dir = if position.length_squared() > f32::EPSILON {
            (-position).normalize()
        } else {
            Vec3::Y
        };

        let mut frame_uniform = self.frame_uniform;
        frame_uniform.model = model.to_cols_array_2d();
        frame_uniform.normal_matrix = normal_matrix.to_cols_array_2d();
        frame_uniform.light_dir = [light_dir.x, light_dir.y, light_dir.z, irradiance];
        frame_uniform.base_color = [0.86, 0.80, 0.68, RING_PARTICLE_ALBEDO];
        frame_uniform.atmosphere_color = [0.0, 0.0, 0.0, ring.geometry.normal_optical_depth as f32];
        frame_uniform.surface_params = [0.0, 0.0, 0.0, 0.0];
        frame_uniform.shape_params = [ring.inner_radius_ratio, 0.0, self.exposure, 0.0];
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
                        load: wgpu::LoadOp::Clear(0.0),
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
                    let body = self.solar_system_bodies[body_index];
                    let orientation = self.body_orientations[body_index];
                    let position = self.solar_system_positions[body_index];
                    let frame_uniform = self.body_frame_uniform(&body, &orientation, position);
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

                    if self.show_atmospheres && body.has_atmosphere() && !matches!(body.class, BodyClass::Star) {
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

                if self.show_rings {
                    for ring_index in 0..self.ring_draws.len() {
                        let frame_uniform = self.ring_frame_uniform(&self.ring_draws[ring_index]);
                        self.queue.write_buffer(
                            &self.ring_draws[ring_index].uniform_buffer,
                            0,
                            bytemuck::bytes_of(&frame_uniform),
                        );

                        pass.set_pipeline(&self.ring_pipeline);
                        pass.set_bind_group(0, &self.ring_draws[ring_index].bind_group, &[]);
                        pass.set_vertex_buffer(0, self.ring_vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            self.ring_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..self.ring_index_count, 0, 0..1);
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

fn load_surface_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    asset_name: &str,
) -> Result<SurfaceTexture, String> {
    let base_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("textures")
        .join(asset_name);
    let lod_names = ["lod0.ppm", "lod1.ppm", "lod2.ppm", "lod3.ppm"];
    let lods: Vec<PpmImage> = lod_names
        .iter()
        .map(|name| load_ppm_rgb(&base_dir.join(name)))
        .collect::<Result<_, _>>()?;

    let texture = create_mipmapped_surface_texture(
        device,
        queue,
        &format!("{asset_name}_surface_map"),
        &lods,
    )?;
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("surface_map_sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    Ok(SurfaceTexture { view, sampler })
}

/// Single-texel neutral map bound to bodies that have no published surface map, so the
/// shader can keep one bind group layout while flagging the absence of real imagery.
fn create_neutral_surface_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> SurfaceTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("neutral_surface_map"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
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
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[255u8, 255, 255, 255],
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("neutral_surface_sampler"),
        ..Default::default()
    });

    SurfaceTexture { view, sampler }
}

fn create_mipmapped_surface_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    lods: &[PpmImage],
) -> Result<wgpu::Texture, String> {
    let Some(base_lod) = lods.first() else {
        return Err(format!("no LOD image supplied for '{label}'"));
    };

    for (index, lod) in lods.iter().enumerate() {
        if lod.rgb.len() != (lod.width as usize * lod.height as usize * 3) {
            return Err(format!("invalid RGB data length for '{label}' LOD {index}"));
        }
        if index > 0 {
            let previous = &lods[index - 1];
            if lod.width != previous.width / 2 || lod.height != previous.height / 2 {
                return Err(format!("'{label}' LOD {index} is not half of the previous level"));
            }
        }
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: base_lod.width,
            height: base_lod.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: lods.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (mip_level, lod) in lods.iter().enumerate() {
        let rgba = expand_rgb_to_rgba(lod);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: mip_level as u32,
                origin: wgpu::Origin3d::ZERO,
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

    Ok(texture)
}

fn expand_rgb_to_rgba(lod: &PpmImage) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(lod.width as usize * lod.height as usize * 4);
    for pixel in lod.rgb.as_chunks::<3>().0 {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    rgba
}

fn load_ppm_rgb(path: &Path) -> Result<PpmImage, String> {
    let display = path.display().to_string();
    read_ppm_file(&display)
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

/// Human readable sidereal rotation period, with the retrograde sense made explicit.
fn format_rotation_period(period_days: f64) -> String {
    if !period_days.is_finite() {
        return "non definie".to_string();
    }

    let sense = if period_days < 0.0 {
        " (retrograde)"
    } else {
        ""
    };
    let magnitude_hours = period_days.abs() * 24.0;
    if magnitude_hours < 48.0 {
        let hours = magnitude_hours.floor();
        let minutes_total = (magnitude_hours - hours) * 60.0;
        let minutes = minutes_total.floor();
        let seconds = (minutes_total - minutes) * 60.0;
        format!("{:.0} h {:02.0} min {:04.1} s{sense}", hours, minutes, seconds)
    } else {
        format!("{:.4} jours{sense}", period_days.abs())
    }
}

fn body_class_id(class: BodyClass) -> f32 {    match class {
        BodyClass::Star => 0.0,
        BodyClass::TerrestrialPlanet => 1.0,
        BodyClass::GasGiant => 2.0,
        BodyClass::IceGiant => 3.0,
        BodyClass::Satellite => 1.0,
        BodyClass::MinorPlanet => 1.0,
    }
}

/// Specular lobe strength: only bodies with a liquid or icy surface show a glint.
fn body_specular_strength(class: BodyClass) -> f32 {
    match class {
        BodyClass::TerrestrialPlanet => 0.05,
        BodyClass::Satellite => 0.02,
        _ => 0.0,
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

/// Right-handed perspective projection mapping the near plane to depth 1 and the far
/// plane to depth 0. Reverse-Z keeps float depth precision usable over the 1e-6 au to
/// several-hundred-au range spanned by the solar system scene.
fn reverse_z_perspective_rh(fov_y_rad: f64, aspect: f64, near: f64, far: f64) -> DMat4 {
    let focal = 1.0 / (fov_y_rad * 0.5).tan();
    let depth_span = far - near;
    DMat4::from_cols(
        DVec4::new(focal / aspect, 0.0, 0.0, 0.0),
        DVec4::new(0.0, focal, 0.0, 0.0),
        DVec4::new(0.0, 0.0, near / depth_span, -1.0),
        DVec4::new(0.0, 0.0, far * near / depth_span, 0.0),
    )
}

fn equatorial_to_scene_direction(ra_deg: f64, dec_deg: f64) -> DVec3 {    let ra_rad = ra_deg.to_radians();
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
    println!("space           : freeze/resume simulated time");
    println!("G               : toggle target guide lines");
    println!("[ and ]         : decrease/increase the simulated time acceleration");
    println!("- and +         : decrease/increase the display exposure");
    println!("R               : reset camera");
    println!("H               : print controls");
}

fn build_title(state: &RenderState) -> String {
    format!(
        "{} | campaign={} phase={} ready={} | targets={} | dist={:.6}au fov={:.1}deg | JJ={:.5} x{:.0} | exposition={:.2} | guides={} fige={}",
        state.runtime.app_name,
        state.runtime.config_name,
        state.runtime.phase,
        state.runtime.ready,
        state.target_count,
        state.camera.distance,
        state.camera.fov_deg,
        state.simulated_julian_day,
        state.time_scale,
        state.exposure,
        state.show_guides,
        state.paused,
    )
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--catalog" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --catalog");
                    std::process::exit(1);
                };
                options.catalog_path = Some(value.clone());
                i += 2;
            }
            "--catalog-limit" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --catalog-limit");
                    std::process::exit(1);
                };
                options.catalog_limit = value.parse::<f64>().unwrap_or_else(|_| {
                    eprintln!("invalid --catalog-limit value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
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
            "--time-scale" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --time-scale");
                    std::process::exit(1);
                };
                options.time_scale = value.parse::<f64>().unwrap_or_else(|_| {
                    eprintln!("invalid --time-scale value: '{value}'");
                    std::process::exit(1);
                });
                i += 2;
            }
            "--display-exposure" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("missing value after --display-exposure");
                    std::process::exit(1);
                };
                options.exposure = value.parse::<f32>().unwrap_or_else(|_| {
                    eprintln!("invalid --display-exposure value: '{value}'");
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
    if options.star_shell_radius <= 1.0 {
        eprintln!("--star-radius must be larger than 1 au");
        std::process::exit(1);
    }
    if options.catalog_limit.is_nan() {
        eprintln!("--catalog-limit must be finite");
        std::process::exit(1);
    }
    if options.exposure <= 0.0 {
        eprintln!("--display-exposure must be strictly positive");
        std::process::exit(1);
    }
    if options.time_scale < 1.0 {
        eprintln!("--time-scale must be at least 1 (real time)");
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
    println!("  --near <value>             near clipping plane in au (default: 0.000001)");
    println!("  --far <value>              far clipping plane in au (default: 400)");
    println!();
    println!("3D scene:");
    println!("  --catalog <path>           stellar catalogue (stars.dat, Hipparcos or CSV/TSV)");
    println!("  --catalog-limit <mag>      faintest apparent magnitude to render (default: 12)");
    println!("  --star-radius <value>      radius of the target star shell in au (default: 45)");
    println!("  --display-exposure <value> display exposure of the tone mapper (default: 1.6)");
    println!("  --time-scale <factor>      simulated time acceleration, 1 = real time");
    println!("  --no-guides                disable guide lines from origin to targets");
    println!();
    println!("Interactive controls:");
    println!("  mouse drag / WASD          orbit camera");
    println!("  wheel, Q, E                zoom");
    println!("  space                      freeze/resume simulated time");
    println!("  [ and ]                    decrease/increase the time acceleration");
    println!("  - and +                    decrease/increase the display exposure");
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

fn selected_body_camera_distance(body: &SolarSystemBody) -> f64 {
    (body.equatorial_radius_au() * AU_TO_SCENE_UNITS * 8.0).max(0.00005)
}

fn selected_body_near_plane(body: &SolarSystemBody) -> f32 {
    ((body.equatorial_radius_au() * AU_TO_SCENE_UNITS * 0.02) as f32).max(0.00000001)
}

/// Heliocentric ecliptic positions converted to scene axes, where the scene Y axis is
/// the ecliptic north pole.
fn compute_scene_positions(bodies: &[SolarSystemBody], julian_day: f64) -> Vec<Vec3> {
    compute_catalogue_positions_au(bodies, julian_day)
        .into_iter()
        .map(|position| {
            Vec3::new(
                (position[0] * AU_TO_SCENE_UNITS) as f32,
                (position[2] * AU_TO_SCENE_UNITS) as f32,
                (position[1] * AU_TO_SCENE_UNITS) as f32,
            )
        })
        .collect()
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
    let ra = target.ra_deg.to_radians();
    let dec = target.dec_deg.to_radians();

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
            color: [1.0; 3],
            irradiance: luminance,
        });
    }

    stars
}

/// Flat annulus lying in the body equatorial plane. Vertices carry a unit direction and
/// the radial fraction in `uv.x`, so one mesh serves every ring system.
fn generate_ring_annulus(angular_segments: u32, radial_segments: u32) -> (Vec<MeshVertex>, Vec<u32>) {
    let angular_segments = angular_segments.max(8);
    let radial_segments = radial_segments.max(1);
    let mut vertices = Vec::with_capacity(((angular_segments + 1) * (radial_segments + 1)) as usize);
    let mut indices = Vec::with_capacity((angular_segments * radial_segments * 6) as usize);

    for radial in 0..=radial_segments {
        let radial_fraction = radial as f32 / radial_segments as f32;
        for angular in 0..=angular_segments {
            let angle_fraction = angular as f32 / angular_segments as f32;
            let angle = angle_fraction * 2.0 * PI;
            vertices.push(MeshVertex {
                position: [angle.cos(), 0.0, angle.sin()],
                normal: [0.0, 1.0, 0.0],
                uv: [radial_fraction, angle_fraction],
            });
        }
    }

    let ring_stride = angular_segments + 1;
    for radial in 0..radial_segments {
        for angular in 0..angular_segments {
            let base = radial * ring_stride + angular;
            let next_ring = base + ring_stride;
            indices.extend_from_slice(&[base, next_ring, base + 1]);
            indices.extend_from_slice(&[base + 1, next_ring, next_ring + 1]);
        }
    }

    (vertices, indices)
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
        body_class_id, body_specular_strength, build_frame_color_image, compute_scene_positions,
        equatorial_to_scene_direction, expand_rgb_to_rgba, format_airmass, format_declination,
        format_hours_hms, format_right_ascension, format_rotation_period,
        generate_background_stars, generate_ring_annulus, generate_sky_dome, generate_uv_sphere,
        parse_cli_args, percentile_value, priority_to_luminance, ra_dec_to_cartesian,
        ra_dec_to_cartesian_f64, reverse_z_perspective_rh, sky_background_color,
        stacking_method_name, workspace_capabilities, Workspace, J2000_JULIAN_DAY,
    };
    use glam::DVec4;
    use observatory_core::{
        ecliptic_vector_to_equatorial, solar_system_catalogue, BodyClass, CampaignTargetConfig,
        PpmImage, StackingMethod,
    };

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
            "--star-radius".to_string(),
            "60".to_string(),
            "--time-scale".to_string(),
            "3600".to_string(),
            "--display-exposure".to_string(),
            "0.8".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.config_path, Some("sample.cfg".to_string()));
        assert_eq!(parsed.width, 1920);
        assert_eq!(parsed.height, 1080);
        assert!((parsed.fov_deg - 62.0).abs() < 1e-6);
        assert!((parsed.time_scale - 3600.0).abs() < 1e-9);
        assert!((parsed.star_shell_radius - 60.0).abs() < 1e-6);
        assert!((parsed.exposure - 0.8).abs() < 1e-6);
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
            assert!((star.irradiance >= 0.15) && (star.irradiance <= 1.0));
        }
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
    fn surface_lod_images_expand_to_opaque_rgba() {
        let lod = PpmImage {
            width: 2,
            height: 1,
            rgb: vec![0x0a, 0x20, 0x30, 0x40, 0x50, 0x60],
        };
        let rgba = expand_rgb_to_rgba(&lod);
        assert_eq!(
            rgba,
            vec![0x0a, 0x20, 0x30, 0xff, 0x40, 0x50, 0x60, 0xff]
        );
    }

    #[test]
    fn scene_positions_place_bodies_on_the_ecliptic_scene_axes() {
        let catalogue = solar_system_catalogue();
        let positions = compute_scene_positions(&catalogue, J2000_JULIAN_DAY);
        assert_eq!(positions.len(), catalogue.len());
        assert!(positions.iter().all(|position| position.is_finite()));

        let sun_index = catalogue.iter().position(|b| b.name == "Sun").unwrap();
        assert_eq!(positions[sun_index], glam::Vec3::ZERO);

        // Earth orbits close to the ecliptic plane, so its scene Y component is tiny.
        let earth_index = catalogue.iter().position(|b| b.name == "Earth").unwrap();
        let earth = positions[earth_index];
        assert!(earth.y.abs() < 1.0e-3, "earth y = {}", earth.y);
        assert!((earth.length() - 0.983).abs() < 0.02);
    }

    #[test]
    fn ring_annulus_has_consistent_topology_and_upward_normals() {
        let (vertices, indices) = generate_ring_annulus(16, 4);
        assert_eq!(vertices.len(), (16 + 1) * (4 + 1));
        assert_eq!(indices.len(), 16 * 4 * 6);
        assert!(indices.iter().all(|index| (*index as usize) < vertices.len()));
        for vertex in &vertices {
            assert_eq!(vertex.normal, [0.0, 1.0, 0.0]);
            assert!(vertex.position[1].abs() < 1.0e-9);
            let radius = vertex.position[0].hypot(vertex.position[2]);
            assert!((radius - 1.0).abs() < 1.0e-6);
            assert!((0.0..=1.0).contains(&vertex.uv[0]));
        }
    }

    #[test]
    fn only_the_star_is_emissive_and_giants_are_tagged_apart() {
        assert_eq!(body_class_id(BodyClass::Star), 0.0);
        assert_eq!(body_class_id(BodyClass::TerrestrialPlanet), 1.0);
        assert_eq!(body_class_id(BodyClass::GasGiant), 2.0);
        assert_eq!(body_class_id(BodyClass::IceGiant), 3.0);
        assert_eq!(body_specular_strength(BodyClass::GasGiant), 0.0);
        assert!(body_specular_strength(BodyClass::TerrestrialPlanet) > 0.0);
    }

    #[test]
    fn rotation_periods_are_formatted_with_their_sense() {
        let earth = format_rotation_period(0.99726968);
        assert!(earth.starts_with("23 h 56 min"), "{earth}");
        let venus = format_rotation_period(-243.025);
        assert!(venus.contains("retrograde"), "{venus}");
        assert_eq!(format_rotation_period(f64::INFINITY), "non definie");
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

    #[test]
    fn reverse_z_projection_maps_near_to_one_and_far_to_zero() {
        let near = 1.0e-6;
        let far = 400.0;
        let projection = reverse_z_perspective_rh(56.0_f64.to_radians(), 16.0 / 9.0, near, far);

        let at_near = projection * DVec4::new(0.0, 0.0, -near, 1.0);
        let at_far = projection * DVec4::new(0.0, 0.0, -far, 1.0);
        assert!((at_near.z / at_near.w - 1.0).abs() < 1e-9);
        assert!((at_far.z / at_far.w).abs() < 1e-9);
    }

    #[test]
    fn reverse_z_depth_decreases_with_distance() {
        let projection = reverse_z_perspective_rh(56.0_f64.to_radians(), 1.6, 1.0e-6, 400.0);
        let mut previous = f64::INFINITY;
        for distance in [1.0e-5, 1.0e-3, 0.1, 1.0, 30.0, 399.0] {
            let clip = projection * DVec4::new(0.0, 0.0, -distance, 1.0);
            let depth = clip.z / clip.w;
            assert!(depth > 0.0 && depth < 1.0);
            assert!(depth < previous);
            previous = depth;
        }
    }
}

fn load_catalog_stars(path: &str, limiting_magnitude: f64, radius: f32) -> Result<Vec<StarVertex>, String> {
    let catalog = observatory_core::load_star_catalog(path)?;
    let stars = catalog.brighter_than(limiting_magnitude);
    let mut vertices = Vec::with_capacity(stars.len());

    for star in stars {
        let position = equatorial_to_scene_direction(
            star.right_ascension_deg,
            star.declination_deg,
        ) * radius as f64;
        let color = observatory_core::blackbody_srgb(
            observatory_core::effective_temperature_from_b_v(star.color_index_b_v),
        );
        let irradiance = 10.0_f64.powf(-0.4 * (star.apparent_magnitude + 1.0))
            .clamp(0.02, 1.0) as f32;
        vertices.push(StarVertex {
            position: [position.x as f32, position.y as f32, position.z as f32],
            color,
            irradiance,
        });
    }

    if vertices.is_empty() {
        return Err(format!(
            "catalogue '{path}' contains no stars at magnitude <= {limiting_magnitude}"
        ));
    }

    Ok(vertices)
}