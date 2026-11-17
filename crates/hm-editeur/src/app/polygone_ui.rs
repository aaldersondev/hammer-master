//! L'outil polygone (« Polygon tool » de Hammer++, Maj+N) : des clics dans une vue 2D posent les sommets d'un
//! contour (calés sur la grille), clic droit retire le dernier, Entrée crée le solide convexe qui l'enveloppe, extrudé
//! sur la profondeur de l'outil bloc ; Échap abandonne.
use egui::{Color32, Stroke};
use glam::Vec2;
use hm_doc::Commande;

use super::{Editeur, Outil};
use crate::gestes;
use crate::vues::Genre;

#[derive(Default)]
pub struct EtatPolygone {
    genre: Option<Genre>,
    points: Vec<Vec2>,
}

impl Editeur {
    pub(super) fn clic_polygone(&mut self, i: usize, rep: &egui::Response) {
        if self.outil != Outil::Polygone {
            return;
        }
        let v = &self.vues[i];
        let Some(p) = rep.interact_pointer_pos() else { return };
        if rep.secondary_clicked() {
            self.polygone.points.pop();
            return;
        }
        if !rep.clicked() {
            return;
        }
        // un contour se trace dans une seule sorte de vue
        if self.polygone.genre != Some(v.genre) {
            self.polygone = EtatPolygone { genre: Some(v.genre), points: Vec::new() };
        }
        let w = v.ecran_vers_monde(p);
        let q = Vec2::new(gestes::aimanter(w.x, self.grille, self.aimant), gestes::aimanter(w.y, self.grille, self.aimant));
        if self.polygone.points.last() != Some(&q) {
            self.polygone.points.push(q);
        }
        self.statut = format!("polygone : {} sommet(s) — Entrée pour créer, clic droit pour retirer le dernier", self.polygone.points.len());
    }

    pub(super) fn creer_polygone(&mut self) {
        let Some(g) = self.polygone.genre else { return };
        if self.polygone.points.len() < 3 {
            self.statut = "polygone : au moins trois sommets".into();
            return;
        }
        let (a, b, c) = g.axes();
        let (c0, c1) = (self.profondeur_bloc.0.min(self.profondeur_bloc.1), self.profondeur_bloc.0.max(self.profondeur_bloc.1).max(self.profondeur_bloc.0.min(self.profondeur_bloc.1) + 1.0));
        let mut points = Vec::new();
        for z in [c0, c1] {
            for p in &self.polygone.points {
                let mut q = [0.0f64; 3];
                q[a] = p.x as f64;
                q[b] = p.y as f64;
                q[c] = z as f64;
                points.push(q);
            }
        }
        self.executer(Commande::CreerEnveloppe { points, materiau: self.materiau.clone(), classe: None });
        self.polygone.points.clear();
    }

    pub(super) fn abandonner_polygone(&mut self) {
        self.polygone.points.clear();
    }

    /// Le contour en cours dans les vues de sa sorte.
    pub(super) fn dessiner_polygone(&self, i: usize, p: &egui::Painter) {
        let v = &self.vues[i];
        if self.outil != Outil::Polygone || self.polygone.genre != Some(v.genre) || self.polygone.points.is_empty() {
            return;
        }
        let e: Vec<egui::Pos2> = self.polygone.points.iter().map(|q| v.monde_vers_ecran(*q)).collect();
        let jaune = Color32::from_rgb(255, 220, 60);
        for k in 0..e.len() {
            let fin = e[(k + 1) % e.len()];
            let trait_ = if k + 1 == e.len() { Stroke::new(1.0, jaune.gamma_multiply(0.5)) } else { Stroke::new(1.5, jaune) };
            if e.len() > 1 {
                p.line_segment([e[k], fin], trait_);
            }
            p.circle_filled(e[k], 3.5, jaune);
        }
    }
}
