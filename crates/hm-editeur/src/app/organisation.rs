//! Onglet « Visgroups » : visgroups de la map (cocher, créer, ajouter, retirer, renommer, supprimer, sélectionner),
//! visgroups automatiques, masquage rapide, cordon. Toute modification de la map passe par une commande du noyau ;
//! le masquage rapide et les visgroups automatiques ne sont que des réglages de la vue.
use egui::Color32;
use hm_doc::{Commande, Proprio, Ref};

use super::{Editeur, Onglet};
use crate::visibilite::{nom, ARBRE};

/// Les visgroups de la racine aux enfants, avec leur profondeur.
fn arbre(vg: &[hm_doc::Visgroup]) -> Vec<(usize, &hm_doc::Visgroup)> {
    fn descendre<'a>(vg: &'a [hm_doc::Visgroup], parent: u64, prof: usize, out: &mut Vec<(usize, &'a hm_doc::Visgroup)>) {
        for v in vg.iter().filter(|v| v.parent == parent) {
            out.push((prof, v));
            if prof < 16 {
                descendre(vg, v.id, prof + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    descendre(vg, 0, 0, &mut out);
    out
}

impl Editeur {
    /// Comme le panneau VisGroups de Hammer : Utilisateur (ceux de la map, et le masquage rapide), Auto, Cordon.
    pub(super) fn panneau_visgroups(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (k, n) in ["Utilisateur", "Auto", "Cordon"].iter().enumerate() {
                if ui.selectable_label(self.sous_onglet_vg == k as u8, *n).clicked() {
                    self.sous_onglet_vg = k as u8;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match self.sous_onglet_vg {
            0 => {
                self.visgroups_de_la_map(ui);
                ui.separator();
                self.masquage_rapide(ui);
            }
            1 => self.visgroups_auto(ui),
            _ => self.cordon(ui),
        });
    }

    fn visgroups_de_la_map(&mut self, ui: &mut egui::Ui) {
        let doc = self.session.doc();
        let vg = doc.visgroups();
        let membres = doc.membres_visgroups();
        ui.horizontal(|ui| {
            ui.strong(format!("Visgroups de la map ({})", vg.len()));
            if ui.checkbox(&mut self.filtres.tout_montrer, "tout montrer").on_hover_text("montre aussi les visgroups décochés, sans rien changer à la map (le bouton « Show » de Hammer)").changed() {
                self.filtres.changer();
            }
        });
        ui.label(egui::RichText::new("décoché : non compilé · clic droit sur un nom : le menu").weak());
        let mut commande: Option<Commande> = None;
        let mut selectionner: Option<Vec<Ref>> = None;
        let ordre = arbre(&vg);
        if vg.is_empty() {
            ui.label(egui::RichText::new("aucun : sélectionner des objets puis « Nouveau depuis la sélection »").weak());
        }
        for (prof, v) in ordre {
            let m = membres.get(&v.id).map(|x| x.as_slice()).unwrap_or(&[]);
            let caches = m.iter().filter(|(_, c)| *c).count();
            let mut coche = caches < m.len() || m.is_empty();
            let mixte = caches > 0 && caches < m.len();
            // ses frères (même parent), dans l'ordre du fichier : les flèches ne bougent que parmi eux
            let freres: Vec<u64> = vg.iter().filter(|x| x.parent == v.parent).map(|x| x.id).collect();
            let rang = freres.iter().position(|x| *x == v.id).unwrap_or(0);
            ui.horizontal(|ui| {
                let esp = ui.spacing().item_spacing.x;
                ui.spacing_mut().item_spacing.x = 2.0;
                for (monter, icone, secours, possible) in [(true, crate::icones::MONTER, "^", rang > 0), (false, crate::icones::DESCENDRE, "v", rang + 1 < freres.len())] {
                    let b = egui::Button::new(crate::icones::texte(icone, secours, 10.0)).small().frame(false);
                    if ui.add_enabled(possible, b).on_hover_text(if monter { "monter" } else { "descendre" }).clicked() {
                        commande = Some(Commande::DeplacerVisgroup { visgroup: v.id, monter });
                    }
                }
                ui.spacing_mut().item_spacing.x = esp;
                ui.add_space(prof as f32 * 14.0);
                let rep = ui.add(egui::Checkbox::without_text(&mut coche).indeterminate(mixte));
                if rep.changed() {
                    commande = Some(Commande::MontrerVisgroup { visgroup: v.id, visible: mixte || coche });
                }
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, Color32::from_rgb(v.couleur[0], v.couleur[1], v.couleur[2]));
                match &mut self.vg_renommer {
                    Some((id, texte)) if *id == v.id => {
                        let t = ui.add(egui::TextEdit::singleline(texte).desired_width(180.0));
                        t.request_focus();
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let n = texte.trim().to_string();
                            if !n.is_empty() && n != v.nom {
                                commande = Some(Commande::RenommerVisgroup { visgroup: v.id, nom: n });
                            }
                            self.vg_renommer = None;
                        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) || t.lost_focus() {
                            self.vg_renommer = None;
                        }
                    }
                    _ => {
                        let etiquette = ui.add(egui::Label::new(format!("{}  ({})", v.nom, m.len())).sense(egui::Sense::click()));
                        if etiquette.double_clicked() {
                            self.vg_renommer = Some((v.id, v.nom.clone()));
                        }
                        etiquette.context_menu(|ui| {
                            let sel = !self.session.selection.is_empty();
                            if ui.add_enabled(sel, egui::Button::new("Ajouter la sélection")).clicked() {
                                commande = Some(Commande::AjouterAuVisgroup { visgroup: v.id, objets: self.session.selection.clone() });
                                ui.close();
                            }
                            if ui.add_enabled(sel, egui::Button::new("Retirer la sélection")).clicked() {
                                commande = Some(Commande::RetirerDuVisgroup { visgroup: v.id, objets: self.session.selection.clone() });
                                ui.close();
                            }
                            if ui.button("Sélectionner ses objets").clicked() {
                                selectionner = Some(m.iter().map(|(r, _)| *r).collect());
                                ui.close();
                            }
                            if ui.button("Renommer").clicked() {
                                self.vg_renommer = Some((v.id, v.nom.clone()));
                                ui.close();
                            }
                            if ui.button("Supprimer (les objets restent)").clicked() {
                                commande = Some(Commande::SupprimerVisgroup { visgroup: v.id });
                                ui.close();
                            }
                        });
                    }
                }
            });
        }
        let n = self.session.selection.len();
        ui.horizontal(|ui| {
            if self.vg_nom.trim().is_empty() || self.vg_nom_auto {
                self.vg_nom = format!("{n} objet{}", if n > 1 { "s" } else { "" });
                self.vg_nom_auto = true;
            }
            if ui.add(egui::TextEdit::singleline(&mut self.vg_nom).desired_width(140.0)).changed() {
                self.vg_nom_auto = false;
            }
            if ui.add_enabled(n > 0, egui::Button::new("Nouveau depuis la sélection")).clicked() {
                commande = Some(Commande::CreerVisgroup { nom: self.vg_nom.trim().to_string(), objets: self.session.selection.clone(), cache: false });
                self.vg_nom_auto = true;
            }
        });
        if let Some(c) = commande {
            self.executer(c);
        }
        if let Some(objets) = selectionner {
            self.selectionner_membres(objets);
        }
    }

    /// Sélectionne ces membres d'un visgroup (un solide d'entité : son entité), comme « Mark » de Hammer.
    fn selectionner_membres(&mut self, objets: Vec<Ref>) {
        {
            let doc = self.session.doc();
            let mut sel: Vec<Ref> = Vec::new();
            for r in objets {
                let r = match r {
                    Ref::Solide(id) => match doc.proprio(id) {
                        Some(Proprio::Entite(e)) => Ref::Entite(e),
                        _ => r,
                    },
                    _ => r,
                };
                if !sel.contains(&r) {
                    sel.push(r);
                }
            }
            self.statut = format!("sélection : {} objet(s) du visgroup", sel.len());
            self.session.selection = sel;
        }
    }

    /// La liste des visgroups de la barre d'objets, comme celle de Hammer++ : cocher, choisir une ligne, puis Show
    /// (tout montrer, sans rien changer à la map), Edit (l'onglet Visgroups), Mark (sélectionner ses objets, wiki
    /// Valve « Grouping and VisGrouping »), monter et descendre.
    pub(super) fn visgroups_compacts(&mut self, ui: &mut egui::Ui) {
        let doc = self.session.doc();
        let vg = doc.visgroups();
        let membres = doc.membres_visgroups();
        let mut commande: Option<Commande> = None;
        ui.label(format!("Visgroups ({})", vg.len()));
        egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).inner_margin(2.0).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("visgroups_compacts").max_height(120.0).auto_shrink([false, true]).show(ui, |ui| {
                if vg.is_empty() {
                    ui.weak("aucun");
                }
                for (prof, v) in arbre(&vg) {
                    let m = membres.get(&v.id).map(|x| x.as_slice()).unwrap_or(&[]);
                    let caches = m.iter().filter(|(_, c)| *c).count();
                    let (mut coche, mixte) = (caches < m.len() || m.is_empty(), caches > 0 && caches < m.len());
                    ui.horizontal(|ui| {
                        ui.add_space(prof as f32 * 12.0);
                        if ui.add(egui::Checkbox::without_text(&mut coche).indeterminate(mixte)).changed() {
                            commande = Some(Commande::MontrerVisgroup { visgroup: v.id, visible: mixte || coche });
                        }
                        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 2.0, Color32::from_rgb(v.couleur[0], v.couleur[1], v.couleur[2]));
                        if ui.selectable_label(self.vg_choisi == Some(v.id), &v.nom).clicked() {
                            self.vg_choisi = Some(v.id);
                        }
                    });
                }
            });
        });
        let choisi = self.vg_choisi.and_then(|id| vg.iter().find(|v| v.id == id));
        let freres: Vec<u64> = choisi.map(|c| vg.iter().filter(|x| x.parent == c.parent).map(|x| x.id).collect()).unwrap_or_default();
        let rang = choisi.and_then(|c| freres.iter().position(|x| *x == c.id));
        let mut marquer = None;
        ui.horizontal(|ui| {
            if ui.selectable_label(self.filtres.tout_montrer, "Show").on_hover_text("montrer aussi les visgroups décochés, sans rien changer à la map").clicked() {
                self.filtres.tout_montrer = !self.filtres.tout_montrer;
                self.filtres.changer();
            }
            if ui.button("Edit").on_hover_text("l'onglet Visgroups : créer, renommer, supprimer, cordons").clicked() {
                self.sous_onglet_vg = 0;
                self.montrer_onglet(Onglet::Visgroups);
            }
            if ui.add_enabled(choisi.is_some(), egui::Button::new("Mark")).on_hover_text("sélectionner les objets du visgroup choisi").clicked() {
                marquer = choisi.map(|c| membres.get(&c.id).map(|x| x.iter().map(|(r, _)| *r).collect::<Vec<Ref>>()).unwrap_or_default());
            }
            for (monter, icone, secours) in [(true, crate::icones::MONTER, "^"), (false, crate::icones::DESCENDRE, "v")] {
                let possible = rang.map_or(false, |k| if monter { k > 0 } else { k + 1 < freres.len() });
                if ui.add_enabled(possible, egui::Button::new(crate::icones::texte(icone, secours, 12.0))).on_hover_text(if monter { "monter" } else { "descendre" }).clicked() {
                    commande = choisi.map(|c| Commande::DeplacerVisgroup { visgroup: c.id, monter });
                }
            }
        });
        if let Some(c) = commande {
            self.executer(c);
        }
        if let Some(m) = marquer {
            self.selectionner_membres(m);
        }
    }

    fn visgroups_auto(&mut self, ui: &mut egui::Ui) {
        ui.strong("Visgroups automatiques");
        ui.label(egui::RichText::new("affichage seulement : tout est compilé").weak());
        let mut change = false;
        for (parent, _, enfants) in ARBRE {
            let mut vu = !self.filtres.auto_caches.contains(parent);
            if ui.checkbox(&mut vu, nom(*parent)).changed() {
                change = true;
                if vu {
                    self.filtres.auto_caches.remove(parent);
                } else {
                    self.filtres.auto_caches.insert(*parent);
                }
            }
            if enfants.is_empty() {
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_space(18.0);
                for e in *enfants {
                    let mut vu = !self.filtres.auto_caches.contains(e);
                    if ui.add_enabled(!self.filtres.auto_caches.contains(parent), egui::Checkbox::new(&mut vu, nom(*e))).changed() {
                        change = true;
                        if vu {
                            self.filtres.auto_caches.remove(e);
                        } else {
                            self.filtres.auto_caches.insert(*e);
                        }
                    }
                }
            });
        }
        if ui.add_enabled(!self.filtres.auto_caches.is_empty(), egui::Button::new("Tout cocher")).clicked() {
            self.filtres.auto_caches.clear();
            change = true;
        }
        if change {
            self.filtres.changer();
        }
    }

    fn masquage_rapide(&mut self, ui: &mut egui::Ui) {
        let n = self.filtres.masques_existants(self.session.doc()).len();
        ui.strong(format!("Masquage rapide : {n} objet(s) masqué(s)"));
        ui.label(egui::RichText::new("pour la session seulement ; tout est compilé").weak());
        ui.horizontal_wrapped(|ui| {
            let sel = !self.session.selection.is_empty();
            if ui.add_enabled(sel, egui::Button::new(format!("Masquer la sélection ({})", self.rc.texte("View_Quickhide")))).clicked() {
                self.masquer(false);
            }
            if ui.add_enabled(sel, egui::Button::new(format!("Masquer le reste ({})", self.rc.texte("View_Quickhide_Unselected")))).clicked() {
                self.masquer(true);
            }
            if ui.add_enabled(n > 0, egui::Button::new(format!("Tout réafficher ({})", self.rc.texte("View_Quick_Unhide")))).clicked() {
                self.reafficher();
            }
            // « Convert QuickHide objects to VisGroup » de Hammer (wiki Valve, « Hammer View Menu »)
            if ui.add_enabled(n > 0, egui::Button::new("En faire un visgroup")).on_hover_text("un visgroup décoché qui garde ces objets cachés, enregistré dans la map (et alors non compilé)").clicked() {
                let objets = self.filtres.masques_existants(self.session.doc());
                self.executer(Commande::CreerVisgroup { nom: format!("_FromQuickHide ({n})"), objets, cache: true });
                self.filtres.masques.clear();
                self.filtres.changer();
            }
        });
    }

    /// Les cordons, comme la liste de Hammer++ : l'état du cordon, chaque cordon coché ou non (renommer d'un
    /// double-clic), et les boîtes de celui qu'on a choisi. Chaque changement est une commande `Cordons`.
    fn cordon(&mut self, ui: &mut egui::Ui) {
        let c = self.session.doc().cordons();
        self.cordon_choisi = self.cordon_choisi.min(c.liste.len().saturating_sub(1));
        let mut nouveau: Option<hm_doc::Cordons> = None;
        // une boîte à prendre : la sélection, sinon le bloc tracé par l'outil bloc (pas encore créé)
        let depuis_sel = hm_doc::boite_de(self.session.doc(), &self.session.selection).map(|(a, b)| [a.to_array(), b.to_array()]);
        let source = depuis_sel.or(self.bloc.map(|(a, b)| [a.as_dvec3().to_array(), b.as_dvec3().to_array()]));
        let aide_source = "la boîte de la sélection, ou le bloc tracé par l'outil bloc";
        ui.horizontal(|ui| {
            ui.strong(format!("Cordons ({})", c.liste.len()));
            let mut actif = c.actif;
            if ui.add_enabled(!c.liste.is_empty(), egui::Checkbox::new(&mut actif, "actif")).on_hover_text("seul ce qui touche une boîte d'un cordon coché est compilé et montré").changed() {
                nouveau = Some(hm_doc::Cordons { actif, liste: c.liste.clone() });
            }
        });
        if c.liste.is_empty() {
            ui.label(egui::RichText::new("aucun : sélectionner des objets puis « Nouveau »").weak());
        } else {
            ui.label(egui::RichText::new("coché : compilé quand le cordon est actif · clic droit : le menu").weak());
        }
        for (i, k) in c.liste.iter().enumerate() {
            ui.horizontal(|ui| {
                let mut coche = k.actif;
                if ui.add(egui::Checkbox::without_text(&mut coche)).changed() {
                    let mut n = c.clone();
                    n.liste[i].actif = coche;
                    nouveau = Some(n);
                }
                match &mut self.cordon_renommer {
                    Some((j, texte)) if *j == i => {
                        let t = ui.add(egui::TextEdit::singleline(texte).desired_width(180.0));
                        t.request_focus();
                        if ui.input(|x| x.key_pressed(egui::Key::Enter)) {
                            let mut n = c.clone();
                            n.liste[i].nom = texte.trim().to_string();
                            if n.liste[i].nom != k.nom {
                                nouveau = Some(n);
                            }
                            self.cordon_renommer = None;
                        } else if ui.input(|x| x.key_pressed(egui::Key::Escape)) || t.lost_focus() {
                            self.cordon_renommer = None;
                        }
                    }
                    _ => {
                        let n = k.boites.len();
                        let r = ui.selectable_label(self.cordon_choisi == i, format!("{}  ({n} boîte{})", k.nom, if n > 1 { "s" } else { "" }));
                        if r.clicked() {
                            self.cordon_choisi = i;
                        }
                        if r.double_clicked() {
                            self.cordon_renommer = Some((i, k.nom.clone()));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Renommer").clicked() {
                                self.cordon_renommer = Some((i, k.nom.clone()));
                                ui.close();
                            }
                            if ui.button("Supprimer").clicked() {
                                let mut n = c.clone();
                                n.liste.remove(i);
                                nouveau = Some(n);
                                ui.close();
                            }
                        });
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(source.is_some(), egui::Button::new("Nouveau")).on_hover_text(format!("un cordon coché sur {aide_source}")).on_disabled_hover_text(format!("prend {aide_source}")).clicked() {
                if let Some(b) = source {
                    let mut n = c.clone();
                    n.liste.push(hm_doc::Cordon { nom: format!("cordon {}", n.liste.len() + 1), actif: true, boites: vec![b] });
                    n.actif = true;
                    self.cordon_choisi = n.liste.len() - 1;
                    nouveau = Some(n);
                    if depuis_sel.is_none() {
                        self.bloc = None;
                    }
                }
            }
            if ui.add_enabled(!c.liste.is_empty(), egui::Button::new("Supprimer")).on_hover_text("le cordon choisi").clicked() {
                let mut n = c.clone();
                n.liste.remove(self.cordon_choisi);
                nouveau = Some(n);
            }
        });
        if let Some(k) = c.liste.get(self.cordon_choisi) {
            ui.separator();
            ui.strong(format!("Boîtes de « {} »", k.nom));
            let mut n = c.clone();
            let (mut change, mut retirer) = (false, None);
            let pas = self.grille as f64 / 4.0;
            for (j, b) in n.liste[self.cordon_choisi].boites.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("boîte {}", j + 1));
                    if ui.add(egui::Button::new(crate::icones::texte(crate::icones::SUPPRIMER, "x", 12.0)).small().frame(false)).on_hover_text("retirer la boîte").clicked() {
                        retirer = Some(j);
                    }
                });
                let [lo, hi] = b;
                for (nom_borne, v) in [("min", lo), ("max", hi)] {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(nom_borne);
                        for (k, axe) in ["x", "y", "z"].iter().enumerate() {
                            ui.label(*axe);
                            change |= ui.add(egui::DragValue::new(&mut v[k]).speed(pas).max_decimals(2)).changed();
                        }
                    });
                }
            }
            if change {
                if let Err(e) = self.session.executer_continu(Commande::Cordons { actif: n.actif, liste: n.liste.clone() }, "cordon") {
                    self.erreur = Some(format!("cordon refusé : {e}"));
                }
            }
            if let Some(j) = retirer {
                n.liste[self.cordon_choisi].boites.remove(j);
                nouveau = Some(n.clone());
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(source.is_some(), egui::Button::new("Ajouter une boîte")).on_hover_text(aide_source).clicked() {
                    if let Some(b) = source {
                        n.liste[self.cordon_choisi].boites.push(b);
                        nouveau = Some(n.clone());
                    }
                }
                if ui.add_enabled(source.is_some(), egui::Button::new("Remplacer les boîtes")).on_hover_text(format!("une seule boîte : {aide_source}")).clicked() {
                    if let Some(b) = source {
                        n.liste[self.cordon_choisi].boites = vec![b];
                        nouveau = Some(n.clone());
                    }
                }
            });
        }
        if let Some(n) = nouveau {
            self.executer(Commande::Cordons { actif: n.actif, liste: n.liste });
        }
    }

    /// Le cordon choisi (créé s'il n'y en a pas) prend la boîte de la sélection, se coche, et le cordon s'active.
    pub(super) fn cordon_depuis_selection(&mut self) {
        let Some((lo, hi)) = hm_doc::boite_de(self.session.doc(), &self.session.selection) else { return };
        let mut c = self.session.doc().cordons();
        let b = [lo.to_array(), hi.to_array()];
        match c.liste.get_mut(self.cordon_choisi) {
            Some(k) => {
                k.boites = vec![b];
                k.actif = true;
            }
            None => {
                c.liste.push(hm_doc::Cordon { nom: "cordon".into(), actif: true, boites: vec![b] });
                self.cordon_choisi = c.liste.len() - 1;
            }
        }
        self.executer(Commande::Cordons { actif: true, liste: c.liste });
    }
}
