//! Lignes : les vues 2D en fil de fer (arêtes des solides, boîtes des entités, grille) et les contours de la
//! sélection par-dessus la vue 3D.
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::{COLOR_FORMAT, DEPTH_FORMAT};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Sommet {
    pub pos: [f32; 3],
    /// RGBA, 0 à 255.
    pub couleur: [u8; 4],
}

const SHADER: &str = r#"
struct G { vp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> g: G;
struct V { @builtin(position) p: vec4<f32>, @location(0) c: vec4<f32> };
@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) c: vec4<f32>) -> V {
    var o: V;
    o.p = g.vp * vec4<f32>(pos, 1.0);
    o.c = c;
    return o;
}
// couleurs données en sRGB, cible en sRGB : on les repasse en linéaire avant l'écriture
fn lin(c: f32) -> f32 { if (c <= 0.04045) { return c / 12.92; } return pow((c + 0.055) / 1.055, 2.4); }
@fragment fn fs(v: V) -> @location(0) vec4<f32> { return vec4<f32>(lin(v.c.r), lin(v.c.g), lin(v.c.b), v.c.a); }
"#;

/// Les pipelines de lignes : seules (vue 2D, cible couleur seule) ou par-dessus une vue 3D (avec ses deux cibles).
pub struct Lignes {
    seules: wgpu::RenderPipeline,
    dessus_3d: wgpu::RenderPipeline,
    /// Cachées par ce qui est devant (aides des entités : liens, cordes, sphères).
    dans_3d: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// Un lot de lignes envoyé à la carte graphique.
pub struct LotLignes {
    tampon: wgpu::Buffer,
    n: u32,
    constantes: wgpu::Buffer,
    groupe: wgpu::BindGroup,
}

impl Lignes {
    pub fn new(device: &wgpu::Device) -> Lignes {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lignes"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lignes"),
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("lignes"), bind_group_layouts: &[Some(&layout)], ..Default::default() });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        let mk = |cibles: &[Option<wgpu::ColorTargetState>], profondeur: Option<wgpu::CompareFunction>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("lignes"),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })] },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: cibles }),
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
                depth_stencil: profondeur.map(|c| wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(false), depth_compare: Some(c), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let couleur = Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL });
        let ids = Some(wgpu::ColorTargetState { format: crate::ID_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() });
        // profondeur inversée (effacée à 0) : plus près = plus grand
        let (seules, dessus_3d, dans_3d) = (mk(&[couleur.clone()], None), mk(&[couleur.clone(), ids.clone()], Some(wgpu::CompareFunction::Always)), mk(&[couleur, ids], Some(wgpu::CompareFunction::GreaterEqual)));
        Lignes { seules, dessus_3d, dans_3d, layout }
    }

    pub fn lot(&self, device: &wgpu::Device, sommets: &[Sommet], vp: Mat4) -> LotLignes {
        let vide = [Sommet::zeroed(), Sommet::zeroed()];
        let tampon = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("lignes"), contents: bytemuck::cast_slice(if sommets.is_empty() { &vide[..] } else { sommets }), usage: wgpu::BufferUsages::VERTEX });
        let constantes = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("lignes"), contents: bytemuck::cast_slice(&vp.to_cols_array()), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        let groupe = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &self.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: constantes.as_entire_binding() }] });
        LotLignes { tampon, n: sommets.len() as u32 & !1, constantes, groupe }
    }

    /// Dessine une vue 2D : fond, puis les lots dans l'ordre (grille d'abord, sélection en dernier).
    pub fn dessiner_2d(&self, enc: &mut wgpu::CommandEncoder, cible: &wgpu::Texture, fond: [f64; 3], lots: &[&LotLignes]) {
        let v = cible.create_view(&Default::default());
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vue 2D"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: fond[0], g: fond[1], b: fond[2], a: 1.0 }), store: wgpu::StoreOp::Store } })],
            ..Default::default()
        });
        rp.set_pipeline(&self.seules);
        for l in lots {
            rp.set_bind_group(0, &l.groupe, &[]);
            rp.set_vertex_buffer(0, l.tampon.slice(..));
            rp.draw(0..l.n, 0..1);
        }
    }

    /// Dessine des lignes sur une vue 3D déjà rendue : `caches` cachés par ce qui est devant (comme Hammer dessine les
    /// aides des entités), puis `dessus` toujours visibles (sélection, aperçus).
    pub fn dessiner_sur_3d(&self, enc: &mut wgpu::CommandEncoder, cibles: &crate::vivant::Cibles, caches: &[&LotLignes], dessus: &[&LotLignes]) {
        let (cv, iv, dv) = (cibles.couleur.create_view(&Default::default()), cibles.ids.create_view(&Default::default()), cibles.profondeur.create_view(&Default::default()));
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lignes 3D"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment { view: &cv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } }),
                Some(wgpu::RenderPassColorAttachment { view: &iv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }),
            ..Default::default()
        });
        for (pipeline, lots) in [(&self.dans_3d, caches), (&self.dessus_3d, dessus)] {
            rp.set_pipeline(pipeline);
            for l in lots {
                rp.set_bind_group(0, &l.groupe, &[]);
                rp.set_vertex_buffer(0, l.tampon.slice(..));
                rp.draw(0..l.n, 0..1);
            }
        }
    }
}

impl LotLignes {
    /// Les mêmes sommets vus par une autre projection (une autre vue) : seule la petite matrice est envoyée.
    pub fn pour_vue(&self, lignes: &Lignes, device: &wgpu::Device, vp: Mat4) -> LotLignes {
        let constantes = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("lignes"), contents: bytemuck::cast_slice(&vp.to_cols_array()), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        let groupe = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &lignes.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: constantes.as_entire_binding() }] });
        LotLignes { tampon: self.tampon.clone(), n: self.n, constantes, groupe }
    }

    /// Change la projection sans renvoyer les sommets (la vue a bougé, pas la map).
    pub fn projection(&self, queue: &wgpu::Queue, vp: Mat4) {
        queue.write_buffer(&self.constantes, 0, bytemuck::cast_slice(&vp.to_cols_array()));
    }
}
