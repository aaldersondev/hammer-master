//! Onglets Sorties et Entrées des propriétés d'une entité (comme ceux de la fenêtre de propriétés de Hammer).
//! Chaque champ a un bouton « … » qui propose ce qui va : les sorties de la classe (FGD), les noms de la map, les
//! entrées des classes visées. Une cible introuvable est en rouge, une entrée inconnue de la cible en orange. Toute
//! modification remplace la liste par la commande `sorties` du noyau.
use egui::Color32;
use hm_doc::io::Sortie;
use hm_doc::{Commande, Ref};

use super::Editeur;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VueProprietes {
    #[default]
    Cles,
    Sorties,
    Entrees,
}

const SPECIAUX: [&str; 5] = ["!activator", "!caller", "!self", "!player", "!picker"];

/// Un champ texte et son bouton de suggestions « … » (filtrées par ce qui est tapé). Rend vrai s'il a changé.
fn champ(ui: &mut egui::Ui, id: (u64, usize, &str), texte: &mut String, largeur: f32, suggestions: &[String], couleur: Option<Color32>) -> bool {
    let mut change = false;
    let mut te = egui::TextEdit::singleline(texte).desired_width(largeur).id_salt(id);
    if let Some(c) = couleur {
        te = te.text_color(c);
    }
    if ui.add(te).changed() {
        change = true;
    }
    if !suggestions.is_empty() {
        ui.menu_button("…", |ui| {
            let f = texte.to_ascii_lowercase();
            let mut liste: Vec<&String> = suggestions.iter().filter(|s| s.to_ascii_lowercase().contains(&f)).collect();
            if liste.is_empty() {
                liste = suggestions.iter().collect();
            }
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for s in liste.into_iter().take(200) {
                    if ui.button(s).clicked() {
                        *texte = s.clone();
                        change = true;
                        ui.close();
                    }
                }
            });
        });
    }
    change
}

impl Editeur {
    /// Les entrées connues des entités qu'un nom désigne (FGD de leurs classes) ; toutes les entrées du FGD si le nom
    /// ne désigne personne.
    fn entrees_de_cible(&self, cible: &str) -> (Vec<String>, bool) {
        let doc = self.session.doc();
        let ids = doc.entites_nommees(cible);
        let mut v: Vec<String> = Vec::new();
        let classes: Vec<String> = if ids.is_empty() { Vec::new() } else { ids.iter().filter_map(|i| doc.entite(*i).map(|e| e.classe().to_string())).collect() };
        for c in &classes {
            if let Some(k) = self.fgd.classe(c) {
                for e in k.entrees {
                    if !v.iter().any(|x| x.eq_ignore_ascii_case(&e.nom)) {
                        v.push(e.nom);
                    }
                }
            }
        }
        v.sort_by_key(|x| x.to_ascii_lowercase());
        (v, !ids.is_empty())
    }

    pub(super) fn onglets_proprietes(&mut self, ui: &mut egui::Ui, e: u64) {
        let doc = self.session.doc();
        let Some(ent) = doc.entite(e) else { return };
        let (n_s, n_e) = (ent.sorties.as_ref().map_or(0, |v| v.len()), doc.entrees_vers(e).len());
        ui.horizontal(|ui| {
            for (v, nom) in [(VueProprietes::Cles, crate::langue::t("Clés").to_string()), (VueProprietes::Sorties, tr!("Sorties ({n_s})", "Outputs ({n_s})")), (VueProprietes::Entrees, tr!("Entrées ({n_e})", "Inputs ({n_e})"))] {
                if ui.selectable_label(self.vue_proprietes == v, nom).clicked() {
                    self.vue_proprietes = v;
                }
            }
        });
        ui.separator();
    }

    pub(super) fn panneau_sorties(&mut self, ui: &mut egui::Ui, e: u64) {
        let doc = self.session.doc();
        let Some(ent) = doc.entite(e) else { return };
        let classe = ent.classe().to_string();
        let mut liste = ent.sorties_lues();
        let sorties_fgd: Vec<String> = self.fgd.classe(&classe).map(|c| {
            let mut v: Vec<String> = c.sorties.iter().map(|s| s.nom.clone()).collect();
            v.sort_by_key(|x| x.to_ascii_lowercase());
            v
        }).unwrap_or_default();
        let mut noms: Vec<String> = SPECIAUX.iter().map(|s| s.to_string()).collect();
        noms.extend(doc.noms());
        let mut change = false;
        let mut retirer = None;
        egui::ScrollArea::vertical().id_salt("sorties").auto_shrink([false, true]).show(ui, |ui| {
            if liste.is_empty() {
                ui.label(egui::RichText::new(crate::langue::t("aucune sortie")).weak());
            }
            for (k, s) in liste.iter_mut().enumerate() {
                let rouge = Color32::from_rgb(255, 110, 100);
                let orange = Color32::from_rgb(255, 180, 70);
                let sortie_connue = sorties_fgd.is_empty() || sorties_fgd.iter().any(|x| x.eq_ignore_ascii_case(&s.sortie));
                let cible_connue = s.cible.trim().is_empty() || s.cible.starts_with('!') || !self.session.doc().entites_nommees(&s.cible).is_empty();
                let (entrees, trouvee) = self.entrees_de_cible(&s.cible);
                let entree_connue = s.entree.trim().is_empty() || !trouvee || entrees.is_empty() || entrees.iter().any(|x| x.eq_ignore_ascii_case(&s.entree));
                ui.horizontal(|ui| {
                    change |= champ(ui, (e, k, "sortie"), &mut s.sortie, 120.0, &sorties_fgd, (!sortie_connue).then_some(orange));
                    ui.label(crate::langue::t("vers"));
                    change |= champ(ui, (e, k, "cible"), &mut s.cible, 120.0, &noms, (!cible_connue).then_some(rouge));
                    if ui.small_button("×").on_hover_text(crate::langue::t("retirer cette sortie")).clicked() {
                        retirer = Some(k);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    change |= champ(ui, (e, k, "entree"), &mut s.entree, 110.0, &entrees, (!entree_connue).then_some(orange));
                    ui.label("(");
                    if ui.add(egui::TextEdit::singleline(&mut s.parametre).desired_width(60.0).id_salt((e, k, "param")).hint_text("param")).changed() {
                        change = true;
                    }
                    ui.label(")");
                    if ui.add(egui::DragValue::new(&mut s.delai).range(0.0..=3600.0).speed(0.05).max_decimals(2).suffix(" s")).changed() {
                        change = true;
                    }
                    let mut une = s.fois == 1;
                    if ui.checkbox(&mut une, "1×").on_hover_text(crate::langue::t("une seule fois")).changed() {
                        s.fois = if une { 1 } else { -1 };
                        change = true;
                    }
                });
                if !cible_connue {
                    ui.colored_label(rouge, tr!("    aucune entité ne s'appelle « {} »", "    no entity is named \"{}\"", s.cible));
                } else if !entree_connue {
                    ui.colored_label(orange, tr!("    {} n'a pas d'entrée « {} » dans le FGD", "    {} has no input \"{}\" in the FGD", s.cible, s.entree));
                }
                ui.separator();
            }
        });
        if ui.button(crate::langue::t("+ Ajouter une sortie")).clicked() {
            liste.push(Sortie { sortie: sorties_fgd.first().cloned().unwrap_or_else(|| "OnTrigger".into()), cible: String::new(), entree: String::new(), parametre: String::new(), delai: 0.0, fois: -1 });
            self.executer(Commande::Sorties { entite: e, sorties: liste });
            return;
        }
        if let Some(k) = retirer {
            liste.remove(k);
            self.executer(Commande::Sorties { entite: e, sorties: liste });
            return;
        }
        if change {
            // une sortie vide de cible reste possible pendant la frappe ; la commande refuse seulement l'impossible
            if let Err(err) = self.session.executer_continu(Commande::Sorties { entite: e, sorties: liste }, &format!("sorties{e}")) {
                self.erreur = Some(tr!("sorties refusées : {err}", "outputs refused: {err}"));
            }
        }
    }

    pub(super) fn panneau_entrees(&mut self, ui: &mut egui::Ui, e: u64) {
        let doc = self.session.doc();
        let entrees = doc.entrees_vers(e);
        if doc.entite(e).map_or(true, |x| x.nom_donne().is_none()) {
            ui.label(egui::RichText::new(crate::langue::t("sans nom (targetname), personne ne peut la viser")).weak());
            return;
        }
        if entrees.is_empty() {
            ui.label(egui::RichText::new(crate::langue::t("aucune entité ne la vise")).weak());
        }
        let mut aller = None;
        for (src, s) in &entrees {
            let classe = doc.entite(*src).map(|x| x.classe().to_string()).unwrap_or_default();
            let nom = doc.entite(*src).and_then(|x| x.nom_donne()).map(|n| format!("« {n} »")).unwrap_or_else(|| tr!("n° {src}", "#{src}"));
            ui.horizontal(|ui| {
                if ui.small_button(crate::langue::t("voir")).on_hover_text(crate::langue::t("sélectionner l'entité source")).clicked() {
                    aller = Some(*src);
                }
                ui.label(tr!("{classe} {nom} : {} > {}({}){}", "{classe} {nom}: {} > {}({}){}", s.sortie, s.entree, s.parametre, if s.delai > 0.0 { tr!(" après {} s", " after {} s", s.delai) } else { String::new() }));
            });
        }
        if let Some(src) = aller {
            self.session.selection = vec![Ref::Entite(src)];
            self.vue_proprietes = VueProprietes::Sorties;
        }
    }
}
