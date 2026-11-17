//! L'aperçu de l'éclairage de la map dans la vue 3D (menu Affichage) : les light, light_spot et la
//! light_environment de la map, avec leurs ombres, à la place de la lumière de contrôle. Le calcul est celui de vrad
//! (vérifié contre les lightmaps d'une compilation) sans les rebonds, que remplace une lumière ajoutée réglable.
//! Les lumières les plus proches de la caméra ont des ombres (`MAX_OMBRES`) ; leurs cartes ne sont refaites que
//! quand elles bougent ou que la géométrie change, et pas pendant un geste à la souris.
use glam::Vec3;
use hm_render::eclairage::{Eclairage, LumiereRendu, MAX_LUMIERES, MAX_OMBRES};

use super::Editeur;
use crate::vues::Genre;

pub struct EtatEclairage {
    pub actif: bool,
    pub ombres: bool,
    /// Lumière ajoutée partout, en part de blanc : les rebonds que l'aperçu ne calcule pas.
    pub ajout: f32,
    /// Lumières de la map lues pour cette version du document : (version, lumières, soleil, ciel).
    cache: Option<(u64, Vec<LumiereRendu>, Option<(Vec3, Vec3)>, Vec3)>,
    /// Position de la caméra et version du document au dernier tri.
    tri: Option<(Vec3, u64)>,
    ordre: Vec<LumiereRendu>,
    /// Version de la géométrie donnée aux ombres (gardée pendant un geste).
    pub version_ombres: u64,
}

impl Default for EtatEclairage {
    fn default() -> Self {
        EtatEclairage { actif: false, ombres: true, ajout: 0.0, cache: None, tri: None, ordre: Vec::new(), version_ombres: 0 }
    }
}

impl Editeur {
    /// Ce que la vue 3D éclaire cette image, si l'aperçu est actif.
    pub(super) fn eclairage_courant(&mut self) -> Option<Eclairage> {
        if !self.eclairage.actif {
            return None;
        }
        let v = self.session.version();
        if self.eclairage.cache.as_ref().map_or(true, |c| c.0 != v) {
            let (ls, soleil) = self.session.doc().lumieres();
            let conv = ls
                .iter()
                .map(|l| LumiereRendu {
                    position: l.position.as_vec3(),
                    couleur: l.couleur.as_vec3(),
                    attenuation: l.attenuation.map(|x| x as f32),
                    fondu: l.fondu.map(|(a, b)| (a as f32, b as f32)),
                    cone: l.cone.map(|c| (c.direction.as_vec3(), c.cos_int as f32, c.cos_ext as f32, c.exposant as f32)),
                })
                .collect();
            let s = soleil.map(|s| (s.direction.as_vec3(), s.couleur.as_vec3()));
            let ciel = soleil.map_or(Vec3::ZERO, |s| s.ambiance.as_vec3());
            self.eclairage.cache = Some((v, conv, s, ciel));
            self.eclairage.tri = None;
        }
        // les plus proches de la caméra d'abord (elles ont les ombres) ; l'ordre n'est refait qu'après un vrai
        // déplacement, pour ne pas refaire les cartes d'ombre à chaque image
        let cam = self.vues.iter().find(|x| x.genre == Genre::Camera3D).map(|x| x.camera().eye).unwrap_or(Vec3::ZERO);
        if self.eclairage.tri.map_or(true, |(p, ver)| ver != v || p.distance(cam) > 256.0) {
            let mut o = self.eclairage.cache.as_ref().unwrap().1.clone();
            o.sort_by(|a, b| a.position.distance_squared(cam).total_cmp(&b.position.distance_squared(cam)));
            self.eclairage.ordre = o;
            self.eclairage.tri = Some((cam, v));
        }
        let c = self.eclairage.cache.as_ref().unwrap();
        let (min, max) = self.geo.as_ref().map_or((Vec3::splat(-512.0), Vec3::splat(512.0)), |g| (g.min, g.max));
        Some(Eclairage { lumieres: self.eclairage.ordre.clone(), soleil: c.2, ciel: c.3, ajout: Vec3::splat(self.eclairage.ajout), ombres: self.eclairage.ombres, bornes: (min, max) })
    }

    /// Le texte de la barre d'état quand l'aperçu est actif.
    pub(super) fn etat_eclairage(&self) -> Option<String> {
        if !self.eclairage.actif {
            return None;
        }
        let c = self.eclairage.cache.as_ref()?;
        let n = c.1.len();
        let mut t = format!("éclairage : {n} lumière(s)");
        if n > MAX_LUMIERES {
            t += &format!(", les {MAX_LUMIERES} plus proches");
        }
        if self.eclairage.ombres && n > 0 {
            t += &format!(", ombres pour {}", n.min(MAX_OMBRES));
        }
        if c.2.is_some() {
            t += ", soleil";
        }
        Some(t)
    }

    pub(super) fn menu_eclairage(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.eclairage.actif, format!("Éclairage de la map (aperçu)\t{}", self.rc.texte("View_Lighting_Preview")))
            .on_hover_text("les lumières de la map et leurs ombres, comme vrad les calcule, sans les rebonds");
        ui.add_enabled_ui(self.eclairage.actif, |ui| {
            ui.checkbox(&mut self.eclairage.ombres, "Ombres");
            ui.horizontal(|ui| {
                ui.label("Rebonds (lumière ajoutée)");
                ui.add(egui::Slider::new(&mut self.eclairage.ajout, 0.0..=0.3).fixed_decimals(2));
            });
        });
    }
}
