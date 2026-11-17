//! Modèles d'aménagement réutilisables, dans l'onglet Préfabs : les modèles du projet (dossier `amenagements` à côté
//! de `hammer-master.toml`, sinon de la map ; format de `hm_model::amenagement`), et « Aménager la boîte de la
//! sélection » : le modèle s'adapte à la boîte, la porte sur le mur choisi ; poses et coffrages en une étape.
use std::path::PathBuf;

use hm_model::amenagement::{trouver, Statut};

use super::Editeur;

pub struct EtatAmenagements {
    choisi: Option<String>,
    /// Mur de la porte : `y-`, `y+`, `x-`, `x+`.
    porte: &'static str,
}

impl Default for EtatAmenagements {
    fn default() -> Self {
        EtatAmenagements { choisi: None, porte: "y-" }
    }
}

impl Editeur {
    fn dossier_amenagements(&self) -> Option<PathBuf> {
        self.session.chemin().and_then(|c| std::path::absolute(c).ok()).map(|c| hm_regles::semantique::dossier_amenagements(&c))
    }

    pub(super) fn section_amenagements(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.strong("Aménagements");
        let Some(dossier) = self.dossier_amenagements() else {
            ui.weak("enregistrer la map : les modèles se rangent dans le dossier « amenagements » du projet");
            return;
        };
        let mut noms: Vec<String> = std::fs::read_dir(&dossier).map(|l| l.flatten().filter_map(|e| e.path().file_stem().filter(|_| e.path().extension().map_or(false, |x| x == "toml")).map(|s| s.to_string_lossy().into_owned())).collect()).unwrap_or_default();
        noms.sort();
        if noms.is_empty() {
            ui.weak(format!("aucun modèle dans {}", dossier.display()));
            return;
        }
        egui::ScrollArea::vertical().id_salt("amenagements").max_height(140.0).show(ui, |ui| {
            for n in &noms {
                let statut = trouver(&dossier, n).map(|(_, s)| s);
                let texte = match &statut {
                    Ok(Statut::Valide) => n.clone(),
                    Ok(s) => format!("{n} ({})", s.nom()),
                    Err(_) => format!("{n} (illisible)"),
                };
                if ui.selectable_label(self.amenagements.choisi.as_deref() == Some(n), texte).clicked() {
                    self.amenagements.choisi = Some(n.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("porte sur le mur");
            for (p, nom) in [("y-", "sud (y−)"), ("y+", "nord (y+)"), ("x-", "ouest (x−)"), ("x+", "est (x+)")] {
                ui.radio_value(&mut self.amenagements.porte, p, nom);
            }
        });
        let pret = self.amenagements.choisi.is_some() && !self.session.selection.is_empty();
        if ui.add_enabled(pret, egui::Button::new("Aménager la boîte de la sélection")).on_hover_text("la boîte de la sélection est la pièce : le modèle s'y adapte, vu depuis la porte").clicked() {
            let n = self.amenagements.choisi.clone().unwrap();
            let r = trouver(&dossier, &n).map_err(|e| format!("{e:#}")).and_then(|(a, _)| {
                let (lo, hi) = hm_doc::boite_de(self.session.doc(), &self.session.selection).ok_or("rien de sélectionné")?;
                hm_edit::commande_amenagement_boite(&a, lo.as_vec3().to_array(), hi.as_vec3().to_array(), self.amenagements.porte, self.session.doc()).map_err(|e| format!("{e:#}"))
            });
            match r {
                Ok(c) => self.executer(c),
                Err(e) => self.erreur = Some(format!("aménagement : {e}")),
            }
        }
    }
}
