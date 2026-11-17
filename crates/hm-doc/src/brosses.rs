//! Opérations sur les brosses, en plans : découper par un plan, soustraire un solide convexe d'un autre (creuser,
//! évider), enveloppe convexe (fusionner), primitives (cylindre, pointe, sphère, coin, arche, escalier).
//!
//! Une brosse est l'intersection de demi-espaces : un solide fait de plans est toujours convexe. Chaque opération
//! rend des faces « gabarits » (plan + texture) ; `solide_de` en fait un solide, sans les faces qui n'ont plus de
//! surface, et refuse un solide sans volume.
use glam::DVec3;
use hm_geom::Plane;

use crate::modele::{axes_monde, Axe, Face, Solide};


/// Un plan orienté : normale sortante unitaire, `n · x = d`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub n: DVec3,
    pub d: f64,
}

impl Plan {
    pub fn de_face(f: &Face) -> Option<Plan> {
        let n = f.normale();
        (n != DVec3::ZERO).then(|| Plan { n, d: n.dot(f.plan[1]) })
    }

    /// Le plan des trois points, normale `(p0 − p1) × (p2 − p1)` (la règle des VMF).
    pub fn de_points(p: [DVec3; 3]) -> Option<Plan> {
        let n = (p[0] - p[1]).cross(p[2] - p[1]);
        (n.length() > 1e-12).then(|| {
            let n = n.normalize();
            Plan { n, d: n.dot(p[1]) }
        })
    }

    pub fn inverse(&self) -> Plan {
        Plan { n: -self.n, d: -self.d }
    }

    /// Distance signée (positive devant, du côté de la normale).
    pub fn cote(&self, p: DVec3) -> f64 {
        self.n.dot(p) - self.d
    }

    fn geom(&self) -> Plane {
        Plane { n: self.n, d: self.d }
    }

    fn meme(&self, o: &Plan) -> bool {
        self.n.dot(o.n) > 1.0 - 1e-9 && (self.d - o.d).abs() < 1e-4
    }
}

/// Une face à construire : son plan et la face dont elle prend la texture.
#[derive(Clone, Debug)]
pub struct Gabarit {
    pub plan: Plan,
    pub texture: Face,
}

impl Gabarit {
    /// Une face neuve : texture alignée sur le monde, comme une face de bloc neuf.
    pub fn neuf(plan: Plan, materiau: &str) -> Gabarit {
        let (u, v) = axes_monde(plan.n);
        let mut f = Face::nouvelle(0, [DVec3::ZERO, DVec3::X, DVec3::Y], materiau);
        f.u = Axe { dir: u, decalage: 0.0, echelle: 0.25 };
        f.v = Axe { dir: v, decalage: 0.0, echelle: 0.25 };
        Gabarit { plan, texture: f }
    }

    pub fn de_face(f: &Face) -> Option<Gabarit> {
        Plan::de_face(f).map(|plan| Gabarit { plan, texture: f.clone() })
    }
}

/// Un nombre proche d'un entier devient cet entier (les plans écrits restent lisibles et sur la grille).
fn net(v: f64) -> f64 {
    let r = v.round();
    if (v - r).abs() < 1e-4 { r } else { v }
}

/// Trois points d'un polygone, bien répartis, ordonnés pour que la normale de la règle des VMF soit `n`.
fn trois_points(poly: &[DVec3], n: DVec3) -> [DVec3; 3] {
    let a = poly[0];
    let (mut ib, mut db) = (1, -1.0);
    for (i, p) in poly.iter().enumerate().skip(1) {
        let d = (*p - a).length_squared();
        if d > db {
            (ib, db) = (i, d);
        }
    }
    let b = poly[ib];
    let (mut ic, mut dc) = (0, -1.0);
    for (i, p) in poly.iter().enumerate() {
        let d = (*p - a).cross(b - a).length_squared();
        if d > dc {
            (ic, dc) = (i, d);
        }
    }
    let c = poly[ic];
    let r = |p: DVec3| DVec3::new(net(p.x), net(p.y), net(p.z));
    let (a, b, c) = (r(a), r(b), r(c));
    // (p0 − p1) × (p2 − p1) doit suivre n
    if (a - b).cross(c - b).dot(n) >= 0.0 { [a, b, c] } else { [c, b, a] }
}

/// Fait un solide des gabarits : les plans en double sont fondus, les faces sans surface retirées. Une face garde
/// le numéro de la face dont elle prend la texture si `garder_ids` (et qu'il n'est pas nul), sinon `face` en tire
/// un neuf. `None` : pas de volume.
pub fn solide_de(id: u64, gabarits: &[Gabarit], face: &mut dyn FnMut() -> u64, editeur: &crate::kv::Bloc, garder_ids: bool) -> Option<Solide> {
    let mut g: Vec<&Gabarit> = Vec::new();
    for x in gabarits {
        if !g.iter().any(|y| y.plan.meme(&x.plan)) {
            g.push(x);
        }
    }
    let plans: Vec<Option<Plane>> = g.iter().map(|x| Some(x.plan.geom())).collect();
    let polys = hm_geom::polygones(&plans);
    let mut faces = Vec::new();
    for (x, poly) in g.iter().zip(&polys) {
        if poly.len() < 3 {
            continue;
        }
        let mut f = x.texture.clone();
        let deja = faces.iter().any(|y: &Face| y.id == f.id);
        if !garder_ids || f.id == 0 || deja {
            f.id = face();
        }
        f.plan = trois_points(poly, x.plan.n);
        f.disp = None;
        f.gabarit = Vec::new();
        faces.push(f);
    }
    if faces.len() < 4 {
        return None;
    }
    let s = Solide { id, faces, editeur: editeur.clone(), cache: false, gabarit: Vec::new() };
    // un volume : au moins un point à plus d'un epsilon de chaque face
    let pts: Vec<DVec3> = s.polygones().into_iter().flatten().collect();
    let c = pts.iter().fold(DVec3::ZERO, |a, p| a + *p) / pts.len().max(1) as f64;
    let epais = s.faces.iter().all(|f| Plan::de_face(f).map_or(false, |p| pts.iter().any(|q| p.cote(*q) < -0.01) && p.cote(c) < 0.0));
    (epais && s.valide()).then_some(s)
}

/// Les gabarits des faces d'un solide.
pub fn gabarits(s: &Solide) -> Vec<Gabarit> {
    s.faces.iter().filter_map(Gabarit::de_face).collect()
}

/// Les sommets d'un solide.
pub fn sommets(s: &Solide) -> Vec<DVec3> {
    let mut v: Vec<DVec3> = Vec::new();
    for p in s.polygones().into_iter().flatten() {
        if !v.iter().any(|q| (*q - p).length() < 1e-4) {
            v.push(p);
        }
    }
    v
}

/// Le plan coupe-t-il le solide (des sommets franchement des deux côtés) ?
pub fn coupe(s: &Solide, p: &Plan) -> bool {
    let v = sommets(s);
    v.iter().any(|q| p.cote(*q) > 0.01) && v.iter().any(|q| p.cote(*q) < -0.01)
}

/// Les deux parties d'un solide coupé par un plan : (derrière, devant), en gabarits. `face` : la texture de la face
/// de coupe.
pub fn couper(s: &Solide, p: &Plan, texture: &Face) -> (Vec<Gabarit>, Vec<Gabarit>) {
    let g = gabarits(s);
    let mut derriere = g.clone();
    derriere.push(Gabarit { plan: *p, texture: texture.clone() });
    let mut devant = g;
    devant.push(Gabarit { plan: p.inverse(), texture: texture.clone() });
    (derriere, devant)
}

/// `a` moins `b` (convexes) : les morceaux, à la manière du « carve » de Hammer — pour chaque plan de `b`, ce qui
/// est devant lui et derrière les plans déjà traités. Les faces neuves prennent la texture de `texture_b(i)`.
pub fn soustraire(a: &[Gabarit], b: &[Plan], texture_b: &dyn Fn(usize) -> Face) -> Vec<Vec<Gabarit>> {
    let mut reste: Vec<Gabarit> = a.to_vec();
    let mut morceaux = Vec::new();
    for (i, p) in b.iter().enumerate() {
        let mut m = reste.clone();
        m.push(Gabarit { plan: p.inverse(), texture: texture_b(i) });
        morceaux.push(m);
        reste.push(Gabarit { plan: *p, texture: texture_b(i) });
    }
    morceaux
}

/// Les plans de l'enveloppe convexe de points (normales sortantes). Rendu vide si les points n'ont pas de volume.
pub fn enveloppe(points: &[DVec3]) -> Vec<Plan> {
    let mut pts: Vec<DVec3> = Vec::new();
    for p in points {
        if !pts.iter().any(|q| (*q - *p).length() < 1e-4) {
            pts.push(*p);
        }
    }
    let n = pts.len();
    let mut plans: Vec<Plan> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let Some(p) = Plan::de_points([pts[i], pts[j], pts[k]]) else { continue };
                for p in [p, p.inverse()] {
                    if plans.iter().any(|q| q.meme(&p)) {
                        continue;
                    }
                    if pts.iter().all(|q| p.cote(*q) < 1e-3) {
                        plans.push(p);
                    }
                }
            }
        }
    }
    if plans.len() < 4 {
        plans.clear();
    }
    plans
}

// ---------------------------------------------------------------------------------------------- primitives

/// Les points d'une ellipse dans le plan XY de la boîte, `n` côtés, en partant de `depart` degrés.
fn ellipse(c: DVec3, rx: f64, ry: f64, n: usize, depart: f64, arc: f64) -> Vec<DVec3> {
    (0..=n).map(|k| {
        let a = (depart + arc * k as f64 / n as f64).to_radians();
        DVec3::new(net(c.x + rx * a.cos()), net(c.y + ry * a.sin()), c.z)
    })
    .collect()
}

fn plan_h(z: f64, haut: bool) -> Plan {
    if haut { Plan { n: DVec3::Z, d: z } } else { Plan { n: -DVec3::Z, d: -z } }
}

/// Plan vertical passant par a et b, normale sortante du côté opposé à `dedans`.
fn plan_vertical(a: DVec3, b: DVec3, dedans: DVec3) -> Option<Plan> {
    let p = Plan::de_points([a, b, b + DVec3::Z])?;
    Some(if p.cote(dedans) > 0.0 { p.inverse() } else { p })
}

/// Plan par trois points, normale sortante du côté opposé à `dedans`.
fn plan_oriente(p: [DVec3; 3], dedans: DVec3) -> Option<Plan> {
    let q = Plan::de_points(p)?;
    Some(if q.cote(dedans) > 0.0 { q.inverse() } else { q })
}

/// Cylindre de `n` côtés inscrit dans la boîte (axe Z).
pub fn cylindre(lo: DVec3, hi: DVec3, n: usize) -> Vec<Plan> {
    let c = (lo + hi) * 0.5;
    let pts = ellipse(DVec3::new(c.x, c.y, lo.z), (hi.x - lo.x) * 0.5, (hi.y - lo.y) * 0.5, n, 0.0, 360.0);
    let mut p = vec![plan_h(hi.z, true), plan_h(lo.z, false)];
    p.extend(pts.windows(2).filter_map(|w| plan_vertical(w[0], w[1], c)));
    p
}

/// Pointe : base de `n` côtés en bas de la boîte, sommet au centre du haut.
pub fn pointe(lo: DVec3, hi: DVec3, n: usize) -> Vec<Plan> {
    let c = (lo + hi) * 0.5;
    let sommet = DVec3::new(c.x, c.y, hi.z);
    let pts = ellipse(DVec3::new(c.x, c.y, lo.z), (hi.x - lo.x) * 0.5, (hi.y - lo.y) * 0.5, n, 0.0, 360.0);
    let dedans = DVec3::new(c.x, c.y, lo.z + (hi.z - lo.z) * 0.25);
    let mut p = vec![plan_h(lo.z, false)];
    p.extend(pts.windows(2).filter_map(|w| plan_oriente([w[0], w[1], sommet], dedans)));
    p
}

/// Sphère (ellipsoïde dans la boîte) de `n` côtés autour et `n / 2` tranches.
pub fn sphere(lo: DVec3, hi: DVec3, n: usize) -> Vec<Plan> {
    let c = (lo + hi) * 0.5;
    let r = (hi - lo) * 0.5;
    let t = (n / 2).max(2);
    let pt = |i: usize, j: usize| {
        let lat = std::f64::consts::PI * (i as f64 / t as f64) - std::f64::consts::FRAC_PI_2;
        let lon = 2.0 * std::f64::consts::PI * (j as f64 / n as f64);
        DVec3::new(c.x + r.x * lat.cos() * lon.cos(), c.y + r.y * lat.cos() * lon.sin(), c.z + r.z * lat.sin())
    };
    let mut p = Vec::new();
    for i in 0..t {
        for j in 0..n {
            let (a, b, cc, d) = (pt(i, j), pt(i, j + 1), pt(i + 1, j + 1), pt(i + 1, j));
            // aux pôles deux des points se confondent : prendre trois points distincts
            let mut q: Vec<DVec3> = Vec::new();
            for x in [a, b, cc, d] {
                if !q.iter().any(|y| (*y - x).length() < 1e-6) {
                    q.push(x);
                }
            }
            if q.len() >= 3 {
                if let Some(pl) = plan_oriente([q[0], q[1], q[2]], c) {
                    p.push(pl);
                }
            }
        }
    }
    p
}

/// Coin : prisme triangulaire, la pente monte vers +Y.
pub fn coin(lo: DVec3, hi: DVec3) -> Vec<Plan> {
    let dedans = DVec3::new((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.75, lo.z + (hi.z - lo.z) * 0.25);
    let mut p = vec![plan_h(lo.z, false), Plan { n: DVec3::Y, d: hi.y }, Plan { n: -DVec3::X, d: -lo.x }, Plan { n: DVec3::X, d: hi.x }];
    if let Some(pente) = plan_oriente([DVec3::new(lo.x, lo.y, lo.z), DVec3::new(hi.x, lo.y, lo.z), DVec3::new(hi.x, hi.y, hi.z)], dedans) {
        p.push(pente);
    }
    p
}

/// Réglages d'une arche (wiki Valve, « Hammer Arch Properties »).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Arche {
    /// Largeur des pièces (de l'extérieur vers l'intérieur).
    pub largeur_mur: f64,
    /// Nombre de pièces, de 3 à 100.
    pub cotes: usize,
    /// Degrés couverts, de 8 à 360.
    pub arc: f64,
    /// Angle de la première pièce.
    pub depart: f64,
    /// Hauteur ajoutée à chaque pièce (escalier en colimaçon).
    #[serde(default)]
    pub hauteur_ajoutee: f64,
}

/// Les pièces d'une arche inscrite dans la boîte : une liste de plans par pièce.
pub fn arche(lo: DVec3, hi: DVec3, a: &Arche) -> Vec<Vec<Plan>> {
    let c = (lo + hi) * 0.5;
    let (rx, ry) = ((hi.x - lo.x) * 0.5, (hi.y - lo.y) * 0.5);
    let w = a.largeur_mur;
    let ext = ellipse(DVec3::new(c.x, c.y, 0.0), rx, ry, a.cotes, a.depart, a.arc);
    let int = ellipse(DVec3::new(c.x, c.y, 0.0), (rx - w).max(0.0), (ry - w).max(0.0), a.cotes, a.depart, a.arc);
    let mut out = Vec::new();
    for k in 0..a.cotes {
        let z0 = lo.z + a.hauteur_ajoutee * k as f64;
        let z1 = hi.z + a.hauteur_ajoutee * k as f64;
        let (e0, e1, i0, i1) = (ext[k], ext[k + 1], int[k], int[k + 1]);
        let milieu = (e0 + e1 + i0 + i1) * 0.25;
        let dedans = DVec3::new(milieu.x, milieu.y, (z0 + z1) * 0.5);
        let mut p = vec![plan_h(z1, true), plan_h(z0, false)];
        for (a1, b1) in [(e0, e1), (i0, i1), (e0, i0), (e1, i1)] {
            if (a1 - b1).length() > 1e-6 {
                if let Some(pl) = plan_vertical(a1, b1, dedans) {
                    p.push(pl);
                }
            }
        }
        out.push(p);
    }
    out
}

/// Escalier plein de `marches` marches dans la boîte, montant vers +X (`sens` 0), +Y (1), −X (2) ou −Y (3) : une
/// colonne par marche, du sol de la boîte à la hauteur de la marche, sans chevauchement.
pub fn escalier(lo: DVec3, hi: DVec3, marches: usize, sens: u8) -> Vec<(DVec3, DVec3)> {
    let n = marches.max(1);
    let h = (hi.z - lo.z) / n as f64;
    let cran = |a: f64, b: f64, k: usize| net(a + (b - a) * k as f64 / n as f64);
    (0..n)
        .map(|i| {
            let z = net(lo.z + h * (i + 1) as f64);
            match sens % 4 {
                0 => (DVec3::new(cran(lo.x, hi.x, i), lo.y, lo.z), DVec3::new(cran(lo.x, hi.x, i + 1), hi.y, z)),
                1 => (DVec3::new(lo.x, cran(lo.y, hi.y, i), lo.z), DVec3::new(hi.x, cran(lo.y, hi.y, i + 1), z)),
                2 => (DVec3::new(cran(hi.x, lo.x, i + 1), lo.y, lo.z), DVec3::new(cran(hi.x, lo.x, i), hi.y, z)),
                _ => (DVec3::new(lo.x, cran(hi.y, lo.y, i + 1), lo.z), DVec3::new(hi.x, cran(hi.y, lo.y, i), z)),
            }
        })
        .collect()
}

/// Volume d'un solide (somme de tétraèdres depuis un point intérieur).
pub fn volume(s: &Solide) -> f64 {
    let polys = s.polygones();
    let n = polys.iter().map(|p| p.len()).sum::<usize>().max(1) as f64;
    let c = polys.iter().flatten().fold(DVec3::ZERO, |a, p| a + *p) / n;
    let mut v = 0.0;
    for p in &polys {
        for i in 1..p.len().saturating_sub(1) {
            v += (p[0] - c).dot((p[i] - c).cross(p[i + 1] - c)).abs() / 6.0;
        }
    }
    v
}
