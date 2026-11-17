//! L'outil Semer (« Sprinkle » de Hammer++, Maj+F) : glisser dans la vue 3D sème des entités du type choisi sur la
//! grille du type, dans le cercle du pinceau (Ctrl, ou le mode Retirer : enlève celles du type). Tout un geste fait
//! une seule étape. Les types sont ceux de Hammer++ (`hammerplusplus/scripts/sprinkle/*.txt`), plus le modèle choisi
//! dans le navigateur de modèles.
use glam::DVec3;
use hm_doc::Commande;
use hm_edit::semis::{self, Pinceau, TypeSemis};
use hm_render::lignes::Sommet;

use super::Editeur;

pub struct EtatSemis {
    types: Option<Vec<TypeSemis>>,
    choisi: usize,
    pinceau: Pinceau,
    retirer: bool,
    /// Le point de surface sous le curseur (cercle d'aperçu), et le dernier point semé du geste en cours.
    apercu: Option<DVec3>,
    dernier: Option<DVec3>,
    graine: u64,
}

impl Default for EtatSemis {
    fn default() -> EtatSemis {
        EtatSemis { types: None, choisi: 0, pinceau: Pinceau { rayon: 256.0, lacet_hasard: true, ecart: true, aligner: false }, retirer: false, apercu: None, dernier: None, graine: 1 }
    }
}

fn types_hammerpp() -> Vec<TypeSemis> {
    let Some(g) = hm_assets::jeu::garrysmod() else { return Vec::new() };
    let d = g.join("bin/win64/hammerplusplus/scripts/sprinkle");
    let mut v = Vec::new();
    for f in std::fs::read_dir(d).into_iter().flatten().flatten() {
        if f.path().extension().map_or(false, |e| e.eq_ignore_ascii_case("txt")) {
            v.extend(semis::lire_types(&std::fs::read_to_string(f.path()).unwrap_or_default()));
        }
    }
    v
}

impl Editeur {
    /// Les types : le modèle choisi dans le navigateur de modèles (s'il y en a un), puis ceux de Hammer++.
    fn types_semis(&mut self) -> Vec<TypeSemis> {
        if self.semis.types.is_none() {
            self.semis.types = Some(types_hammerpp());
        }
        let mut v = Vec::new();
        if let Some((m, ..)) = &self.modele_entite {
            let g = self.grille as f64;
            v.push(TypeSemis { nom: tr!("modèle choisi ({})", "chosen model ({})", m.rsplit('/').next().unwrap_or(m)), base: vec![("classname".into(), "prop_static".into())], grille: [g, g], cas: vec![(1.0, vec![("model".into(), m.clone())])] });
        }
        v.extend(self.semis.types.clone().unwrap_or_default());
        v
    }

    pub(super) fn reglages_semis(&mut self, ui: &mut egui::Ui) {
        let types = self.types_semis();
        if types.is_empty() {
            ui.weak(crate::langue::t("aucun type : choisir un modèle dans l'onglet Modèles, ou des types dans hammerplusplus/scripts/sprinkle"));
            return;
        }
        let s = &mut self.semis;
        s.choisi = s.choisi.min(types.len() - 1);
        ui.label(crate::langue::t("Type"));
        egui::ComboBox::from_id_salt("type_semis").width(220.0).selected_text(&types[s.choisi].nom).show_ui(ui, |ui| {
            for (k, t) in types.iter().enumerate() {
                ui.selectable_value(&mut s.choisi, k, &t.nom);
            }
        });
        ui.label(tr!("grille {} × {}", "grid {} × {}", types[s.choisi].grille[0], types[s.choisi].grille[1]));
        ui.label(crate::langue::t("rayon"));
        ui.add(egui::DragValue::new(&mut s.pinceau.rayon).range(16.0..=4096.0).speed(4.0));
        ui.checkbox(&mut s.pinceau.lacet_hasard, crate::langue::t("lacet au hasard"));
        ui.checkbox(&mut s.pinceau.ecart, crate::langue::t("écart dans la case"));
        ui.checkbox(&mut s.pinceau.aligner, crate::langue::t("aligné sur la surface"));
        ui.radio_value(&mut s.retirer, false, crate::langue::t("semer"));
        ui.radio_value(&mut s.retirer, true, crate::langue::t("retirer"));
        ui.label(egui::RichText::new(crate::langue::t("3D : glisser ; Ctrl inverse")).small().weak());
    }

    pub(super) fn gestes_semis(&mut self, i: usize, rep: &egui::Response, ctx: &egui::Context) {
        let point = rep.hover_pos().or(rep.interact_pointer_pos()).and_then(|p| self.impact_3d(i, p)).map(|imp| imp.point);
        self.semis.apercu = point;
        let types = self.types_semis();
        let (Some(c), Some(t)) = (point, types.get(self.semis.choisi)) else { return };
        let actif = rep.dragged_by(egui::PointerButton::Primary) || rep.drag_started_by(egui::PointerButton::Primary) || rep.clicked();
        if actif {
            // un nouveau passage seulement après un demi-pas de grille
            let pas = t.grille[0].min(t.grille[1]) * 0.5;
            if self.semis.dernier.map_or(true, |d| (d - c).length() >= pas) {
                self.semis.dernier = Some(c);
                self.semis.graine = self.semis.graine.wrapping_add(0x9E37_79B9);
                let retirer = self.semis.retirer ^ ctx.input(|x| x.modifiers.command);
                let commande = if retirer {
                    semis::retirer(self.session.doc(), t, c, self.semis.pinceau.rayon)
                } else {
                    let v = semis::semer(self.session.doc(), t, c, self.semis.pinceau, self.semis.graine);
                    (!v.is_empty()).then(|| Commande::Lot { nom: tr!("semer « {} »", "sprinkle \"{}\"", t.nom), commandes: v })
                };
                if let Some(cmd) = commande {
                    // semer ne change pas la sélection
                    let sel = self.session.selection.clone();
                    let r = self.session.executer_continu(cmd, "semis");
                    self.session.selection = sel;
                    match r {
                        Ok(_) => self.statut = tr!("{} : {} entité(s)", "{}: {} entity(ies)", if retirer { crate::langue::t("retiré") } else { crate::langue::t("semé") }, self.session.doc().nombre_objets().1),
                        Err(e) => self.erreur = Some(tr!("semis : {e}", "sprinkle: {e}")),
                    }
                }
            }
        }
        if rep.drag_stopped() || rep.clicked() {
            // le geste est fini : le prochain fera une autre étape
            self.semis.dernier = None;
            self.session.couper_continu();
        }
    }

    /// Le cercle du pinceau, posé sur la surface sous le curseur.
    pub(super) fn apercu_semis(&self) -> Vec<Sommet> {
        let Some(c) = self.semis.apercu.filter(|_| self.outil == super::Outil::Semer) else { return Vec::new() };
        let r = self.semis.pinceau.rayon;
        let couleur = if self.semis.retirer { [255, 90, 60, 255] } else { [120, 255, 120, 255] };
        let mut v = Vec::new();
        for k in 0..48 {
            for j in [k, k + 1] {
                let a = j as f64 / 48.0 * std::f64::consts::TAU;
                v.push(Sommet { pos: (c + DVec3::new(a.cos() * r, a.sin() * r, 2.0)).as_vec3().to_array(), couleur });
            }
        }
        v
    }
}
