//! La barre d'objets de Hammer, en haut du panneau de droite : mode de sélection (groupes, objets, solides), texture
//! courante (vignette, récentes, taille, Parcourir… qui ouvre la fenêtre du navigateur de textures, Remplacer…),
//! déplacer la sélection vers le monde ou vers une entité, catégories et objets (primitives de l'outil bloc ou
//! préfabriqués) et créer un préfabriqué. Et ce que la fenêtre du navigateur de textures demande (marquer, remplacer,
//! montrer le VMT, relire).
use std::collections::HashSet;

use hm_doc::Ref;

use super::brosses_ui::FormeBloc;
use super::{Editeur, Outil};
use crate::textures::Choix;

#[derive(Default)]
pub struct EtatBarreObjets {
    /// 0 primitives, 1 préfabriqués.
    categorie: u8,
    /// Le VMT montré (nom, texte).
    source: Option<(String, String)>,
    /// Matériaux de la map, pour une version du document.
    employes: Option<(u64, HashSet<String>)>,
}

impl Editeur {
    pub(super) fn traiter_choix_texture(&mut self, c: Choix) {
        match c {
            Choix::Choisir(m) => {
                self.materiau = m.clone();
                self.navigateur.noter_recent(&m);
            }
            Choix::Appliquer(m) => {
                self.materiau = m.clone();
                self.navigateur.noter_recent(&m);
                self.appliquer_materiau();
            }
            Choix::Marquer(m) => {
                // les objets dont une face l'emploie, comme on les sélectionnerait d'un clic
                let doc = self.session.doc();
                let mut sel: Vec<Ref> = Vec::new();
                for (proprio, s) in doc.tous_les_solides() {
                    if s.faces.iter().any(|f| f.materiau.eq_ignore_ascii_case(&m)) {
                        let objet = match proprio {
                            hm_doc::Proprio::Monde => Ref::Solide(s.id),
                            hm_doc::Proprio::Entite(e) => Ref::Entite(e),
                        };
                        let r = self.a_selectionner(crate::scene::Designe { objet, solide: Some(s.id) });
                        if !sel.contains(&r) {
                            sel.push(r);
                        }
                    }
                }
                self.statut = tr!("{} objet(s) emploient {m}", "{} object(s) use {m}", sel.len());
                self.session.selection = sel;
            }
            Choix::Remplacer(m) => self.remplacer = Some((m, self.materiau.clone(), false)),
            Choix::Source(m) => {
                let texte = self.vfs.read(&format!("materials/{m}.vmt")).map(|f| String::from_utf8_lossy(&f.bytes).into_owned()).unwrap_or_else(|| crate::langue::t("(VMT introuvable dans les fichiers du jeu)").into());
                self.barre_objets.source = Some((m, texte));
            }
            Choix::Recharger => {
                self.navigateur.oublier();
                if let Some(g) = &mut self.gfx {
                    g.bib = hm_render::vivant::Bibliotheque::nouvelle(&g.r);
                    g.bib.outils_reels = true;
                    g.monde = None;
                }
                self.version_geo = None;
                self.statut = crate::langue::t("matériaux relus").into();
            }
        }
    }

    /// La fenêtre du navigateur de textures et celle du VMT montré.
    pub(super) fn fenetres_textures(&mut self, ctx: &egui::Context) {
        if self.navigateur.fenetre.ouverte {
            let v = self.session.version();
            if self.barre_objets.employes.as_ref().map_or(true, |(x, _)| *x != v) {
                let e = self.session.doc().tous_les_solides().flat_map(|(_, s)| s.faces.iter().map(|f| f.materiau.to_ascii_lowercase())).collect();
                self.barre_objets.employes = Some((v, e));
            }
            let (courant, rs, gi) = (self.materiau.clone(), self.rs.clone(), self.gameinfo.clone());
            let employes = self.barre_objets.employes.as_ref().map(|(_, e)| e.clone()).unwrap_or_default();
            if let Some(c) = self.navigateur.ui_fenetre(ctx, &self.vfs, &gi, rs.as_ref(), &courant, &employes) {
                self.traiter_choix_texture(c);
            }
        }
        if let Some((nom, texte)) = &self.barre_objets.source {
            let mut ouvert = true;
            super::barres::fenetre(ctx, crate::langue::t("Source du matériau")).open(&mut ouvert).default_size([460.0, 320.0]).show(ctx, |ui| {
                ui.strong(format!("materials/{nom}.vmt"));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(texte).monospace()).selectable(true));
                });
            });
            if !ouvert {
                self.barre_objets.source = None;
            }
        }
    }

    pub(super) fn barre_objets(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(crate::langue::t("Barre d'objets")).default_open(true).show(ui, |ui| {
            // mode de sélection
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Sélection"));
                let mode = if self.selection_solides { 2 } else if self.ignorer_groupes { 1 } else { 0 };
                for (k, n, aide) in [(0, crate::langue::t("Groupes"), crate::langue::t("un clic prend tout le groupe")), (1, crate::langue::t("Objets"), crate::langue::t("groupes ignorés : un solide, ou une entité à brosses entière")), (2, crate::langue::t("Solides"), crate::langue::t("chaque solide seul, même dans une entité à brosses"))] {
                    if ui.selectable_label(mode == k, n).on_hover_text(aide).clicked() {
                        self.selection_solides = k == 2;
                        self.ignorer_groupes = k >= 1;
                    }
                }
            });
            // texture courante
            ui.horizontal(|ui| {
                let (rect, rep) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::click());
                let m = self.materiau.to_ascii_lowercase();
                let v = self.rs.clone().and_then(|rs| self.navigateur.vignette(&m, &self.vfs, &rs));
                match v {
                    Some((id, _)) => {
                        ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                    }
                }
                if rep.on_hover_text(crate::langue::t("ouvrir le navigateur de textures")).clicked() {
                    self.navigateur.fenetre.ouverte = true;
                }
                ui.vertical(|ui| {
                    egui::ComboBox::from_id_salt("texture_courante").width(ui.available_width().min(240.0)).selected_text(&self.materiau).show_ui(ui, |ui| {
                        if self.navigateur.recents.is_empty() {
                            ui.weak(crate::langue::t("pas encore de texture récente"));
                        }
                        for r in self.navigateur.recents.clone() {
                            if ui.selectable_label(r.eq_ignore_ascii_case(&self.materiau), &r).clicked() {
                                self.materiau = r.clone();
                                self.navigateur.noter_recent(&r);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if let Some((_, t)) = v {
                            ui.label(format!("{}×{}", t[0], t[1]));
                        }
                        if ui.button(crate::langue::t("Parcourir…")).clicked() {
                            self.navigateur.fenetre.ouverte = true;
                        }
                        if ui.button(crate::langue::t("Remplacer…")).clicked() {
                            self.remplacer = Some((self.materiau.clone(), self.materiau.clone(), false));
                        }
                    });
                });
            });
            // déplacer la sélection
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Déplacer la sélection"));
                let actif = !self.session.selection.is_empty();
                if ui.add_enabled(actif, egui::Button::new(crate::langue::t("vers le monde"))).on_hover_text(self.rc.texte("Edit_To_World")).clicked() {
                    self.declenchee = Some("Edit_To_World");
                }
                if ui.add_enabled(actif, egui::Button::new(crate::langue::t("vers une entité"))).on_hover_text(self.rc.texte("Edit_To_Entity")).clicked() {
                    self.declenchee = Some("Edit_To_Entity");
                }
            });
            // catégories et objets
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Catégorie"));
                egui::ComboBox::from_id_salt("categorie_objets").width(110.0).selected_text([crate::langue::t("Primitives"), crate::langue::t("Préfabriqués")][self.barre_objets.categorie as usize]).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.barre_objets.categorie, 0, crate::langue::t("Primitives"));
                    ui.selectable_value(&mut self.barre_objets.categorie, 1, crate::langue::t("Préfabriqués"));
                });
            });
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Objet"));
                if self.barre_objets.categorie == 0 {
                    let mut forme = self.brosses.forme;
                    egui::ComboBox::from_id_salt("objet_primitive").width(110.0).selected_text(forme.nom()).show_ui(ui, |ui| {
                        for f in FormeBloc::TOUTES {
                            ui.selectable_value(&mut forme, f, f.nom());
                        }
                    });
                    if forme != self.brosses.forme {
                        // comme Hammer : choisir une primitive prend l'outil bloc
                        self.brosses.forme = forme;
                        self.outil = Outil::Bloc;
                    }
                    match forme {
                        FormeBloc::Cylindre | FormeBloc::Pointe | FormeBloc::Sphere => {
                            ui.add(egui::DragValue::new(&mut self.brosses.cotes).range(3..=64)).on_hover_text(crate::langue::t("côtés"));
                        }
                        FormeBloc::Arche => {
                            ui.add(egui::DragValue::new(&mut self.brosses.arche.cotes).range(3..=100)).on_hover_text(crate::langue::t("pièces"));
                        }
                        FormeBloc::Escalier => {
                            ui.add(egui::DragValue::new(&mut self.brosses.marches).range(1..=256)).on_hover_text(crate::langue::t("marches"));
                        }
                        _ => {}
                    }
                } else {
                    let liste = self.liste_prefabs();
                    let nom = |p: &std::path::PathBuf| p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    let choisi = self.prefabs.choisi.as_ref().map(nom).unwrap_or_else(|| crate::langue::t("(aucun)").into());
                    egui::ComboBox::from_id_salt("objet_prefab").width(150.0).selected_text(choisi).show_ui(ui, |ui| {
                        for p in &liste {
                            if ui.selectable_label(self.prefabs.choisi.as_ref() == Some(p), nom(p)).on_hover_text(p.display().to_string()).clicked() {
                                self.prefabs.choisi = Some(p.clone());
                            }
                        }
                    });
                    if let Some(p) = self.prefabs.choisi.clone() {
                        if ui.button(crate::langue::t("Insérer")).on_hover_text(crate::langue::t("au point que vise la vue 3D")).clicked() {
                            self.inserer_prefab(&p);
                        }
                    }
                }
            });
            if ui.add_enabled(!self.session.selection.is_empty(), egui::Button::new(crate::langue::t("Créer un préfabriqué"))).on_hover_text(self.rc.texte("Tools_Create_Prefab")).clicked() {
                self.enregistrer_prefab();
            }
            ui.separator();
            self.visgroups_compacts(ui);
        });
        ui.separator();
    }
}
