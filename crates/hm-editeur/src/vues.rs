//! Les vues : une caméra 3D en vol libre, ou une vue 2D orthographique (X/Y, Y/Z, X/Z) avec sa grille.
use egui::{Pos2, Rect};
use glam::{Mat4, Vec2, Vec3};
use hm_render::lignes::Sommet;
use hm_render::{Camera, Projection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Genre {
    Camera3D,
    /// X/Y, vue de dessus
    Dessus,
    /// Y/Z
    Face,
    /// X/Z
    Cote,
}

impl Genre {
    pub fn nom(self) -> &'static str {
        match self {
            Genre::Camera3D => crate::langue::t("3D texturée"),
            Genre::Dessus => crate::langue::t("Dessus (X/Y)"),
            Genre::Face => crate::langue::t("Face (Y/Z)"),
            Genre::Cote => crate::langue::t("Côté (X/Z)"),
        }
    }

    /// Axes horizontal et vertical à l'écran, et l'axe de profondeur.
    pub fn axes(self) -> (usize, usize, usize) {
        match self {
            Genre::Dessus | Genre::Camera3D => (0, 1, 2),
            Genre::Face => (1, 2, 0),
            Genre::Cote => (0, 2, 1),
        }
    }

    pub fn suivant_2d(self) -> Genre {
        match self {
            Genre::Dessus => Genre::Face,
            Genre::Face => Genre::Cote,
            _ => Genre::Dessus,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cam3 {
    pub pos: Vec3,
    /// Lacet en degrés (0 = +X).
    pub lacet: f32,
    /// Tangage en degrés, positif vers le bas (comme le jeu).
    pub tangage: f32,
    /// Champ de vision du jeu (horizontal pour un écran 4:3).
    pub fov: f32,
}

impl Cam3 {
    pub fn avant(&self) -> Vec3 {
        hm_geom::angles_forward(self.tangage as f64, self.lacet as f64).as_vec3()
    }

    pub fn droite(&self) -> Vec3 {
        let (s, c) = (self.lacet - 90.0).to_radians().sin_cos();
        Vec3::new(c, s, 0.0)
    }

    pub fn camera(&self) -> Camera {
        Camera::from_angles(self.pos, self.tangage, self.lacet, hm_render::fov_y_from_game(self.fov))
    }

    /// Regarder un point depuis une position.
    pub fn viser(pos: Vec3, cible: Vec3) -> Cam3 {
        let d = (cible - pos).normalize_or(Vec3::X);
        Cam3 { pos, lacet: d.y.atan2(d.x).to_degrees(), tangage: (-d.z).asin().to_degrees(), fov: 90.0 }
    }
}

pub struct Vue {
    pub genre: Genre,
    pub cam: Cam3,
    /// Vues 2D : point du monde au centre (axes horizontal, vertical de la vue).
    pub centre: Vec2,
    /// Vues 2D : pixels par unité.
    pub zoom: f32,
    /// Rectangle à l'écran (points), relevé à chaque image.
    pub rect: Rect,
    pub cibles: Option<(hm_render::vivant::Cibles, egui::TextureId)>,
    pub constantes: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    /// Constantes des deux passes du ciel (ciel 2D, skybox 3D).
    pub constantes_ciel: Option<[(wgpu::Buffer, wgpu::BindGroup); 2]>,
}

impl Vue {
    pub fn nouvelle(genre: Genre) -> Vue {
        Vue { genre, cam: Cam3 { pos: Vec3::new(-512.0, -512.0, 384.0), lacet: 45.0, tangage: 25.0, fov: 90.0 }, centre: Vec2::ZERO, zoom: 0.5, rect: Rect::NOTHING, cibles: None, constantes: None, constantes_ciel: None }
    }

    /// Point du monde (axes de la vue) sous un point de l'écran.
    pub fn ecran_vers_monde(&self, p: Pos2) -> Vec2 {
        let c = self.rect.center();
        Vec2::new(self.centre.x + (p.x - c.x) / self.zoom, self.centre.y - (p.y - c.y) / self.zoom)
    }

    pub fn monde_vers_ecran(&self, w: Vec2) -> Pos2 {
        let c = self.rect.center();
        Pos2::new(c.x + (w.x - self.centre.x) * self.zoom, c.y - (w.y - self.centre.y) * self.zoom)
    }

    /// Projection des lignes d'une vue 2D : monde → coordonnées de découpe.
    pub fn projection_2d(&self) -> Mat4 {
        let (a, b, _) = self.genre.axes();
        let (w, h) = (self.rect.width().max(1.0), self.rect.height().max(1.0));
        let (sx, sy) = (2.0 * self.zoom / w, 2.0 * self.zoom / h);
        let mut cols = [glam::Vec4::ZERO; 4];
        cols[a].x = sx;
        cols[b].y = sy;
        cols[3] = glam::Vec4::new(-sx * self.centre.x, -sy * self.centre.y, 0.5, 1.0);
        Mat4::from_cols(cols[0], cols[1], cols[2], cols[3])
    }

    pub fn camera(&self) -> Camera {
        match self.genre {
            Genre::Camera3D => self.cam.camera(),
            _ => Camera { eye: Vec3::ZERO, dir: Vec3::NEG_Z, up: Vec3::Y, proj: Projection::Ortho { half_height: self.rect.height() * 0.5 / self.zoom }, near: 0.0, far: 1.0 },
        }
    }

    /// Zoom autour d'un point de l'écran.
    pub fn zoomer(&mut self, facteur: f32, autour: Pos2) {
        let avant = self.ecran_vers_monde(autour);
        self.zoom = (self.zoom * facteur).clamp(0.002, 64.0);
        let apres = self.ecran_vers_monde(autour);
        self.centre += avant - apres;
    }

    /// Cadre une boîte du monde.
    /// À la caméra `pos` → `cible`, comme Hammer rouvre une map : la vue 3D y est placée (son champ gardé), une vue 2D
    /// centrée sur `pos`, au zoom `zoom` s'il est donné.
    pub fn sur_camera(&mut self, pos: Vec3, cible: Vec3, zoom: Option<f32>) {
        if self.genre == Genre::Camera3D {
            let fov = self.cam.fov;
            self.cam = Cam3::viser(pos, cible);
            self.cam.fov = fov;
        } else {
            let (a, b, _) = self.genre.axes();
            self.centre = Vec2::new(pos[a], pos[b]);
            if let Some(z) = zoom {
                self.zoom = z;
            }
        }
    }

    pub fn cadrer(&mut self, lo: Vec3, hi: Vec3) {
        match self.genre {
            Genre::Camera3D => {
                let c = (lo + hi) * 0.5;
                let r = (hi - lo).length().max(256.0);
                self.cam = Cam3::viser(c + Vec3::new(-0.55, -0.75, 0.45).normalize() * r * 0.9, c);
            }
            _ => {
                let (a, b, _) = self.genre.axes();
                self.centre = Vec2::new((lo[a] + hi[a]) * 0.5, (lo[b] + hi[b]) * 0.5);
                let (w, h) = (self.rect.width().max(100.0), self.rect.height().max(100.0));
                self.zoom = ((w / (hi[a] - lo[a]).max(64.0)).min(h / (hi[b] - lo[b]).max(64.0)) * 0.85).clamp(0.002, 64.0);
            }
        }
    }

    /// Lignes de la grille visibles dans la vue : pas `pas`, plus marquées tous les 64 et 1024, axes du monde.
    /// Couleurs de Hammer++ (`hammerplusplus_settings.ini`) : grille (60 60 60), repères (115 115 115), 1024 (100 50 5).
    /// La grille de pas `pas` ; `opts` : (lignes de 64 plus claires, une ligne sur n plus claire, cacher la grille de
    /// moins de 4 pixels, en pointillés : une petite croix à chaque croisement).
    pub fn grille(&self, pas: f32, opts: (bool, u32, bool, bool)) -> Vec<Sommet> {
        let mut out = Vec::new();
        let (a, b, _) = self.genre.axes();
        let lo = self.ecran_vers_monde(self.rect.left_bottom());
        let hi = self.ecran_vers_monde(self.rect.right_top());
        // pas affiché : on saute les lignes de moins de 4 pixels d'écart (comme « cacher la petite grille »)
        let mut p = pas.max(1.0);
        // même sans la cacher, jamais moins de 2 pixels (des milliers de lignes ne se voient plus)
        // en pointillés, des croix serrées ne se voient plus : 8 pixels au moins
        let mini = if opts.3 { 8.0 } else if opts.2 { 4.0 } else { 2.0 };
        while p * self.zoom < mini {
            p *= 2.0;
        }
        let ligne = |out: &mut Vec<Sommet>, u0: f32, v0: f32, u1: f32, v1: f32, c: [u8; 4]| {
            let mut x = [0.0f32; 3];
            let mut y = [0.0f32; 3];
            x[a] = u0;
            x[b] = v0;
            y[a] = u1;
            y[b] = v1;
            out.push(Sommet { pos: x, couleur: c });
            out.push(Sommet { pos: y, couleur: c });
        };
        let couleur = |v: f32| {
            let r = |m: f32| (v / m).round() * m == v;
            if v == 0.0 {
                [40, 90, 140, 255]
            } else if r(1024.0) {
                [100, 50, 5, 255]
            } else if opts.0 && r(64.0) && p < 64.0 {
                [115, 115, 115, 255]
            } else if opts.1 > 0 && ((v / p).round() as i64).rem_euclid(opts.1 as i64) == 0 {
                [90, 90, 90, 255]
            } else {
                [60, 60, 60, 255]
            }
        };
        if opts.3 {
            let r = 1.5 / self.zoom;
            let fond = couleur(0.5 * p);
            let mut u = (lo.x / p).floor() * p;
            while u <= hi.x {
                let mut v = (lo.y / p).floor() * p;
                while v <= hi.y {
                    let c = if couleur(u) != fond { couleur(u) } else { couleur(v) };
                    ligne(&mut out, u - r, v, u + r, v, c);
                    ligne(&mut out, u, v - r, u, v + r, c);
                    v += p;
                }
                u += p;
            }
            return out;
        }
        let mut u = (lo.x / p).floor() * p;
        while u <= hi.x {
            ligne(&mut out, u, lo.y, u, hi.y, couleur(u));
            u += p;
        }
        let mut v = (lo.y / p).floor() * p;
        while v <= hi.y {
            ligne(&mut out, lo.x, v, hi.x, v, couleur(v));
            v += p;
        }
        out
    }
}

/// Les quatre rectangles des vues dans la zone centrale, séparés aux fractions `sep`.
pub fn disposer(zone: Rect, sep: [f32; 2], agrandie: Option<usize>) -> [Rect; 4] {
    if let Some(i) = agrandie {
        let mut r = [Rect::NOTHING; 4];
        r[i] = zone;
        return r;
    }
    let x = zone.left() + zone.width() * sep[0];
    let y = zone.top() + zone.height() * sep[1];
    let g = 2.0; // épaisseur du séparateur
    [
        Rect::from_min_max(zone.min, Pos2::new(x - g, y - g)),
        Rect::from_min_max(Pos2::new(x + g, zone.top()), Pos2::new(zone.right(), y - g)),
        Rect::from_min_max(Pos2::new(zone.left(), y + g), Pos2::new(x - g, zone.bottom())),
        Rect::from_min_max(Pos2::new(x + g, y + g), zone.max),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecran_et_monde() {
        let mut v = Vue::nouvelle(Genre::Dessus);
        v.rect = Rect::from_min_size(Pos2::new(100.0, 50.0), egui::vec2(400.0, 300.0));
        v.centre = Vec2::new(64.0, -32.0);
        v.zoom = 2.0;
        let w = Vec2::new(80.0, 16.0);
        let e = v.monde_vers_ecran(w);
        assert!((v.ecran_vers_monde(e) - w).length() < 1e-4);
        // nord en haut : un y plus grand est plus haut à l'écran
        assert!(v.monde_vers_ecran(Vec2::new(0.0, 100.0)).y < v.monde_vers_ecran(Vec2::new(0.0, 0.0)).y);
        // la projection envoie le centre au centre
        let c = v.projection_2d() * glam::Vec4::new(64.0, -32.0, 999.0, 1.0);
        assert!(c.x.abs() < 1e-5 && c.y.abs() < 1e-5);
        // zoom autour d'un point : ce point ne bouge pas
        let p = Pos2::new(150.0, 80.0);
        let avant = v.ecran_vers_monde(p);
        v.zoomer(1.7, p);
        assert!((v.ecran_vers_monde(p) - avant).length() < 1e-3);
    }

    #[test]
    fn camera_viser() {
        let c = Cam3::viser(Vec3::ZERO, Vec3::new(100.0, 100.0, -100.0));
        let a = c.avant();
        assert!((a - Vec3::new(1.0, 1.0, -1.0).normalize()).length() < 1e-4);
    }
}
