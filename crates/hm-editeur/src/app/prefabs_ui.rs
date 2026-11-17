//! Préfabriqués et instances dans la fenêtre.
//! - Onglet « Préfabs » : les VMF du dossier de préfabriqués de Hammer (`bin\Prefabs` de Garry's Mod, sous-dossiers
//!   = catégories ; wiki Valve, « Prefab »), sinon `%APPDATA%\HammerMaster\Prefabs` ; double clic : insérer au point
//!   que vise le centre de la vue 3D ; Ctrl+R : enregistrer la sélection (ramenée à l'origine) sous un nom.
//! - Propriétés d'un `func_instance` : le fichier trouvé ou non, « Ouvrir l'instance », « Relire les instances ».
use std::path::{Path, PathBuf};

use hm_doc::Commande;

use super::{Editeur, Onglet};
use crate::vues::Genre;

/// Les dossiers de préfabriqués : celui de Hammer (s'il existe), puis le nôtre.
pub fn dossiers_prefabs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(g) = hm_assets::jeu::garrysmod() {
        let p = g.join("bin").join("Prefabs");
        if p.is_dir() {
            v.push(p);
        }
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(a).join("HammerMaster").join("Prefabs"));
    }
    v
}

fn lister(d: &Path, out: &mut Vec<PathBuf>, prof: usize) {
    let Ok(l) = std::fs::read_dir(d) else { return };
    for e in l.flatten() {
        let p = e.path();
        if p.is_dir() && prof < 4 {
            lister(&p, out, prof + 1);
        } else if p.extension().map_or(false, |x| x.eq_ignore_ascii_case("vmf")) {
            out.push(p);
        }
    }
}

#[derive(Default)]
pub struct EtatPrefabs {
    pub liste: Option<Vec<PathBuf>>,
    pub filtre: String,
    pub choisi: Option<PathBuf>,
    pub nom: String,
}

impl Editeur {
    /// Le point que vise le centre de la vue 3D (surface touchée), sinon l'origine.
    fn point_vise_3d(&self) -> [f64; 3] {
        let Some(i) = self.vues.iter().position(|v| v.genre == Genre::Camera3D && v.rect.is_positive()) else { return [0.0; 3] };
        let (o, d) = self.rayon_vue(i, self.vues[i].rect.center());
        match hm_doc::geo::rayon(self.session.doc(), o, d, &|_| false) {
            Some(imp) => {
                let g = self.grille as f64;
                let a = |x: f64| if self.aimant { (x / g).round() * g } else { x };
                [a(imp.point.x), a(imp.point.y), imp.point.z]
            }
            None => [0.0; 3],
        }
    }

    pub(super) fn inserer_prefab(&mut self, p: &Path) {
        match std::fs::read(p) {
            Ok(b) => {
                let vmf = String::from_utf8_lossy(&b).into_owned();
                let origine = self.point_vise_3d();
                self.executer(Commande::InsererPrefab { vmf, origine });
            }
            Err(e) => self.erreur = Some(format!("{} : {e}", p.display())),
        }
    }

    /// Ctrl+R : la sélection devient un préfabriqué (nom tapé dans l'onglet, sinon « prefab_<n> »).
    pub(super) fn enregistrer_prefab(&mut self) {
        if self.session.selection.is_empty() {
            self.statut = crate::langue::t("préfabriqué : rien de sélectionné").into();
            self.onglet = Onglet::Prefabs;
            return;
        }
        let vmf = match hm_doc::presse::prefabriquer(self.session.doc(), &self.session.selection) {
            Ok(v) => v,
            Err(e) => {
                self.erreur = Some(tr!("préfabriqué : {e}", "prefab: {e}"));
                return;
            }
        };
        let nom: String = self.prefabs.nom.trim().chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
        let nom = if nom.is_empty() { format!("prefab_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() % 100000).unwrap_or(0)) } else { nom };
        // le premier dossier où l'on peut écrire
        for d in dossiers_prefabs() {
            if std::fs::create_dir_all(&d).is_err() {
                continue;
            }
            let p = d.join(format!("{nom}.vmf"));
            if std::fs::write(&p, &vmf).is_ok() {
                self.statut = tr!("préfabriqué enregistré : {}", "prefab saved: {}", p.display());
                self.prefabs.liste = None;
                self.prefabs.choisi = Some(p);
                self.onglet = Onglet::Prefabs;
                return;
            }
        }
        self.erreur = Some(crate::langue::t("aucun dossier de préfabriqués où écrire").into());
    }

    /// Les préfabriqués trouvés (lus une fois).
    pub(super) fn liste_prefabs(&mut self) -> Vec<PathBuf> {
        if self.prefabs.liste.is_none() {
            let mut v = Vec::new();
            for d in dossiers_prefabs() {
                lister(&d, &mut v, 0);
            }
            v.sort();
            self.prefabs.liste = Some(v);
        }
        self.prefabs.liste.clone().unwrap_or_default()
    }

    pub(super) fn panneau_prefabs(&mut self, ui: &mut egui::Ui) {
        self.liste_prefabs();
        ui.horizontal(|ui| {
            ui.label(crate::langue::t("Nom"));
            ui.add(egui::TextEdit::singleline(&mut self.prefabs.nom).desired_width(150.0).hint_text("escalier_bois"));
            if ui.add_enabled(!self.session.selection.is_empty(), egui::Button::new(tr!("Enregistrer la sélection ({})", "Save the selection ({})", self.rc.texte("Tools_Create_Prefab")))).clicked() {
                self.enregistrer_prefab();
            }
        });
        ui.label(egui::RichText::new(crate::langue::t("le préfabriqué est ramené au milieu de son dessous ; « &i » dans un nom devient un numéro à chaque insertion")).weak());
        ui.horizontal(|ui| {
            ui.label(crate::langue::t("Chercher"));
            ui.add(egui::TextEdit::singleline(&mut self.prefabs.filtre).desired_width(f32::INFINITY));
        });
        let dossiers = dossiers_prefabs();
        let f = self.prefabs.filtre.to_ascii_lowercase();
        let liste: Vec<PathBuf> = self.prefabs.liste.clone().unwrap_or_default().into_iter().filter(|p| p.to_string_lossy().to_ascii_lowercase().contains(&f)).collect();
        ui.horizontal(|ui| {
            ui.label(tr!("{} préfabriqué(s) · double clic : insérer au point visé par la vue 3D", "{} prefab(s) · double click: insert at the point the 3D view is aiming at", liste.len()));
            if ui.small_button(crate::langue::t("relire")).clicked() {
                self.prefabs.liste = None;
            }
        });
        let mut inserer = None;
        egui::ScrollArea::vertical().id_salt("prefabs").max_height((ui.available_height() - 260.0).max(120.0)).auto_shrink([false, true]).show(ui, |ui| {
            if liste.is_empty() {
                ui.label(egui::RichText::new(tr!("aucun : dossiers {}", "none: folders {}", dossiers.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", "))).weak());
            }
            for p in &liste {
                let court = dossiers.iter().find_map(|d| p.strip_prefix(d).ok()).map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
                let r = ui.selectable_label(self.prefabs.choisi.as_ref() == Some(p), court);
                if r.double_clicked() {
                    inserer = Some(p.clone());
                } else if r.clicked() {
                    self.prefabs.choisi = Some(p.clone());
                }
            }
        });
        if ui.add_enabled(self.prefabs.choisi.is_some(), egui::Button::new(crate::langue::t("Insérer"))).clicked() {
            inserer = self.prefabs.choisi.clone();
        }
        if let Some(p) = inserer {
            self.inserer_prefab(&p);
        }
        self.section_amenagements(ui);
    }

    /// Dans les propriétés d'un func_instance : le fichier, l'ouvrir, relire les instances.
    pub(super) fn section_instance(&mut self, ui: &mut egui::Ui, e: u64) {
        let Some(ent) = self.session.doc().entite(e) else { return };
        if !ent.classe().eq_ignore_ascii_case("func_instance") {
            return;
        }
        let fichier = ent.get("file").unwrap_or("").to_string();
        let chemin = self.session.chemin().and_then(|c| c.parent()).map(|d| d.join(&fichier));
        let existe = chemin.as_ref().map_or(false, |p| p.is_file() && !fichier.trim().is_empty());
        ui.horizontal(|ui| {
            ui.strong(crate::langue::t("Instance"));
            if fichier.trim().is_empty() {
                ui.weak(crate::langue::t("aucun fichier (clé file)"));
            } else if existe {
                ui.label(&fichier);
            } else {
                ui.colored_label(egui::Color32::from_rgb(255, 130, 100), tr!("{fichier} introuvable{}", "{fichier} not found{}", if self.session.chemin().is_none() { crate::langue::t(" (map pas encore enregistrée : chemin relatif à quoi ?)") } else { "" }));
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(existe, egui::Button::new(crate::langue::t("Ouvrir l'instance"))).clicked() {
                if self.session.modifiee() {
                    self.statut = crate::langue::t("enregistrer la map avant d'ouvrir l'instance").into();
                } else if let Some(p) = chemin.clone() {
                    self.ouvrir(&p);
                }
            }
            if ui.button(crate::langue::t("Relire les instances")).on_hover_text(crate::langue::t("après avoir modifié un fichier d'instance")).clicked() {
                self.instances.borrow_mut().clear();
                self.version_geo = None;
            }
        });
        ui.separator();
    }
}
