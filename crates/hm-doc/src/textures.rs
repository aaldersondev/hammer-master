//! Calculs d'alignement des textures sur les faces : régler, justifier, aligner sur le monde ou sur la face,
//! enrouler une texture d'une face sur sa voisine.
//!
//! Rappel : `u = (P · U) / échelle_u + décalage_u` en texels (de même pour v), U et V étant les axes de la face.
use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};

use crate::modele::{axes_monde, Axe, Face};

/// Ce qu'on change d'une face ; un champ absent n'est pas touché.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReglageFaces {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub echelle_u: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub echelle_v: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decalage_u: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decalage_v: Option<f64>,
    /// Rotation de la texture en degrés (valeur visée ; les axes tournent de l'écart).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// Unités par luxel (16 par défaut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub echelle_lumiere: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lissage: Option<String>,
    /// Axes U et V donnés tels quels (comme les « UV Vectors » de Hammer++).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axe_u: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axe_v: Option<[f64; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Justif {
    Gauche,
    Droite,
    Haut,
    Bas,
    Centre,
    /// La texture couvre exactement la face (une seule répétition).
    Ajuster,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignement {
    Monde,
    Face,
}

/// Tourne les axes d'une face autour de la normale de leur plan (U × V), de `degres`.
pub fn tourner_axes(f: &mut Face, degres: f64) {
    let axe = f.u.dir.cross(f.v.dir);
    let axe = if axe.length() > 1e-9 { axe.normalize() } else { f.normale() };
    let r = DMat3::from_axis_angle(axe, degres.to_radians());
    f.u.dir = r * f.u.dir;
    f.v.dir = r * f.v.dir;
}

pub fn appliquer_reglage(f: &mut Face, r: &ReglageFaces) {
    if let Some(a) = r.axe_u {
        f.u.dir = DVec3::from(a);
    }
    if let Some(a) = r.axe_v {
        f.v.dir = DVec3::from(a);
    }
    if let Some(rot) = r.rotation {
        let d = rot - f.rotation;
        if d.abs() > 1e-9 {
            tourner_axes(f, d);
        }
        f.rotation = rot;
    }
    if let Some(e) = r.echelle_u.filter(|e| e.abs() > 1e-9) {
        f.u.echelle = e;
    }
    if let Some(e) = r.echelle_v.filter(|e| e.abs() > 1e-9) {
        f.v.echelle = e;
    }
    if let Some(d) = r.decalage_u {
        f.u.decalage = d;
    }
    if let Some(d) = r.decalage_v {
        f.v.decalage = d;
    }
    if let Some(l) = r.echelle_lumiere.filter(|l| *l > 0.0) {
        f.echelle_lumiere = l;
    }
    if let Some(l) = &r.lissage {
        f.lissage = l.clone();
    }
}

/// Coordonnées de texture sans décalage (`P · U / échelle`) des points d'un polygone : (u min, u max, v min, v max).
pub fn etendue(f: &Face, poly: &[DVec3]) -> Option<[f64; 4]> {
    let mut e = [f64::MAX, f64::MIN, f64::MAX, f64::MIN];
    for p in poly {
        let u = p.dot(f.u.dir) / f.u.echelle;
        let v = p.dot(f.v.dir) / f.v.echelle;
        e = [e[0].min(u), e[1].max(u), e[2].min(v), e[3].max(v)];
    }
    (e[0] <= e[1]).then_some(e)
}

/// Un décalage ramené dans une répétition de la texture.
fn modulo(d: f64, taille: f64) -> f64 {
    if taille > 0.0 {
        let r = d.rem_euclid(taille);
        // ni « -0 » ni une répétition entière
        if (r - taille).abs() < 1e-6 || r.abs() < 1e-9 { 0.0 } else { r }
    } else {
        d
    }
}

/// Justifie la texture d'une face sur l'étendue donnée (celle de la face, ou de plusieurs faces traitées comme une).
/// `etendue` doit être mesurée avec les axes et échelles de cette face ; `taille` : taille de la texture en texels.
pub fn justifier(f: &mut Face, etendue_monde: (DVec3, DVec3, &[DVec3]), taille: [f64; 2], j: Justif) {
    let pts = etendue_monde.2;
    let [w, h] = taille;
    if j == Justif::Ajuster {
        // l'étendue en unités du monde le long de chaque axe devient une répétition
        let (mut a0, mut a1, mut b0, mut b1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in pts {
            let (a, b) = (p.dot(f.u.dir), p.dot(f.v.dir));
            a0 = a0.min(a);
            a1 = a1.max(a);
            b0 = b0.min(b);
            b1 = b1.max(b);
        }
        if a1 - a0 > 1e-6 && w > 0.0 {
            f.u.echelle = (a1 - a0) / w;
        }
        if b1 - b0 > 1e-6 && h > 0.0 {
            f.v.echelle = (b1 - b0) / h;
        }
    }
    let Some([u0, u1, v0, v1]) = etendue(f, pts) else { return };
    match j {
        Justif::Gauche | Justif::Ajuster => f.u.decalage = modulo(-u0, w),
        Justif::Droite => f.u.decalage = modulo(w - u1, w),
        Justif::Centre => {
            f.u.decalage = modulo(w / 2.0 - (u0 + u1) / 2.0, w);
            f.v.decalage = modulo(h / 2.0 - (v0 + v1) / 2.0, h);
        }
        _ => {}
    }
    match j {
        Justif::Haut | Justif::Ajuster => f.v.decalage = modulo(-v0, h),
        Justif::Bas => f.v.decalage = modulo(h - v1, h),
        _ => {}
    }
}

/// Aligne la texture sur le monde (axes de Hammer, `TextureAxisFromPlane`) ou sur la face (axes du monde projetés dans
/// le plan de la face). Les échelles et décalages sont gardés ; la rotation repart de 0.
pub fn aligner(f: &mut Face, a: Alignement) {
    let n = f.normale();
    let (u, v) = axes_monde(n);
    match a {
        Alignement::Monde => {
            f.u.dir = u;
            f.v.dir = v;
        }
        Alignement::Face => {
            let pu = (u - n * n.dot(u)).normalize_or(u);
            let mut pv = n.cross(pu).normalize_or(v);
            if pv.dot(v) < 0.0 {
                pv = -pv;
            }
            f.u.dir = pu;
            f.v.dir = pv;
        }
    }
    f.rotation = 0.0;
}

/// Enroule la texture de `source` sur `cible` : les axes tournent avec le pli entre les deux plans, et la texture
/// est continue sur le point commun `pivot` (un point de l'arête partagée quand il y en a une).
pub fn enrouler(source: &Face, cible: &mut Face, pivot: DVec3) {
    let (ns, nt) = (source.normale(), cible.normale());
    let axe = ns.cross(nt);
    let r = if axe.length() > 1e-9 {
        let ang = ns.dot(nt).clamp(-1.0, 1.0).acos();
        DMat3::from_axis_angle(axe.normalize(), ang)
    } else if ns.dot(nt) < 0.0 {
        // plans opposés : demi-tour autour d'un axe de la face source
        DMat3::from_axis_angle(source.u.dir.normalize_or(DVec3::X), std::f64::consts::PI)
    } else {
        DMat3::IDENTITY
    };
    let (u, v) = (r * source.u.dir, r * source.v.dir);
    // même coordonnée au pivot des deux côtés
    let us = pivot.dot(source.u.dir) / source.u.echelle + source.u.decalage;
    let vs = pivot.dot(source.v.dir) / source.v.echelle + source.v.decalage;
    cible.u = Axe { dir: u, echelle: source.u.echelle, decalage: us - pivot.dot(u) / source.u.echelle };
    cible.v = Axe { dir: v, echelle: source.v.echelle, decalage: vs - pivot.dot(v) / source.v.echelle };
    cible.rotation = source.rotation;
    cible.materiau = source.materiau.clone();
}

/// Le point commun à deux polygones (milieu de l'arête partagée), sinon le centre de la cible.
pub fn pivot_commun(source: &[DVec3], cible: &[DVec3]) -> DVec3 {
    let communs: Vec<DVec3> = cible.iter().filter(|p| source.iter().any(|q| (**p - *q).length() < 0.05)).copied().collect();
    if !communs.is_empty() {
        return communs.iter().fold(DVec3::ZERO, |a, p| a + *p) / communs.len() as f64;
    }
    cible.iter().fold(DVec3::ZERO, |a, p| a + *p) / cible.len().max(1) as f64
}
