//! Édition de sommets (Maj+V de Hammer) : un solide vu comme des sommets et des faces (listes de sommets), qu'on
//! déplace, dont on coupe une face entre deux points (Ctrl+F), puis qu'on refait en solide. Un résultat est refusé
//! quand une face n'est plus plane, quand le solide n'est plus convexe, ou quand deux faces sont dans le même plan
//! (une face coupée qu'on n'a pas encore pliée) : le wiki Valve (« Hammer Vertex Tool ») prévient que Hammer laisse
//! alors un solide invalide que le jeu refuse ; ici il n'est pas écrit.
use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::brosses::Plan;
use crate::modele::{Face, Solide};

const SOUDURE: f64 = 0.01;
const PLAT: f64 = 0.02;

#[derive(Clone, Debug)]
pub struct FaceM {
    /// Sommets dans l'ordre du polygone.
    pub indices: Vec<usize>,
    /// La face d'origine : texture, numéro (0 pour une face neuve).
    pub texture: Face,
}

#[derive(Clone, Debug)]
pub struct Maillage {
    pub sommets: Vec<DVec3>,
    pub faces: Vec<FaceM>,
}

/// Une opération, ses points désignés par leur position (stable d'une relecture à l'autre).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum OpSommet {
    /// Déplace les sommets à ces positions (le milieu d'une arête désigne ses deux sommets).
    Deplacer { points: Vec<[f64; 3]>, delta: [f64; 3] },
    /// Coupe la face qui contient ces deux points (sommets ou milieux d'arêtes) le long du segment qui les joint.
    CouperFace { a: [f64; 3], b: [f64; 3] },
}

impl Maillage {
    pub fn de(s: &Solide) -> Maillage {
        let polys = s.polygones();
        let mut m = Maillage { sommets: Vec::new(), faces: Vec::new() };
        for (f, poly) in s.faces.iter().zip(polys) {
            if poly.len() < 3 {
                continue;
            }
            let indices = poly.iter().map(|p| m.indice(*p)).collect();
            m.faces.push(FaceM { indices, texture: f.clone() });
        }
        m
    }

    fn indice(&mut self, p: DVec3) -> usize {
        match self.sommets.iter().position(|q| (*q - p).length() < SOUDURE) {
            Some(i) => i,
            None => {
                self.sommets.push(p);
                self.sommets.len() - 1
            }
        }
    }

    /// Les arêtes, chacune une fois : (sommet, sommet).
    pub fn aretes(&self) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for f in &self.faces {
            for k in 0..f.indices.len() {
                let (a, b) = (f.indices[k], f.indices[(k + 1) % f.indices.len()]);
                let e = (a.min(b), a.max(b));
                if a != b && !out.contains(&e) {
                    out.push(e);
                }
            }
        }
        out
    }

    /// Les sommets que désigne un point : un sommet, ou les deux sommets d'une arête dont c'est le milieu.
    pub fn designes(&self, p: DVec3) -> Vec<usize> {
        if let Some(i) = self.sommets.iter().position(|q| (*q - p).length() < SOUDURE * 10.0) {
            return vec![i];
        }
        for (a, b) in self.aretes() {
            if ((self.sommets[a] + self.sommets[b]) * 0.5 - p).length() < SOUDURE * 10.0 {
                return vec![a, b];
            }
        }
        Vec::new()
    }

    pub fn appliquer(&mut self, op: &OpSommet) -> Result<(), String> {
        match op {
            OpSommet::Deplacer { points, delta } => {
                let mut idx: Vec<usize> = Vec::new();
                for p in points {
                    let d = self.designes(DVec3::from(*p));
                    if d.is_empty() {
                        return Err(hm_langue::tr!("aucun sommet ni arête en ({}, {}, {})", "no vertex or edge at ({}, {}, {})", p[0], p[1], p[2]));
                    }
                    for i in d {
                        if !idx.contains(&i) {
                            idx.push(i);
                        }
                    }
                }
                for i in idx {
                    self.sommets[i] += DVec3::from(*delta);
                }
                Ok(())
            }
            OpSommet::CouperFace { a, b } => self.couper_face(DVec3::from(*a), DVec3::from(*b)),
        }
    }

    /// Place un point sur la face `f` : un sommet existant, ou un sommet neuf inséré au milieu d'une arête (dans
    /// toutes les faces qui la partagent). Rend son indice.
    fn point_sur(&mut self, p: DVec3) -> Result<usize, String> {
        if let Some(i) = self.sommets.iter().position(|q| (*q - p).length() < SOUDURE * 10.0) {
            return Ok(i);
        }
        for (a, b) in self.aretes() {
            if ((self.sommets[a] + self.sommets[b]) * 0.5 - p).length() < SOUDURE * 10.0 {
                self.sommets.push(p);
                let n = self.sommets.len() - 1;
                for f in &mut self.faces {
                    let l = f.indices.len();
                    for k in 0..l {
                        let (x, y) = (f.indices[k], f.indices[(k + 1) % l]);
                        if (x == a && y == b) || (x == b && y == a) {
                            f.indices.insert(k + 1, n);
                            break;
                        }
                    }
                }
                return Ok(n);
            }
        }
        Err(hm_langue::t("le point n'est ni un sommet ni le milieu d'une arête").into())
    }

    pub fn couper_face(&mut self, a: DVec3, b: DVec3) -> Result<(), String> {
        let ia = self.point_sur(a)?;
        let ib = self.point_sur(b)?;
        if ia == ib {
            return Err(hm_langue::t("les deux points sont confondus").into());
        }
        let k = self
            .faces
            .iter()
            .position(|f| {
                let (pa, pb) = (f.indices.iter().position(|x| *x == ia), f.indices.iter().position(|x| *x == ib));
                matches!((pa, pb), (Some(x), Some(y)) if { let l = f.indices.len(); let d = (x as isize - y as isize).rem_euclid(l as isize) as usize; d != 1 && d != l - 1 })
            })
            .ok_or(hm_langue::t("les deux points ne sont pas sur une même face, ou ils sont déjà reliés par une arête"))?;
        let f = self.faces[k].clone();
        let l = f.indices.len();
        let (x, y) = (f.indices.iter().position(|v| *v == ia).unwrap(), f.indices.iter().position(|v| *v == ib).unwrap());
        let tour = |de: usize, a: usize| -> Vec<usize> {
            let mut v = vec![f.indices[de]];
            let mut i = de;
            while i != a {
                i = (i + 1) % l;
                v.push(f.indices[i]);
            }
            v
        };
        let (p1, p2) = (tour(x, y), tour(y, x));
        let mut t2 = f.texture.clone();
        t2.id = 0;
        self.faces[k] = FaceM { indices: p1, texture: f.texture.clone() };
        self.faces.push(FaceM { indices: p2, texture: t2 });
        Ok(())
    }

    /// Le solide que forme le maillage, ou ce qui ne va pas. `face` tire un numéro pour une face neuve.
    pub fn solide(&self, id: u64, editeur: &crate::kv::Bloc, face: &mut dyn FnMut() -> u64) -> Result<Solide, String> {
        // sommets confondus soudés (deux sommets glissés l'un sur l'autre n'en font plus qu'un)
        let mut repr: Vec<usize> = (0..self.sommets.len()).collect();
        for i in 0..self.sommets.len() {
            for j in 0..i {
                if repr[j] == j && (self.sommets[i] - self.sommets[j]).length() < SOUDURE {
                    repr[i] = j;
                    break;
                }
            }
        }
        let c = self.sommets.iter().fold(DVec3::ZERO, |a, p| a + *p) / self.sommets.len().max(1) as f64;
        let mut faces: Vec<(Vec<DVec3>, Plan, Face)> = Vec::new();
        for (k, f) in self.faces.iter().enumerate() {
            let mut v: Vec<usize> = Vec::new();
            for i in &f.indices {
                let r = repr[*i];
                if v.last() != Some(&r) {
                    v.push(r);
                }
            }
            while v.len() > 1 && v.first() == v.last() {
                v.pop();
            }
            if v.len() < 3 {
                continue; // la face a disparu en soudant ses sommets
            }
            let pts: Vec<DVec3> = v.iter().map(|i| self.sommets[*i]).collect();
            // normale de Newell, orientée vers l'extérieur
            let mut n = DVec3::ZERO;
            for i in 0..pts.len() {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                n += DVec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y));
            }
            if n.length() < 1e-9 {
                continue; // face sans surface
            }
            let mut n = n.normalize();
            let centre = pts.iter().fold(DVec3::ZERO, |a, p| a + *p) / pts.len() as f64;
            if n.dot(centre - c) < 0.0 {
                n = -n;
            }
            let plan = Plan { n, d: n.dot(centre) };
            if let Some(p) = pts.iter().find(|p| plan.cote(**p).abs() > PLAT) {
                let _ = p;
                return Err(hm_langue::tr!("la face n° {} n'est plus plane : la couper (Ctrl+F) avant de déplacer ce sommet, ou déplacer aussi ses voisins", "face #{} is no longer flat: split it (Ctrl+F) before moving this vertex, or move its neighbors too", if f.texture.id != 0 { f.texture.id.to_string() } else { hm_langue::tr!("neuve {k}", "new {k}") }));
            }
            faces.push((pts, plan, f.texture.clone()));
        }
        if faces.len() < 4 {
            return Err(hm_langue::t("le solide n'a plus de volume").into());
        }
        for i in 0..faces.len() {
            for j in 0..i {
                if faces[i].1.n.dot(faces[j].1.n) > 1.0 - 1e-6 && (faces[i].1.d - faces[j].1.d).abs() < PLAT {
                    return Err(hm_langue::t("deux faces sont dans le même plan : déplacer un sommet de la coupe pour plier la face").into());
                }
            }
        }
        // convexe : chaque sommet derrière chaque face
        for (_, p, _) in &faces {
            if self.sommets.iter().enumerate().filter(|(i, _)| repr[*i] == *i).any(|(_, s)| p.cote(*s) > PLAT) {
                return Err(hm_langue::t("le solide n'est plus convexe (un sommet passe devant une face)").into());
            }
        }
        let mut sortie = Vec::new();
        for (pts, plan, mut t) in faces {
            if t.id == 0 || sortie.iter().any(|x: &Face| x.id == t.id) {
                t.id = face();
            }
            t.plan = trois_points(&pts, plan.n);
            t.disp = None;
            t.gabarit = Vec::new();
            sortie.push(t);
        }
        let s = Solide { id, faces: sortie, editeur: editeur.clone(), cache: false, gabarit: Vec::new() };
        if !s.valide() {
            return Err(hm_langue::t("le solide obtenu est invalide").into());
        }
        Ok(s)
    }
}

fn trois_points(poly: &[DVec3], n: DVec3) -> [DVec3; 3] {
    let a = poly[0];
    let b = *poly.iter().max_by(|p, q| (**p - a).length_squared().total_cmp(&(**q - a).length_squared())).unwrap();
    let c = *poly.iter().max_by(|p, q| (**p - a).cross(b - a).length_squared().total_cmp(&(**q - a).cross(b - a).length_squared())).unwrap();
    if (a - b).cross(c - b).dot(n) >= 0.0 { [a, b, c] } else { [c, b, a] }
}
