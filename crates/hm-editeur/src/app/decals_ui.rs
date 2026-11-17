//! Outils Décal (Maj+D) et Overlay (Maj+O) : un clic dans la vue 3D sur une surface y pose un `infodecal` ou un
//! `info_overlay` du matériau courant (wiki Valve, « Hammer Overlay Tool » : on choisit le matériau, puis on clique
//! la surface dans la 3D). L'overlay prend la taille de sa texture fois son `$decalscale` (comme un décal) ; ses coins se règlent ensuite
//! dans ses propriétés (`uv0`…`uv3`).
use glam::DVec3;
use hm_doc::{Commande, RefFace};

use super::{Editeur, Outil};

impl Editeur {
    /// Le rayon qui part de la caméra de la vue 3D `i` par le pixel `p`.
    pub(super) fn rayon_vue(&self, i: usize, p: egui::Pos2) -> (DVec3, DVec3) {
        let v = &self.vues[i];
        let r = v.rect;
        let (x, y) = ((p.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (p.y - r.top()) / r.height() * 2.0);
        let (f, droite) = (v.cam.avant(), v.cam.droite());
        let haut = droite.cross(f);
        let t = (hm_render::fov_y_from_game(v.cam.fov).to_radians() * 0.5).tan();
        let dir = (f + droite * x * t * (r.width() / r.height()) + haut * y * t).normalize();
        (v.cam.pos.as_dvec3(), dir.as_dvec3())
    }

    pub(super) fn clic_decal_overlay(&mut self, i: usize, p: egui::Pos2) {
        let (o, d) = self.rayon_vue(i, p);
        let visibles: std::collections::HashSet<u64> = self.geo.as_ref().map(|g| g.formes.iter().filter_map(|f| f.designe.solide).collect()).unwrap_or_default();
        let Some(imp) = hm_doc::geo::rayon(self.session.doc(), o, d, &|s| !visibles.contains(&s)) else {
            self.statut = "aucune surface sous le curseur".into();
            return;
        };
        let materiau = self.materiau.clone();
        match self.outil {
            Outil::Decal => self.executer(Commande::PoserDecal { point: imp.point.to_array(), materiau }),
            Outil::Overlay => {
                // la face touchée : celle du solide dont le plan passe par le point, de même normale
                let Some(s) = self.session.doc().solide(imp.solide) else { return };
                let Some(face) = s.faces.iter().filter(|f| f.normale().dot(imp.normale) > 0.999).min_by(|a, b| {
                    let e = |f: &hm_doc::Face| (f.normale().dot(imp.point) - f.normale().dot(f.plan[1])).abs();
                    e(a).total_cmp(&e(b))
                }) else {
                    return;
                };
                let rf = RefFace { solide: s.id, face: face.id };
                // la taille de la texture fois son $decalscale, comme un décal
                let k = hm_assets::load_material(&self.vfs, &materiau).decalscale as f64;
                let taille = match self.gfx.as_mut() {
                    Some(g) => {
                        let t = g.bib.materiau(&g.r, &self.vfs, &materiau).taille;
                        [(t.0 as f64 * k).max(8.0), (t.1 as f64 * k).max(8.0)]
                    }
                    None => [128.0, 128.0],
                };
                self.executer(Commande::PoserOverlay { face: rf, point: imp.point.to_array(), materiau, taille });
            }
            _ => {}
        }
    }
}
