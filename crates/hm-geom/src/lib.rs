//! Géométrie d'une map : des plans d'un solide à ses polygones, displacements, repères.
//!
//! Conventions (lues dans le code de vbsp, `CMapFile::PlaneFromPoints`) :
//! - les trois points d'un plan sont en sens horaire vus de l'extérieur du solide ;
//! - la normale sortante vaut (p0 − p1) × (p2 − p1) ;
//! - tous les polygones rendus ici gardent ce sens : horaire vus de l'extérieur.
use glam::{DMat3, DVec2, DVec3};
use hm_vmf::{parse_axis, parse_floats, parse_plane, parse_vec3, Node, TexAxis};

/// Tolérance de découpe, en unités de map.
pub const EPS: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    /// Normale sortante, unitaire.
    pub n: DVec3,
    /// `n · x = d` sur le plan.
    pub d: f64,
}

impl Plane {
    pub fn from_points(p: [[f64; 3]; 3]) -> Option<Self> {
        let (p0, p1, p2) = (DVec3::from(p[0]), DVec3::from(p[1]), DVec3::from(p[2]));
        let n = (p0 - p1).cross(p2 - p1);
        let len = n.length();
        if len < 1e-9 {
            return None;
        }
        let n = n / len;
        Some(Self { n, d: n.dot(p0) })
    }

    pub fn dist(&self, p: DVec3) -> f64 {
        self.n.dot(p) - self.d
    }
}

/// Un grand carré sur le plan, en sens horaire vu de l'extérieur.
fn base_winding(pl: &Plane, r: f64) -> Vec<DVec3> {
    let a = if pl.n.x.abs() < 0.8 { DVec3::X } else { DVec3::Y };
    let u = pl.n.cross(a).normalize();
    let v = pl.n.cross(u); // u × v = n
    let o = pl.n * pl.d;
    vec![o - u * r - v * r, o - u * r + v * r, o + u * r + v * r, o + u * r - v * r]
}

/// Garde la partie du polygone du côté intérieur du plan (`dist <= 0`).
fn clip(poly: &[DVec3], pl: &Plane, out: &mut Vec<DVec3>) {
    out.clear();
    let k = poly.len();
    for i in 0..k {
        let (a, b) = (poly[i], poly[(i + 1) % k]);
        let (da, db) = (pl.dist(a), pl.dist(b));
        if da <= EPS {
            out.push(a);
        }
        if (da < -EPS && db > EPS) || (da > EPS && db < -EPS) {
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
}

/// Faces du polytope convexe `∩ { n · x <= d }`, en sens horaire vues de l'extérieur. Vide si les plans ne
/// ferment aucun volume. Sert à mesurer l'intersection de deux convexes : il suffit de réunir leurs plans.
pub fn polytope(planes: &[Plane]) -> Vec<Vec<DVec3>> {
    let mut faces = Vec::new();
    let mut tmp = Vec::with_capacity(16);
    for (i, pl) in planes.iter().enumerate() {
        // un plan déjà vu (deux convexes réunis partagent souvent une face) donnerait la même face deux fois
        if planes[..i].iter().any(|o| o.n.dot(pl.n) > 0.99999 && (o.d - pl.d).abs() < EPS) {
            continue;
        }
        let mut poly = base_winding(pl, 131072.0);
        for (j, other) in planes.iter().enumerate() {
            if j == i {
                continue;
            }
            clip(&poly, other, &mut tmp);
            std::mem::swap(&mut poly, &mut tmp);
            if poly.len() < 3 {
                break;
            }
        }
        dedup(&mut poly);
        if poly.len() >= 3 {
            faces.push(poly);
        }
    }
    if faces.len() < 4 {
        faces.clear();
    }
    faces
}

/// Aire d'un polygone plan.
pub fn aire(poly: &[DVec3]) -> f64 {
    (1..poly.len().saturating_sub(1)).map(|i| (poly[i] - poly[0]).cross(poly[i + 1] - poly[0]).length()).sum::<f64>() * 0.5
}

/// Volume d'un polytope donné par ses faces (somme de pyramides depuis un point intérieur).
pub fn volume(faces: &[Vec<DVec3>]) -> f64 {
    let n: usize = faces.iter().map(|f| f.len()).sum();
    if n == 0 {
        return 0.0;
    }
    let c = faces.iter().flatten().fold(DVec3::ZERO, |a, p| a + *p) / n as f64;
    faces.iter().map(|f| (1..f.len().saturating_sub(1)).map(|i| (f[0] - c).dot((f[i] - c).cross(f[i + 1] - c)).abs()).sum::<f64>()).sum::<f64>() / 6.0
}

/// Boîte englobante d'un ensemble de faces.
pub fn boite(faces: &[Vec<DVec3>]) -> Option<(DVec3, DVec3)> {
    let mut it = faces.iter().flatten();
    let first = *it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
}

fn dedup(poly: &mut Vec<DVec3>) {
    let mut i = 0;
    while poly.len() >= 2 && i < poly.len() {
        let j = (i + 1) % poly.len();
        if poly[i].distance_squared(poly[j]) < EPS * EPS {
            poly.remove(j);
            if j < i {
                i -= 1;
            }
        } else {
            i += 1;
        }
    }
}

/// Défauts relevés en construisant un solide. Ils serviront aux règles de la phase 3.
#[derive(Clone, Debug, PartialEq)]
pub enum BrushIssue {
    /// Les trois points du plan sont alignés.
    DegeneratePlane { side: usize },
    /// La face ne garde aucun polygone : plan en double ou inutile.
    EmptyFace { side: usize },
    /// Moins de quatre faces utiles : le solide n'enferme aucun volume.
    NoVolume,
}

/// Une face d'un solide, avec ce qu'il faut pour la texturer.
#[derive(Clone, Debug)]
pub struct Face {
    /// Rang du bloc `side` dans le solide.
    pub side: usize,
    pub side_id: u64,
    pub plane: Plane,
    /// Polygone convexe, sens horaire vu de l'extérieur. Vide si la face est un displacement.
    pub poly: Vec<DVec3>,
    pub material: String,
    pub uaxis: TexAxis,
    pub vaxis: TexAxis,
    pub lightmap_scale: f64,
    pub disp: Option<Displacement>,
}

impl Face {
    /// Coordonnées de texture en texels (à diviser par la taille de la texture).
    pub fn uv_texels(&self, p: DVec3) -> DVec2 {
        let u = p.dot(DVec3::from(self.uaxis.dir)) / self.uaxis.scale + self.uaxis.shift;
        let v = p.dot(DVec3::from(self.vaxis.dir)) / self.vaxis.scale + self.vaxis.shift;
        DVec2::new(u, v)
    }
}

/// La grille d'un displacement, déjà déplacée.
#[derive(Clone, Debug)]
pub struct Displacement {
    pub power: u32,
    /// (2^power + 1)² positions, rangées `i * k + j` : `i` va du coin de départ vers le coin suivant du polygone,
    /// `j` vers le coin précédent.
    pub points: Vec<DVec3>,
    /// Positions à plat correspondantes, pour le calcul des coordonnées de texture.
    pub flat: Vec<DVec3>,
    /// Mélange vers la seconde texture, de 0 à 255.
    pub alphas: Vec<f32>,
}

impl Displacement {
    pub fn k(&self) -> usize {
        (1usize << self.power) + 1
    }
}

#[derive(Clone, Debug, Default)]
pub struct Brush {
    pub id: u64,
    pub faces: Vec<Face>,
    pub issues: Vec<BrushIssue>,
}

impl Brush {
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut it = self.faces.iter().flat_map(|f| f.poly.iter().chain(f.disp.iter().flat_map(|d| d.points.iter())));
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
    }
}

/// Polygone de chaque plan d'un solide convexe (sens horaire vu de l'extérieur), dans l'ordre des plans. Un plan
/// absent (points alignés) ou qui ne garde aucune surface rend un polygone vide.
pub fn polygones(planes: &[Option<Plane>]) -> Vec<Vec<DVec3>> {
    let mut tmp = Vec::with_capacity(16);
    planes
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let Some(pl) = pl else { return Vec::new() };
            let mut poly = base_winding(pl, 131072.0);
            for (j, other) in planes.iter().enumerate() {
                if j == i {
                    continue;
                }
                if let Some(o) = other {
                    clip(&poly, o, &mut tmp);
                    std::mem::swap(&mut poly, &mut tmp);
                    if poly.len() < 3 {
                        break;
                    }
                }
            }
            dedup(&mut poly);
            if poly.len() < 3 {
                poly.clear();
            }
            poly
        })
        .collect()
}

/// Construit les polygones d'un bloc `solid`.
pub fn build_brush(solid: &Node) -> Brush {
    let sides: Vec<&Node> = solid.children_named("side").collect();
    let mut brush = Brush { id: solid.id().unwrap_or(0), ..Default::default() };
    let planes: Vec<Option<Plane>> = sides.iter().map(|s| s.get("plane").and_then(parse_plane).and_then(Plane::from_points)).collect();
    let polys = polygones(&planes);
    for (i, (s, poly)) in sides.iter().zip(polys).enumerate() {
        let Some(pl) = planes[i] else {
            brush.issues.push(BrushIssue::DegeneratePlane { side: i });
            continue;
        };
        if poly.len() < 3 {
            brush.issues.push(BrushIssue::EmptyFace { side: i });
            continue;
        }
        let axis = |k: &str| s.get(k).and_then(parse_axis).unwrap_or(TexAxis { dir: [1.0, 0.0, 0.0], shift: 0.0, scale: 0.25 });
        let mut face = Face {
            side: i,
            side_id: s.id().unwrap_or(0),
            plane: pl,
            poly,
            material: s.get("material").unwrap_or("").replace('\\', "/").to_ascii_lowercase(),
            uaxis: axis("uaxis"),
            vaxis: axis("vaxis"),
            lightmap_scale: s.f64("lightmapscale").unwrap_or(16.0),
            disp: None,
        };
        if let Some(d) = s.child("dispinfo") {
            face.disp = build_displacement(d, &face.poly, pl.n);
        }
        brush.faces.push(face);
    }
    if brush.faces.len() < 4 {
        brush.issues.push(BrushIssue::NoVolume);
    }
    brush
}

fn rows(block: Option<&Node>, k: usize, width: usize) -> Option<Vec<f64>> {
    let block = block?;
    let mut out = Vec::with_capacity(k * k * width);
    let mut tmp = Vec::new();
    for r in 0..k {
        parse_floats(block.get(&format!("row{r}"))?, &mut tmp);
        if tmp.len() != k * width {
            return None;
        }
        out.extend_from_slice(&tmp);
    }
    Some(out)
}

/// Grille d'un displacement. Formule de `CCoreDispInfo::GenerateDispSurf` : le coin 0 est celui de
/// `startposition`, les coins suivent le sens du polygone, la ligne `i` avance du coin 0 vers le coin 1 et la
/// colonne `j` de l'arête 0-1 vers l'arête 3-2.
pub fn build_displacement(d: &Node, poly: &[DVec3], n: DVec3) -> Option<Displacement> {
    if poly.len() != 4 {
        return None;
    }
    let power: u32 = d.get("power")?.trim().parse().ok()?;
    if !(2..=4).contains(&power) {
        return None;
    }
    let k = (1usize << power) + 1;
    let start = DVec3::from(d.get("startposition").and_then(parse_vec3)?);
    let i0 = (0..4).min_by(|&a, &b| poly[a].distance_squared(start).total_cmp(&poly[b].distance_squared(start)))?;
    let c: [DVec3; 4] = std::array::from_fn(|j| poly[(i0 + j) % 4]);
    let elevation = d.f64("elevation").unwrap_or(0.0);
    let normals = rows(d.child("normals"), k, 3);
    let dists = rows(d.child("distances"), k, 1);
    let offsets = rows(d.child("offsets"), k, 3);
    let alphas = rows(d.child("alphas"), k, 1);
    let mut points = Vec::with_capacity(k * k);
    let mut flat = Vec::with_capacity(k * k);
    let inv = 1.0 / (k - 1) as f64;
    for i in 0..k {
        let a = i as f64 * inv;
        let (e0, e1) = (c[0].lerp(c[1], a), c[3].lerp(c[2], a));
        for j in 0..k {
            let base = e0.lerp(e1, j as f64 * inv);
            let ndx = i * k + j;
            let mut p = base + n * elevation;
            if let (Some(nr), Some(ds)) = (&normals, &dists) {
                p += DVec3::new(nr[ndx * 3], nr[ndx * 3 + 1], nr[ndx * 3 + 2]) * ds[ndx];
            }
            if let Some(of) = &offsets {
                p += DVec3::new(of[ndx * 3], of[ndx * 3 + 1], of[ndx * 3 + 2]);
            }
            flat.push(base);
            points.push(p);
        }
    }
    let alphas = alphas.map(|a| a.iter().map(|&v| v as f32).collect()).unwrap_or_else(|| vec![0.0; k * k]);
    Some(Displacement { power, points, flat, alphas })
}

/// Matrice de rotation d'angles Source `pitch yaw roll` en degrés (`AngleMatrix`) : Rz(yaw) · Ry(pitch) · Rx(roll).
pub fn angles_to_mat(pitch: f64, yaw: f64, roll: f64) -> DMat3 {
    DMat3::from_rotation_z(yaw.to_radians()) * DMat3::from_rotation_y(pitch.to_radians()) * DMat3::from_rotation_x(roll.to_radians())
}

/// Direction du regard pour des angles Source (pitch positif = vers le bas).
pub fn angles_forward(pitch: f64, yaw: f64) -> DVec3 {
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();
    DVec3::new(cp * cy, cp * sy, -sp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hm_vmf::Vmf;

    fn cube() -> String {
        // exemple à six plans de la page VMF du wiki Valve, boîte de (-128,-128,-128) à (128,128,128) réduite ici
        let mut s = String::from("world\n{\nsolid\n{\n\"id\" \"7\"\n");
        let sides = [
            "(-64 64 32) (64 64 32) (64 -64 32)",    // dessus
            "(-64 -64 0) (64 -64 0) (64 64 0)",      // dessous
            "(-64 64 32) (-64 -64 32) (-64 -64 0)",  // x-
            "(64 64 0) (64 -64 0) (64 -64 32)",      // x+
            "(64 64 32) (-64 64 32) (-64 64 0)",     // y+
            "(64 -64 0) (-64 -64 0) (-64 -64 32)",   // y-
        ];
        for (i, p) in sides.iter().enumerate() {
            s += &format!("side\n{{\n\"id\" \"{}\"\n\"plane\" \"{p}\"\n\"material\" \"DEV/DEV_MEASUREWALL01A\"\n\"uaxis\" \"[1 0 0 0] 0.25\"\n\"vaxis\" \"[0 -1 0 0] 0.25\"\n}}\n", i + 1);
        }
        s + "}\n}\n"
    }

    #[test]
    fn boite() {
        let v = Vmf::parse(cube().into_bytes()).unwrap();
        let b = build_brush(v.world().unwrap().solids().next().unwrap());
        assert!(b.issues.is_empty(), "{:?}", b.issues);
        assert_eq!(b.faces.len(), 6);
        assert!((b.faces[0].plane.n - DVec3::Z).length() < 1e-9, "le dessus regarde vers +Z");
        assert!((b.faces[1].plane.n + DVec3::Z).length() < 1e-9);
        let (lo, hi) = b.bounds().unwrap();
        assert!((lo - DVec3::new(-64.0, -64.0, 0.0)).length() < 1e-6 && (hi - DVec3::new(64.0, 64.0, 32.0)).length() < 1e-6);
        for f in &b.faces {
            assert_eq!(f.poly.len(), 4);
            // sens horaire vu de l'extérieur : le produit vectoriel de deux arêtes consécutives est opposé à la normale
            let c = (f.poly[1] - f.poly[0]).cross(f.poly[2] - f.poly[1]);
            assert!(c.dot(f.plane.n) < 0.0);
            assert_eq!(f.material, "dev/dev_measurewall01a");
        }
        // 0,25 unité par texel : 128 unités = 512 texels
        let uv = b.faces[0].uv_texels(DVec3::new(64.0, 0.0, 32.0)) - b.faces[0].uv_texels(DVec3::new(-64.0, 0.0, 32.0));
        assert!((uv.x - 512.0).abs() < 1e-9);
    }

    #[test]
    fn angles() {
        let f = angles_to_mat(0.0, 90.0, 0.0) * DVec3::X;
        assert!((f - DVec3::Y).length() < 1e-9);
        // pitch positif : l'avant plonge
        assert!((angles_to_mat(30.0, 0.0, 0.0) * DVec3::X - angles_forward(30.0, 0.0)).length() < 1e-9);
        assert!(angles_forward(30.0, 0.0).z < 0.0);
    }
}
