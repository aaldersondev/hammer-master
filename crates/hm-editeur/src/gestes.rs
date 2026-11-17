//! Les gestes de la souris dans les vues : ils ne modifient rien pendant le glisser (un aperçu est dessiné) et
//! deviennent une commande du noyau au relâcher.
use egui::{Pos2, Rect};
use glam::{DVec3, Vec2, Vec3};
use hm_doc::geo::Affine;

use crate::vues::{Genre, Vue};

/// Les poignées de la sélection dans une vue 2D : taille (Hammer : premier clic) ou rotation (second clic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModePoignees {
    Taille,
    Rotation,
}

#[derive(Clone, Debug)]
pub enum Geste {
    /// Cadre de sélection (coordonnées du monde dans les axes de la vue).
    Cadre { vue: usize, debut: Vec2, fin: Vec2 },
    /// Déplacer la sélection ; `cloner` : Maj tenue, une copie est déplacée.
    Deplacer { vue: usize, debut: Vec2, delta: Vec3, cloner: bool },
    /// Tirer une poignée de taille : `cote` = -1, 0 ou 1 sur chacun des deux axes de la vue.
    Taille { cote: [i8; 2], boite0: (Vec3, Vec3), boite: (Vec3, Vec3) },
    /// Tourner autour du centre de la sélection, dans le plan de la vue.
    Tourner { vue: usize, centre: Vec3, depart: f32, angle: f32 },
    /// Flèche du manipulateur 3D le long d'un axe (`vecteur` : l'axe du monde, ou celui de l'entité en repère local).
    Fleche { axe: usize, vecteur: Vec3, depart: Pos2, delta: f32 },
    /// Tracer un bloc avec l'outil bloc.
    Bloc { vue: usize, debut: Vec2, fin: Vec2 },
}

impl Geste {
    /// La transformation que le geste montre en aperçu.
    pub fn affine(&self, vues: &[Vue]) -> Option<Affine> {
        match self {
            Geste::Deplacer { delta, .. } => Some(Affine::translation(delta.as_dvec3())),
            Geste::Taille { boite0, boite, .. } => Some(Affine::boite((boite0.0.as_dvec3(), boite0.1.as_dvec3()), (boite.0.as_dvec3(), boite.1.as_dvec3()))),
            Geste::Tourner { vue, centre, angle, .. } => Some(Affine::rotation(centre.as_dvec3(), axe_rotation(vues[*vue].genre), *angle as f64)),
            Geste::Fleche { vecteur, delta, .. } => Some(Affine::translation((*vecteur * *delta).as_dvec3())),
            _ => None,
        }
    }
}

/// Axe de rotation d'une vue 2D : tourner dans le sens direct à l'écran (axe horizontal vers l'axe vertical).
pub fn axe_rotation(g: Genre) -> DVec3 {
    let (a, b, _) = g.axes();
    let mut ea = DVec3::ZERO;
    let mut eb = DVec3::ZERO;
    ea[a] = 1.0;
    eb[b] = 1.0;
    ea.cross(eb)
}

pub fn aimanter(v: f32, pas: f32, actif: bool) -> f32 {
    if actif && pas > 0.0 {
        (v / pas).round() * pas
    } else {
        v
    }
}

/// Le rectangle de la sélection dans une vue 2D (points de l'écran).
pub fn rect_ecran(v: &Vue, lo: Vec3, hi: Vec3) -> Rect {
    let (a, b, _) = v.genre.axes();
    let p0 = v.monde_vers_ecran(Vec2::new(lo[a], lo[b]));
    let p1 = v.monde_vers_ecran(Vec2::new(hi[a], hi[b]));
    Rect::from_two_pos(p0, p1)
}

/// Les huit poignées de taille (côté sur chaque axe, position à l'écran), ou les quatre de rotation.
pub fn poignees(r: Rect, mode: ModePoignees) -> Vec<([i8; 2], Pos2)> {
    let (x0, x1, y0, y1) = (r.left(), r.right(), r.bottom(), r.top());
    let (xm, ym) = (r.center().x, r.center().y);
    // l'axe vertical de la vue monte vers le haut de l'écran : côté +1 = haut
    let coins = vec![([-1, -1], Pos2::new(x0, y0)), ([1, -1], Pos2::new(x1, y0)), ([1, 1], Pos2::new(x1, y1)), ([-1, 1], Pos2::new(x0, y1))];
    match mode {
        ModePoignees::Rotation => coins,
        ModePoignees::Taille => {
            let mut p = coins;
            p.extend([([0, -1], Pos2::new(xm, y0)), ([0, 1], Pos2::new(xm, y1)), ([-1, 0], Pos2::new(x0, ym)), ([1, 0], Pos2::new(x1, ym))]);
            p
        }
    }
}

/// Décale les poignées hors du rectangle (comme Hammer, elles sont posées autour de la boîte).
pub fn ecarter(r: Rect, p: Pos2, cote: [i8; 2]) -> Pos2 {
    let _ = r;
    p + egui::vec2(cote[0] as f32 * 7.0, -(cote[1] as f32) * 7.0)
}

/// Nouvelle boîte quand on tire la poignée `cote` jusqu'au point du monde `w` (axes de la vue).
pub fn tirer(boite0: (Vec3, Vec3), g: Genre, cote: [i8; 2], w: Vec2, pas: f32, aimant: bool) -> (Vec3, Vec3) {
    let (a, b, _) = g.axes();
    let (mut lo, mut hi) = boite0;
    let min_taille = if aimant { pas.max(1.0) } else { 1.0 };
    for (k, axe) in [(0usize, a), (1usize, b)] {
        let v = aimanter(w[k], pas, aimant);
        match cote[k] {
            1 => hi[axe] = v.max(lo[axe] + min_taille),
            -1 => lo[axe] = v.min(hi[axe] - min_taille),
            _ => {}
        }
    }
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_dans_le_plan_de_la_vue() {
        // vue de dessus : +90° envoie +X sur +Y
        let f = Affine::rotation(DVec3::ZERO, axe_rotation(Genre::Dessus), 90.0);
        assert!((f.point(DVec3::X) - DVec3::Y).length() < 1e-9);
        // côté X/Z : +90° envoie +X sur +Z
        let f = Affine::rotation(DVec3::ZERO, axe_rotation(Genre::Cote), 90.0);
        assert!((f.point(DVec3::X) - DVec3::Z).length() < 1e-9);
        // face Y/Z : +90° envoie +Y sur +Z
        let f = Affine::rotation(DVec3::ZERO, axe_rotation(Genre::Face), 90.0);
        assert!((f.point(DVec3::Y) - DVec3::Z).length() < 1e-9);
    }

    #[test]
    fn tirer_une_poignee() {
        let b0 = (Vec3::ZERO, Vec3::splat(64.0));
        let (lo, hi) = tirer(b0, Genre::Dessus, [1, 0], Vec2::new(130.0, 999.0), 16.0, true);
        assert_eq!((lo, hi), (Vec3::ZERO, Vec3::new(128.0, 64.0, 64.0)));
        // jamais plat : la poignée s'arrête un pas avant le côté opposé
        let (lo, _) = tirer(b0, Genre::Face, [0, -1], Vec2::new(0.0, 200.0), 16.0, true);
        assert_eq!(lo.z, 48.0);
    }
}
