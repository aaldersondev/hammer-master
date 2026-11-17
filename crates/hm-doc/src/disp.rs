//! Displacements : le bloc `dispinfo` d'une face (wiki Valve, « VMF (Valve Map Format) », section Dispinfo) lu en
//! valeurs, et ses opérations : créer (puissance 2 à 4, face à quatre côtés), sculpter (relever, abaisser, lisser),
//! peindre l'alpha, coudre les bords communs. Les points suivent la convention de `hm-geom` : lignes du coin le plus
//! proche de `startposition` vers le coin suivant du polygone, colonnes vers le coin précédent.
use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::kv::{nombre, nombres, Bloc, Element};

#[derive(Clone, Debug, PartialEq)]
pub struct Disp {
    pub puissance: u32,
    pub depart: DVec3,
    pub elevation: f64,
    pub subdiv: bool,
    /// Clé `flags` d'origine, gardée telle quelle.
    pub flags: Option<String>,
    pub normales: Vec<DVec3>,
    pub distances: Vec<f64>,
    pub decalages: Vec<DVec3>,
    pub normales_decalage: Vec<DVec3>,
    pub alphas: Vec<f64>,
    /// Deux étiquettes par carré : 9 plat, 1 en pente praticable, 0 trop raide.
    pub etiquettes: Vec<u8>,
    pub autorises: Vec<i64>,
}

/// Ce que fait un coup de pinceau.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum Pinceau {
    /// Monte (négatif : descend) le long de la normale de la face, `hauteur` au centre.
    Relever { hauteur: f64 },
    /// Rapproche chaque point de la moyenne de ses voisins (`force` de 0 à 1 au centre).
    Lisser { force: f64 },
    /// Pousse l'alpha vers `valeur` (0 à 255), `force` de 0 à 1 au centre.
    Alpha { valeur: f64, force: f64 },
}

impl Disp {
    pub fn cote(&self) -> usize {
        (1usize << self.puissance) + 1
    }

    pub fn neuf(puissance: u32, depart: DVec3, normale: DVec3) -> Disp {
        let k = (1usize << puissance) + 1;
        let q = (k - 1) * (k - 1) * 2;
        Disp {
            puissance,
            depart,
            elevation: 0.0,
            subdiv: false,
            flags: Some("0".into()),
            normales: vec![normale; k * k],
            distances: vec![0.0; k * k],
            decalages: vec![DVec3::ZERO; k * k],
            normales_decalage: vec![normale; k * k],
            alphas: vec![0.0; k * k],
            etiquettes: vec![9; q],
            autorises: vec![-1; 10],
        }
    }

    pub fn lire(b: &Bloc) -> Option<Disp> {
        let puissance: u32 = b.get("power").and_then(|v| v.trim().parse().ok()).unwrap_or(4);
        if !(2..=4).contains(&puissance) {
            return None;
        }
        let k = (1usize << puissance) + 1;
        let depart = b.get("startposition").map(|v| nombres(&v.replace(['[', ']'], " "))).filter(|v| v.len() == 3).map(|v| DVec3::new(v[0], v[1], v[2]))?;
        // `par_ligne` valeurs dans chacune des `n` lignes (manquantes : 0)
        let lignes = |nom: &str, par_ligne: usize, n: usize| -> Option<Vec<f64>> {
            let c = b.enfant(nom)?;
            let mut out = Vec::with_capacity(n * par_ligne);
            for r in 0..n {
                let mut v = nombres(c.get(&format!("row{r}")).unwrap_or(""));
                v.resize(par_ligne, 0.0);
                out.extend(v);
            }
            Some(out)
        };
        let vecs = |v: Option<Vec<f64>>, defaut: DVec3| -> Vec<DVec3> { v.map(|v| v.chunks(3).map(|c| DVec3::new(c[0], c[1], c[2])).collect()).unwrap_or_else(|| vec![defaut; k * k]) };
        let n0 = DVec3::Z;
        Some(Disp {
            puissance,
            depart,
            elevation: b.get("elevation").and_then(|v| v.trim().parse().ok()).unwrap_or(0.0),
            subdiv: b.get("subdiv").map_or(false, |v| v.trim() == "1"),
            flags: b.get("flags").map(String::from),
            normales: vecs(lignes("normals", 3 * k, k), n0),
            distances: lignes("distances", k, k).unwrap_or_else(|| vec![0.0; k * k]),
            decalages: vecs(lignes("offsets", 3 * k, k), DVec3::ZERO),
            normales_decalage: vecs(lignes("offset_normals", 3 * k, k), n0),
            alphas: lignes("alphas", k, k).unwrap_or_else(|| vec![0.0; k * k]),
            etiquettes: lignes("triangle_tags", (k - 1) * 2, k - 1).map(|v| v.iter().map(|x| *x as u8).collect()).unwrap_or_else(|| vec![9; (k - 1) * (k - 1) * 2]),
            autorises: b.enfant("allowed_verts").and_then(|c| c.get("10")).map(|v| v.split_whitespace().filter_map(|x| x.parse().ok()).collect()).unwrap_or_else(|| vec![-1; 10]),
        })
    }

    pub fn ecrire(&self) -> Bloc {
        let k = self.cote();
        let mut b = Bloc::nouveau("dispinfo");
        let p = |b: &mut Bloc, k: &str, v: String| b.elements.push(Element::Paire(k.into(), v));
        p(&mut b, "power", self.puissance.to_string());
        p(&mut b, "startposition", format!("[{} {} {}]", nombre(self.depart.x), nombre(self.depart.y), nombre(self.depart.z)));
        if let Some(f) = &self.flags {
            p(&mut b, "flags", f.clone());
        }
        p(&mut b, "elevation", nombre(self.elevation));
        p(&mut b, "subdiv", if self.subdiv { "1".into() } else { "0".into() });
        let bloc_v = |nom: &str, v: &[DVec3]| {
            let mut c = Bloc::nouveau(nom);
            for r in 0..k {
                let l: Vec<String> = v[r * k..(r + 1) * k].iter().map(|x| format!("{} {} {}", nombre(x.x), nombre(x.y), nombre(x.z))).collect();
                c.elements.push(Element::Paire(format!("row{r}"), l.join(" ")));
            }
            c
        };
        let bloc_f = |nom: &str, v: &[f64], par_ligne: usize, lignes: usize| {
            let mut c = Bloc::nouveau(nom);
            for r in 0..lignes {
                let l: Vec<String> = v[r * par_ligne..(r + 1) * par_ligne].iter().map(|x| nombre(*x)).collect();
                c.elements.push(Element::Paire(format!("row{r}"), l.join(" ")));
            }
            c
        };
        b.elements.push(Element::Bloc(bloc_v("normals", &self.normales)));
        b.elements.push(Element::Bloc(bloc_f("distances", &self.distances, k, k)));
        b.elements.push(Element::Bloc(bloc_v("offsets", &self.decalages)));
        b.elements.push(Element::Bloc(bloc_v("offset_normals", &self.normales_decalage)));
        b.elements.push(Element::Bloc(bloc_f("alphas", &self.alphas, k, k)));
        let tags: Vec<f64> = self.etiquettes.iter().map(|t| *t as f64).collect();
        b.elements.push(Element::Bloc(bloc_f("triangle_tags", &tags, (k - 1) * 2, k - 1)));
        let mut av = Bloc::nouveau("allowed_verts");
        av.elements.push(Element::Paire("10".into(), self.autorises.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")));
        b.elements.push(Element::Bloc(av));
        b
    }

    /// Les coins du polygone dans l'ordre de la grille (coin de départ d'abord).
    fn coins(&self, poly: &[DVec3]) -> Option<[DVec3; 4]> {
        if poly.len() != 4 {
            return None;
        }
        let i0 = (0..4).min_by(|&a, &b| poly[a].distance_squared(self.depart).total_cmp(&poly[b].distance_squared(self.depart)))?;
        Some(std::array::from_fn(|j| poly[(i0 + j) % 4]))
    }

    /// Positions à plat et dans le monde de chaque point (`i * k + j`).
    pub fn points(&self, poly: &[DVec3], normale: DVec3) -> Option<(Vec<DVec3>, Vec<DVec3>)> {
        let c = self.coins(poly)?;
        let k = self.cote();
        let inv = 1.0 / (k - 1) as f64;
        let (mut plat, mut monde) = (Vec::with_capacity(k * k), Vec::with_capacity(k * k));
        for i in 0..k {
            let a = i as f64 * inv;
            let (e0, e1) = (c[0].lerp(c[1], a), c[3].lerp(c[2], a));
            for j in 0..k {
                let base = e0.lerp(e1, j as f64 * inv);
                let x = i * k + j;
                plat.push(base);
                monde.push(base + normale * self.elevation + self.normales[x] * self.distances[x] + self.decalages[x]);
            }
        }
        Some((plat, monde))
    }

    /// Place un point à une position du monde (garde les décalages, refait normale et distance).
    pub fn placer_public(&mut self, x: usize, plat: DVec3, normale: DVec3, cible: DVec3) {
        self.placer(x, plat, normale, cible)
    }

    fn placer(&mut self, x: usize, plat: DVec3, normale: DVec3, cible: DVec3) {
        let d = cible - plat - normale * self.elevation - self.decalages[x];
        let l = d.length();
        if l < 1e-6 {
            self.normales[x] = normale;
            self.distances[x] = 0.0;
        } else {
            // la normale reste du côté de la face : une bosse vers l'intérieur s'écrit en distance négative
            let (n, l) = if d.dot(normale) < 0.0 { (-d / l, -l) } else { (d / l, l) };
            self.normales[x] = n;
            self.distances[x] = (l * 1e4).round() / 1e4;
        }
    }

    /// Recalcule les étiquettes des triangles d'après leur pente (normale z ≥ 0,7 praticable, ≥ 0,8 plat ;
    /// seuils du moteur non relus).
    pub fn etiqueter(&mut self, monde: &[DVec3]) {
        let k = self.cote();
        let mut t = Vec::with_capacity((k - 1) * (k - 1) * 2);
        for i in 0..k - 1 {
            for j in 0..k - 1 {
                let (a, b, c, d) = (monde[i * k + j], monde[(i + 1) * k + j], monde[(i + 1) * k + j + 1], monde[i * k + j + 1]);
                for (p, q, r) in [(a, b, c), (a, c, d)] {
                    let n = (q - p).cross(r - p).normalize_or_zero();
                    let z = n.z.abs();
                    t.push(if z >= 0.8 { 9 } else if z >= 0.7 { 1 } else { 0 });
                }
            }
        }
        self.etiquettes = t;
    }

    /// Un coup de pinceau centré en `centre`, de rayon `rayon` (atténuation douce vers le bord). Rend vrai si un
    /// point a bougé.
    pub fn peindre(&mut self, poly: &[DVec3], normale: DVec3, centre: DVec3, rayon: f64, pinceau: Pinceau) -> bool {
        let Some((plat, monde)) = self.points(poly, normale) else { return false };
        let k = self.cote();
        let poids = |p: DVec3| {
            let d = (p - centre).length() / rayon;
            if d >= 1.0 { 0.0 } else { let x = 1.0 - d; x * x * (3.0 - 2.0 * x) }
        };
        let mut change = false;
        let mut nouveaux = monde.clone();
        for x in 0..k * k {
            let w = poids(monde[x]);
            if w <= 0.0 {
                continue;
            }
            match pinceau {
                Pinceau::Relever { hauteur } => nouveaux[x] = monde[x] + normale * hauteur * w,
                Pinceau::Lisser { force } => {
                    let (i, j) = (x / k, x % k);
                    let mut s = DVec3::ZERO;
                    let mut n = 0.0;
                    for (di, dj) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                        let (a, b) = (i as i32 + di, j as i32 + dj);
                        if a >= 0 && b >= 0 && (a as usize) < k && (b as usize) < k {
                            s += monde[a as usize * k + b as usize];
                            n += 1.0;
                        }
                    }
                    // seulement le long de la normale : le lissage ne fait pas glisser la grille
                    let moy = s / n;
                    nouveaux[x] = monde[x] + normale * normale.dot(moy - monde[x]) * force.clamp(0.0, 1.0) * w;
                }
                Pinceau::Alpha { valeur, force } => {
                    let a = self.alphas[x] + (valeur.clamp(0.0, 255.0) - self.alphas[x]) * force.clamp(0.0, 1.0) * w;
                    let a = (a * 100.0).round() / 100.0;
                    if (a - self.alphas[x]).abs() > 1e-9 {
                        self.alphas[x] = a;
                        change = true;
                    }
                }
            }
        }
        for x in 0..k * k {
            if (nouveaux[x] - monde[x]).length() > 1e-6 {
                self.placer(x, plat[x], normale, nouveaux[x]);
                change = true;
            }
        }
        if change {
            if let Some((_, m)) = self.points(poly, normale) {
                self.etiqueter(&m);
            }
        }
        change
    }
}

/// Le premier point d'un displacement que touche le rayon `o + t·d` : (point, normale de la face, face). Calcul sur
/// les triangles de chaque displacement de la map.
pub fn rayon_disp(doc: &crate::modele::Document, o: DVec3, d: DVec3) -> Option<(DVec3, DVec3, crate::commandes::RefFace)> {
    let d = d.normalize_or_zero();
    let mut meilleur: Option<(f64, DVec3, DVec3, crate::commandes::RefFace)> = None;
    for (_, s) in doc.tous_les_solides() {
        if s.cache || !s.faces.iter().any(|f| f.disp.is_some()) {
            continue;
        }
        let polys = s.polygones();
        for (f, poly) in s.faces.iter().zip(&polys) {
            let Some(disp) = f.disp.as_ref().and_then(Disp::lire) else { continue };
            let n = f.normale();
            let Some((_, p)) = disp.points(poly, n) else { continue };
            let k = disp.cote();
            for i in 0..k - 1 {
                for j in 0..k - 1 {
                    let (a, b, c, e) = (p[i * k + j], p[(i + 1) * k + j], p[(i + 1) * k + j + 1], p[i * k + j + 1]);
                    for (x, y, z) in [(a, b, c), (a, c, e)] {
                        // Möller–Trumbore, des deux côtés
                        let (e1, e2) = (y - x, z - x);
                        let h = d.cross(e2);
                        let det = e1.dot(h);
                        if det.abs() < 1e-12 {
                            continue;
                        }
                        let inv = 1.0 / det;
                        let sv = o - x;
                        let u = sv.dot(h) * inv;
                        if !(0.0..=1.0).contains(&u) {
                            continue;
                        }
                        let q = sv.cross(e1);
                        let v = d.dot(q) * inv;
                        if v < 0.0 || u + v > 1.0 {
                            continue;
                        }
                        let t = e2.dot(q) * inv;
                        if t > 0.0 && meilleur.map_or(true, |m| t < m.0) {
                            meilleur = Some((t, o + d * t, n, crate::commandes::RefFace { solide: s.id, face: f.id }));
                        }
                    }
                }
            }
        }
    }
    meilleur.map(|(_, p, n, f)| (p, n, f))
}
