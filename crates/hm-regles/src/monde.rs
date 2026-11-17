//! Les corps d'une map (solides, modèles posés, terrain) sous une forme commune : des convexes en repère du monde,
//! rangés dans une grille pour retrouver vite les voisins d'un endroit.
use std::collections::HashMap;

use glam::{Mat3, Vec3};
use hm_assets::Tool;
use hm_scene::Scene;
use hm_vmf::Node;
use rayon::prelude::*;

use crate::{motif, Config};

#[derive(Clone, Debug)]
pub struct Convexe {
    /// `n · x <= d` à l'intérieur.
    pub plans: Vec<[f32; 4]>,
    pub pts: Vec<Vec3>,
    pub min: Vec3,
    pub max: Vec3,
}

impl Convexe {
    pub fn boite(min: Vec3, max: Vec3) -> Convexe {
        let plans = vec![[1.0, 0.0, 0.0, max.x], [-1.0, 0.0, 0.0, -min.x], [0.0, 1.0, 0.0, max.y], [0.0, -1.0, 0.0, -min.y], [0.0, 0.0, 1.0, max.z], [0.0, 0.0, -1.0, -min.z]];
        let pts = (0..8).map(|c| Vec3::new(if c & 1 == 0 { min.x } else { max.x }, if c & 2 == 0 { min.y } else { max.y }, if c & 4 == 0 { min.z } else { max.z })).collect();
        Convexe { plans, pts, min, max }
    }

    /// Profondeur d'un point dans le convexe : positive à l'intérieur (distance à la face la plus proche),
    /// négative dehors.
    pub fn profondeur(&self, p: Vec3) -> f32 {
        self.plans.iter().map(|pl| pl[3] - (pl[0] * p.x + pl[1] * p.y + pl[2] * p.z)).fold(f32::MAX, f32::min)
    }

    /// Portion `[t0, t1]` du segment `a + t (b − a)`, `t` de 0 à 1, à l'intérieur du convexe.
    pub fn segment(&self, a: Vec3, b: Vec3) -> Option<(f32, f32)> {
        let d = b - a;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for pl in &self.plans {
            let n = Vec3::new(pl[0], pl[1], pl[2]);
            let (dn, dist) = (n.dot(d), n.dot(a) - pl[3]);
            if dn.abs() < 1e-7 {
                if dist > 0.0 {
                    return None;
                }
            } else if dn < 0.0 {
                t0 = t0.max(-dist / dn);
            } else {
                t1 = t1.min(-dist / dn);
            }
            if t0 > t1 {
                return None;
            }
        }
        Some((t0, t1))
    }
}

/// Deux convexes se touchent-ils, à `jeu` près ? Séparation cherchée sur les plans des faces des deux convexes
/// seulement (pas sur les arêtes croisées) : deux convexes séparés par une arête peuvent être dits « en contact ».
/// Ce biais va dans le sens de moins d'alertes.
pub fn se_touchent(a: &Convexe, b: &Convexe, jeu: f32) -> bool {
    if (0..3).any(|k| a.min[k] > b.max[k] + jeu || b.min[k] > a.max[k] + jeu) {
        return false;
    }
    let separe = |x: &Convexe, y: &Convexe| x.plans.iter().any(|pl| y.pts.iter().all(|p| pl[0] * p.x + pl[1] * p.y + pl[2] * p.z - pl[3] > jeu));
    !separe(a, b) && !separe(b, a)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Genre {
    /// Solide du monde.
    Monde,
    /// `func_detail`.
    Detail,
    /// Entité à solides fixe et visible (`func_brush`, `func_illusionary`, `func_breakable`…).
    Brosse,
    /// Entité à solides qui bouge (porte, train, ascenseur) ou attachée à une autre.
    Mobile,
    /// Modèle posé.
    Prop,
    /// Displacement.
    Terrain,
}

pub struct Corps {
    /// Rang dans `Scene::objects`.
    pub objet: u32,
    pub genre: Genre,
    pub convexes: Vec<Convexe>,
    pub min: Vec3,
    pub max: Vec3,
    /// Au moins une face dessinée (pas seulement des textures d'outil).
    pub visible: bool,
    /// Solide de ciel.
    pub ciel: bool,
    /// Rang dans `Scene::props` pour un modèle.
    pub prop: Option<u32>,
    /// Modèle sans collision : sa boîte orientée sert de convexe.
    pub approche: bool,
    /// Suspendu exprès (motif du projet), attaché à une autre entité, ou soumis à la physique.
    pub libre: bool,
}

const CELLULE: f32 = 128.0;
/// Au-delà de cette taille, un convexe va dans la liste des grands plutôt que dans la grille.
const GRAND: f32 = 1536.0;

pub struct Monde<'a> {
    pub scene: &'a Scene,
    pub corps: Vec<Corps>,
    /// Corps de chaque objet de la scène (`u32::MAX` : aucun).
    pub de_objet: Vec<u32>,
    grille: HashMap<(i32, i32, i32), Vec<(u32, u16)>>,
    grands: Vec<(u32, u16)>,
    /// Les corps entiers, par leur boîte (celle du maillage pour un modèle, souvent plus large que sa collision).
    grille_corps: HashMap<(i32, i32, i32), Vec<u32>>,
    grands_corps: Vec<u32>,
    /// Entités du VMF par `id`.
    pub entites: HashMap<u64, &'a Node>,
}

/// Classes d'entités à solides qui bougent en jeu.
const MOBILES: &[&str] = &["func_door", "func_door_rotating", "func_movelinear", "func_tracktrain", "func_train", "func_rotating", "func_physbox", "func_pushable", "func_plat", "func_platrot", "func_water_analog", "func_guntarget", "momentary_door", "momentary_rot_button", "func_rot_button"];

fn cellule(p: Vec3) -> (i32, i32, i32) {
    ((p.x / CELLULE).floor() as i32, (p.y / CELLULE).floor() as i32, (p.z / CELLULE).floor() as i32)
}

impl<'a> Monde<'a> {
    pub fn construire(scene: &'a Scene, config: &Config) -> Monde<'a> {
        let entites: HashMap<u64, &Node> = scene.vmf.entities().filter_map(|e| Some((e.id()?, e))).collect();
        let outil: HashMap<&str, Option<Tool>> = scene.materials.iter().map(|m| (m.name.as_str(), m.tool)).collect();
        let suspendu = |texte: &str| config.suspendus.iter().any(|m| motif(m, texte));
        let mut corps: Vec<Corps> = Vec::new();
        // ---- solides
        for b in &scene.brushes {
            if b.object == 0 {
                continue;
            }
            let o = &scene.objects[b.object as usize];
            if o.hidden {
                continue; // déclencheurs, areaportals, clips de véhicule : ni vus ni heurtés par le joueur
            }
            let outils: Vec<Option<Tool>> = b.brush.faces.iter().map(|f| outil.get(f.material.as_str()).copied().flatten()).collect();
            let visible = outils.iter().any(|t| t.is_none());
            let ciel = outils.iter().filter(|t| **t == Some(Tool::Sky)).count() * 2 > outils.len();
            // tout en outil, hors nodraw et ciel : clip, hint, skip, invisible… ni vu ni porteur
            if !visible && !ciel && !outils.iter().all(|t| *t == Some(Tool::Nodraw)) {
                continue;
            }
            let ent = entites.get(&b.entity_id);
            let attache = ent.and_then(|e| e.get("parentname")).map_or(false, |p| !p.is_empty());
            let genre = if b.brush.faces.iter().any(|f| f.disp.is_some()) {
                Genre::Terrain
            } else if b.world {
                Genre::Monde
            } else if b.class == "func_detail" {
                Genre::Detail
            } else if MOBILES.contains(&b.class.as_str()) || attache {
                Genre::Mobile
            } else {
                Genre::Brosse
            };
            let convexes = if genre == Genre::Terrain {
                // le terrain : une boîte par maille, épaissie vers le bas
                let mut v = Vec::new();
                for f in &b.brush.faces {
                    if let Some(d) = &f.disp {
                        let k = d.k();
                        for i in 0..k - 1 {
                            for j in 0..k - 1 {
                                let q = [d.points[i * k + j], d.points[(i + 1) * k + j], d.points[(i + 1) * k + j + 1], d.points[i * k + j + 1]];
                                let lo = q.iter().fold(q[0], |a, p| a.min(*p)).as_vec3();
                                let hi = q.iter().fold(q[0], |a, p| a.max(*p)).as_vec3();
                                v.push(Convexe::boite(lo - Vec3::Z * 16.0, hi));
                            }
                        }
                    }
                }
                v
            } else {
                let plans: Vec<[f32; 4]> = b.brush.faces.iter().map(|f| [f.plane.n.x as f32, f.plane.n.y as f32, f.plane.n.z as f32, f.plane.d as f32]).collect();
                let mut pts: Vec<Vec3> = Vec::new();
                for f in &b.brush.faces {
                    for p in &f.poly {
                        let p = p.as_vec3();
                        if !pts.iter().any(|q| q.distance_squared(p) < 1e-4) {
                            pts.push(p);
                        }
                    }
                }
                vec![Convexe { plans, pts, min: o.min, max: o.max }]
            };
            if convexes.is_empty() {
                continue;
            }
            corps.push(Corps { objet: b.object, genre, convexes, min: o.min, max: o.max, visible, ciel, prop: None, approche: false, libre: suspendu(&o.name) || attache });
        }
        // ---- modèles
        let props: Vec<Corps> = scene
            .props
            .par_iter()
            .enumerate()
            .map(|(k, p)| {
                let o = &scene.objects[p.object as usize];
                let sm = &scene.models[p.model as usize];
                let m = p.transform;
                let lin = Mat3::from_mat4(m);
                let echelle = lin.x_axis.length().max(1e-6);
                let rot = lin * (1.0 / echelle);
                let tr = m.w_axis.truncate();
                let vers_monde = |plans: &[[f32; 4]], pts: &[[f32; 3]]| {
                    let pts: Vec<Vec3> = pts.iter().map(|q| m.transform_point3(Vec3::from(*q))).collect();
                    let plans = plans
                        .iter()
                        .map(|pl| {
                            let n = rot * Vec3::new(pl[0], pl[1], pl[2]);
                            [n.x, n.y, n.z, pl[3] * echelle + n.dot(tr)]
                        })
                        .collect();
                    let lo = pts.iter().fold(Vec3::splat(f32::MAX), |a, q| a.min(*q));
                    let hi = pts.iter().fold(Vec3::splat(f32::MIN), |a, q| a.max(*q));
                    Convexe { plans, pts, min: lo, max: hi }
                };
                let mut convexes: Vec<Convexe> = sm.collision.iter().map(|h| vers_monde(&h.planes, &h.points)).collect();
                let approche = convexes.is_empty();
                if approche {
                    let (lo, hi) = sm.model.as_ref().map_or((Vec3::splat(-16.0), Vec3::splat(16.0)), |md| (Vec3::from(md.bounds.0), Vec3::from(md.bounds.1)));
                    let b = Convexe::boite(lo, hi);
                    let pts: Vec<[f32; 3]> = b.pts.iter().map(|p| (*p).into()).collect();
                    convexes.push(vers_monde(&b.plans, &pts));
                }
                let ent = entites.get(&o.vmf_id);
                let attache = ent.and_then(|e| e.get("parentname")).map_or(false, |p| !p.is_empty());
                let physique = o.class.starts_with("prop_physics") || o.class == "prop_ragdoll";
                Corps { objet: p.object, genre: Genre::Prop, convexes, min: o.min, max: o.max, visible: true, ciel: false, prop: Some(k as u32), approche, libre: attache || physique || suspendu(&o.model) || suspendu(&o.name) }
            })
            .collect();
        corps.extend(props);

        let mut de_objet = vec![u32::MAX; scene.objects.len()];
        let mut grille: HashMap<(i32, i32, i32), Vec<(u32, u16)>> = HashMap::new();
        let mut grands = Vec::new();
        let mut grille_corps: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
        let mut grands_corps = Vec::new();
        for (i, c) in corps.iter().enumerate() {
            de_objet[c.objet as usize] = i as u32;
            if (c.max - c.min).max_element() > GRAND {
                grands_corps.push(i as u32);
            } else {
                let (a, b) = (cellule(c.min), cellule(c.max));
                for x in a.0..=b.0 {
                    for y in a.1..=b.1 {
                        for z in a.2..=b.2 {
                            grille_corps.entry((x, y, z)).or_default().push(i as u32);
                        }
                    }
                }
            }
            for (j, cv) in c.convexes.iter().enumerate() {
                let e = cv.max - cv.min;
                if e.max_element() > GRAND {
                    grands.push((i as u32, j as u16));
                    continue;
                }
                let (a, b) = (cellule(cv.min), cellule(cv.max));
                for x in a.0..=b.0 {
                    for y in a.1..=b.1 {
                        for z in a.2..=b.2 {
                            grille.entry((x, y, z)).or_default().push((i as u32, j as u16));
                        }
                    }
                }
            }
        }
        Monde { scene, corps, de_objet, grille, grands, grille_corps, grands_corps, entites }
    }

    /// Convexes dont la boîte coupe la boîte donnée : (corps, convexe).
    pub fn pres(&self, min: Vec3, max: Vec3) -> Vec<(u32, u16)> {
        let mut out: Vec<(u32, u16)> = Vec::new();
        let coupe = |c: &Convexe| (0..3).all(|k| c.min[k] <= max[k] && min[k] <= c.max[k]);
        let (a, b) = (cellule(min), cellule(max));
        let cellules = (b.0 - a.0 + 1) as i64 * (b.1 - a.1 + 1) as i64 * (b.2 - a.2 + 1) as i64;
        if cellules > 200_000 {
            // boîte immense : parcours direct
            for (i, c) in self.corps.iter().enumerate() {
                for (j, cv) in c.convexes.iter().enumerate() {
                    if coupe(cv) {
                        out.push((i as u32, j as u16));
                    }
                }
            }
            return out;
        }
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                for z in a.2..=b.2 {
                    if let Some(l) = self.grille.get(&(x, y, z)) {
                        out.extend(l.iter().filter(|(i, j)| coupe(&self.corps[*i as usize].convexes[*j as usize])));
                    }
                }
            }
        }
        out.extend(self.grands.iter().filter(|(i, j)| coupe(&self.corps[*i as usize].convexes[*j as usize])));
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Corps dont la boîte entière coupe la boîte donnée.
    pub fn corps_pres(&self, min: Vec3, max: Vec3) -> Vec<u32> {
        let coupe = |c: &Corps| (0..3).all(|k| c.min[k] <= max[k] && min[k] <= c.max[k]);
        let mut out: Vec<u32> = self.grands_corps.iter().copied().filter(|&i| coupe(&self.corps[i as usize])).collect();
        let (a, b) = (cellule(min), cellule(max));
        if (b.0 - a.0 + 1) as i64 * (b.1 - a.1 + 1) as i64 * (b.2 - a.2 + 1) as i64 > 200_000 {
            return (0..self.corps.len() as u32).filter(|&i| coupe(&self.corps[i as usize])).collect();
        }
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                for z in a.2..=b.2 {
                    if let Some(l) = self.grille_corps.get(&(x, y, z)) {
                        out.extend(l.iter().copied().filter(|&i| coupe(&self.corps[i as usize])));
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Points d'un corps qui servent à chercher ses contacts : les sommets de son maillage pour un modèle (tous
    /// jusqu'à 4 000, puis à pas régulier), les coins de ses convexes pour un solide.
    pub fn points_de_contact(&self, i: u32) -> Vec<Vec3> {
        let c = &self.corps[i as usize];
        match c.prop {
            Some(k) => {
                let v = self.sommets(k, 4000);
                if v.is_empty() {
                    c.convexes.iter().flat_map(|cv| cv.pts.iter().copied()).collect()
                } else {
                    v
                }
            }
            None => c.convexes.iter().flat_map(|cv| cv.pts.iter().copied()).collect(),
        }
    }

    /// Deux corps se touchent-ils, à `jeu` près ? Par leurs convexes exacts (solides, collisions), ou parce qu'un
    /// point de l'un est dans un convexe de l'autre. Un modèle sans collision est jugé sur sa boîte orientée.
    pub fn contact(&self, i: u32, pts_i: &[Vec3], j: u32, jeu: f32) -> bool {
        let (a, b) = (&self.corps[i as usize], &self.corps[j as usize]);
        if (0..3).any(|k| a.min[k] > b.max[k] + jeu || b.min[k] > a.max[k] + jeu) {
            return false;
        }
        if !a.approche && !b.approche && a.convexes.iter().any(|ca| b.convexes.iter().any(|cb| se_touchent(ca, cb, jeu))) {
            return true;
        }
        let dans = |pts: &[Vec3], c: &Corps| {
            pts.iter().any(|p| (0..3).all(|k| p[k] >= c.min[k] - jeu && p[k] <= c.max[k] + jeu) && c.convexes.iter().any(|cv| (0..3).all(|k| p[k] >= cv.min[k] - jeu && p[k] <= cv.max[k] + jeu) && cv.profondeur(*p) > -jeu))
        };
        if dans(pts_i, b) {
            return true;
        }
        // l'autre sens ne coûte que si l'autre est un modèle ou a peu de points
        let pts_j = self.points_de_contact(j);
        dans(&pts_j, a)
    }

    pub fn convexe(&self, r: (u32, u16)) -> &Convexe {
        &self.corps[r.0 as usize].convexes[r.1 as usize]
    }

    /// Premier corps touché par le segment `a → b` : (part du segment parcourue, corps). `garde` choisit les
    /// corps qui arrêtent le rayon.
    pub fn rayon(&self, a: Vec3, b: Vec3, garde: impl Fn(&Corps, u32) -> bool) -> Option<(f32, u32)> {
        let mut best: Option<(f32, u32)> = None;
        for r in self.pres(a.min(b), a.max(b)) {
            let c = &self.corps[r.0 as usize];
            if !garde(c, r.0) {
                continue;
            }
            if let Some((t0, _)) = self.convexe(r).segment(a, b) {
                if best.map_or(true, |x| t0 < x.0) {
                    best = Some((t0, r.0));
                }
            }
        }
        best
    }

    /// Plus grande profondeur d'un point dans un corps qui convient : (profondeur, corps).
    pub fn dans(&self, p: Vec3, garde: impl Fn(&Corps, u32) -> bool) -> Option<(f32, u32)> {
        let mut best: Option<(f32, u32)> = None;
        for r in self.pres(p, p) {
            if !garde(&self.corps[r.0 as usize], r.0) {
                continue;
            }
            let d = self.convexe(r).profondeur(p);
            if d > 0.0 && best.map_or(true, |x| d > x.0) {
                best = Some((d, r.0));
            }
        }
        best
    }

    /// Sommets du maillage d'un modèle posé, en repère du monde ; `max` sommets au plus, pris à pas régulier.
    pub fn sommets(&self, prop: u32, max: usize) -> Vec<Vec3> {
        let p = &self.scene.props[prop as usize];
        let Some(m) = &self.scene.models[p.model as usize].model else { return Vec::new() };
        let pas = (m.vertices.len() / max.max(1)).max(1);
        m.vertices.iter().step_by(pas).map(|v| p.transform.transform_point3(Vec3::from(v.pos))).collect()
    }

    /// Nom court d'un objet, pour les messages.
    pub fn nommer(&self, objet: u32) -> String {
        let o = &self.scene.objects[objet as usize];
        if !o.model.is_empty() {
            format!("{} n° {}", o.model.rsplit('/').next().unwrap_or(""), o.vmf_id)
        } else if !o.name.is_empty() {
            format!("{} « {} » n° {}", o.class, o.name, o.vmf_id)
        } else {
            format!("{} ({}) n° {}", if o.class == "worldspawn" { "solide du monde" } else { o.class.as_str() }, o.material, o.vmf_id)
        }
    }
}

impl Monde<'_> {
    /// D'où regarder une boîte : un œil en espace libre, à bonne distance, d'où la boîte se voit le mieux.
    /// `cibles` : les objets qu'on veut voir (ils n'arrêtent pas le regard).
    pub fn point_de_vue(&self, min: Vec3, max: Vec3, cibles: &[u32]) -> (Vec3, Vec3) {
        let centre = (min + max) * 0.5;
        let rayon = ((max - min).length() * 0.5).clamp(40.0, 900.0);
        let est_cible = |c: &Corps| cibles.contains(&c.objet);
        let opaque = |c: &Corps, _: u32| c.visible && !est_cible(c) && c.genre != Genre::Mobile;
        let mut meilleur = (f32::MIN, centre + Vec3::new(rayon * 2.0, 0.0, rayon));
        for recul in [2.4f32, 1.6, 3.6] {
            let dist = (rayon * recul).max(110.0);
            for site in [22.0f32, 42.0, 8.0, 65.0, -12.0] {
                for k in 0..16 {
                    let az = (k as f32 * 22.5 + 11.0).to_radians();
                    let (ss, cs) = site.to_radians().sin_cos();
                    let oeil = centre + Vec3::new(az.cos() * cs, az.sin() * cs, ss) * dist;
                    if self.dans(oeil, |c, _| c.visible || c.ciel).is_some() {
                        continue;
                    }
                    // part du regard qui arrive à la boîte sans rencontrer autre chose, sur cinq points de la boîte
                    let mut vu = 0.0;
                    for p in [centre, Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, max.y, max.z), Vec3::new(min.x, max.y, min.z), Vec3::new(max.x, min.y, min.z)] {
                        let p = centre.lerp(p, 0.7);
                        vu += match self.rayon(oeil, p, opaque) {
                            None => 1.0,
                            Some((t, _)) => {
                                if t > 0.97 {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                        };
                    }
                    let note = vu - (recul - 2.4).abs() * 0.3 - (site - 22.0).abs() * 0.006;
                    if note > meilleur.0 {
                        meilleur = (note, oeil);
                    }
                }
            }
            if meilleur.0 >= 4.0 {
                break;
            }
        }
        (meilleur.1, centre)
    }
}
