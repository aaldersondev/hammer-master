//! Aperçu de l'éclairage de la map dans la vue 3D : lumières ponctuelles et projecteurs, soleil et ciel d'une
//! light_environment, chacun avec ses ombres (cartes de profondeur : six faces par lumière, une pour le soleil, une
//! par direction du ciel). L'éclairement suit la formule de `hm_doc::lumiere`, vérifiée contre les lightmaps d'une
//! vraie compilation ; ce qui manque par rapport à vrad : les rebonds (remplacés par une lumière ajoutée réglable)
//! et la douceur des ombres d'une lumière étendue.
//!
//! Les cartes d'ombre ne sont refaites que pour ce qui a changé : une lumière déplacée refait ses six faces, une
//! géométrie modifiée (nouvelle version) les refait toutes.
use std::collections::HashMap;
use std::sync::Mutex;

use glam::{Mat4, Vec3};

use crate::vivant::{Bibliotheque, Geo, MondeGpu};
use crate::Pass;

pub const MAX_LUMIERES: usize = 96;
/// Les lumières qui ont des ombres : les premières de la liste (l'appelant met devant celles qui comptent).
pub const MAX_OMBRES: usize = 32;
pub const N_CIEL: usize = 16;
pub const TAILLE_CUBE: u32 = 512;
pub const TAILLE_SOLEIL: u32 = 2048;
pub const TAILLE_CIEL: u32 = 1024;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Alignement des décalages dynamiques du tampon des matrices d'ombre.
const PAS: u64 = 256;
const N_MATS: usize = 6 * MAX_OMBRES + 1 + N_CIEL;
/// En dessous de cet éclairement, une lumière ne compte plus (sa portée).
const SEUIL: f32 = 1.0 / 1024.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LumiereRendu {
    pub position: Vec3,
    /// Intensité linéaire (1 = blanc plein).
    pub couleur: Vec3,
    /// Éclairement = couleur × cos / (c + l·d + q·d²).
    pub attenuation: [f32; 3],
    /// Fondu linéaire jusqu'à zéro entre ces deux distances.
    pub fondu: Option<(f32, f32)>,
    /// Projecteur : direction, cosinus intérieur, cosinus extérieur, exposant.
    pub cone: Option<(Vec3, f32, f32, f32)>,
}

impl LumiereRendu {
    fn attenuation(&self, d: f32) -> f32 {
        let [c, l, q] = self.attenuation;
        let den = c + l * d + q * d * d;
        let mut k = if den <= 1e-9 { 0.0 } else { 1.0 / den };
        if let Some((a, b)) = self.fondu {
            if d > a {
                k *= (1.0 - (d - a) / (b - a)).clamp(0.0, 1.0);
            }
        }
        k
    }

    /// La distance au-delà de laquelle elle n'éclaire plus rien de visible.
    pub fn portee(&self) -> f32 {
        let m = self.couleur.max_element();
        let mut hi = 64.0f32;
        while hi < 32768.0 && m * self.attenuation(hi) > SEUIL {
            hi *= 2.0;
        }
        if hi >= 32768.0 {
            return 32768.0;
        }
        let mut lo = hi / 2.0;
        for _ in 0..20 {
            let mi = (lo + hi) / 2.0;
            if m * self.attenuation(mi) > SEUIL {
                lo = mi;
            } else {
                hi = mi;
            }
        }
        hi
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Eclairage {
    pub lumieres: Vec<LumiereRendu>,
    /// Vers le soleil, couleur.
    pub soleil: Option<(Vec3, Vec3)>,
    /// La lumière diffuse du ciel (reçue en entier par une face tournée vers le haut sous un ciel dégagé).
    pub ciel: Vec3,
    /// Ajoutée partout : tient lieu des rebonds que l'aperçu ne calcule pas.
    pub ajout: Vec3,
    pub ombres: bool,
    /// La boîte de la map (cadre des ombres du soleil et du ciel).
    pub bornes: (Vec3, Vec3),
}

/// Les directions du ciel (vers le ciel) et leur poids : trois couronnes (15°, 45°, 75° de hauteur) pondérées par
/// l'angle solide de leur bande, ramenées pour qu'une face tournée vers le haut sous un ciel entier reçoive 1.
pub fn directions_ciel() -> [(Vec3, f32); N_CIEL] {
    let couronnes = [(15.0f32, 8usize, 0.0f32, 30.0f32), (45.0, 5, 0.0, 60.0), (75.0, 3, 0.0, 90.0)];
    let mut v = [(Vec3::Z, 0.0f32); N_CIEL];
    let mut k = 0;
    let mut bas = 0.0f32;
    for (h, n, decal, haut) in couronnes {
        let omega = 2.0 * std::f32::consts::PI * (haut.to_radians().sin() - bas.to_radians().sin());
        bas = haut;
        for i in 0..n {
            let az = (decal + 360.0 * i as f32 / n as f32 + h).to_radians();
            let (e, a) = (h.to_radians(), az);
            v[k] = (Vec3::new(e.cos() * a.cos(), e.cos() * a.sin(), e.sin()), omega / n as f32);
            k += 1;
        }
    }
    let somme: f32 = v.iter().map(|(d, w)| w * d.z).sum();
    for x in v.iter_mut() {
        x.1 /= somme;
    }
    v
}

/// Les six vues d'une face de cube : +X, -X, +Y, -Y, +Z, -Z.
fn vues_cube(p: Vec3, portee: f32) -> [Mat4; 6] {
    // un peu plus de 90° : les bords des faces se recouvrent
    let proj = Mat4::perspective_rh(91.0f32.to_radians(), 1.0, 1.0, portee.max(2.0));
    let faces = [(Vec3::X, Vec3::Z), (-Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z), (-Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::Y), (-Vec3::Z, Vec3::Y)];
    faces.map(|(d, h)| proj * Mat4::look_at_rh(p, p + d, h))
}

/// La vue orthographique qui couvre la boîte depuis la direction `vers` (vers la source).
fn vue_dir(bornes: (Vec3, Vec3), vers: Vec3) -> (Mat4, f32) {
    let c = (bornes.0 + bornes.1) * 0.5;
    let r = ((bornes.1 - bornes.0).length() * 0.5).max(64.0) + 16.0;
    let haut = if vers.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
    let m = Mat4::orthographic_rh(-r, r, -r, r, 0.0, 2.0 * r) * Mat4::look_at_rh(c + vers.normalize() * r, c, haut);
    (m, r)
}

pub(crate) struct Moteur {
    pub(crate) layout: wgpu::BindGroupLayout,
    pipe_monde: wgpu::RenderPipeline,
    pipe_prop: wgpu::RenderPipeline,
    mats: wgpu::Buffer,
    mats_bg: wgpu::BindGroup,
    echantillon: wgpu::Sampler,
    pub(crate) eteint: wgpu::BindGroup,
    etat: Mutex<Etat>,
}

struct Etat {
    uniforme: wgpu::Buffer,
    actif: Option<wgpu::BindGroup>,
    cubes: Option<(wgpu::Texture, u32)>,
    soleil: Option<wgpu::Texture>,
    ciel: Option<wgpu::Texture>,
    /// Ce que contient chaque emplacement d'ombre de lumière (position, portée).
    emplacements: Vec<Option<[u32; 4]>>,
    version: Option<u64>,
    /// Bornes et direction du soleil des ombres directionnelles déjà faites.
    cle_dir: Option<[u32; 9]>,
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

impl Moteur {
    pub(crate) fn new(d: &wgpu::Device) -> Moteur {
        let entree_tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false },
            count: None,
        };
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("éclairage"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                entree_tex(1),
                entree_tex(2),
                entree_tex(3),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
            ],
        });
        let mats_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ombre"),
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(64) }, count: None }],
        });
        let mats = d.create_buffer(&wgpu::BufferDescriptor { label: Some("ombres"), size: PAS * N_MATS as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mats_bg = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ombre"),
            layout: &mats_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &mats, offset: 0, size: wgpu::BufferSize::new(64) }) }],
        });
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ombre"), source: wgpu::ShaderSource::Wgsl(include_str!("ombre.wgsl").into()) });
        let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ombre"), bind_group_layouts: &[Some(&mats_layout)], ..Default::default() });
        let monde_attrs = [wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }];
        let modele_attrs = [wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }];
        let inst_attrs = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 4 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 5 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 6 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 7 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 84, shader_location: 10 },
        ];
        let pipe = |prop: bool| {
            let monde = [Some(wgpu::VertexBufferLayout { array_stride: 40, step_mode: wgpu::VertexStepMode::Vertex, attributes: &monde_attrs })];
            let props = [
                Some(wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &modele_attrs }),
                Some(wgpu::VertexBufferLayout { array_stride: 96, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst_attrs }),
            ];
            d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ombre"),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(if prop { "vs_prop" } else { "vs_monde" }), compilation_options: Default::default(), buffers: if prop { &props[..] } else { &monde[..] } },
                fragment: None,
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let (pipe_monde, pipe_prop) = (pipe(false), pipe(true));
        let echantillon = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ombre"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let uniforme = d.create_buffer(&wgpu::BufferDescriptor { label: Some("éclairage"), size: TAILLE_UNIFORME as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        // éteint : un tampon à zéro (aucune lumière, aperçu inactif) et des textures d'une couche
        let zero = d.create_buffer(&wgpu::BufferDescriptor { label: Some("éclairage éteint"), size: TAILLE_UNIFORME as u64, usage: wgpu::BufferUsages::UNIFORM, mapped_at_creation: true });
        zero.unmap();
        let petite = profondeur(d, 1, 1);
        let eteint = groupe(d, &layout, &zero, &petite, &petite, &petite, &echantillon);
        Moteur { layout, pipe_monde, pipe_prop, mats, mats_bg, echantillon, eteint, etat: Mutex::new(Etat { uniforme, actif: None, cubes: None, soleil: None, ciel: None, emplacements: Vec::new(), version: None, cle_dir: None }) }
    }

    /// Le groupe à lier pour dessiner avec l'éclairage de la map (préparé par `preparer`), sinon l'éteint.
    pub(crate) fn groupe_actif(&self) -> Option<wgpu::BindGroup> {
        self.etat.lock().unwrap().actif.clone()
    }

    /// Prépare l'éclairage : écrit les lumières et refait les cartes d'ombre de ce qui a changé. `version` : celle de
    /// la géométrie (une autre version refait toutes les ombres).
    pub(crate) fn preparer(&self, d: &wgpu::Device, q: &wgpu::Queue, bib: &Bibliotheque, monde: &MondeGpu, version: u64, e: &Eclairage, enc: &mut wgpu::CommandEncoder) {
        let mut et = self.etat.lock().unwrap();
        let et = &mut *et;
        let n = e.lumieres.len().min(MAX_LUMIERES);
        let portees: Vec<f32> = e.lumieres[..n].iter().map(|l| l.portee()).collect();
        let n_ombres = if e.ombres { n.min(MAX_OMBRES) } else { 0 };
        let geo_change = et.version != Some(version);
        et.version = Some(version);
        // ---- textures (refaites si elles ne suffisent plus : toutes les ombres sont alors à refaire)
        let mut refaire_groupe = et.actif.is_none();
        let besoin = ((n_ombres.max(1) + 3) / 4 * 4) as u32 * 6;
        if et.cubes.as_ref().map_or(true, |(_, c)| *c < besoin) {
            et.cubes = Some((profondeur(d, TAILLE_CUBE, besoin), besoin));
            et.emplacements.clear();
            refaire_groupe = true;
        }
        let avec_dir = e.ombres && (e.soleil.is_some() || e.ciel.max_element() > 0.0);
        if et.soleil.is_none() || (avec_dir && et.soleil.as_ref().unwrap().width() < TAILLE_SOLEIL) {
            let t = if avec_dir { TAILLE_SOLEIL } else { 1 };
            et.soleil = Some(profondeur(d, t, 1));
            et.ciel = Some(profondeur(d, if avec_dir { TAILLE_CIEL } else { 1 }, if avec_dir { N_CIEL as u32 } else { 1 }));
            et.cle_dir = None;
            refaire_groupe = true;
        }
        if refaire_groupe {
            et.actif = Some(groupe(d, &self.layout, &et.uniforme, &et.cubes.as_ref().unwrap().0, et.soleil.as_ref().unwrap(), et.ciel.as_ref().unwrap(), &self.echantillon));
        }
        if geo_change {
            et.emplacements.iter_mut().for_each(|x| *x = None);
            et.cle_dir = None;
        }
        // ---- emplacements d'ombre : une lumière qui n'a pas bougé garde le sien
        let cles: Vec<[u32; 4]> = (0..n_ombres).map(|i| {
            let p = e.lumieres[i].position;
            [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), portees[i].to_bits()]
        }).collect();
        let places = (et.cubes.as_ref().unwrap().1 / 6) as usize;
        et.emplacements.resize(places, None);
        let mut deja: HashMap<[u32; 4], usize> = HashMap::new();
        for (k, c) in et.emplacements.iter().enumerate() {
            if let Some(c) = c {
                deja.insert(*c, k);
            }
        }
        let mut emplacement = vec![usize::MAX; n_ombres];
        let mut pris = vec![false; places];
        for i in 0..n_ombres {
            if let Some(&k) = deja.get(&cles[i]) {
                if !pris[k] {
                    emplacement[i] = k;
                    pris[k] = true;
                }
            }
        }
        let mut a_faire = Vec::new();
        for i in 0..n_ombres {
            if emplacement[i] == usize::MAX {
                let k = (0..places).find(|k| !pris[*k]).expect("emplacement d'ombre");
                pris[k] = true;
                emplacement[i] = k;
                et.emplacements[k] = Some(cles[i]);
                a_faire.push(i);
            }
        }
        // ---- matrices
        let mut mats = vec![Mat4::IDENTITY; N_MATS];
        for i in 0..n_ombres {
            let v = vues_cube(e.lumieres[i].position, portees[i]);
            for f in 0..6 {
                mats[emplacement[i] * 6 + f] = v[f];
            }
        }
        let ciel = directions_ciel();
        let (m_soleil, r) = vue_dir(e.bornes, e.soleil.map_or(Vec3::Z, |s| s.0));
        mats[6 * MAX_OMBRES] = m_soleil;
        for j in 0..N_CIEL {
            mats[6 * MAX_OMBRES + 1 + j] = vue_dir(e.bornes, ciel[j].0).0;
        }
        let mut octets = vec![0u8; PAS as usize * N_MATS];
        for (k, m) in mats.iter().enumerate() {
            octets[k * PAS as usize..k * PAS as usize + 64].copy_from_slice(bytemuck::cast_slice(&m.to_cols_array()));
        }
        q.write_buffer(&self.mats, 0, &octets);
        // ---- uniforme de l'éclairage (même disposition que `Eclairage` dans shader.wgsl)
        let mut u: Vec<f32> = Vec::with_capacity(TAILLE_UNIFORME / 4);
        let ciel_actif = e.ciel.max_element() > 0.0;
        u.extend([f32::from_bits(n as u32), f32::from_bits(e.soleil.is_some() as u32), f32::from_bits(if ciel_actif { N_CIEL as u32 } else { 0 }), f32::from_bits(1)]);
        let sd = e.soleil.map_or(Vec3::Z, |s| s.0.normalize());
        u.extend([sd.x, sd.y, sd.z, if avec_dir && e.soleil.is_some() { 0.0 } else { -1.0 }]);
        let sc = e.soleil.map_or(Vec3::ZERO, |s| s.1);
        u.extend([sc.x, sc.y, sc.z, 0.0]);
        u.extend([e.ciel.x, e.ciel.y, e.ciel.z, if avec_dir && ciel_actif { 1.0 } else { 0.0 }]);
        u.extend([e.ajout.x, e.ajout.y, e.ajout.z, 0.0]);
        // tailles d'un texel en unités : soleil et ciel (orthographiques), cube (par unité de distance)
        u.extend([2.0 * r / TAILLE_SOLEIL as f32, 2.0 * r / TAILLE_CIEL as f32, 2.0 / TAILLE_CUBE as f32, 0.0]);
        for (dj, w) in ciel {
            u.extend([dj.x, dj.y, dj.z, w]);
        }
        for i in 0..MAX_LUMIERES {
            match e.lumieres.get(i).filter(|_| i < n) {
                Some(l) => {
                    let ombre = if i < n_ombres { (emplacement[i] * 6) as f32 } else { -1.0 };
                    u.extend([l.position.x, l.position.y, l.position.z, ombre]);
                    u.extend([l.couleur.x, l.couleur.y, l.couleur.z, portees[i]]);
                    let (f0, f1) = l.fondu.unwrap_or((0.0, 0.0));
                    u.extend([l.attenuation[0], l.attenuation[1], l.attenuation[2], f0]);
                    match l.cone {
                        Some((dir, ci, ce, ex)) => {
                            let dir = dir.normalize_or(Vec3::NEG_Z);
                            u.extend([dir.x, dir.y, dir.z, ce]);
                            u.extend([ci, ex, f1, 0.0]);
                        }
                        None => {
                            u.extend([0.0, 0.0, -1.0, -2.0]);
                            u.extend([1.0, 1.0, f1, 0.0]);
                        }
                    }
                }
                None => u.extend([0.0; 20]),
            }
        }
        for m in &mats {
            u.extend(m.to_cols_array());
        }
        debug_assert_eq!(u.len() * 4, TAILLE_UNIFORME);
        q.write_buffer(&et.uniforme, 0, bytemuck::cast_slice(&u));
        // ---- cartes d'ombre à refaire
        let cubes = &et.cubes.as_ref().unwrap().0;
        for i in a_faire {
            for f in 0..6 {
                let couche = (emplacement[i] * 6 + f) as u32;
                self.passe(enc, cubes, couche, (emplacement[i] * 6 + f) as u32, bib, monde);
            }
        }
        if avec_dir {
            let s = e.soleil.map_or(Vec3::Z, |s| s.0);
            let mut cle = [0u32; 9];
            cle[..6].copy_from_slice(&bits(&[e.bornes.0.x, e.bornes.0.y, e.bornes.0.z, e.bornes.1.x, e.bornes.1.y, e.bornes.1.z]));
            cle[6..].copy_from_slice(&bits(&[s.x, s.y, s.z]));
            if et.cle_dir != Some(cle) {
                et.cle_dir = Some(cle);
                self.passe(enc, et.soleil.as_ref().unwrap(), 0, (6 * MAX_OMBRES) as u32, bib, monde);
                for j in 0..N_CIEL {
                    self.passe(enc, et.ciel.as_ref().unwrap(), j as u32, (6 * MAX_OMBRES + 1 + j) as u32, bib, monde);
                }
            }
        }
    }

    /// Dessine la profondeur de ce qui fait de l'ombre dans une couche, vue par la matrice `m`.
    fn passe(&self, enc: &mut wgpu::CommandEncoder, t: &wgpu::Texture, couche: u32, m: u32, bib: &Bibliotheque, monde: &MondeGpu) {
        let vue = t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: couche, array_layer_count: Some(1), ..Default::default() });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ombre"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &vue, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
            ..Default::default()
        });
        rp.set_bind_group(0, &self.mats_bg, &[m * PAS as u32]);
        if monde.ombre_n > 0 {
            rp.set_pipeline(&self.pipe_monde);
            rp.set_vertex_buffer(0, monde.world_vb.slice(..));
            rp.set_index_buffer(monde.ombre_ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..monde.ombre_n, 0, 0..1);
        }
        rp.set_pipeline(&self.pipe_prop);
        rp.set_vertex_buffer(0, bib.model_vb.slice(..));
        rp.set_vertex_buffer(1, monde.instances.slice(..));
        rp.set_index_buffer(bib.model_ib.slice(..), wgpu::IndexFormat::Uint32);
        for (p, _, _, geo, _) in &monde.draws {
            if let (Pass::Opaque, Geo::Prop { index_start, index_count, base_vertex, inst_start, inst_count }) = (p, geo) {
                rp.draw_indexed(*index_start..index_start + index_count, *base_vertex, *inst_start..inst_start + inst_count);
            }
        }
    }
}

/// Octets de l'uniforme : 6 vecteurs d'en-tête, les directions du ciel, les lumières (5 vecteurs), les matrices.
pub(crate) const TAILLE_UNIFORME: usize = 16 * 6 + 16 * N_CIEL + 80 * MAX_LUMIERES + 64 * N_MATS;

fn profondeur(d: &wgpu::Device, taille: u32, couches: u32) -> wgpu::Texture {
    d.create_texture(&wgpu::TextureDescriptor {
        label: Some("ombres"),
        size: wgpu::Extent3d { width: taille, height: taille, depth_or_array_layers: couches.max(1) },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn groupe(d: &wgpu::Device, layout: &wgpu::BindGroupLayout, u: &wgpu::Buffer, cubes: &wgpu::Texture, soleil: &wgpu::Texture, ciel: &wgpu::Texture, s: &wgpu::Sampler) -> wgpu::BindGroup {
    let vue = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
    let (a, b, c) = (vue(cubes), vue(soleil), vue(ciel));
    d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("éclairage"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: u.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&a) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&b) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&c) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(s) },
        ],
    })
}
