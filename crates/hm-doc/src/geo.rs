//! Opérations géométriques pures : elles prennent un objet et rendent un objet. Les commandes s'en servent, et la
//! fenêtre aussi pour montrer un aperçu pendant un glisser, sans rien modifier.
use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};

use crate::kv::{nombre, nombres, Bloc, Element};
use crate::modele::{lire_vec3, texte_vec3, Axe, Document, Entite, Face, Proprio, Solide};

/// Transformation affine : `p' = l · p + t`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    pub l: DMat3,
    pub t: DVec3,
}

impl Affine {
    pub fn translation(d: DVec3) -> Affine {
        Affine { l: DMat3::IDENTITY, t: d }
    }

    /// Rotation de `degres` autour de l'axe donné, passant par `centre` (sens direct).
    pub fn rotation(centre: DVec3, axe: DVec3, degres: f64) -> Affine {
        let l = DMat3::from_axis_angle(axe.normalize_or(DVec3::Z), degres.to_radians());
        Affine { l, t: centre - l * centre }
    }

    /// Envoie la boîte `de` sur la boîte `vers` (redimensionner ; une taille négative fait un miroir).
    pub fn boite(de: (DVec3, DVec3), vers: (DVec3, DVec3)) -> Affine {
        let ds = de.1 - de.0;
        let vs = vers.1 - vers.0;
        let k = DVec3::new(if ds.x.abs() > 1e-9 { vs.x / ds.x } else { 1.0 }, if ds.y.abs() > 1e-9 { vs.y / ds.y } else { 1.0 }, if ds.z.abs() > 1e-9 { vs.z / ds.z } else { 1.0 });
        let l = DMat3::from_diagonal(k);
        Affine { l, t: vers.0 - l * de.0 }
    }

    /// Miroir par rapport au plan d'axe `axe` (0, 1, 2) passant par `centre`.
    pub fn miroir(centre: DVec3, axe: usize) -> Affine {
        let mut k = DVec3::ONE;
        k[axe] = -1.0;
        let l = DMat3::from_diagonal(k);
        Affine { l, t: centre - l * centre }
    }

    pub fn point(&self, p: DVec3) -> DVec3 {
        self.l * p + self.t
    }

    /// La partie linéaire est une rotation (orthonormée, sans miroir).
    pub fn rigide(&self) -> bool {
        let m = self.l;
        let mt = m.transpose() * m;
        (mt - DMat3::IDENTITY).to_cols_array().iter().all(|v| v.abs() < 1e-9) && m.determinant() > 0.0
    }
}

/// Une boîte alignée sur les axes, en solide de six faces.
pub fn boite(id: u64, faces: [u64; 6], lo: DVec3, hi: DVec3, materiau: &str) -> Solide {
    let (a, b) = (lo, hi);
    // points en sens horaire vus de l'extérieur (vérifié par les normales du test de hm-geom)
    let plans = [
        [DVec3::new(a.x, b.y, b.z), DVec3::new(b.x, b.y, b.z), DVec3::new(b.x, a.y, b.z)], // dessus
        [DVec3::new(a.x, a.y, a.z), DVec3::new(b.x, a.y, a.z), DVec3::new(b.x, b.y, a.z)], // dessous
        [DVec3::new(a.x, b.y, b.z), DVec3::new(a.x, a.y, b.z), DVec3::new(a.x, a.y, a.z)], // x−
        [DVec3::new(b.x, b.y, a.z), DVec3::new(b.x, a.y, a.z), DVec3::new(b.x, a.y, b.z)], // x+
        [DVec3::new(b.x, b.y, b.z), DVec3::new(a.x, b.y, b.z), DVec3::new(a.x, b.y, a.z)], // y+
        [DVec3::new(b.x, a.y, a.z), DVec3::new(a.x, a.y, a.z), DVec3::new(a.x, a.y, b.z)], // y−
    ];
    Solide {
        id,
        faces: plans.iter().zip(faces).map(|(p, fid)| Face::nouvelle(fid, *p, materiau)).collect(),
        editeur: editeur_neuf(id),
        cache: false,
        gabarit: Vec::new(),
    }
}

/// Bloc `editor` d'un objet neuf : une couleur tirée du numéro, comme Hammer qui en tire une au hasard.
pub fn editeur_neuf(graine: u64) -> Bloc {
    let h = graine.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let c = [0 + (h >> 8) % 100, 100 + (h >> 16) % 156, 100 + (h >> 24) % 156];
    let mut b = Bloc::nouveau("editor");
    b.elements.push(Element::Paire("color".into(), format!("{} {} {}", c[0], c[1], c[2])));
    b.elements.push(Element::Paire("visgroupshown".into(), "1".into()));
    b.elements.push(Element::Paire("visgroupautoshown".into(), "1".into()));
    b
}

fn axe_transforme(a: &Axe, f: &Affine, verrou: bool, ancre: DVec3) -> Axe {
    if !verrou {
        return *a;
    }
    if f.rigide() {
        // exact : la texture suit l'objet
        let d = f.l * a.dir;
        Axe { dir: d, echelle: a.echelle, decalage: a.decalage - f.t.dot(d) / a.echelle }
    } else {
        // redimensionnement : la texture garde sa taille et suit le point d'ancrage
        let p = f.point(ancre);
        Axe { dir: a.dir, echelle: a.echelle, decalage: a.decalage + (ancre - p).dot(a.dir) / a.echelle }
    }
}

/// Transforme un solide. `verrou_texture` : la texture suit l'objet (sinon elle reste fixe dans le monde).
pub fn transformer_solide(s: &Solide, f: &Affine, verrou_texture: bool) -> Solide {
    let mut s = s.clone();
    let miroir = f.l.determinant() < 0.0;
    let ancre = s.boite().map(|b| b.0).unwrap_or(DVec3::ZERO);
    for face in &mut s.faces {
        let mut p = face.plan.map(|q| f.point(q));
        if miroir {
            p.swap(0, 2); // garder le sens horaire vu de l'extérieur
        }
        face.plan = p;
        face.u = axe_transforme(&face.u, f, verrou_texture, ancre);
        face.v = axe_transforme(&face.v, f, verrou_texture, ancre);
        if let Some(d) = &mut face.disp {
            transformer_disp(d, f);
        }
    }
    s
}

/// Le point de départ d'un displacement suit la face ; ses vecteurs tournent avec elle.
fn transformer_disp(d: &mut Bloc, f: &Affine) {
    for e in &mut d.elements {
        match e {
            Element::Paire(k, v) if k.eq_ignore_ascii_case("startposition") => {
                if let Some(p) = lire_vec3(v) {
                    *v = format!("[{}]", texte_vec3(f.point(p)));
                }
            }
            Element::Bloc(b) if matches!(b.nom.to_ascii_lowercase().as_str(), "normals" | "offsets" | "offset_normals") => {
                if f.l == DMat3::IDENTITY {
                    continue;
                }
                let normaliser = b.nom.eq_ignore_ascii_case("normals") || b.nom.eq_ignore_ascii_case("offset_normals");
                for r in &mut b.elements {
                    if let Element::Paire(_, v) = r {
                        let n = nombres(v);
                        let mut out = Vec::with_capacity(n.len());
                        for c in n.chunks(3) {
                            if c.len() < 3 {
                                break;
                            }
                            let mut w = f.l * DVec3::new(c[0], c[1], c[2]);
                            if normaliser {
                                w = w.normalize_or_zero();
                            }
                            out.push(format!("{} {} {}", nombre(w.x), nombre(w.y), nombre(w.z)));
                        }
                        *v = out.join(" ");
                    }
                }
            }
            _ => {}
        }
    }
}

/// Angles Source (tangage, lacet, roulis) d'une matrice de rotation `Rz(lacet) · Ry(tangage) · Rx(roulis)`.
pub fn angles_de(m: DMat3) -> DVec3 {
    let x = m.x_axis; // (cy cp, sy cp, −sp)
    let sp = (-x.z).clamp(-1.0, 1.0);
    let pitch = sp.asin();
    let cp = pitch.cos();
    let (yaw, roll) = if cp.abs() > 1e-6 {
        (x.y.atan2(x.x), m.y_axis.z.atan2(m.z_axis.z))
    } else {
        // regard vertical : le lacet absorbe le roulis
        ((-m.y_axis.x).atan2(m.y_axis.y), 0.0)
    };
    let net = |v: f64| {
        let d = v.to_degrees();
        let r = (d * 1e4).round() / 1e4;
        if r == 0.0 { 0.0 } else { r }
    };
    DVec3::new(net(pitch), net(yaw), net(roll))
}

/// Ce qu'un rayon touche : la distance, le point, la normale sortante de la face, le solide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub t: f64,
    pub point: DVec3,
    pub normale: DVec3,
    pub solide: u64,
}

/// Textures d'outils sur lesquelles on ne pose rien (le joueur et les props les traversent, ou ne les voient pas).
fn traversable(materiau: &str) -> bool {
    let m = materiau.to_ascii_lowercase().replace('\\', "/");
    matches!(
        m.as_str(),
        "tools/toolsskybox" | "tools/toolsskybox2d" | "tools/toolshint" | "tools/toolsskip" | "tools/toolstrigger" | "tools/toolsareaportal" | "tools/toolsoccluder" | "tools/toolsfog" | "tools/toolslight"
    )
}

/// Le premier solide que touche le rayon `o + t·d` (t ≥ 0), parmi ceux qui portent : ni déclencheurs, ni brosses
/// faites seulement de textures traversables (ciel, hint, skip, trigger, areaportal…). `ignorer` écarte des solides
/// (ceux de l'entité qu'on pose). Un rayon qui part de l'intérieur d'un solide ne touche pas ce solide.
/// Les displacements sont pris à plat (la face de base), les modèles ne comptent pas.
pub fn rayon(doc: &Document, o: DVec3, d: DVec3, ignorer: &dyn Fn(u64) -> bool) -> Option<Impact> {
    let d = d.normalize_or_zero();
    if d == DVec3::ZERO {
        return None;
    }
    let mut meilleur: Option<Impact> = None;
    for (p, s) in doc.tous_les_solides() {
        if ignorer(s.id) || s.faces.iter().all(|f| traversable(&f.materiau)) {
            continue;
        }
        if let Proprio::Entite(e) = p {
            let c = doc.entite(e).map(|e| e.classe().to_ascii_lowercase()).unwrap_or_default();
            if c.starts_with("trigger_") || c.starts_with("func_areaportal") || c == "func_occluder" || c == "func_viscluster" {
                continue;
            }
        }
        if let Some((t, n)) = rayon_solide(s, o, d) {
            if meilleur.map_or(true, |m| t < m.t) {
                meilleur = Some(Impact { t, point: o + d * t, normale: n, solide: s.id });
            }
        }
    }
    meilleur
}

/// L'entrée du rayon `o + t·d` (d unitaire, t ≥ 0) dans un solide : (t, normale sortante de la face d'entrée).
fn rayon_solide(s: &crate::Solide, o: DVec3, d: DVec3) -> Option<(f64, DVec3)> {
    // plans orientés vers l'extérieur, d'après le centre du solide
    let polys = s.polygones();
    let pts: Vec<DVec3> = polys.iter().flatten().copied().collect();
    if pts.is_empty() {
        return None;
    }
    let c = pts.iter().fold(DVec3::ZERO, |a, p| a + *p) / pts.len() as f64;
    let (mut t_in, mut t_out, mut n_in) = (f64::NEG_INFINITY, f64::INFINITY, DVec3::ZERO);
    for f in &s.faces {
        let mut n = f.normale();
        if n == DVec3::ZERO {
            continue;
        }
        let mut dist = n.dot(f.plan[1]);
        if n.dot(c) > dist {
            n = -n;
            dist = -dist;
        }
        let den = n.dot(d);
        let ecart = dist - n.dot(o);
        if den.abs() < 1e-12 {
            if ecart < 0.0 {
                return None;
            }
            continue;
        }
        let t = ecart / den;
        if den < 0.0 {
            if t > t_in {
                t_in = t;
                n_in = n;
            }
        } else if t < t_out {
            t_out = t;
        }
    }
    if t_in > t_out || t_in < 0.0 || !t_in.is_finite() {
        return None;
    }
    Some((t_in, n_in))
}

/// La skybox 3D : origine et échelle (`scale`, 16 par défaut) de la première `sky_camera`, et la boîte de son
/// contenu, fermée par les brosses de ciel (`toolsskybox`) que touchent des rayons partis de la caméra le long des
/// six axes ; à défaut de ciel dans une direction, la première brosse touchée. `None` sans sky_camera ou si une
/// direction ne touche rien (la caméra n'est pas enfermée).
pub fn boite_skybox(doc: &Document) -> Option<(DVec3, f64, (DVec3, DVec3))> {
    let cam = doc.entites().find(|e| e.classe().eq_ignore_ascii_case("sky_camera") && !e.cache)?;
    let o = cam.origine()?;
    let echelle = cam.get("scale").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| *v > 0.0).unwrap_or(16.0);
    let ciel = |m: &str| m.to_ascii_lowercase().replace('\\', "/").starts_with("tools/toolsskybox");
    let (mut lo, mut hi) = (o, o);
    for d in [DVec3::X, DVec3::NEG_X, DVec3::Y, DVec3::NEG_Y, DVec3::Z, DVec3::NEG_Z] {
        let (mut t_ciel, mut t_autre) = (f64::INFINITY, f64::INFINITY);
        for (_, s) in doc.tous_les_solides() {
            if let Some((t, _)) = rayon_solide(s, o, d) {
                if s.faces.iter().any(|f| ciel(&f.materiau)) {
                    t_ciel = t_ciel.min(t);
                } else {
                    t_autre = t_autre.min(t);
                }
            }
        }
        let t = if t_ciel.is_finite() { t_ciel } else { t_autre };
        if !t.is_finite() {
            return None;
        }
        let p = o + d * t;
        lo = lo.min(p);
        hi = hi.max(p);
    }
    Some((o, echelle, (lo, hi)))
}

/// Origine et angles d'un objet posé sur une surface : son bas (à `bas` sous son origine) touche `point` ; avec
/// `aligner`, son axe haut suit `normale` (sinon il reste droit) ; `lacet` : sa rotation autour de cet axe, en degrés.
pub fn pose_sur(point: DVec3, normale: DVec3, bas: f64, aligner: bool, angles: DVec3) -> (DVec3, DVec3) {
    let n = normale.normalize_or(DVec3::Z);
    if !aligner || (n - DVec3::Z).length() < 1e-9 {
        let haut = if aligner { n } else { DVec3::Z };
        return (point + haut * bas, if aligner { DVec3::new(0.0, angles.y, 0.0) } else { angles });
    }
    // rotation la plus courte de Z vers la normale, après le lacet
    let r = DMat3::from_quat(glam::DQuat::from_rotation_arc(DVec3::Z, n)) * DMat3::from_rotation_z(angles.y.to_radians());
    (point + n * bas, angles_de(r))
}

/// Transforme une entité : origine, angles (si la transformation tourne) et solides.
pub fn transformer_entite(e: &Entite, f: &Affine, verrou_texture: bool) -> Entite {
    let mut e = e.clone();
    if let Some(o) = e.origine() {
        e.set("origin", &texte_vec3(f.point(o)));
    }
    if e.ponctuelle() && f.rigide() && f.l != DMat3::IDENTITY {
        let a = e.angles();
        let m = f.l * hm_geom::angles_to_mat(a.x, a.y, a.z);
        let n = angles_de(m);
        e.set("angles", &texte_vec3(n));
    }
    // un overlay suit : origine de sa base, et sa base tourne avec lui (comme Hammer)
    if let Some(b) = e.get("BasisOrigin").and_then(lire_vec3) {
        e.set("BasisOrigin", &texte_vec3(f.point(b)));
        if f.rigide() && f.l != DMat3::IDENTITY {
            for k in ["BasisU", "BasisV", "BasisNormal"] {
                if let Some(v) = e.get(k).and_then(lire_vec3) {
                    e.set(k, &texte_vec3(f.l * v));
                }
            }
        }
    }
    for s in &mut e.solides {
        *s = transformer_solide(s, f, verrou_texture);
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boite_valide() {
        let s = boite(1, [1, 2, 3, 4, 5, 6], DVec3::new(-64.0, -32.0, 0.0), DVec3::new(64.0, 32.0, 128.0), "dev/dev_measuregeneric01");
        assert!(s.valide());
        let (lo, hi) = s.boite().unwrap();
        assert!((lo - DVec3::new(-64.0, -32.0, 0.0)).length() < 1e-9);
        assert!((hi - DVec3::new(64.0, 32.0, 128.0)).length() < 1e-9);
        // normales sortantes
        let c = (lo + hi) / 2.0;
        for f in &s.faces {
            assert!((f.plan[0] - c).dot(f.normale()) > 0.0);
        }
    }

    #[test]
    fn angles_aller_retour() {
        for (p, y, r) in [(0.0, 0.0, 0.0), (30.0, 45.0, 0.0), (-20.0, 170.0, 10.0), (10.0, -90.0, -35.0)] {
            let m = hm_geom::angles_to_mat(p, y, r);
            let a = angles_de(m);
            let m2 = hm_geom::angles_to_mat(a.x, a.y, a.z);
            assert!((m - m2).to_cols_array().iter().all(|v| v.abs() < 1e-6), "{p} {y} {r} -> {a:?}");
        }
    }

    #[test]
    fn verrou_de_texture_en_translation_et_rotation() {
        let s = boite(1, [1, 2, 3, 4, 5, 6], DVec3::ZERO, DVec3::splat(64.0), "x");
        for f in [Affine::translation(DVec3::new(13.0, -7.0, 32.0)), Affine::rotation(DVec3::new(10.0, 20.0, 0.0), DVec3::Z, 30.0)] {
            let t = transformer_solide(&s, &f, true);
            for (a, b) in s.faces.iter().zip(&t.faces) {
                // un point de la face d'avant et son image ont les mêmes coordonnées de texture
                let p = a.plan[1];
                let q = f.point(p);
                let ua = p.dot(a.u.dir) / a.u.echelle + a.u.decalage;
                let ub = q.dot(b.u.dir) / b.u.echelle + b.u.decalage;
                assert!((ua - ub).abs() < 1e-6);
            }
            assert!(t.valide());
        }
    }

    #[test]
    fn miroir_garde_un_solide_valide() {
        let s = boite(1, [1, 2, 3, 4, 5, 6], DVec3::ZERO, DVec3::new(64.0, 32.0, 16.0), "x");
        let t = transformer_solide(&s, &Affine::miroir(DVec3::ZERO, 0), true);
        assert!(t.valide());
        let (lo, hi) = t.boite().unwrap();
        assert!((lo.x + 64.0).abs() < 1e-9 && hi.x.abs() < 1e-9);
    }
}
