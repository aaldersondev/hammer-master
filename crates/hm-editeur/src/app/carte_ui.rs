//! Le menu Carte de Hammer : rapport d'entités (« Entity Report »), galerie d'entités (« Entity Gallery » : une de
//! chaque classe ponctuelle du FGD), différence avec un autre VMF (« Diff Map File »), unités des cotes.
use std::path::PathBuf;

use hm_doc::{Commande, Ref};

use super::Editeur;

pub struct EtatCarte {
    pub rapport: bool,
    filtre: String,
    cle: String,
    valeur: String,
    ponctuelles: bool,
    a_brosses: bool,
    choisie: Option<u64>,
    difference: Option<(PathBuf, hm_edit::difference::Difference)>,
    /// Unités des cotes de la sélection : 0 unités, 1 mètres, 2 pieds et pouces (1 unité = 1 pouce, comme Hammer).
    pub unites: u8,
}

impl Default for EtatCarte {
    fn default() -> EtatCarte {
        EtatCarte { rapport: false, filtre: String::new(), cle: String::new(), valeur: String::new(), ponctuelles: true, a_brosses: true, choisie: None, difference: None, unites: 0 }
    }
}

/// Une longueur de la map dans les unités choisies.
pub fn longueur(v: f64, unites: u8) -> String {
    match unites {
        1 => format!("{:.2} m", v * 0.0254),
        2 => {
            let pouces = v.round() as i64;
            format!("{}' {}\"", pouces / 12, pouces.rem_euclid(12))
        }
        _ => hm_doc::kv::nombre(v),
    }
}

impl Editeur {
    /// « Entity Gallery » : une entité de chaque classe ponctuelle du FGD, en carré à côté de la map, en une étape.
    pub(super) fn galerie_entites(&mut self) {
        let classes: Vec<String> = self.fgd.classes(false).into_iter().map(String::from).collect();
        if classes.is_empty() {
            self.statut = "aucune classe dans le FGD".into();
            return;
        }
        let (lo, hi) = self.geo.as_ref().map_or((glam::Vec3::ZERO, glam::Vec3::ZERO), |g| (g.min, g.max));
        let cote = (classes.len() as f64).sqrt().ceil() as usize;
        let (x0, y0, z0) = (lo.x as f64, hi.y as f64 + 512.0, lo.z.max(0.0) as f64);
        let commandes = classes
            .iter()
            .enumerate()
            .map(|(i, c)| Commande::CreerEntite { classe: c.clone(), origine: [x0 + (i % cote) as f64 * 128.0, y0 + (i / cote) as f64 * 128.0, z0], cles: vec![] })
            .collect();
        self.executer(Commande::Lot { nom: format!("galerie d'entités ({} classes)", classes.len()), commandes });
    }

    /// « Diff Map File » : ce qui sépare un autre VMF (avant) de la map ouverte (après).
    pub(super) fn difference_avec_un_fichier(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("Map Hammer", &["vmf"]).pick_file() else { return };
        let avant = match std::fs::read(&p) {
            Ok(o) => o,
            Err(e) => {
                self.erreur = Some(format!("{} : {e}", p.display()));
                return;
            }
        };
        match hm_edit::difference::comparer(&avant, &self.session.doc().ecrire()) {
            Ok(d) => {
                self.statut = d.resume();
                self.carte.difference = Some((p, d));
            }
            Err(e) => self.erreur = Some(format!("comparaison impossible : {e}")),
        }
    }

    pub(super) fn fenetres_carte(&mut self, ctx: &egui::Context) {
        self.fenetre_rapport(ctx);
        self.fenetre_difference(ctx);
    }

    fn fenetre_rapport(&mut self, ctx: &egui::Context) {
        if !self.carte.rapport {
            return;
        }
        let doc = self.session.doc();
        let c = &mut self.carte;
        let f = c.filtre.to_lowercase();
        let (k, v) = (c.cle.trim().to_lowercase(), c.valeur.trim().to_lowercase());
        let lignes: Vec<(u64, String, String)> = doc
            .entites()
            .filter(|e| if e.ponctuelle() { c.ponctuelles } else { c.a_brosses })
            .filter(|e| f.is_empty() || e.classe().to_lowercase().contains(&f) || e.nom().to_lowercase().contains(&f))
            .filter(|e| k.is_empty() || e.cles.iter().any(|(x, y)| x.to_lowercase() == k && (v.is_empty() || y.to_lowercase().contains(&v))))
            .map(|e| (e.id, e.classe().to_string(), e.nom().to_string()))
            .collect();
        let (mut ouvert, mut action) = (true, None::<(u8, u64)>);
        super::barres::fenetre(ctx, "Rapport d'entités").open(&mut ouvert).default_size([560.0, 460.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Classe ou nom");
                ui.add(egui::TextEdit::singleline(&mut c.filtre).desired_width(160.0));
                ui.label("Clé");
                ui.add(egui::TextEdit::singleline(&mut c.cle).desired_width(90.0));
                ui.label("contient");
                ui.add(egui::TextEdit::singleline(&mut c.valeur).desired_width(90.0));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut c.ponctuelles, "Ponctuelles");
                ui.checkbox(&mut c.a_brosses, "À brosses");
                ui.label(format!("{} entité(s)", lignes.len()));
            });
            ui.separator();
            let h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
            egui::ScrollArea::vertical().max_height(ui.available_height() - 40.0).auto_shrink(false).show_rows(ui, h, lignes.len(), |ui, rangs| {
                for (id, classe, nom) in &lignes[rangs] {
                    let r = ui.selectable_label(c.choisie == Some(*id), format!("{classe}   {}   n° {id}", if nom.is_empty() { "—" } else { nom }));
                    if r.clicked() {
                        c.choisie = Some(*id);
                    }
                    if r.double_clicked() {
                        action = Some((0, *id));
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let choisie = c.choisie.filter(|id| doc.entite(*id).is_some());
                for (k, n) in ["Aller à", "Propriétés", "Supprimer"].iter().enumerate() {
                    if ui.add_enabled(choisie.is_some(), egui::Button::new(*n)).clicked() {
                        action = choisie.map(|id| (k as u8, id));
                    }
                }
            });
        });
        self.carte.rapport = ouvert;
        match action {
            Some((0, id)) => self.aller_a(Ref::Entite(id), None),
            Some((1, id)) => {
                self.aller_a(Ref::Entite(id), None);
                self.montrer_onglet(super::Onglet::Proprietes);
            }
            Some((_, id)) => self.executer(Commande::Supprimer { objets: vec![Ref::Entite(id)] }),
            None => {}
        }
    }

    fn fenetre_difference(&mut self, ctx: &egui::Context) {
        let Some((p, d)) = &self.carte.difference else { return };
        let (mut ouvert, mut aller) = (true, None::<[f64; 3]>);
        super::barres::fenetre(ctx, "Différence avec un autre VMF").open(&mut ouvert).default_size([560.0, 420.0]).show(ctx, |ui| {
            ui.label(format!("avant : {}", p.display()));
            ui.label("après : la map ouverte");
            ui.strong(d.resume());
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut ligne = |ui: &mut egui::Ui, quoi: &str, e: &hm_edit::difference::Element, vers: Option<[f64; 3]>| {
                    let r = ui.selectable_label(false, format!("{quoi} : {}", e.decrire()));
                    if r.clicked() {
                        aller = vers.or(e.origine);
                    }
                };
                for e in d.ajoutes.iter().take(500) {
                    ligne(ui, "ajoutée", e, None);
                }
                for e in d.retires.iter().take(500) {
                    ligne(ui, "retirée", e, None);
                }
                for (e, vers) in d.deplaces.iter().take(500) {
                    ligne(ui, "déplacée", e, Some(*vers));
                }
                for e in d.modifies.iter().take(500) {
                    ligne(ui, "modifiée", e, None);
                }
                for c in d.solides_ajoutes.iter().take(500) {
                    if ui.selectable_label(false, format!("solide ajouté en ({:.0} {:.0} {:.0})", c[0], c[1], c[2])).clicked() {
                        aller = Some(*c);
                    }
                }
                for c in d.solides_retires.iter().take(500) {
                    if ui.selectable_label(false, format!("solide retiré en ({:.0} {:.0} {:.0})", c[0], c[1], c[2])).clicked() {
                        aller = Some(*c);
                    }
                }
            });
        });
        if let Some(c) = aller {
            let c = glam::DVec3::from(c);
            self.cadrer_sur(c, c);
        }
        if !ouvert {
            self.carte.difference = None;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn longueurs() {
        assert_eq!(super::longueur(128.0, 0), "128");
        assert_eq!(super::longueur(100.0, 1), "2.54 m");
        assert_eq!(super::longueur(76.0, 2), "6' 4\"");
    }
}
