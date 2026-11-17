//! La fenêtre Propper++ (Outils, Ctrl+Alt+P) : les brosses sélectionnées deviennent un modèle compilé par studiomdl
//! (`hm_edit::propper`), puis, si on le demande, un `prop_static` prend leur place en une étape. La compilation
//! tourne à côté : la fenêtre reste utilisable. Les sources (SMD, QC) vont à côté de la map, dans `modelsrc/<nom>/`.
use std::path::PathBuf;
use std::sync::mpsc;

use hm_doc::{Commande, Ref};
use hm_edit::propper::{self, OptionsPropper};

use super::Editeur;

pub struct EtatPropper {
    pub ouvert: bool,
    options: OptionsPropper,
    remplacer: bool,
    /// Compilation en cours : (résultat, objets remplacés, origine du prop).
    en_cours: Option<(mpsc::Receiver<Result<String, String>>, Vec<Ref>, [f64; 3])>,
    journal: Vec<String>,
    /// Modèles déjà compilés ici : les recompiler demande de recharger les modèles de la vue.
    compiles: Vec<String>,
}

impl Default for EtatPropper {
    fn default() -> EtatPropper {
        EtatPropper { ouvert: false, options: OptionsPropper { modele: String::new(), surfaceprop: "concrete".into(), masse: 500.0, collision: true }, remplacer: true, en_cours: None, journal: Vec::new(), compiles: Vec::new() }
    }
}

impl Editeur {
    pub(super) fn ouvrir_propper(&mut self) {
        if self.propper.options.modele.is_empty() {
            let carte = self.nom_compile.to_lowercase().replace(' ', "_");
            self.propper.options.modele = format!("hammer_master/{}_propper", if carte.is_empty() { "sans_titre".into() } else { carte });
        }
        self.propper.ouvert = true;
    }

    fn lancer_propper(&mut self) {
        let sel = self.session.selection.clone();
        let p = match propper::preparer(self.session.doc(), &sel, &self.vfs, &self.propper.options) {
            Ok(p) => p,
            Err(e) => {
                self.propper.journal = vec![format!("refusé : {e}")];
                return;
            }
        };
        let jeu = self.gameinfo.parent().map(PathBuf::from).unwrap_or_default();
        let sources = self.session.chemin().and_then(|c| c.parent()).map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("modelsrc").join(&p.nom);
        self.propper.journal = vec![format!("{} triangles, {} pièce(s) de collision, {} face(s) enfouie(s) retirée(s)", p.triangles, p.pieces, p.faces_enfouies), format!("sources : {}", sources.display()), "compilation…".into()];
        let (tx, rx) = mpsc::channel();
        let origine = p.origine.to_array();
        // seulement ce qui est passé dans le modèle : pas une lumière ni un trigger sélectionnés avec les brosses
        let a_remplacer = p.utilises.clone();
        std::thread::spawn(move || {
            let r = propper::ecrire(&p, &sources, &jeu).and_then(|qc| propper::compiler(&qc, &jeu));
            let _ = tx.send(r);
        });
        self.propper.en_cours = Some((rx, a_remplacer, origine));
    }

    pub(super) fn fenetre_propper(&mut self, ctx: &egui::Context) {
        // fin de compilation
        if let Some((rx, sel, origine)) = &self.propper.en_cours {
            match rx.try_recv() {
                Ok(r) => {
                    let (sel, origine) = (sel.clone(), *origine);
                    self.propper.en_cours = None;
                    let modele = format!("models/{}.mdl", self.propper.options.modele.trim().trim_end_matches(".mdl"));
                    match r {
                        Ok(journal) => {
                            self.propper.journal.extend(journal.lines().filter(|l| l.contains("convex") || l.contains("WARNING")).map(String::from));
                            self.propper.journal.push(format!("compilé : {modele}"));
                            // déjà affiché : les modèles de la vue sont rechargés
                            if self.propper.compiles.contains(&modele) {
                                if let Some(g) = &mut self.gfx {
                                    g.bib = hm_render::vivant::Bibliotheque::nouvelle(&g.r);
                                    g.bib.outils_reels = true;
                                    g.monde = None;
                                }
                                self.infos_modeles.clear();
                            }
                            self.propper.compiles.push(modele.clone());
                            self.version_geo = None;
                            if self.propper.remplacer {
                                let cles = vec![("model".to_string(), modele.clone())];
                                self.executer(Commande::Lot { nom: format!("propper : {modele}"), commandes: vec![Commande::Supprimer { objets: sel }, Commande::CreerEntite { classe: "prop_static".into(), origine, cles }] });
                            } else {
                                self.statut = format!("compilé : {modele}");
                            }
                        }
                        Err(e) => self.propper.journal.extend(e.lines().map(String::from)),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(mpsc::TryRecvError::Disconnected) => self.propper.en_cours = None,
            }
        }
        if !self.propper.ouvert {
            return;
        }
        let mut ouvert = true;
        let mut lancer = false;
        let n = self.session.selection.len();
        let occupe = self.propper.en_cours.is_some();
        let s = &mut self.propper;
        super::barres::fenetre(ctx, "Propper++ : la sélection en modèle").resizable(false).open(&mut ouvert).show(ctx, |ui| {
            egui::Grid::new("propper").num_columns(2).show(ui, |ui| {
                ui.label("Modèle (sous models/)");
                ui.add(egui::TextEdit::singleline(&mut s.options.modele).desired_width(260.0));
                ui.end_row();
                ui.label("Matière (surfaceprop)");
                egui::ComboBox::from_id_salt("surfaceprop").selected_text(&s.options.surfaceprop).show_ui(ui, |ui| {
                    for m in ["concrete", "brick", "metal", "solidmetal", "wood", "wood_solid", "plastic", "glass", "tile", "rock", "dirt", "default"] {
                        ui.selectable_value(&mut s.options.surfaceprop, m.to_string(), m);
                    }
                });
                ui.end_row();
                ui.label("Masse");
                ui.add(egui::DragValue::new(&mut s.options.masse).range(1.0..=50000.0).speed(10.0));
                ui.end_row();
            });
            ui.checkbox(&mut s.options.collision, "Collision (une pièce convexe par brosse)");
            ui.checkbox(&mut s.remplacer, "Remplacer les brosses par le prop_static");
            ui.weak("les faces en texture d'outil (nodraw, clip…) ne se dessinent pas ; leurs brosses comptent pour la collision");
            ui.weak("le modèle et ses matériaux vont dans le dossier du jeu (models/, materials/models/) ; studiomdl 32 bits");
            if ui.add_enabled(n > 0 && !occupe, egui::Button::new(if occupe { "compilation…" } else { "Construire et compiler" })).clicked() {
                lancer = true;
            }
            if !s.journal.is_empty() {
                ui.separator();
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    for l in &s.journal {
                        ui.label(egui::RichText::new(l).monospace().size(11.0));
                    }
                });
            }
        });
        self.propper.ouvert = ouvert;
        if lancer {
            self.lancer_propper();
        }
    }
}
