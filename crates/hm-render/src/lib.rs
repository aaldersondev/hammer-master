//! Rendu hors écran d'une scène : une image en couleur et une image d'identifiants d'objets, sans fenêtre.
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use hm_assets::{PixelFormat, Tool};
use hm_scene::Scene;
use wgpu::util::DeviceExt;

pub mod ciel;
pub mod eclairage;
pub mod lignes;
pub mod planche;
pub mod vivant;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

const F_ALPHATEST: u32 = 1;
const F_SELFILLUM: u32 = 2;
const F_UNLIT: u32 = 4;
const F_TEX_ALPHA: u32 = 8;
const F_FLAT: u32 = 16;
/// WorldVertexTransition : mélange vers la seconde texture selon l'alpha du sommet.
const F_BLEND: u32 = 32;
/// Masque du reflet : alpha de la texture de base ; alpha de la seconde texture ; couleur de la seconde texture.
const F_ENV_ALPHA_BASE: u32 = 64;
const F_ENV_ALPHA_2: u32 = 128;
const F_ENV_COULEUR_2: u32 = 256;

// ------------------------------------------------------------------ périphérique

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Adaptateur retenu, pour l'état de session.
    pub info: String,
    pub bc: bool,
}

impl Gpu {
    /// Un périphérique déjà créé (celui de la fenêtre).
    pub fn depuis(device: wgpu::Device, queue: wgpu::Queue, info: String) -> Self {
        let bc = device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        Self { device, queue, info, bc }
    }

    /// Choisit un adaptateur sans fenêtre : carte graphique en DX12, puis Vulkan, puis OpenGL, puis l'adaptateur
    /// logiciel de Windows. `HM_ADAPTATEUR=logiciel` force ce dernier.
    pub fn new() -> Result<Self> {
        pollster::block_on(Self::new_async())
    }

    async fn new_async() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let mut adapters = instance.enumerate_adapters(wgpu::Backends::all()).await;
        let logiciel = std::env::var("HM_ADAPTATEUR").map_or(false, |v| v.eq_ignore_ascii_case("logiciel"));
        let rank = |a: &wgpu::Adapter| {
            let i = a.get_info();
            let cpu = i.device_type == wgpu::DeviceType::Cpu;
            let backend = match i.backend {
                wgpu::Backend::Dx12 => 0,
                wgpu::Backend::Vulkan => 1,
                wgpu::Backend::Metal => 1,
                wgpu::Backend::Gl => 2,
                _ => 3,
            };
            let kind = match i.device_type {
                wgpu::DeviceType::DiscreteGpu => 0,
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::Cpu => 3,
                _ => 2,
            };
            (cpu != logiciel, kind, backend)
        };
        adapters.sort_by_key(rank);
        let mut errors = Vec::new();
        for a in adapters {
            let i = a.get_info();
            let bc = a.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
            let desc = wgpu::DeviceDescriptor {
                label: Some("hammer-master"),
                required_features: a.features() & wgpu::Features::TEXTURE_COMPRESSION_BC,
                required_limits: a.limits(),
                ..Default::default()
            };
            match a.request_device(&desc).await {
                Ok((device, queue)) => {
                    return Ok(Self { device, queue, info: format!("{} ({:?}, {:?})", i.name, i.backend, i.device_type), bc });
                }
                Err(e) => errors.push(format!("{} : {e}", i.name)),
            }
        }
        Err(anyhow!("aucun adaptateur graphique utilisable : {}", errors.join(" ; ")))
    }
}

// ------------------------------------------------------------------ caméra

#[derive(Clone, Copy, Debug)]
pub enum Projection {
    /// Champ de vision vertical, en degrés.
    Perspective { fov_y: f32 },
    /// Demi-hauteur de la vue, en unités de map.
    Ortho { half_height: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub dir: Vec3,
    pub up: Vec3,
    pub proj: Projection,
    pub near: f32,
    pub far: f32,
}

/// Champ vertical correspondant au `fov` du jeu (horizontal pour un écran 4:3, étendu sur les écrans larges).
pub fn fov_y_from_game(fov: f32) -> f32 {
    2.0 * ((fov.to_radians() / 2.0).tan() * 0.75).atan().to_degrees()
}

impl Camera {
    pub fn look_at(eye: Vec3, target: Vec3, fov_y: f32) -> Self {
        let dir = (target - eye).normalize_or(Vec3::X);
        let up = if dir.z.abs() > 0.999 { Vec3::Y } else { Vec3::Z };
        Self { eye, dir, up, proj: Projection::Perspective { fov_y }, near: 2.0, far: 80000.0 }
    }

    /// Angles du jeu : pitch positif vers le bas, yaw autour de Z.
    pub fn from_angles(eye: Vec3, pitch: f32, yaw: f32, fov_y: f32) -> Self {
        let (sp, cp) = pitch.to_radians().sin_cos();
        let (sy, cy) = yaw.to_radians().sin_cos();
        let dir = Vec3::new(cp * cy, cp * sy, -sp);
        let up = if dir.z.abs() > 0.999 { Vec3::new(cy, sy, 0.0) } else { Vec3::Z };
        Self { eye, dir, up, proj: Projection::Perspective { fov_y }, near: 2.0, far: 80000.0 }
    }

    /// Vue de dessus, nord (+Y) en haut, depuis la hauteur `z_top`.
    pub fn top_down(center: Vec3, half_height: f32, z_top: f32, depth: f32) -> Self {
        Self { eye: Vec3::new(center.x, center.y, z_top), dir: -Vec3::Z, up: Vec3::Y, proj: Projection::Ortho { half_height }, near: 0.0, far: depth }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = Mat4::look_to_rh(self.eye, self.dir, self.up);
        // profondeur inversée (proche = 1) : la précision reste bonne à 30 000 unités
        let proj = match self.proj {
            Projection::Perspective { fov_y } => Mat4::perspective_infinite_reverse_rh(fov_y.to_radians(), aspect, self.near.max(0.1)),
            Projection::Ortho { half_height } => Mat4::orthographic_rh(-half_height * aspect, half_height * aspect, -half_height, half_height, self.far, self.near),
        };
        proj * view
    }

    /// Position à l'écran (pixels) d'un point du monde, s'il est devant la caméra.
    pub fn project(&self, p: Vec3, width: u32, height: u32) -> Option<(f32, f32)> {
        let c = self.view_proj(width as f32 / height as f32) * p.extend(1.0);
        if c.w <= 1e-6 {
            return None;
        }
        let (x, y) = (c.x / c.w, c.y / c.w);
        Some(((x * 0.5 + 0.5) * width as f32, (0.5 - y * 0.5) * height as f32))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    /// Vers le soleil.
    pub sun_dir: Vec3,
    pub sun_color: Vec3,
    pub ambient: Vec3,
    pub headlight: f32,
    pub sky: [f32; 3],
}

impl Lighting {
    /// Jour neutre : la direction du soleil de la map si elle en a un, des couleurs fixes. La couleur du ciel et
    /// de la lumière de la map est ignorée, pour qu'une map de nuit reste lisible.
    pub fn neutral(scene: &Scene) -> Self {
        let dir = scene.sun.map(|s| s.dir).filter(|d| d.z > 0.2).unwrap_or(Vec3::new(0.45, 0.35, 0.82)).normalize();
        Self { sun_dir: dir, sun_color: Vec3::new(0.92, 0.88, 0.80), ambient: Vec3::new(0.40, 0.42, 0.47), headlight: 0.22, sky: [0.50, 0.62, 0.78] }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub camera: Camera,
    pub width: u32,
    pub height: u32,
    /// Affiche les faces d'outil (nodraw, clip, déclencheurs…) en couleurs codées.
    pub show_tools: bool,
    /// Ne garde que ce qui est dans cette boîte du monde (vues de dessus par étage, coupes).
    pub clip: Option<(Vec3, Vec3)>,
    pub light: Lighting,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Identifiant de l'objet sous chaque pixel (rang dans `Scene::objects`, 0 = rien).
    pub ids: Vec<u32>,
    pub ms: f32,
}

impl Frame {
    pub fn id_at(&self, x: u32, y: u32) -> u32 {
        self.ids[(y.min(self.height - 1) * self.width + x.min(self.width - 1)) as usize]
    }

    pub fn rgb(&self) -> Vec<u8> {
        self.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
    }

    pub fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>> {
        planche::jpeg(&self.rgb(), self.width, self.height, quality)
    }
}

// ------------------------------------------------------------------ scène côté carte graphique

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    view_dir: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    clip_min: [f32; 4],
    clip_max: [f32; 4],
    fog: [f32; 4],
    fog_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MatU {
    color: [f32; 4],
    flags: u32,
    alpha_ref: f32,
    /// Force du reflet d'un vitrage (0 = aucun).
    reflet: f32,
    pad: f32,
    /// Reflet de la cubemap : teinte ; w = 0 aucun, 1 ajouté (masque selon les drapeaux), 2 eau (Fresnel).
    envmap: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    m: [[f32; 4]; 4],
    tint: [f32; 4],
    object: u32,
    pad: [u32; 3],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Opaque,
    Blend,
    Additive,
    /// Faces d'outil et solides masqués : seulement sur demande.
    Tools,
}

#[derive(Clone, Copy)]
enum Geo {
    World { start: u32, count: u32 },
    Prop { index_start: u32, index_count: u32, base_vertex: i32, inst_start: u32, inst_count: u32 },
}

struct Draw {
    pass: Pass,
    cull: bool,
    material: u32,
    geo: Geo,
}

pub struct GpuScene {
    world_vb: wgpu::Buffer,
    world_ib: wgpu::Buffer,
    model_vb: wgpu::Buffer,
    model_ib: wgpu::Buffer,
    instances: wgpu::Buffer,
    materials: Vec<wgpu::BindGroup>,
    draws: Vec<Draw>,
    _textures: Vec<wgpu::Texture>,
    /// Durée de l'envoi à la carte graphique, en secondes.
    pub upload_s: f32,
    /// Mémoire vidéo occupée par les textures, en octets.
    pub texture_bytes: u64,
}

pub struct Renderer {
    pub gpu: Gpu,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    globals_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Serré aux bords : faces du ciel.
    sampler_bord: wgpu::Sampler,
    /// Cubemap grise d'un pixel, pour les matériaux sans reflet.
    pub(crate) cube_neutre: wgpu::TextureView,
    /// [monde, modèles] × [opaque, mélange, additif] × [avec élimination des faces arrière, sans], puis `PIPE_CIEL`
    pipelines: Vec<wgpu::RenderPipeline>,
    targets: Mutex<Option<Targets>>,
    /// Aperçu de l'éclairage de la map (groupe 2 des pipelines).
    eclairage: eclairage::Moteur,
}

struct Targets {
    width: u32,
    height: u32,
    color: wgpu::Texture,
    ids: wgpu::Texture,
    depth: wgpu::Texture,
    color_buf: wgpu::Buffer,
    ids_buf: wgpu::Buffer,
    bpr: u32,
}

/// Les deux pipelines (avec élimination des faces arrière, sans) des faces de ciel dessinées en profondeur seulement.
const PIPE_CIEL: usize = 12;

fn pipe_index(prop: bool, pass: Pass, cull: bool) -> usize {
    let p = match pass {
        Pass::Opaque => 0,
        Pass::Blend | Pass::Tools => 1,
        Pass::Additive => 2,
    };
    (prop as usize) * 6 + p * 2 + (!cull) as usize
}

/// Une cubemap sur la carte graphique (`faces[f][mip]`, RGBA sRGB), vue comme cube.
pub(crate) fn cubemap(d: &wgpu::Device, q: &wgpu::Queue, taille: u32, faces: &[Vec<Vec<u8>>]) -> wgpu::TextureView {
    let mips = faces[0].len() as u32;
    let t = d.create_texture_with_data(
        q,
        &wgpu::TextureDescriptor {
            label: Some("cubemap"),
            size: wgpu::Extent3d { width: taille, height: taille, depth_or_array_layers: 6 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &faces.concat().concat(),
    );
    t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::Cube), ..Default::default() })
}

impl Renderer {
    pub fn new(gpu: Gpu) -> Self {
        let d = &gpu.device;
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("hm"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let globals_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("globals"), entries: &[uniform(0)] });
        let material_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                uniform(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                // seconde texture (`$basetexture2`) ; la première quand le matériau n'en a pas
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                // cubemap des reflets (`$envmap`)
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::Cube, multisampled: false },
                    count: None,
                },
            ],
        });
        let eclairage = eclairage::Moteur::new(d);
        let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("hm"), bind_group_layouts: &[Some(&globals_layout), Some(&material_layout), Some(&eclairage.layout)], ..Default::default() });
        let world_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Uint32, 10 => Float32];
        let model_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let inst_attrs = wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Uint32];
        let mut pipelines = Vec::new();
        let mut combos = Vec::new();
        for prop in [false, true] {
            for pass in [Pass::Opaque, Pass::Blend, Pass::Additive] {
                for cull in [true, false] {
                    combos.push((prop, pass, cull, true));
                }
            }
        }
        // PIPE_CIEL : faces de ciel du monde en profondeur seulement (le ciel déjà peint se voit au travers)
        combos.extend([(false, Pass::Opaque, true, false), (false, Pass::Opaque, false, false)]);
        for (prop, pass, cull, couleur) in combos {
            let world_buffers = [Some(wgpu::VertexBufferLayout { array_stride: 40, step_mode: wgpu::VertexStepMode::Vertex, attributes: &world_attrs })];
            let prop_buffers = [
                Some(wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &model_attrs }),
                Some(wgpu::VertexBufferLayout { array_stride: 96, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst_attrs }),
            ];
            let blend = match pass {
                Pass::Opaque => None,
                Pass::Additive => Some(wgpu::BlendState {
                    color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                }),
                _ => Some(wgpu::BlendState::ALPHA_BLENDING),
            };
            pipelines.push(d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("hm"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if prop { "vs_prop" } else { "vs_world" }),
                    compilation_options: Default::default(),
                    buffers: if prop { &prop_buffers[..] } else { &world_buffers[..] },
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[
                        Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend, write_mask: if couleur { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::empty() } }),
                        // les surfaces additives (halos, hologrammes) ne cachent pas l'objet qui est derrière
                        Some(wgpu::ColorTargetState { format: ID_FORMAT, blend: None, write_mask: if pass == Pass::Additive { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL } }),
                    ],
                }),
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: if cull { Some(wgpu::Face::Back) } else { None },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(pass == Pass::Opaque),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            }));
        }
        let globals = d.create_buffer(&wgpu::BufferDescriptor { label: Some("globals"), size: std::mem::size_of::<Globals>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals_bg = d.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }] });
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let sampler_bord = d.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let cube_neutre = cubemap(d, &gpu.queue, 1, &vec![vec![vec![128, 128, 128, 255]]; 6]);
        Self { gpu, globals, globals_bg, globals_layout, material_layout, sampler, sampler_bord, cube_neutre, pipelines, targets: Mutex::new(None), eclairage }
    }

    /// Textures filtrées (lissées, par défaut) ou au plus proche (pixels nets, « Filter textures » décoché dans
    /// Hammer). Vaut pour les matériaux préparés ensuite : la bibliothèque est à refaire.
    pub fn regler_filtrage(&mut self, lisse: bool) {
        let f = if lisse { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
        self.sampler = self.gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: f,
            min_filter: f,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: if lisse { 8 } else { 1 },
            ..Default::default()
        });
    }

    /// Envoie la scène à la carte graphique.
    pub fn upload(&self, scene: &Scene) -> GpuScene {
        let t0 = Instant::now();
        let (d, q) = (&self.gpu.device, &self.gpu.queue);
        // ---- textures
        let mut texture_bytes = 0u64;
        let mk_tex = |w: u32, h: u32, format, mips: &[Vec<u8>]| {
            let data: Vec<u8> = mips.concat();
            d.create_texture_with_data(
                q,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: mips.len() as u32,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &data,
            )
        };
        let white = mk_tex(1, 1, wgpu::TextureFormat::Rgba8UnormSrgb, &[vec![255u8; 4]]);
        let mut textures = Vec::with_capacity(scene.textures.len());
        for t in &scene.textures {
            let bc_ok = self.gpu.bc || t.format == PixelFormat::Rgba8;
            let tex = if bc_ok {
                let format = match t.format {
                    PixelFormat::Bc1 => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
                    PixelFormat::Bc2 => wgpu::TextureFormat::Bc2RgbaUnormSrgb,
                    PixelFormat::Bc3 => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
                    PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8UnormSrgb,
                };
                texture_bytes += t.mips.iter().map(|m| m.len() as u64).sum::<u64>();
                mk_tex(t.width, t.height, format, &t.mips)
            } else {
                mk_tex(1, 1, wgpu::TextureFormat::Rgba8UnormSrgb, &[vec![160, 160, 160, 255]])
            };
            textures.push(tex);
        }
        // ---- matériaux : ceux de la scène, puis le matériau d'alerte des modèles illisibles
        let mut materials = Vec::with_capacity(scene.materials.len() + 1);
        let mut passes: Vec<(Pass, bool)> = Vec::with_capacity(scene.materials.len() + 1);
        let mk_mat = |u: MatU, tex: &wgpu::Texture, tex2: &wgpu::Texture| {
            let buf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.material_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tex.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tex2.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.cube_neutre) },
                ],
            })
        };
        for (i, m) in scene.materials.iter().enumerate() {
            let tex = scene.material_texture[i].map(|t| &textures[t as usize]);
            let tex2 = scene.material_texture2[i].map(|t| &textures[t as usize]);
            let (u, pass) = if let Some(tool) = m.tool {
                let c = match tool {
                    Tool::Nodraw => [0.90, 0.80, 0.20, 0.22],
                    Tool::Sky => [0.30, 0.60, 1.00, 0.18],
                    Tool::Clip => [0.85, 0.20, 0.85, 0.35],
                    Tool::Trigger => [1.00, 0.55, 0.10, 0.35],
                    Tool::Areaportal => [0.20, 0.90, 0.90, 0.40],
                    Tool::Hint => [0.90, 0.90, 0.90, 0.12],
                    Tool::Invisible => [0.50, 0.50, 0.95, 0.30],
                    Tool::Other => [0.70, 0.70, 0.70, 0.30],
                };
                (MatU { color: c, flags: F_FLAT | F_UNLIT, alpha_ref: 0.5, reflet: 0.0, pad: 0.0, envmap: [0.0; 4] }, Pass::Tools)
            } else if !m.found {
                // matériau introuvable : magenta, comme le damier du jeu
                (MatU { color: [1.0, 0.0, 1.0, 1.0], flags: F_FLAT | F_UNLIT, alpha_ref: 0.5, reflet: 0.0, pad: 0.0, envmap: [0.0; 4] }, Pass::Opaque)
            } else {
                let mut flags = 0;
                let blend = m.translucent || m.alpha < 0.999;
                if tex.is_none() {
                    flags |= F_FLAT;
                }
                if m.alphatest {
                    flags |= F_ALPHATEST;
                }
                if m.selfillum {
                    flags |= F_SELFILLUM;
                }
                if m.unlit {
                    flags |= F_UNLIT;
                }
                if m.translucent || m.additive {
                    flags |= F_TEX_ALPHA;
                }
                if tex.is_some() && tex2.is_some() {
                    flags |= F_BLEND;
                }
                let mut color = [m.color[0], m.color[1], m.color[2], m.alpha];
                if tex.is_none() && m.base.is_some() {
                    color = [0.55, 0.55, 0.58, m.alpha]; // texture introuvable : gris
                }
                let pass = if m.additive { Pass::Additive } else if blend { Pass::Blend } else { Pass::Opaque };
                (MatU { color, flags, alpha_ref: m.alpha_ref, reflet: m.reflet, pad: 0.0, envmap: [0.0; 4] }, pass)
            };
            materials.push(mk_mat(u, tex.unwrap_or(&white), tex2.or(tex).unwrap_or(&white)));
            passes.push((pass, !m.nocull));
        }
        let alert = materials.len() as u32;
        materials.push(mk_mat(MatU { color: [1.0, 0.05, 0.05, 1.0], flags: F_FLAT | F_UNLIT, alpha_ref: 0.5, reflet: 0.0, pad: 0.0, envmap: [0.0; 4] }, &white, &white));
        passes.push((Pass::Opaque, true));

        // ---- monde
        let mut draws = Vec::new();
        let mut indices: Vec<u32> = Vec::with_capacity(scene.batches.iter().map(|b| b.indices.len()).sum());
        for b in &scene.batches {
            let (mut pass, cull) = passes[b.material as usize];
            if b.hidden {
                pass = Pass::Tools;
            }
            draws.push(Draw { pass, cull, material: b.material, geo: Geo::World { start: indices.len() as u32, count: b.indices.len() as u32 } });
            indices.extend_from_slice(&b.indices);
        }
        let empty_v = [hm_scene::WVertex::zeroed()];
        let world_vb = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("monde"), contents: bytemuck::cast_slice(if scene.vertices.is_empty() { &empty_v[..] } else { &scene.vertices[..] }), usage: wgpu::BufferUsages::VERTEX });
        if indices.is_empty() {
            indices.push(0);
        }
        let world_ib = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("monde"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX });

        // ---- modèles : un seul tampon de sommets et d'indices ; le cube d'alerte en premier
        let mut mverts: Vec<MVertex> = Vec::new();
        let mut mindices: Vec<u32> = Vec::new();
        let cube = cube_mesh(&mut mverts, &mut mindices);
        // (début d'indices, nombre, sommet de base) de chaque maillage de chaque modèle
        let mut ranges: Vec<Vec<(u32, u32, i32)>> = Vec::with_capacity(scene.models.len());
        for sm in &scene.models {
            match &sm.model {
                Some(m) => {
                    let base = mverts.len() as i32;
                    mverts.extend(m.vertices.iter().map(|v| MVertex { pos: v.pos, normal: v.normal, uv: v.uv }));
                    let mut r = Vec::with_capacity(m.meshes.len());
                    for mesh in &m.meshes {
                        r.push((mindices.len() as u32, mesh.indices.len() as u32, base));
                        mindices.extend_from_slice(&mesh.indices);
                    }
                    ranges.push(r);
                }
                None => ranges.push(Vec::new()),
            }
        }
        let model_vb = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("modèles"), contents: bytemuck::cast_slice(&mverts), usage: wgpu::BufferUsages::VERTEX });
        let model_ib = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("modèles"), contents: bytemuck::cast_slice(&mindices), usage: wgpu::BufferUsages::INDEX });

        // ---- instances, groupées par (modèle, skin)
        let mut order: Vec<usize> = (0..scene.props.len()).collect();
        let skin_of = |p: &hm_scene::Prop| {
            let n = scene.models[p.model as usize].materials.len().max(1) as u32;
            p.skin.min(n - 1)
        };
        order.sort_by_key(|&i| (scene.props[i].model, skin_of(&scene.props[i])));
        let mut inst: Vec<Instance> = Vec::with_capacity(order.len().max(1));
        let mut k = 0;
        while k < order.len() {
            let p0 = &scene.props[order[k]];
            let (model, skin) = (p0.model, skin_of(p0));
            let start = inst.len() as u32;
            while k < order.len() && scene.props[order[k]].model == model && skin_of(&scene.props[order[k]]) == skin {
                let p = &scene.props[order[k]];
                let mut m = p.transform;
                if scene.models[model as usize].model.is_none() {
                    m = m * Mat4::from_scale(Vec3::splat(16.0));
                }
                inst.push(Instance { m: m.to_cols_array_2d(), tint: p.tint, object: p.object, pad: [0; 3] });
                k += 1;
            }
            let count = inst.len() as u32 - start;
            let sm = &scene.models[model as usize];
            if sm.model.is_some() {
                for (j, &(index_start, index_count, base_vertex)) in ranges[model as usize].iter().enumerate() {
                    let material = sm.materials[skin as usize][j];
                    let (pass, cull) = passes[material as usize];
                    draws.push(Draw { pass, cull, material, geo: Geo::Prop { index_start, index_count, base_vertex, inst_start: start, inst_count: count } });
                }
            } else {
                draws.push(Draw { pass: Pass::Opaque, cull: true, material: alert, geo: Geo::Prop { index_start: cube.0, index_count: cube.1, base_vertex: 0, inst_start: start, inst_count: count } });
            }
        }
        if inst.is_empty() {
            inst.push(Instance::zeroed());
        }
        let instances = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("instances"), contents: bytemuck::cast_slice(&inst), usage: wgpu::BufferUsages::VERTEX });
        q.submit([]);
        GpuScene { world_vb, world_ib, model_vb, model_ib, instances, materials, draws, _textures: textures, upload_s: t0.elapsed().as_secs_f32(), texture_bytes }
    }

    fn targets(&self, width: u32, height: u32) -> std::sync::MutexGuard<'_, Option<Targets>> {
        let mut guard = self.targets.lock().unwrap();
        if guard.as_ref().map_or(true, |t| t.width != width || t.height != height) {
            let d = &self.gpu.device;
            let mk = |format, usage| {
                d.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let bpr = (width * 4).div_ceil(256) * 256;
            let buf = || d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            *guard = Some(Targets {
                width,
                height,
                color: mk(COLOR_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
                ids: mk(ID_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
                depth: mk(DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT),
                color_buf: buf(),
                ids_buf: buf(),
                bpr,
            });
        }
        guard
    }

    /// Rend une vue et relit l'image et les identifiants.
    pub fn render(&self, gs: &GpuScene, view: &View) -> Frame {
        let t0 = Instant::now();
        let (d, q) = (&self.gpu.device, &self.gpu.queue);
        let (w, h) = (view.width.max(8), view.height.max(8));
        let guard = self.targets(w, h);
        let t = guard.as_ref().unwrap();
        let cam = &view.camera;
        let (cmin, cmax) = view.clip.unwrap_or((Vec3::splat(-1e9), Vec3::splat(1e9)));
        let persp = matches!(cam.proj, Projection::Perspective { .. });
        let l = &view.light;
        let globals = Globals {
            view_proj: cam.view_proj(w as f32 / h as f32).to_cols_array_2d(),
            eye: cam.eye.extend(if persp { 1.0 } else { 0.0 }).to_array(),
            view_dir: cam.dir.extend(0.0).to_array(),
            sun_dir: l.sun_dir.normalize().extend(0.0).to_array(),
            sun_color: l.sun_color.extend(0.0).to_array(),
            ambient: l.ambient.extend(l.headlight).to_array(),
            clip_min: cmin.extend(0.0).to_array(),
            clip_max: cmax.extend(0.0).to_array(),
            fog: [0.0; 4],
            fog_color: [0.0; 4],
        };
        q.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let (cv, iv, dv) = (t.color.create_view(&Default::default()), t.ids.create_view(&Default::default()), t.depth.create_view(&Default::default()));
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &cv,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: srgb_to_linear(l.sky[0]), g: srgb_to_linear(l.sky[1]), b: srgb_to_linear(l.sky[2]), a: 1.0 }), store: wgpu::StoreOp::Store },
                    }),
                    Some(wgpu::RenderPassColorAttachment { view: &iv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                ..Default::default()
            });
            rp.set_bind_group(0, &self.globals_bg, &[]);
            rp.set_bind_group(2, &self.eclairage.eteint, &[]);
            for pass in [Pass::Opaque, Pass::Blend, Pass::Additive, Pass::Tools] {
                if pass == Pass::Tools && !view.show_tools {
                    continue;
                }
                for prop in [false, true] {
                    if prop {
                        rp.set_vertex_buffer(0, gs.model_vb.slice(..));
                        rp.set_vertex_buffer(1, gs.instances.slice(..));
                        rp.set_index_buffer(gs.model_ib.slice(..), wgpu::IndexFormat::Uint32);
                    } else {
                        rp.set_vertex_buffer(0, gs.world_vb.slice(..));
                        rp.set_index_buffer(gs.world_ib.slice(..), wgpu::IndexFormat::Uint32);
                    }
                    for cull in [true, false] {
                        rp.set_pipeline(&self.pipelines[pipe_index(prop, pass, cull)]);
                        for dr in gs.draws.iter().filter(|dr| dr.pass == pass && dr.cull == cull) {
                            match dr.geo {
                                Geo::World { start, count } if !prop => {
                                    rp.set_bind_group(1, &gs.materials[dr.material as usize], &[]);
                                    rp.draw_indexed(start..start + count, 0, 0..1);
                                }
                                Geo::Prop { index_start, index_count, base_vertex, inst_start, inst_count } if prop => {
                                    rp.set_bind_group(1, &gs.materials[dr.material as usize], &[]);
                                    rp.draw_indexed(index_start..index_start + index_count, base_vertex, inst_start..inst_start + inst_count);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(t.bpr), rows_per_image: Some(h) };
        enc.copy_texture_to_buffer(t.color.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &t.color_buf, layout }, extent);
        enc.copy_texture_to_buffer(t.ids.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &t.ids_buf, layout }, extent);
        q.submit([enc.finish()]);
        let (cs, is) = (t.color_buf.slice(..), t.ids_buf.slice(..));
        cs.map_async(wgpu::MapMode::Read, |_| {});
        is.map_async(wgpu::MapMode::Read, |_| {});
        d.poll(wgpu::PollType::wait_indefinitely()).expect("attente de la carte graphique");
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        let mut ids = Vec::with_capacity((w * h) as usize);
        {
            let (cd, id) = (cs.get_mapped_range().expect("lecture de l'image"), is.get_mapped_range().expect("lecture des identifiants"));
            let (cd, id): (&[u8], &[u8]) = (&cd, &id);
            for y in 0..h as usize {
                let row = y * t.bpr as usize;
                rgba.extend_from_slice(&cd[row..row + (w * 4) as usize]);
                ids.extend(id[row..row + (w * 4) as usize].chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
            }
        }
        t.color_buf.unmap();
        t.ids_buf.unmap();
        Frame { width: w, height: h, rgba, ids, ms: t0.elapsed().as_secs_f32() * 1e3 }
    }
}

fn srgb_to_linear(c: f32) -> f64 {
    (if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }) as f64
}

/// Ajoute un cube de côté 2 (de −1 à 1) ; rend (début d'indices, nombre).
fn cube_mesh(verts: &mut Vec<MVertex>, indices: &mut Vec<u32>) -> (u32, u32) {
    let start = indices.len() as u32;
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    for (n, u, v) in faces {
        let (n3, u3, v3) = (Vec3::from(n), Vec3::from(u), Vec3::from(v));
        let base = verts.len() as u32;
        // u × v = n : l'ordre (−u−v, −u+v, +u+v, +u−v) est horaire vu de l'extérieur
        for (a, b) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)] {
            verts.push(MVertex { pos: (n3 + u3 * a + v3 * b).to_array(), normal: n, uv: [a * 0.5 + 0.5, b * 0.5 + 0.5] });
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (start, indices.len() as u32 - start)
}
