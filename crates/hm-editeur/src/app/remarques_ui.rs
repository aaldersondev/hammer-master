//! Remarques faites en jeu (addon `hammer_master_signalement`, `!defaut …`) pour la map ouverte : un marqueur dans
//! chaque vue (trait de l'œil du joueur au point visé, croix sur le point ; magenta si ouverte, gris sinon), et
//! l'onglet « Remarques » : texte, état, capture du jeu ; double clic : la caméra 3D à l'œil du joueur, dans sa
//! direction ; « Corrigée », « Écartée », « Rouvrir » (suivi gardé près du VMF, `.hammer-master/signalements.json`).
use std::path::PathBuf;

use hm_render::lignes::Sommet;
use hm_signal::{Etats, Signalement};

use super::Editeur;
use crate::vues::Genre;

#[derive(Default)]
pub struct EtatRemarques {
    /// Pour quelle map (nom de fichier) : les remarques, leur suivi.
    lues: Option<(String, Vec<Signalement>, Etats)>,
    choisie: Option<usize>,
    image: Option<(usize, egui::TextureHandle)>,
    erreur: Option<String>,
}

impl Editeur {
    fn nom_carte(&self) -> Option<String> {
        self.session.chemin().and_then(|c| c.file_stem()).map(|s| s.to_string_lossy().to_ascii_lowercase())
    }

    fn suivi(&self) -> Option<PathBuf> {
        self.session.chemin().and_then(|c| c.parent()).map(|d| d.join(".hammer-master"))
    }

    /// Relit les remarques si la map ouverte a changé.
    fn remarques_a_jour(&mut self, forcer: bool) {
        let Some(nom) = self.nom_carte() else {
            self.remarques.lues = None;
            return;
        };
        if !forcer && self.remarques.lues.as_ref().map_or(false, |(n, _, _)| *n == nom) {
            return;
        }
        let etats = self.suivi().map(|s| Etats::charger(&s)).unwrap_or_default();
        match hm_signal::lire(&hm_signal::dossiers_par_defaut(&nom)) {
            Ok(l) => {
                self.remarques.erreur = None;
                self.remarques.lues = Some((nom, l, etats));
            }
            Err(e) => {
                self.remarques.erreur = Some(format!("{e:#}"));
                self.remarques.lues = Some((nom, Vec::new(), etats));
            }
        }
        self.remarques.choisie = None;
        self.remarques.image = None;
    }

    /// Les marqueurs des remarques, pour les vues.
    pub(super) fn lignes_remarques(&mut self) -> Vec<Sommet> {
        self.remarques_a_jour(false);
        let mut v = Vec::new();
        let Some((_, l, etats)) = &self.remarques.lues else { return v };
        for (i, s) in l.iter().enumerate() {
            let ouverte = etats.de(s).etat == "ouvert";
            let choisie = self.remarques.choisie == Some(i);
            let c = if choisie { [255, 255, 255, 255] } else if ouverte { [255, 150, 0, 255] } else { [140, 140, 140, 255] };
            let (o, p) = (glam::Vec3::from(s.oeil), glam::Vec3::from(s.vise));
            v.push(Sommet { pos: o.to_array(), couleur: c });
            v.push(Sommet { pos: p.to_array(), couleur: c });
            for d in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z] {
                v.push(Sommet { pos: (p - d * 16.0).to_array(), couleur: c });
                v.push(Sommet { pos: (p + d * 16.0).to_array(), couleur: c });
            }
        }
        v
    }

    /// La caméra 3D à l'œil du joueur, dans sa direction ; les vues 2D centrées sur le point visé.
    fn aller_a_remarque(&mut self, i: usize) {
        let Some(s) = self.remarques.lues.as_ref().and_then(|(_, l, _)| l.get(i)).cloned() else { return };
        for v in &mut self.vues {
            if v.genre == Genre::Camera3D {
                v.cam.pos = glam::Vec3::from(s.oeil);
                v.cam.tangage = s.angles[0];
                v.cam.lacet = s.angles[1];
            } else {
                let p = glam::Vec3::from(s.vise);
                v.cadrer(p - glam::Vec3::splat(256.0), p + glam::Vec3::splat(256.0));
            }
        }
    }

    pub(super) fn panneau_remarques(&mut self, ui: &mut egui::Ui) {
        self.remarques_a_jour(false);
        ui.horizontal(|ui| {
            if ui.button(crate::langue::t("Relire")).on_hover_text(crate::langue::t("après de nouvelles remarques en jeu")).clicked() {
                self.remarques_a_jour(true);
            }
            match &self.remarques.lues {
                Some((nom, l, _)) => ui.label(tr!("{} remarque(s) pour {nom}", "{} remark(s) for {nom}", l.len())),
                None => ui.weak(crate::langue::t("enregistrer la map : les remarques se rangent par nom de map")),
            };
        });
        if let Some(e) = &self.remarques.erreur {
            ui.colored_label(egui::Color32::from_rgb(255, 130, 100), e);
        }
        let Some((_, l, etats)) = &self.remarques.lues else { return };
        let mut aller = None;
        let mut choisir = None;
        egui::ScrollArea::vertical().id_salt("remarques").max_height(ui.available_height() * 0.45).auto_shrink(false).show(ui, |ui| {
            for (i, s) in l.iter().enumerate() {
                let e = etats.de(s).etat;
                let couleur = if e == "ouvert" { egui::Color32::from_rgb(255, 170, 60) } else { ui.visuals().weak_text_color() };
                let t = egui::RichText::new(format!("{} · {} · {} — {}", s.n, s.quand, e, s.texte)).color(couleur);
                let r = ui.add(egui::Button::selectable(self.remarques.choisie == Some(i), t).wrap());
                if r.double_clicked() {
                    aller = Some(i);
                } else if r.clicked() {
                    choisir = Some(i);
                }
            }
        });
        if let Some(i) = aller.or(choisir) {
            self.remarques.choisie = Some(i);
        }
        if let Some(i) = aller {
            self.aller_a_remarque(i);
        }
        let Some(i) = self.remarques.choisie else { return };
        ui.horizontal(|ui| {
            if ui.button(crate::langue::t("Aller (vue du joueur)")).clicked() {
                self.aller_a_remarque(i);
            }
            for (etat, libelle) in [("corrige", crate::langue::t("Corrigée")), ("ecarte", crate::langue::t("Écartée")), ("ouvert", crate::langue::t("Rouvrir"))] {
                if ui.button(libelle).clicked() {
                    if let (Some(suivi), Some((_, l, etats))) = (self.suivi(), &mut self.remarques.lues) {
                        let r = etats.fixer(&l[i], etat, "", &hm_signal::maintenant()).and_then(|_| etats.enregistrer(&suivi));
                        if let Err(e) = r {
                            self.remarques.erreur = Some(format!("{e:#}"));
                        }
                    }
                }
            }
        });
        // la capture du jeu, chargée une fois
        let Some(s) = self.remarques.lues.as_ref().and_then(|(_, l, _)| l.get(i)).cloned() else { return };
        if self.remarques.image.as_ref().map_or(true, |(k, _)| *k != i) {
            self.remarques.image = s.capture.as_ref().and_then(|p| image::open(p).ok()).map(|img| {
                let rgba = img.to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &rgba);
                (i, ui.ctx().load_texture(tr!("remarque {i}", "remark {i}"), ci, Default::default()))
            });
        }
        ui.label(egui::RichText::new(&s.texte).strong());
        ui.weak(tr!("visé : {} ({:.0}, {:.0}, {:.0})", "aimed at: {} ({:.0}, {:.0}, {:.0})", if s.texture.is_empty() { "?" } else { &s.texture }, s.vise[0], s.vise[1], s.vise[2]));
        if let Some((_, t)) = &self.remarques.image {
            let w = ui.available_width();
            let sz = t.size_vec2();
            ui.image((t.id(), egui::vec2(w, w * sz.y / sz.x.max(1.0))));
        }
    }
}
