//! Navigateur de modèles : tous les `.mdl` du jeu, des addons montés et du projet, recherche instantanée, favoris,
//! récents ; le modèle choisi est montré en 3D, tournant, avec son skin et ses bodygroups (l'aperçu est dessiné par
//! la fenêtre, qui a la carte graphique).
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;

use hm_assets::model::Partie;
use hm_assets::Vfs;

/// Ce qu'on sait d'un modèle sans le dessiner.
#[derive(Clone, Debug)]
pub struct InfoModele {
    pub skins: usize,
    pub parties: Vec<Partie>,
    pub statique: bool,
    pub prop_data: bool,
    pub bornes: ([f32; 3], [f32; 3]),
    pub triangles: usize,
}

impl InfoModele {
    pub fn lire(vfs: &Vfs, nom: &str) -> Result<InfoModele, String> {
        let m = hm_assets::load_model(vfs, nom)?;
        Ok(InfoModele { skins: m.skins.len().max(1), parties: m.parties, statique: m.static_prop, prop_data: m.prop_data, bornes: m.bounds, triangles: m.triangles })
    }

    /// Les bodyparts qu'on peut choisir (plus d'un sous-modèle).
    pub fn choisissables(&self) -> impl Iterator<Item = (usize, &Partie)> {
        self.parties.iter().enumerate().filter(|(_, p)| p.sous_modeles.len() > 1)
    }
}

/// Ce que l'utilisateur a demandé dans le navigateur.
pub enum ActionModele {
    /// Le modèle (avec ce skin et cette valeur `body`) devient celui de l'outil entité.
    PourOutil(String, u32, u32),
    /// Il est donné aux entités sélectionnées.
    Appliquer(String, u32, u32),
}

pub struct NavModeles {
    tous: Option<Vec<String>>,
    reception: Option<mpsc::Receiver<Vec<String>>>,
    pub filtre: String,
    pub recents: Vec<String>,
    pub favoris: HashSet<String>,
    seulement_favoris: bool,
    /// Le modèle montré, son skin et son choix de sous-modèles.
    pub choisi: Option<String>,
    pub skin: u32,
    pub choix: Vec<usize>,
    pub info: Option<Result<InfoModele, String>>,
}

impl NavModeles {
    pub fn new() -> NavModeles {
        NavModeles { tous: None, reception: None, filtre: String::new(), recents: Vec::new(), favoris: HashSet::new(), seulement_favoris: false, choisi: None, skin: 0, choix: Vec::new(), info: None }
    }

    pub fn oublier(&mut self) {
        self.tous = None;
        self.reception = None;
    }

    fn lister(&mut self, gameinfo: PathBuf) {
        if self.tous.is_some() || self.reception.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut vfs = Vfs::new();
            let _ = vfs.add_gameinfo(&gameinfo);
            let mut l: Vec<String> = vfs.list_dans("models", ".mdl").into_iter().map(|(p, _)| p).collect();
            l.sort();
            l.dedup();
            let _ = tx.send(l);
        });
        self.reception = Some(rx);
    }

    pub fn noter_recent(&mut self, m: &str) {
        let m = m.to_ascii_lowercase();
        self.recents.retain(|x| *x != m);
        self.recents.insert(0, m);
        self.recents.truncate(24);
    }

    /// Montre un modèle : lit ses skins et bodyparts.
    pub fn montrer(&mut self, vfs: &Vfs, nom: &str, skin: u32, body: u32) {
        let info = InfoModele::lire(vfs, nom);
        self.choix = match &info {
            Ok(i) => hm_assets::model::bodygroups(&i.parties, body),
            Err(_) => Vec::new(),
        };
        self.skin = match &info {
            Ok(i) => skin.min(i.skins as u32 - 1),
            Err(_) => 0,
        };
        self.info = Some(info);
        self.choisi = Some(nom.to_ascii_lowercase());
    }

    /// La valeur `body` du choix montré.
    pub fn body(&self) -> u32 {
        match &self.info {
            Some(Ok(i)) => hm_assets::model::body_de(&i.parties, &self.choix),
            _ => 0,
        }
    }

    /// Ce que l'aperçu doit dessiner.
    pub fn apercu(&self) -> Option<(String, u32, u32)> {
        match (&self.choisi, &self.info) {
            (Some(m), Some(Ok(_))) => Some((m.clone(), self.skin, self.body())),
            _ => None,
        }
    }

    /// `apercu` : l'image tournante du modèle choisi ; `n_sel` : entités sélectionnées qui prennent un modèle.
    pub fn ui(&mut self, ui: &mut egui::Ui, vfs: &Vfs, gameinfo: &std::path::Path, apercu: Option<egui::TextureId>, n_sel: usize) -> Option<ActionModele> {
        self.lister(gameinfo.to_path_buf());
        if let Some(rx) = &self.reception {
            if let Ok(l) = rx.try_recv() {
                self.tous = Some(l);
                self.reception = None;
            }
        }
        let mut action = None;
        // ------------------------------------------------------------ aperçu et réglages du modèle choisi
        let cote = ui.available_width().min(300.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), cote * 0.75), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, egui::Color32::from_gray(28));
        match (apercu, &self.info) {
            (Some(id), Some(Ok(_))) => {
                let r = egui::Rect::from_center_size(rect.center(), egui::vec2(cote, cote * 0.75));
                ui.painter().image(id, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            (_, Some(Err(e))) => {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, tr!("illisible : {e}", "unreadable: {e}"), egui::FontId::proportional(12.0), egui::Color32::from_rgb(255, 140, 100));
            }
            _ => {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, crate::langue::t("cliquer un modèle de la liste"), egui::FontId::proportional(13.0), egui::Color32::GRAY);
            }
        }
        if let (Some(m), Some(Ok(info))) = (self.choisi.clone(), self.info.clone()) {
            ui.label(egui::RichText::new(&m).strong());
            let (lo, hi) = info.bornes;
            ui.label(tr!("{:.0} × {:.0} × {:.0} unités · {} triangles", "{:.0} × {:.0} × {:.0} units · {} triangles", hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2], info.triangles));
            if info.prop_data {
                ui.colored_label(egui::Color32::from_rgb(255, 170, 60), crate::langue::t("a un bloc prop_data : vbsp le retire d'un prop_static (prop_physics ou prop_dynamic)"));
            } else if !info.statique {
                ui.colored_label(egui::Color32::from_rgb(255, 170, 60), crate::langue::t("non compilé en $staticprop : pas en prop_static"));
            }
            ui.horizontal(|ui| {
                ui.label("skin");
                ui.add_enabled(info.skins > 1, egui::DragValue::new(&mut self.skin).range(0..=(info.skins as u32 - 1)));
                ui.weak(tr!("sur {}", "on {}", info.skins));
            });
            for (i, p) in info.choisissables() {
                ui.horizontal(|ui| {
                    ui.label(&p.nom);
                    let mut c = self.choix.get(i).copied().unwrap_or(0);
                    egui::ComboBox::from_id_salt(("bodypart", i)).selected_text(p.sous_modeles.get(c).cloned().unwrap_or_default()).width(170.0).show_ui(ui, |ui| {
                        for (k, s) in p.sous_modeles.iter().enumerate() {
                            ui.selectable_value(&mut c, k, if s.is_empty() { tr!("{k} (vide)", "{k} (empty)") } else { s.clone() });
                        }
                    });
                    if self.choix.len() <= i {
                        self.choix.resize(i + 1, 0);
                    }
                    self.choix[i] = c;
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(crate::langue::t("Pour l'outil entité")).on_hover_text(crate::langue::t("le prochain clic de l'outil entité pose ce modèle")).clicked() {
                    action = Some(ActionModele::PourOutil(m.clone(), self.skin, self.body()));
                    self.noter_recent(&m);
                }
                if ui.add_enabled(n_sel > 0, egui::Button::new(tr!("Appliquer à la sélection ({n_sel})", "Apply to selection ({n_sel})"))).clicked() {
                    action = Some(ActionModele::Appliquer(m.clone(), self.skin, self.body()));
                    self.noter_recent(&m);
                }
            });
        }
        ui.separator();
        // ------------------------------------------------------------ recherche et liste
        ui.horizontal(|ui| {
            ui.label(crate::langue::t("Chercher"));
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("chair, props_c17/, crate…").desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.seulement_favoris, crate::langue::t("favoris"));
            ui.label(crate::langue::t("récents :"));
            if self.recents.is_empty() {
                ui.weak(crate::langue::t("aucun"));
            }
        });
        let mut montrer = None;
        if !self.recents.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for r in self.recents.iter().take(6) {
                    let court = r.rsplit('/').next().unwrap_or(r).trim_end_matches(".mdl");
                    if ui.small_button(court).on_hover_text(r).clicked() {
                        montrer = Some(r.clone());
                    }
                }
            });
        }
        let Some(tous) = &self.tous else {
            ui.spinner();
            ui.label(crate::langue::t("liste des modèles en cours…"));
            ui.ctx().request_repaint();
            return action;
        };
        let mots: Vec<String> = self.filtre.to_ascii_lowercase().split_whitespace().map(String::from).collect();
        let liste: Vec<String> = tous.iter().filter(|m| !self.seulement_favoris || self.favoris.contains(*m)).filter(|m| mots.iter().all(|w| m.contains(w.as_str()))).cloned().collect();
        ui.label(tr!("{} modèle(s) · clic : voir · double clic : pour l'outil entité · clic droit : favori", "{} model(s) · click: view · double click: for the entity tool · right click: favorite", liste.len()));
        let haut = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        egui::ScrollArea::vertical().id_salt("liste_modeles").auto_shrink(false).show_rows(ui, haut, liste.len(), |ui, rangs| {
            for m in &liste[rangs] {
                let etoile = if self.favoris.contains(m) { "* " } else { "" };
                let court = m.strip_prefix("models/").unwrap_or(m);
                let rep = ui.selectable_label(self.choisi.as_deref() == Some(m.as_str()), format!("{etoile}{court}"));
                if rep.double_clicked() {
                    montrer = Some(m.clone());
                    action = Some(ActionModele::PourOutil(m.clone(), 0, 0));
                } else if rep.clicked() {
                    montrer = Some(m.clone());
                }
                if rep.secondary_clicked() && !self.favoris.remove(m) {
                    self.favoris.insert(m.clone());
                }
            }
        });
        if let Some(m) = montrer {
            self.montrer(vfs, &m, 0, 0);
            if let Some(ActionModele::PourOutil(a, ..)) = &action {
                self.noter_recent(a);
            }
        }
        action
    }
}
