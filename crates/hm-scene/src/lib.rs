//! La scène : ce que le rendu, les requêtes et les règles consultent. Elle est construite d'un VMF et d'un système
//! de fichiers du jeu : triangles du monde par matériau, instances de modèles, table des objets.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{DMat3, DVec3, Mat4, Vec3};
use hm_assets::{load_collision, load_material, load_model, load_texture, Hull, Material, Model, Texture, Vfs};
use hm_geom::{angles_to_mat, build_brush, Brush};
use hm_vmf::{Node, Vmf};
use rayon::prelude::*;

/// Classes d'entités à solides que le joueur ne voit pas : masquées par défaut, affichables sur demande.
const HIDDEN_CLASSES: &[&str] = &[
    "func_areaportal",
    "func_areaportalwindow",
    "func_viscluster",
    "func_occluder",
    "func_clip_vphysics",
    "func_ladder",
    "func_nav_blocker",
    "func_nav_avoid",
    "func_precipitation",
    "func_smokevolume",
    "func_dustmotes",
    "func_vehicleclip",
    "env_embers",
];

fn hidden_class(class: &str) -> bool {
    HIDDEN_CLASSES.contains(&class) || class.starts_with("trigger_")
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct WVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub object: u32,
    /// Mélange vers la seconde texture d'un matériau WorldVertexTransition (alpha d'un displacement), de 0 à 1.
    pub alpha: f32,
}

/// Les triangles du monde qui partagent un matériau.
#[derive(Clone, Debug)]
pub struct Batch {
    pub material: u32,
    /// Solides d'une entité masquée par défaut (déclencheur, areaportal…).
    pub hidden: bool,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjKind {
    WorldBrush,
    EntityBrush,
    Prop,
}

/// Un objet désignable : un solide ou un modèle posé. Son rang dans `Scene::objects` est son identifiant dans
/// l'image d'identifiants ; le rang 0 veut dire « rien ».
#[derive(Clone, Debug)]
pub struct Object {
    pub kind: ObjKind,
    /// `id` du solide ou de l'entité dans le VMF.
    pub vmf_id: u64,
    pub class: String,
    pub name: String,
    pub model: String,
    /// Matériau dominant d'un solide (celui de sa plus grande face visible).
    pub material: String,
    pub min: Vec3,
    pub max: Vec3,
    pub hidden: bool,
}

/// Un solide gardé pour les requêtes d'espace : ses plans (`n · x <= d` à l'intérieur) et sa boîte.
#[derive(Clone, Debug)]
pub struct Solid {
    pub object: u32,
    pub planes: Vec<[f32; 4]>,
    pub min: Vec3,
    pub max: Vec3,
    /// Fait de `tools/toolsskybox` : c'est le ciel, pas un plafond.
    pub sky: bool,
    /// Toutes ses faces sont parallèles aux axes.
    pub boxy: bool,
}

pub struct SceneModel {
    pub name: String,
    pub model: Option<Model>,
    /// Convexes de collision (fichier `.phy`), en repère du modèle. Vide : le modèle ne bloque rien.
    pub collision: Vec<Hull>,
    pub error: Option<String>,
    /// `materials[skin][mesh]` : indice dans `Scene::materials`.
    pub materials: Vec<Vec<u32>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub model: u32,
    pub skin: u32,
    pub transform: Mat4,
    pub object: u32,
    pub tint: [f32; 4],
    /// Bloque le passage (classe et clé `solid`), si son modèle a une collision.
    pub solid: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Sun {
    /// Vers le soleil.
    pub dir: Vec3,
    pub color: Vec3,
    pub ambient: Vec3,
}

#[derive(Clone, Debug)]
pub struct PointEntity {
    pub class: String,
    pub name: String,
    pub pos: Vec3,
    pub angles: Vec3,
    pub vmf_id: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub timings: Vec<(String, f32)>,
    pub missing_materials: Vec<String>,
    pub missing_textures: Vec<String>,
    pub missing_models: Vec<String>,
    pub bad_brushes: usize,
    pub solids: usize,
    pub faces: usize,
    pub displacements: usize,
    pub world_triangles: usize,
    pub prop_triangles: u64,
}

pub struct SceneOptions {
    /// Côté maximal des textures gardées en mémoire.
    pub max_texture: u32,
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self { max_texture: 1024 }
    }
}

pub struct Scene {
    pub path: String,
    /// Empreinte BLAKE3 du VMF.
    pub revision: String,
    pub vmf: Vmf,
    pub materials: Vec<Material>,
    /// Texture de base de chaque matériau : indice dans `textures`.
    pub material_texture: Vec<Option<u32>>,
    /// La seconde texture (`$basetexture2`) des matériaux qui en ont une.
    pub material_texture2: Vec<Option<u32>>,
    pub textures: Vec<Texture>,
    pub vertices: Vec<WVertex>,
    pub batches: Vec<Batch>,
    pub models: Vec<SceneModel>,
    pub props: Vec<Prop>,
    pub objects: Vec<Object>,
    /// Tous les solides du VMF, dans l'ordre du fichier (monde, puis entités).
    pub brushes: Vec<SceneBrush>,
    /// Solides qui bloquent le joueur (monde, détail, portes en brosses, clips), hors displacements et eau.
    pub solids: Vec<Solid>,
    /// Boîte de chaque maille des displacements : le terrain, pour les requêtes d'espace.
    pub disp_cells: Vec<(Vec3, Vec3)>,
    pub points: Vec<PointEntity>,
    pub sun: Option<Sun>,
    pub skyname: String,
    pub min: Vec3,
    pub max: Vec3,
    pub report: Report,
}

/// Un solide du VMF avec ses polygones, gardé pour les règles de vérification.
pub struct SceneBrush {
    pub brush: Brush,
    pub class: String,
    pub name: String,
    pub world: bool,
    /// `id` de l'entité qui le porte (0 pour le monde).
    pub entity_id: u64,
    /// Rang dans `Scene::objects` (0 : solide sans polygone, donc sans objet).
    pub object: u32,
}

fn v3(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

fn entity_transform(e: &Node) -> Mat4 {
    let o = e.vec3("origin").unwrap_or([0.0; 3]);
    let a = e.vec3("angles").unwrap_or([0.0; 3]);
    let s = e.f64("modelscale").or_else(|| e.f64("uniformscale")).filter(|s| *s > 0.0).unwrap_or(1.0);
    let r: DMat3 = angles_to_mat(a[0], a[1], a[2]) * DMat3::from_diagonal(DVec3::splat(s));
    let m = glam::DMat4::from_cols(r.x_axis.extend(0.0), r.y_axis.extend(0.0), r.z_axis.extend(0.0), DVec3::from(o).extend(1.0));
    m.as_mat4()
}

impl Scene {
    pub fn load(path: impl AsRef<Path>, vfs: &Vfs, opts: &SceneOptions) -> Result<Scene> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?;
        Scene::from_bytes(path, bytes, vfs, opts)
    }

    /// Construit la scène d'un VMF déjà en mémoire (une map en cours d'édition). `path` ne sert qu'à la nommer.
    pub fn from_bytes(path: &Path, bytes: Vec<u8>, vfs: &Vfs, opts: &SceneOptions) -> Result<Scene> {
        let mut report = Report::default();
        let mut t = Instant::now();
        let mut lap = |report: &mut Report, what: &str| {
            report.timings.push((what.to_string(), t.elapsed().as_secs_f32()));
            t = Instant::now();
        };
        let revision = hm_vmf::revision(&bytes);
        let vmf = Vmf::parse(bytes).map_err(|e| anyhow::anyhow!("{} : {e}", path.display()))?;
        lap(&mut report, "lecture du VMF");

        // ---------------------------------------------------------------- solides
        let mut solids: Vec<(&Node, String, String, bool, u64)> = Vec::new();
        if let Some(w) = vmf.world() {
            solids.extend(w.solids().map(|s| (s, "worldspawn".to_string(), String::new(), true, 0)));
        }
        for e in vmf.entities() {
            let class = e.classname().to_ascii_lowercase();
            let name = e.get("targetname").unwrap_or("").to_string();
            let eid = e.id().unwrap_or(0);
            solids.extend(e.solids().map(|s| (s, class.clone(), name.clone(), false, eid)));
        }
        let mut brushes: Vec<SceneBrush> = solids.par_iter().map(|(s, class, name, world, eid)| SceneBrush { brush: build_brush(s), class: class.clone(), name: name.clone(), world: *world, entity_id: *eid, object: 0 }).collect();
        drop(solids);
        report.solids = brushes.len();
        report.bad_brushes = brushes.iter().filter(|b| !b.brush.issues.is_empty()).count();
        lap(&mut report, "polygones des solides");

        // ---------------------------------------------------------------- modèles
        let mut model_names: BTreeSet<String> = BTreeSet::new();
        for e in vmf.entities() {
            if let Some(m) = e.get("model") {
                let m = hm_assets::model::normalize_model(m);
                if m.ends_with(".mdl") {
                    model_names.insert(m);
                }
            }
        }
        let model_names: Vec<String> = model_names.into_iter().collect();
        let loaded: Vec<Result<Model, String>> = model_names.par_iter().map(|n| load_model(vfs, n)).collect();
        let mut collisions: Vec<Vec<Hull>> = model_names.par_iter().map(|n| load_collision(vfs, n).unwrap_or_default()).collect();
        lap(&mut report, "modèles");

        // ---------------------------------------------------------------- matériaux
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        for b in &brushes {
            for f in &b.brush.faces {
                wanted.insert(f.material.clone());
            }
        }
        for m in loaded.iter().flatten() {
            for skin in &m.skins {
                for cands in skin {
                    wanted.extend(cands.iter().cloned());
                }
            }
        }
        let wanted: Vec<String> = wanted.into_iter().collect();
        let mats: Vec<Material> = wanted.par_iter().map(|n| load_material(vfs, n)).collect();
        let by_name: HashMap<&str, usize> = wanted.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
        // seuls les matériaux réellement retenus entrent dans la scène
        let mut materials: Vec<Material> = Vec::new();
        let mut mat_index: HashMap<String, u32> = HashMap::new();
        let mut intern = |name: &str, materials: &mut Vec<Material>| -> u32 {
            if let Some(&i) = mat_index.get(name) {
                return i;
            }
            let m = by_name.get(name).map(|&i| mats[i].clone()).unwrap_or_else(|| Material::missing(name));
            materials.push(m);
            mat_index.insert(name.to_string(), materials.len() as u32 - 1);
            materials.len() as u32 - 1
        };
        let mut face_mats: Vec<Vec<u32>> = Vec::with_capacity(brushes.len());
        for b in &brushes {
            face_mats.push(b.brush.faces.iter().map(|f| intern(&f.material, &mut materials)).collect());
        }
        let mut models: Vec<SceneModel> = Vec::with_capacity(model_names.len());
        for (k, (name, res)) in model_names.iter().zip(loaded).enumerate() {
            let collision = std::mem::take(&mut collisions[k]);
            match res {
                Ok(m) => {
                    let ids = m
                        .skins
                        .iter()
                        .map(|skin| {
                            skin.iter()
                                .map(|cands| {
                                    let pick = cands.iter().find(|c| by_name.get(c.as_str()).map_or(false, |&i| mats[i].found)).or(cands.first());
                                    intern(pick.map(|s| s.as_str()).unwrap_or(""), &mut materials)
                                })
                                .collect()
                        })
                        .collect();
                    report.prop_triangles += 0;
                    models.push(SceneModel { name: name.clone(), model: Some(m), collision, error: None, materials: ids });
                }
                Err(e) => {
                    report.missing_models.push(e.clone());
                    models.push(SceneModel { name: name.clone(), model: None, collision, error: Some(e), materials: Vec::new() });
                }
            }
        }
        report.missing_materials = materials.iter().filter(|m| !m.found && m.tool.is_none() && !m.name.is_empty()).map(|m| m.name.clone()).collect();
        lap(&mut report, "matériaux");

        // ---------------------------------------------------------------- textures
        let tex_names: Vec<String> = materials.iter().flat_map(|m| m.base.iter().chain(&m.base2).cloned()).collect::<BTreeSet<_>>().into_iter().collect();
        let loaded_tex: Vec<Option<Texture>> = tex_names.par_iter().map(|n| load_texture(vfs, n, opts.max_texture)).collect();
        let mut textures = Vec::new();
        let mut tex_index: HashMap<&str, u32> = HashMap::new();
        for (n, t) in tex_names.iter().zip(loaded_tex) {
            match t {
                Some(t) => {
                    tex_index.insert(n.as_str(), textures.len() as u32);
                    textures.push(t);
                }
                None => report.missing_textures.push(n.clone()),
            }
        }
        let material_texture: Vec<Option<u32>> = materials.iter().map(|m| m.base.as_deref().and_then(|b| tex_index.get(b).copied())).collect();
        let material_texture2: Vec<Option<u32>> = materials.iter().map(|m| m.base2.as_deref().and_then(|b| tex_index.get(b).copied())).collect();
        lap(&mut report, "textures");

        // ---------------------------------------------------------------- maillage du monde
        let mut objects = vec![Object { kind: ObjKind::WorldBrush, vmf_id: 0, class: String::new(), name: String::new(), model: String::new(), material: String::new(), min: Vec3::ZERO, max: Vec3::ZERO, hidden: true }];
        let mut solid_list: Vec<Solid> = Vec::new();
        let mut disp_cells: Vec<(Vec3, Vec3)> = Vec::new();
        let mut vertices: Vec<WVertex> = Vec::new();
        let mut batch_of: HashMap<(u32, bool), usize> = HashMap::new();
        let mut batches: Vec<Batch> = Vec::new();
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (b, fm) in brushes.iter_mut().zip(&face_mats) {
            let Some((bmin, bmax)) = b.brush.bounds() else { continue };
            let hidden = !b.world && hidden_class(&b.class);
            let object = objects.len() as u32;
            b.object = object;
            let mut best = (0.0f64, "");
            // d'une brosse à displacement, vbsp ne garde que les displacements
            let has_disp = b.brush.faces.iter().any(|f| f.disp.is_some());
            for (f, &mi) in b.brush.faces.iter().zip(fm) {
                report.faces += 1;
                if has_disp && f.disp.is_none() {
                    continue;
                }
                let mat = &materials[mi as usize];
                let (tw, th) = material_texture[mi as usize].map(|t| (textures[t as usize].full_width as f64, textures[t as usize].full_height as f64)).unwrap_or((512.0, 512.0));
                let key = (mi, hidden);
                let bi = *batch_of.entry(key).or_insert_with(|| {
                    batches.push(Batch { material: mi, hidden, indices: Vec::new() });
                    batches.len() - 1
                });
                let n = v3(f.plane.n);
                let base = vertices.len() as u32;
                if let Some(d) = &f.disp {
                    report.displacements += 1;
                    let k = d.k();
                    // normales : somme des normales des triangles voisins, orientées vers l'extérieur
                    let mut normals = vec![DVec3::ZERO; k * k];
                    let mut tris: Vec<[usize; 3]> = Vec::with_capacity((k - 1) * (k - 1) * 2);
                    for i in 0..k - 1 {
                        for j in 0..k - 1 {
                            let (a, bb, c, dd) = (i * k + j, (i + 1) * k + j, (i + 1) * k + j + 1, i * k + j + 1);
                            tris.push([a, bb, c]);
                            tris.push([a, c, dd]);
                        }
                    }
                    for t in &tris {
                        let nn = (d.points[t[2]] - d.points[t[0]]).cross(d.points[t[1]] - d.points[t[0]]);
                        for &v in t {
                            normals[v] += nn;
                        }
                    }
                    for v in 0..k * k {
                        let uv = f.uv_texels(d.flat[v]);
                        vertices.push(WVertex { pos: v3(d.points[v]), normal: v3(normals[v].normalize_or(f.plane.n)), uv: [(uv.x / tw) as f32, (uv.y / th) as f32], object, alpha: d.alphas[v] / 255.0 });
                    }
                    batches[bi].indices.extend(tris.iter().flat_map(|t| t.iter().map(|&v| base + v as u32)));
                    report.world_triangles += tris.len();
                } else {
                    for p in &f.poly {
                        let uv = f.uv_texels(*p);
                        vertices.push(WVertex { pos: v3(*p), normal: n, uv: [(uv.x / tw) as f32, (uv.y / th) as f32], object, alpha: 0.0 });
                    }
                    for i in 1..f.poly.len() as u32 - 1 {
                        batches[bi].indices.extend([base, base + i, base + i + 1]);
                    }
                    report.world_triangles += f.poly.len() - 2;
                }
                if mat.tool.is_none() {
                    let area: f64 = (1..f.poly.len().saturating_sub(1)).map(|i| (f.poly[i] - f.poly[0]).cross(f.poly[i + 1] - f.poly[0]).length()).sum();
                    if area >= best.0 {
                        best = (area, f.material.as_str());
                    }
                }
            }
            let (bmin, bmax) = (bmin.as_vec3(), bmax.as_vec3());
            // ce que le joueur heurte : pas les entités masquées, pas l'eau, pas les brosses non solides
            let water = fm.iter().any(|&mi| materials[mi as usize].shader.starts_with("water"));
            let non_solid = matches!(b.class.as_str(), "func_illusionary" | "func_dustcloud" | "func_fish_pool" | "env_bubbles");
            if has_disp {
                for f in &b.brush.faces {
                    if let Some(d) = &f.disp {
                        let k = d.k();
                        for i in 0..k - 1 {
                            for j in 0..k - 1 {
                                let q = [d.points[i * k + j], d.points[(i + 1) * k + j], d.points[(i + 1) * k + j + 1], d.points[i * k + j + 1]];
                                let lo = q.iter().fold(q[0], |a, p| a.min(*p)).as_vec3();
                                let hi = q.iter().fold(q[0], |a, p| a.max(*p)).as_vec3();
                                disp_cells.push((lo, hi));
                            }
                        }
                    }
                }
            } else if !hidden && !water && !non_solid {
                let n_sky = fm.iter().filter(|&&mi| materials[mi as usize].tool == Some(hm_assets::Tool::Sky)).count();
                let planes: Vec<[f32; 4]> = b.brush.faces.iter().map(|f| [f.plane.n.x as f32, f.plane.n.y as f32, f.plane.n.z as f32, f.plane.d as f32]).collect();
                let boxy = planes.iter().all(|p| p[0].abs().max(p[1].abs()).max(p[2].abs()) > 0.9999);
                solid_list.push(Solid { object, planes, min: bmin, max: bmax, sky: n_sky * 2 > fm.len(), boxy });
            }
            if !hidden {
                lo = lo.min(bmin);
                hi = hi.max(bmax);
            }
            objects.push(Object {
                kind: if b.world { ObjKind::WorldBrush } else { ObjKind::EntityBrush },
                vmf_id: b.brush.id,
                class: b.class.clone(),
                name: b.name.clone(),
                model: String::new(),
                material: best.1.to_string(),
                min: bmin,
                max: bmax,
                hidden,
            });
        }
        lap(&mut report, "maillage du monde");

        // ---------------------------------------------------------------- entités ponctuelles
        let model_id: HashMap<&str, u32> = models.iter().enumerate().map(|(i, m)| (m.name.as_str(), i as u32)).collect();
        let mut props = Vec::new();
        let mut points = Vec::new();
        let mut sun = None;
        for e in vmf.entities() {
            let class = e.classname().to_ascii_lowercase();
            let name = e.get("targetname").unwrap_or("").to_string();
            let origin = e.vec3("origin").map(|o| Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32));
            let mdl = e.get("model").map(hm_assets::model::normalize_model).filter(|m| m.ends_with(".mdl"));
            if let (Some(mdl), Some(_)) = (&mdl, origin) {
                let mi = model_id[mdl.as_str()];
                let transform = entity_transform(e);
                let (bmin, bmax) = match &models[mi as usize].model {
                    Some(m) => {
                        report.prop_triangles += m.triangles as u64;
                        (Vec3::from(m.bounds.0), Vec3::from(m.bounds.1))
                    }
                    None => (Vec3::splat(-16.0), Vec3::splat(16.0)),
                };
                // boîte englobante de la boîte du modèle transformée
                let (mut wmin, mut wmax) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for c in 0..8 {
                    let p = transform.transform_point3(Vec3::new(if c & 1 == 0 { bmin.x } else { bmax.x }, if c & 2 == 0 { bmin.y } else { bmax.y }, if c & 4 == 0 { bmin.z } else { bmax.z }));
                    wmin = wmin.min(p);
                    wmax = wmax.max(p);
                }
                lo = lo.min(wmin);
                hi = hi.max(wmax);
                let skin = e.f64("skin").unwrap_or(0.0).max(0.0) as u32;
                let tint = e
                    .vec3("rendercolor")
                    .map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0])
                    .unwrap_or([1.0; 4]);
                let solid_key = e.f64("solid").unwrap_or(6.0) != 0.0;
                let solid = solid_key && (class == "prop_static" || class.starts_with("prop_dynamic") || class.starts_with("prop_door"));
                props.push(Prop { model: mi, skin, transform, object: objects.len() as u32, tint, solid });
                objects.push(Object { kind: ObjKind::Prop, vmf_id: e.id().unwrap_or(0), class: class.clone(), name: name.clone(), model: mdl.clone(), material: String::new(), min: wmin, max: wmax, hidden: false });
            }
            if class == "light_environment" {
                let a = e.vec3("angles").unwrap_or([0.0; 3]);
                let pitch = e.f64("pitch").filter(|p| *p != 0.0).unwrap_or(a[0]);
                let (sp, cp) = pitch.to_radians().sin_cos();
                let (sy, cy) = a[1].to_radians().sin_cos();
                // sens de la lumière dans vrad : (cos yaw · cos pitch, sin yaw · cos pitch, sin pitch) ; pitch négatif = vers le bas
                let travel = DVec3::new(cy * cp, sy * cp, sp);
                let col = |k: &str| {
                    let mut v = Vec::new();
                    hm_vmf::parse_floats(e.get(k).unwrap_or("255 255 255 200"), &mut v);
                    while v.len() < 4 {
                        v.push(200.0);
                    }
                    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) / 255.0 * (v[3] as f32 / 255.0)
                };
                sun = Some(Sun { dir: (-travel).as_vec3(), color: col("_light"), ambient: col("_ambient") });
            }
            if let Some(o) = origin {
                if mdl.is_none() && e.solids().next().is_none() {
                    let a = e.vec3("angles").unwrap_or([0.0; 3]);
                    points.push(PointEntity { class, name, pos: o, angles: Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32), vmf_id: e.id().unwrap_or(0) });
                }
            }
        }
        lap(&mut report, "entités");
        let skyname = vmf.world().and_then(|w| w.get("skyname")).unwrap_or("").to_string();
        if lo.x > hi.x {
            (lo, hi) = (Vec3::ZERO, Vec3::ZERO);
        }
        Ok(Scene { path: path.display().to_string(), revision, vmf, materials, material_texture, material_texture2, textures, vertices, batches, models, props, objects, brushes, solids: solid_list, disp_cells, points, sun, skyname, min: lo, max: hi, report })
    }

    /// Premier obstacle sur un rayon : (distance, rang de l'objet). Les solides et les displacements sont exacts ;
    /// les modèles sont approchés par leur boîte englobante.
    pub fn rayon(&self, o: Vec3, d: Vec3) -> Option<(f32, u32)> {
        let mut best: Option<(f32, u32)> = None;
        for s in &self.solids {
            // convexe : portion [t0, t1] du rayon à l'intérieur de tous les demi-espaces
            let (mut t0, mut t1) = (0.0f32, f32::MAX);
            let mut ok = true;
            for p in &s.planes {
                let n = Vec3::new(p[0], p[1], p[2]);
                let (dn, dist) = (n.dot(d), n.dot(o) - p[3]);
                if dn.abs() < 1e-7 {
                    if dist > 0.0 {
                        ok = false;
                        break;
                    }
                } else if dn < 0.0 {
                    t0 = t0.max(-dist / dn);
                } else {
                    t1 = t1.min(-dist / dn);
                }
                if t0 > t1 {
                    ok = false;
                    break;
                }
            }
            if ok && t0 > 0.0 && best.map_or(true, |b| t0 < b.0) {
                best = Some((t0, s.object));
            }
        }
        // displacements : les deux triangles de chaque maille, découpée comme pour le dessin ; la boîte des points
        // écarte d'abord les autres (celle de l'objet est celle de la brosse de base, sans le relief)
        for b in &self.brushes {
            if b.object == 0 || self.objects[b.object as usize].hidden {
                continue;
            }
            for d_ in b.brush.faces.iter().filter_map(|f| f.disp.as_ref()) {
                let pts: Vec<Vec3> = d_.points.iter().map(|p| p.as_vec3()).collect();
                let (lo, hi) = pts.iter().fold((Vec3::MAX, Vec3::MIN), |(a, c), p| (a.min(*p), c.max(*p)));
                if boite_rayon(o, d, lo - Vec3::ONE, hi + Vec3::ONE).is_none() {
                    continue;
                }
                let k = d_.k();
                for i in 0..k - 1 {
                    for j in 0..k - 1 {
                        let (a, bb, c, dd) = (pts[i * k + j], pts[(i + 1) * k + j], pts[(i + 1) * k + j + 1], pts[i * k + j + 1]);
                        for t in [[a, bb, c], [a, c, dd]] {
                            if let Some(dist) = triangle_rayon(o, d, t) {
                                if best.map_or(true, |x| dist < x.0) {
                                    best = Some((dist, b.object));
                                }
                            }
                        }
                    }
                }
            }
        }
        for (i, obj) in self.objects.iter().enumerate() {
            if obj.kind != ObjKind::Prop || obj.hidden {
                continue;
            }
            let (mut t0, mut t1) = (0.0f32, f32::MAX);
            for a in 0..3 {
                if d[a].abs() < 1e-7 {
                    if o[a] < obj.min[a] || o[a] > obj.max[a] {
                        t0 = f32::MAX;
                    }
                } else {
                    let (x, y) = ((obj.min[a] - o[a]) / d[a], (obj.max[a] - o[a]) / d[a]);
                    t0 = t0.max(x.min(y));
                    t1 = t1.min(x.max(y));
                }
            }
            if t0 <= t1 && t0 > 0.0 && best.map_or(true, |b| t0 < b.0) {
                best = Some((t0, i as u32));
            }
        }
        best
    }

    /// Résumé en quelques lignes, pour l'état d'une session.
    pub fn summary(&self) -> String {
        let r = &self.report;
        let mut s = format!(
            "{} (révision {})\n{} solides ({} à défaut), {} faces, {} displacements, {} triangles de monde\n{} modèles posés ({} modèles distincts, {:.1} M de triangles), {} matériaux, {} textures\nétendue ({:.0} {:.0} {:.0}) à ({:.0} {:.0} {:.0})",
            self.path, self.revision, r.solids, r.bad_brushes, r.faces, r.displacements, r.world_triangles,
            self.props.len(), self.models.len(), r.prop_triangles as f64 / 1e6, self.materials.len(), self.textures.len(),
            self.min.x, self.min.y, self.min.z, self.max.x, self.max.y, self.max.z
        );
        for (what, list) in [("matériaux introuvables", &r.missing_materials), ("textures introuvables", &r.missing_textures), ("modèles illisibles", &r.missing_models)] {
            if !list.is_empty() {
                s += &format!("\n⚠ {} {what} : {}{}", list.len(), list.iter().take(6).cloned().collect::<Vec<_>>().join(", "), if list.len() > 6 { ", …" } else { "" });
            }
        }
        s += &format!("\nchargement : {}", r.timings.iter().map(|(w, t)| format!("{w} {t:.2} s")).collect::<Vec<_>>().join(", "));
        s
    }
}

/// Entrée du rayon dans une boîte alignée (distance ≥ 0), s'il la traverse.
fn boite_rayon(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::MAX);
    for a in 0..3 {
        if d[a].abs() < 1e-7 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return None;
            }
        } else {
            let (x, y) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
            t0 = t0.max(x.min(y));
            t1 = t1.min(x.max(y));
        }
    }
    (t0 <= t1).then_some(t0)
}

/// Distance du rayon à un triangle, des deux côtés (Möller-Trumbore).
fn triangle_rayon(o: Vec3, d: Vec3, t: [Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let s = o - t[0];
    let u = s.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = e2.dot(q) / det;
    (dist > 1e-4).then_some(dist)
}
