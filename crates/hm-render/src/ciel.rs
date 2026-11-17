//! Le ciel dans la vue 3D, comme en jeu : le ciel 2D (six matériaux `skybox/<skyname>rt|bk|lf|ft|up|dn` sur un
//! cube centré sur la caméra) puis la skybox 3D (le contenu de la boîte de `sky_camera`, redessiné à son échelle
//! autour de la caméra), derrière la map ; les faces `toolsskybox` laissent voir les deux.
//!
//! Faces du ciel 2D : le wiki Valve (« Skybox (2D) ») les donne en axes DirectX sans ceux de Hammer. Mesuré sur des
//! ciels du jeu par le raccord des bords (test `ciel_2d_raccorde` de `tests/ciel.rs`) : rt = +X, bk = +Y, lf = −X,
//! ft = −Y, up = +Z, dn = −Z, chaque face orientée comme dans le code du ciel de Quake (`st_to_vec`) dont Source a
//! hérité ; les côtés de demi-hauteur (512 × 256) couvrent le haut de la face. L'orientation de dn n'a pas pu être
//! vérifiée : les ciels du jeu ont des bords de dn unis.
use glam::{Mat4, Vec3};

use crate::vivant::{Bibliotheque, Cibles, MondeGpu, WVertex};
use crate::{pipe_index, srgb_to_linear, Pass, Renderer, View};

/// Les suffixes des six faces, dans l'ordre de `CielRendu::faces`.
pub const SUFFIXES: [&str; 6] = ["rt", "bk", "lf", "ft", "up", "dn"];

/// Ce que la vue montre du ciel.
#[derive(Clone, Copy, Debug)]
pub struct CielRendu {
    /// Matériaux des six faces du ciel 2D (indices de la bibliothèque), dans l'ordre de `SUFFIXES`.
    pub faces: Option<[u32; 6]>,
    /// La texture de la face est de demi-hauteur (côtés de 512 × 256 de beaucoup de ciels de HL2).
    pub demi: [bool; 6],
    /// Skybox 3D : origine de `sky_camera`, échelle, boîte de son contenu.
    pub ciel3d: Option<(Vec3, f32, (Vec3, Vec3))>,
}

/// Le point du cube (demi-côté 1) pour la face `i` et ses coordonnées `s`, `t` dans [-1, 1].
fn point(i: usize, s: f32, t: f32) -> Vec3 {
    let b = [s, t, 1.0];
    const ST: [[i32; 3]; 6] = [[3, -1, 2], [-3, 1, 2], [1, 3, 2], [-1, -3, 2], [-2, -1, 3], [2, -1, -3]];
    let mut v = [0.0f32; 3];
    for j in 0..3 {
        let k = ST[i][j];
        v[j] = if k < 0 { -b[(-k - 1) as usize] } else { b[(k - 1) as usize] };
    }
    Vec3::from(v)
}

/// La face du cube (`point`) où va chaque suffixe : rt +X, bk +Y, lf −X, ft −Y, up +Z, dn −Z.
const FACE: [usize; 6] = [0, 2, 1, 3, 4, 5];

/// Les sommets et indices du cube du ciel : les six faces dans l'ordre de `SUFFIXES`, puis les mêmes pour une
/// texture de demi-hauteur (le haut de la face seulement, sa dernière rangée étirée en dessous), 12 indices par face
/// (les deux côtés).
pub(crate) fn cube() -> (Vec<WVertex>, Vec<u32>) {
    let (mut v, mut idx) = (Vec::new(), Vec::new());
    for demi in [false, true] {
        for k in 0..6 {
            let i = FACE[k];
            let b = v.len() as u32;
            for (s, t) in [(-1.0f32, -1.0f32), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)] {
                let p = point(i, s, t) * 10.0;
                let vv = if demi { 1.0 - t } else { 1.0 - (t + 1.0) * 0.5 };
                v.push(WVertex { pos: p.to_array(), normal: (-point(i, 0.0, 0.0)).to_array(), uv: [(s + 1.0) * 0.5, vv], object: 0, alpha: 0.0 });
            }
            idx.extend([b, b + 1, b + 2, b, b + 2, b + 3, b, b + 2, b + 1, b, b + 3, b + 2]);
        }
    }
    (v, idx)
}

impl Renderer {
    /// Dessine le ciel dans les cibles de la vue, avant la vue elle-même (qui garde alors la couleur : option
    /// `ciel` de `OptionsVue`). `constantes` : deux tampons de constantes propres à cette vue.
    pub fn dessiner_ciel(&self, bib: &Bibliotheque, monde: &MondeGpu, view: &View, cibles: &Cibles, ciel: &CielRendu, constantes: &[(wgpu::Buffer, wgpu::BindGroup); 2], eclaire: bool, enc: &mut wgpu::CommandEncoder) {
        let (w, h) = (cibles.largeur, cibles.hauteur);
        let aspect = w as f32 / h as f32;
        let cv = cibles.couleur.create_view(&Default::default());
        let iv = cibles.ids.create_view(&Default::default());
        let dv = cibles.profondeur.create_view(&Default::default());
        let l = &view.light;
        let groupe_eclairage = if eclaire { self.eclairage.groupe_actif() } else { None };
        let eclairage = groupe_eclairage.as_ref().unwrap_or(&self.eclairage.eteint);
        // ---- ciel 2D : la caméra au centre du cube, seule sa direction compte
        let mut cam0 = view.camera;
        cam0.eye = Vec3::ZERO;
        let g = self.globals_de(view, w, h, Some(cam0.view_proj(aspect)), None);
        self.gpu.queue.write_buffer(&constantes[0].0, 0, bytemuck::bytes_of(&g));
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ciel 2D"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment { view: &cv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: srgb_to_linear(l.sky[0]), g: srgb_to_linear(l.sky[1]), b: srgb_to_linear(l.sky[2]), a: 1.0 }), store: wgpu::StoreOp::Store } }),
                    Some(wgpu::RenderPassColorAttachment { view: &iv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                ..Default::default()
            });
            if let Some(faces) = ciel.faces {
                rp.set_bind_group(0, &constantes[0].1, &[]);
                rp.set_bind_group(2, &self.eclairage.eteint, &[]);
                rp.set_pipeline(&self.pipelines[pipe_index(false, Pass::Opaque, false)]);
                rp.set_vertex_buffer(0, bib.ciel_vb.slice(..));
                rp.set_index_buffer(bib.ciel_ib.slice(..), wgpu::IndexFormat::Uint32);
                for (i, m) in faces.iter().enumerate() {
                    let k = (i + if ciel.demi[i] { 6 } else { 0 }) as u32;
                    rp.set_bind_group(1, &bib.groupes[*m as usize], &[]);
                    rp.draw_indexed(k * 12..k * 12 + 12, 0, 0..1);
                }
            }
        }
        // ---- skybox 3D : son contenu ramené autour de la caméra (p ↦ (p − origine) × échelle), découpé à sa boîte
        let Some((origine, echelle, boite)) = ciel.ciel3d else { return };
        let m = Mat4::from_scale(Vec3::splat(echelle)) * Mat4::from_translation(-origine);
        let mut g = self.globals_de(view, w, h, Some(view.camera.view_proj(aspect) * m), Some(boite));
        g.eye[3] = 0.0;
        self.gpu.queue.write_buffer(&constantes[1].0, 0, bytemuck::bytes_of(&g));
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ciel 3D"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment { view: &cv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } }),
                Some(wgpu::RenderPassColorAttachment { view: &iv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
            ..Default::default()
        });
        rp.set_bind_group(0, &constantes[1].1, &[]);
        rp.set_bind_group(2, eclairage, &[]);
        // comme en jeu : ni ciel ni faces d'outil (nodraw…), que l'éditeur dessine sinon avec leur vraie texture
        self.dessiner_lots(&mut rp, bib, monde, false, true, true);
    }
}
