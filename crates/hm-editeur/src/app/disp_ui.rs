//! Displacements dans la fenêtre : créer, détruire, coudre depuis l'outil texture (faces choisies) ; outil Sculpter
//! (dans la vue 3D : glisser le bouton gauche applique le pinceau, Ctrl l'inverse ; le droit reste à la caméra ; un glisser = une étape
//! d'historique). Le pinceau se voit en cercle sous le curseur ; en mode alpha, l'alpha de chaque sommet se voit en
//! croix plus ou moins claires.
use glam::{DVec3, Vec3};
use hm_doc::disp::{Disp, Pinceau};
use hm_doc::Commande;
use hm_render::lignes::Sommet;

use super::{Editeur, Outil};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeSculpture {
    Relever,
    Lisser,
    Alpha,
}

pub struct ReglagesSculpture {
    pub mode: ModeSculpture,
    pub rayon: f64,
    /// Hauteur d'un coup (relever) ; un glisser en donne un par image.
    pub hauteur: f64,
    pub force: f64,
    pub alpha: f64,
    pub puissance: u32,
    /// Point visé sous le curseur (pour le cercle) : (point, normale).
    pub vise: Option<(DVec3, DVec3)>,
}

impl Default for ReglagesSculpture {
    fn default() -> Self {
        ReglagesSculpture { mode: ModeSculpture::Relever, rayon: 128.0, hauteur: 4.0, force: 0.3, alpha: 255.0, puissance: 3, vise: None }
    }
}

impl Editeur {
    /// La section Displacement de l'outil texture (sur les faces choisies).
    pub(super) fn section_disp(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Displacement");
            egui::ComboBox::from_id_salt("puissance_disp").width(40.0).selected_text(self.sculpture.puissance.to_string()).show_ui(ui, |ui| {
                for p in 2..=4 {
                    ui.selectable_value(&mut self.sculpture.puissance, p, format!("{p} ({0}×{0})", (1 << p) + 1));
                }
            });
            let n = self.faces.len();
            if ui.add_enabled(n > 0, egui::Button::new("Créer")).on_hover_text("faces à quatre côtés seulement").clicked() {
                let faces = self.faces.clone();
                self.executer(Commande::CreerDisp { faces, puissance: self.sculpture.puissance });
            }
            let avec: Vec<hm_doc::RefFace> = self.faces.iter().copied().filter(|rf| self.session.doc().solide(rf.solide).and_then(|s| s.faces.iter().find(|f| f.id == rf.face)).map_or(false, |f| f.disp.is_some())).collect();
            if ui.add_enabled(!avec.is_empty(), egui::Button::new("Détruire")).clicked() {
                self.executer(Commande::DetruireDisp { faces: avec.clone() });
            }
            if ui.add_enabled(avec.len() >= 2, egui::Button::new("Coudre")).on_hover_text("les bords communs prennent leur position moyenne").clicked() {
                self.executer(Commande::CoudreDisp { faces: avec });
            }
        });
    }

    /// Les réglages du pinceau, sous les outils à gauche.
    pub(super) fn reglages_sculpture(&mut self, ui: &mut egui::Ui) {
        let r = &mut self.sculpture;
        for (m, n) in [(ModeSculpture::Relever, "relever"), (ModeSculpture::Lisser, "lisser"), (ModeSculpture::Alpha, "alpha")] {
            ui.radio_value(&mut r.mode, m, n);
        }
        ui.label(egui::RichText::new("rayon").small());
        ui.add(egui::DragValue::new(&mut r.rayon).range(8.0..=2048.0).speed(2.0));
        match r.mode {
            ModeSculpture::Relever => {
                ui.label(egui::RichText::new("hauteur/coup").small());
                ui.add(egui::DragValue::new(&mut r.hauteur).range(0.1..=128.0).speed(0.2));
            }
            ModeSculpture::Lisser => {
                ui.label(egui::RichText::new("force").small());
                ui.add(egui::DragValue::new(&mut r.force).range(0.01..=1.0).speed(0.01));
            }
            ModeSculpture::Alpha => {
                ui.label(egui::RichText::new("alpha visé").small());
                ui.add(egui::DragValue::new(&mut r.alpha).range(0.0..=255.0).speed(1.0));
                ui.label(egui::RichText::new("force").small());
                ui.add(egui::DragValue::new(&mut r.force).range(0.01..=1.0).speed(0.01));
            }
        }
        ui.label(egui::RichText::new("3D : gauche applique, Ctrl+gauche inverse").small().weak());
    }

    /// Dans la vue 3D avec l'outil Sculpter : vise sous le curseur, peint en glissant.
    pub(super) fn gestes_sculpture(&mut self, i: usize, rep: &egui::Response, ctrl: bool) {
        let Some(p) = rep.hover_pos().or(rep.interact_pointer_pos()) else {
            self.sculpture.vise = None;
            return;
        };
        let (o, d) = self.rayon_vue(i, p);
        let touche = hm_doc::disp::rayon_disp(self.session.doc(), o, d);
        self.sculpture.vise = touche.map(|(p, n, _)| (p, n));
        let applique = rep.dragged_by(egui::PointerButton::Primary) || rep.clicked();
        let (gauche, droit) = (applique && !ctrl, applique && ctrl);
        if rep.drag_started() || rep.clicked() {
            self.session.couper_continu();
        }
        let Some((centre, _, _)) = touche else { return };
        if !(gauche || droit) {
            return;
        }
        let r = &self.sculpture;
        let signe = if droit { -1.0 } else { 1.0 };
        let pinceau = match r.mode {
            ModeSculpture::Relever => Pinceau::Relever { hauteur: r.hauteur * signe },
            ModeSculpture::Lisser => Pinceau::Lisser { force: r.force },
            ModeSculpture::Alpha => Pinceau::Alpha { valeur: if droit { 0.0 } else { r.alpha }, force: r.force },
        };
        let c = Commande::PeindreDisp { faces: None, centre: centre.to_array(), rayon: r.rayon, pinceau };
        match self.session.executer_continu(c, "sculpter") {
            Ok(r) => self.statut = format!("sculpter : {}", r.message),
            Err(e) => self.erreur = Some(format!("sculpture refusée : {e}")),
        }
        if rep.drag_stopped() {
            self.session.couper_continu();
        }
    }

    /// Le cercle du pinceau et, en mode alpha, l'alpha des sommets.
    pub(super) fn apercu_sculpture(&self) -> Vec<Sommet> {
        let mut out = Vec::new();
        if self.outil != Outil::Sculpter {
            return out;
        }
        if let Some((p, n)) = self.sculpture.vise {
            let a = if n.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
            let u = n.cross(a).normalize();
            let v = n.cross(u);
            let r = self.sculpture.rayon;
            for k in 0..48 {
                let (t0, t1) = (std::f64::consts::TAU * k as f64 / 48.0, std::f64::consts::TAU * (k + 1) as f64 / 48.0);
                for t in [t0, t1] {
                    out.push(Sommet { pos: (p + n * 1.0 + (u * t.cos() + v * t.sin()) * r).as_vec3().to_array(), couleur: [255, 220, 60, 255] });
                }
            }
        }
        if self.sculpture.mode == ModeSculpture::Alpha {
            for (_, s) in self.session.doc().tous_les_solides() {
                let polys = s.polygones();
                for (f, poly) in s.faces.iter().zip(&polys) {
                    let Some(d) = f.disp.as_ref().and_then(Disp::lire) else { continue };
                    let Some((_, pts)) = d.points(poly, f.normale()) else { continue };
                    for (p, a) in pts.iter().zip(&d.alphas) {
                        let c = (40.0 + a.clamp(0.0, 255.0) * 0.84) as u8;
                        let p = p.as_vec3() + f.normale().as_vec3() * 1.0;
                        for e in [Vec3::X, Vec3::Y] {
                            out.push(Sommet { pos: (p - e * 4.0).to_array(), couleur: [c, c, 255 - c / 2, 255] });
                            out.push(Sommet { pos: (p + e * 4.0).to_array(), couleur: [c, c, 255 - c / 2, 255] });
                        }
                    }
                }
            }
        }
        out
    }
}
