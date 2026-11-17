//! Onglet « Vérification » (Alt+P) : la liste des problèmes de la map, comme la fenêtre « Check for Problems » de
//! Hammer : aller à l'objet (double clic), corriger, tout corriger du même type (une seule étape d'historique),
//! parties visibles seulement.
use glam::DVec3;
use hm_doc::{Commande, Ref};

use super::{Editeur, Onglet};
use hm_edit::verification::{self, Probleme};

#[derive(Default)]
pub struct EtatVerification {
    liste: Option<Vec<Probleme>>,
    choisi: Option<usize>,
    visibles: bool,
    erreur: Option<String>,
    budgets: Option<hm_regles::budgets::Budgets>,
}

impl Editeur {
    pub(super) fn verifier_map(&mut self) {
        let t = std::time::Instant::now();
        let log = self.log_compile();
        let (v, b, e, _) = verification::tout(self.session.doc(), &self.fgd, &self.vfs, self.session.chemin(), log.as_deref(), false);
        self.statut = tr!("vérification : {} problème(s) en {:.1} s", "check: {} problem(s) in {:.1} s", v.len(), t.elapsed().as_secs_f32());
        self.montrer_verification(v, b, e);
        self.montrer_onglet(Onglet::Verification);
    }

    pub(super) fn montrer_verification(&mut self, v: Vec<Probleme>, b: Option<hm_regles::budgets::Budgets>, e: Option<String>) {
        self.verification.liste = Some(v);
        self.verification.budgets = b;
        self.verification.erreur = e;
        self.verification.choisi = None;
    }

    /// La boîte de l'objet dans les vues (celle du problème, sinon celle de sa forme).
    pub(super) fn boite_objet(&self, r: Ref) -> Option<(DVec3, DVec3)> {
        let geo = self.geo.as_ref()?;
        let formes = geo.formes.iter().filter(|f| f.designe.objet == r || matches!(r, Ref::Solide(s) if f.designe.solide == Some(s)));
        formes.fold(None, |acc: Option<(DVec3, DVec3)>, f| {
            let (a, b) = (f.min.as_dvec3(), f.max.as_dvec3());
            Some(acc.map_or((a, b), |(lo, hi)| (lo.min(a), hi.max(b))))
        })
    }

    fn aller_au_probleme(&mut self, i: usize) {
        let Some(p) = self.verification.liste.as_ref().and_then(|l| l.get(i)).cloned() else { return };
        match p.objet {
            Some(r) => self.aller_a(r, p.boite),
            None => {
                if let Some((lo, hi)) = p.boite {
                    self.cadrer_sur(lo, hi);
                }
            }
        }
    }

    /// Sélectionne l'objet et cadre les vues dessus (sur `boite` si elle est donnée).
    pub(super) fn aller_a(&mut self, r: Ref, boite: Option<(DVec3, DVec3)>) {
        self.session.selection = vec![r];
        if let Some((lo, hi)) = boite.or_else(|| self.boite_objet(r)) {
            self.cadrer_sur(lo, hi);
        }
    }

    pub(super) fn cadrer_sur(&mut self, lo: DVec3, hi: DVec3) {
        let m = glam::Vec3::splat(64.0);
        for v in &mut self.vues {
            v.cadrer(lo.as_vec3() - m, hi.as_vec3() + m);
        }
    }

    pub(super) fn panneau_verification(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(tr!("Vérifier ({})", "Check ({})", self.rc.texte("Map_Check_For_Problems"))).clicked() {
                self.verifier_map();
            }
            ui.checkbox(&mut self.verification.visibles, crate::langue::t("Parties visibles seulement"));
        });
        if let Some(e) = &self.verification.erreur {
            ui.colored_label(egui::Color32::from_rgb(255, 130, 100), e);
        }
        let Some(liste) = self.verification.liste.clone() else {
            ui.weak(crate::langue::t("pas encore vérifiée"));
            return;
        };
        // un objet caché n'est pas dans les formes des vues
        let vus = |p: &Probleme| match p.objet {
            None => true,
            Some(r) => self.boite_objet(r).is_some(),
        };
        let montres: Vec<usize> = (0..liste.len()).filter(|i| !self.verification.visibles || vus(&liste[*i])).collect();
        ui.label(tr!("{} problème(s){}", "{} problem(s){}", montres.len(), if montres.len() < liste.len() { tr!(" visibles sur {}", " visible out of {}", liste.len()) } else { String::new() }));
        let mut aller = None;
        let place_budgets = if self.verification.budgets.is_some() { 190.0 } else { 0.0 };
        egui::ScrollArea::vertical().id_salt("verification").max_height(ui.available_height() - 40.0 - place_budgets).auto_shrink(false).show(ui, |ui| {
            for &i in &montres {
                let p = &liste[i];
                let couleur = match p.grave {
                    2 => egui::Color32::from_rgb(255, 120, 100),
                    1 => egui::Color32::from_rgb(240, 200, 90),
                    _ => ui.visuals().weak_text_color(),
                };
                let texte = egui::RichText::new(tr!("{} : {}", "{}: {}", p.genre, p.message)).color(couleur);
                let r = ui.add(egui::Button::selectable(self.verification.choisi == Some(i), texte).wrap());
                if r.double_clicked() {
                    aller = Some(i);
                } else if r.clicked() {
                    self.verification.choisi = Some(i);
                }
            }
        });
        if let Some(i) = aller {
            self.verification.choisi = Some(i);
            self.aller_au_probleme(i);
        }
        let choisi = self.verification.choisi.and_then(|i| liste.get(i).map(|p| (i, p.clone())));
        ui.horizontal(|ui| {
            let objet = choisi.as_ref().map_or(false, |(_, p)| p.objet.is_some() || p.boite.is_some());
            if ui.add_enabled(objet, egui::Button::new(crate::langue::t("Aller à"))).clicked() {
                self.aller_au_probleme(choisi.as_ref().unwrap().0);
            }
            let corr = choisi.as_ref().and_then(|(i, p)| p.correction.clone().map(|c| (*i, c)));
            if ui.add_enabled(corr.is_some(), egui::Button::new(crate::langue::t("Corriger"))).clicked() {
                let (i, c) = corr.unwrap();
                self.executer(c);
                if let Some(l) = &mut self.verification.liste {
                    l.remove(i);
                }
                self.verification.choisi = None;
            }
            let genre = choisi.as_ref().filter(|(_, p)| p.correction.is_some()).map(|(_, p)| p.genre.clone());
            if ui.add_enabled(genre.is_some(), egui::Button::new(crate::langue::t("Tout corriger (ce type)"))).clicked() {
                let g = genre.unwrap();
                let commandes: Vec<Commande> = liste.iter().filter(|p| p.genre == g).filter_map(|p| p.correction.clone()).collect();
                self.executer(Commande::Lot { nom: tr!("corriger : {g}", "fix: {g}"), commandes });
                if let Some(l) = &mut self.verification.liste {
                    l.retain(|p| p.genre != g || p.correction.is_none());
                }
                self.verification.choisi = None;
            }
        });
        if let Some(b) = &self.verification.budgets {
            ui.separator();
            ui.strong(format!("Budgets{}", if b.log.is_some() { "" } else { crate::langue::t(" (limites après une compilation complète)") }));
            egui::ScrollArea::vertical().id_salt("budgets").max_height(160.0).show(ui, |ui| {
                egui::Grid::new("budgets").striped(true).num_columns(3).show(ui, |ui| {
                    for l in &b.lignes {
                        ui.label(&l.libelle);
                        ui.label(l.valeur.to_string());
                        match l.limite.filter(|m| *m > 0) {
                            Some(m) => {
                                let f = l.valeur as f64 / m as f64;
                                let c = if f > 1.0 { egui::Color32::from_rgb(255, 120, 100) } else if f >= 0.9 { egui::Color32::from_rgb(240, 200, 90) } else { ui.visuals().text_color() };
                                ui.colored_label(c, tr!("{:.0} % de {m}", "{:.0} % of {m}", f * 100.0));
                            }
                            None => {
                                ui.weak("—");
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        }
    }
}
