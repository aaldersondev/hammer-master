//! La vue d'un constat des règles : d'où le regarder, et son image encadrée.
use glam::Vec3;
use hm_regles::{Constat, Monde};
use hm_render::planche::{Image, Police};
use hm_render::{Camera, GpuScene, Lighting, Renderer, View};
use hm_scene::Scene;

pub struct VueConstat {
    /// L'image sans légende, avec le cadre jaune autour de la boîte du constat.
    pub image: Image,
    pub oeil: Vec3,
    pub cible: Vec3,
}

/// Rend la vue d'un constat : l'œil est choisi seul, en espace libre, là d'où la boîte du constat se voit le mieux.
pub fn vue_constat(renderer: &Renderer, gs: &GpuScene, scene: &Scene, monde: &Monde, c: &Constat, w: u32, h: u32) -> VueConstat {
    let (min, max) = (Vec3::from(c.min), Vec3::from(c.max));
    // un constat sur une pièce entière (règles de rôle, pièce inaccessible) se regarde de l'intérieur
    if c.objet == 0 && c.regle.starts_with("sem.") && (max.x - min.x).min(max.y - min.y) >= 96.0 && max.z - min.z <= 400.0 {
        if let Some(v) = vue_interieure(renderer, gs, scene, monde, min, max, w, h) {
            return v;
        }
    }
    let mut cibles = vec![c.objet];
    cibles.extend(&c.autres);
    let (oeil, cible) = monde.point_de_vue(min, max, &cibles);
    let camera = Camera::look_at(oeil, cible, 62.0);
    let frame = renderer.render(gs, &View { camera, width: w * 2, height: h * 2, show_tools: false, clip: None, light: Lighting::neutral(scene) });
    let mut img = crate::reduire2(&frame.rgba, w * 2, h * 2);
    let coins: Vec<(f32, f32)> = (0..8).filter_map(|i| camera.project(Vec3::new(if i & 1 == 0 { min.x } else { max.x }, if i & 2 == 0 { min.y } else { max.y }, if i & 4 == 0 { min.z } else { max.z }), w, h)).collect();
    if coins.len() == 8 {
        let (x0, x1) = (coins.iter().map(|p| p.0).fold(f32::MAX, f32::min).max(2.0) as i32, coins.iter().map(|p| p.0).fold(f32::MIN, f32::max).min(w as f32 - 3.0) as i32);
        let (y0, y1) = (coins.iter().map(|p| p.1).fold(f32::MAX, f32::min).max(2.0) as i32, coins.iter().map(|p| p.1).fold(f32::MIN, f32::max).min(h as f32 - 3.0) as i32);
        let jaune = [255, 220, 40];
        img.fill_rect(x0, y0, x1 - x0, 2, jaune, 0.9);
        img.fill_rect(x0, y1, x1 - x0, 2, jaune, 0.9);
        img.fill_rect(x0, y0, 2, y1 - y0, jaune, 0.9);
        img.fill_rect(x1, y0, 2, y1 - y0 + 2, jaune, 0.9);
    }
    VueConstat { image: img, oeil, cible }
}

/// Légende d'un constat : son nom de vue et sa position, puis son message coupé en lignes.
pub fn legende(nom: &str, c: &Constat, largeur: usize) -> Vec<String> {
    let mut lignes = vec![format!("{nom} · vers ({:.0} {:.0} {:.0})", c.pos[0], c.pos[1], c.pos[2])];
    let mut ligne = String::new();
    for mot in c.message.split(' ') {
        if ligne.chars().count() + mot.chars().count() > largeur {
            lignes.push(std::mem::take(&mut ligne));
        }
        if !ligne.is_empty() {
            ligne.push(' ');
        }
        ligne.push_str(mot);
    }
    lignes.push(ligne);
    lignes.truncate(4);
    lignes
}

/// L'image seule, avec sa légende écrite dessus.
pub fn avec_legende(v: &VueConstat, lignes: &[String], police: &Police) -> Image {
    let mut seule = Image::new(v.image.width, v.image.height, [0, 0, 0]);
    seule.blit(&v.image, 0, 0);
    for (i, l) in lignes.iter().enumerate() {
        police.etiquette(&mut seule, 0, i as i32 * 15, 11.5, l, if i == 0 { [255, 232, 80] } else { [230, 230, 230] });
    }
    seule
}

/// Vue d'une pièce prise de l'intérieur : l'œil dans le coin d'où le regard porte le plus loin, à 64 du sol, tourné
/// vers le coin opposé. `None` si aucun coin n'est libre.
#[allow(clippy::too_many_arguments)]
pub fn vue_interieure(renderer: &Renderer, gs: &GpuScene, scene: &Scene, monde: &Monde, lo: Vec3, hi: Vec3, w: u32, h: u32) -> Option<VueConstat> {
    let centre = (lo + hi) * 0.5;
    let mut meilleur: Option<(f32, Vec3)> = None;
    for (fx, fy) in [(0.12f32, 0.12f32), (0.88, 0.12), (0.12, 0.88), (0.88, 0.88)] {
        let oeil = Vec3::new(lo.x + (hi.x - lo.x) * fx, lo.y + (hi.y - lo.y) * fy, lo.z + 64.0);
        if monde.dans(oeil, |k, _| k.visible).is_some() {
            continue;
        }
        // un meuble juste sous l'œil (un classeur de 62 de haut) boucherait la moitié de la vue
        if monde.rayon(oeil, oeil - Vec3::Z * 56.0, |k, _| k.visible).is_some() {
            continue;
        }
        let vise = Vec3::new(lo.x + (hi.x - lo.x) * (1.0 - fx), lo.y + (hi.y - lo.y) * (1.0 - fy), lo.z + 40.0);
        let libre = monde.rayon(oeil, vise, |k, _| k.visible).map_or(1.0, |x| x.0);
        if meilleur.map_or(true, |m| libre > m.0) {
            meilleur = Some((libre, oeil));
        }
    }
    let (_, oeil) = meilleur?;
    let cible = Vec3::new(centre.x, centre.y, lo.z + 40.0) + (Vec3::new(centre.x, centre.y, lo.z + 64.0) - oeil) * 0.6;
    let camera = Camera::look_at(oeil, cible, 80.0);
    let frame = renderer.render(gs, &View { camera, width: w * 2, height: h * 2, show_tools: false, clip: None, light: Lighting::neutral(scene) });
    Some(VueConstat { image: crate::reduire2(&frame.rgba, w * 2, h * 2), oeil, cible })
}
