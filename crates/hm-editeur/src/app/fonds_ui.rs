//! Images de fond des vues 2D (« Set Background Images » de Hammer) : une image par type de vue (dessus, face, côté),
//! posée à une place et une largeur du monde, en transparence par-dessus la grille, pour calquer un plan.
use std::path::PathBuf;

use egui::{Color32, Pos2, Rect};

use super::Editeur;
use crate::vues::Genre;

pub struct Fond {
    chemin: PathBuf,
    texture: Option<egui::TextureHandle>,
    /// Taille de l'image en pixels.
    taille: [usize; 2],
    /// Centre de l'image dans le monde (axes de la vue) et sa largeur en unités.
    centre: [f32; 2],
    largeur: f32,
    opacite: f32,
}

#[derive(Default)]
pub struct EtatFonds {
    pub dialogue: bool,
    /// Dessus, face, côté.
    fonds: [Option<Fond>; 3],
}

fn rang(g: Genre) -> Option<usize> {
    match g {
        Genre::Dessus => Some(0),
        Genre::Face => Some(1),
        Genre::Cote => Some(2),
        Genre::Camera3D => None,
    }
}

impl Editeur {
    pub(super) fn dessiner_fond(&self, i: usize, p: &egui::Painter) {
        let v = &self.vues[i];
        let Some(f) = rang(v.genre).and_then(|k| self.fonds.fonds[k].as_ref()) else { return };
        let Some(t) = &f.texture else { return };
        let hauteur = f.largeur * f.taille[1] as f32 / f.taille[0].max(1) as f32;
        let (a, b) = (v.monde_vers_ecran(glam::Vec2::new(f.centre[0] - f.largeur / 2.0, f.centre[1] + hauteur / 2.0)), v.monde_vers_ecran(glam::Vec2::new(f.centre[0] + f.largeur / 2.0, f.centre[1] - hauteur / 2.0)));
        let teinte = Color32::from_white_alpha((f.opacite.clamp(0.0, 1.0) * 255.0) as u8);
        p.image(t.id(), Rect::from_two_pos(a, b), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), teinte);
    }

    pub(super) fn fenetre_fonds(&mut self, ctx: &egui::Context) {
        if !self.fonds.dialogue {
            return;
        }
        let mut ouvert = true;
        let mut charger = None;
        super::barres::fenetre(ctx, "Images de fond des vues 2D").resizable(false).open(&mut ouvert).show(ctx, |ui| {
            for (k, nom) in ["Dessus (X/Y)", "Face (Y/Z)", "Côté (X/Z)"].iter().enumerate() {
                ui.strong(*nom);
                ui.horizontal(|ui| {
                    if ui.button("Choisir une image…").clicked() {
                        charger = Some(k);
                    }
                    if self.fonds.fonds[k].is_some() && ui.button("Retirer").clicked() {
                        self.fonds.fonds[k] = None;
                    }
                });
                if let Some(f) = &mut self.fonds.fonds[k] {
                    ui.weak(f.chemin.display().to_string());
                    ui.horizontal(|ui| {
                        ui.label("centre");
                        ui.add(egui::DragValue::new(&mut f.centre[0]).speed(4.0));
                        ui.add(egui::DragValue::new(&mut f.centre[1]).speed(4.0));
                        ui.label("largeur");
                        ui.add(egui::DragValue::new(&mut f.largeur).speed(4.0).range(1.0..=65536.0));
                    });
                    ui.add(egui::Slider::new(&mut f.opacite, 0.05..=1.0).text("opacité"));
                }
                ui.separator();
            }
        });
        self.fonds.dialogue = ouvert;
        if let Some(k) = charger {
            if let Some(p) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                self.charger_fond(ctx, k, p);
            }
        }
    }

    /// L'image `p` en fond des vues de type `k` (0 dessus, 1 face, 2 côté) : une unité par pixel, centrée sur l'origine.
    pub(super) fn charger_fond(&mut self, ctx: &egui::Context, k: usize, p: PathBuf) {
        match image::open(&p) {
            Ok(img) => {
                let img = img.to_rgba8();
                let taille = [img.width() as usize, img.height() as usize];
                let c = egui::ColorImage::from_rgba_unmultiplied(taille, img.as_raw());
                let texture = Some(ctx.load_texture(format!("fond_{k}"), c, egui::TextureOptions::LINEAR));
                self.fonds.fonds[k] = Some(Fond { chemin: p, texture, taille, centre: [0.0, 0.0], largeur: taille[0] as f32, opacite: 0.4 });
            }
            Err(e) => self.erreur = Some(format!("{} : {e}", p.display())),
        }
    }
}
