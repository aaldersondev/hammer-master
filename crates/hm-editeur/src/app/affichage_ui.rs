//! Le menu Affichage de Hammer, suite : liens d'entrées / sorties entre entités (« Show Connections »), noms des
//! entités dans les vues 2D (« Hide Entity Names » pour les cacher), masquage rapide changé en visgroup
//! (« Convert QuickHide Objects to Visgroup ») et sélection déplacée vers un visgroup (« Move Selection To Visgroup »).
use std::collections::HashSet;

use egui::Color32;
use hm_doc::{Commande, Ref};
use hm_render::lignes::Sommet;

use super::Editeur;
use crate::vues::Genre;

#[derive(Default)]
pub struct EtatAffichage {
    pub liens: bool,
    pub cacher_noms: bool,
    /// Traits des liens pour une version du document.
    cache: Option<(u64, Vec<Sommet>)>,
}

impl Editeur {
    /// Un trait de chaque entité vers chaque cible de ses sorties (version du document gardée en cache).
    pub(super) fn lignes_liens(&mut self) -> Vec<Sommet> {
        if !self.affichage.liens {
            return Vec::new();
        }
        let v = self.session.version();
        if self.affichage.cache.as_ref().map_or(true, |(x, _)| *x != v) {
            let doc = self.session.doc();
            let centre = |id: u64| doc.entite(id).and_then(|e| e.origine()).or_else(|| hm_doc::boite_de(doc, &[Ref::Entite(id)]).map(|(lo, hi)| (lo + hi) * 0.5));
            let mut l = Vec::new();
            for e in doc.entites() {
                let Some(a) = centre(e.id) else { continue };
                for s in e.sorties_lues() {
                    for cible in doc.entites_nommees(&s.cible) {
                        if let Some(b) = centre(cible) {
                            l.push(Sommet { pos: a.as_vec3().to_array(), couleur: [200, 120, 255, 255] });
                            l.push(Sommet { pos: b.as_vec3().to_array(), couleur: [200, 120, 255, 255] });
                        }
                    }
                }
            }
            self.affichage.cache = Some((v, l));
        }
        self.affichage.cache.as_ref().unwrap().1.clone()
    }

    /// Le nom (sinon la classe) des entités à l'écran dans la vue 2D `i`, quand on est assez près (300 au plus).
    pub(super) fn dessiner_noms(&self, i: usize, p: &egui::Painter) {
        let v = &self.vues[i];
        if self.affichage.cacher_noms || v.genre == Genre::Camera3D || v.zoom < 0.4 {
            return;
        }
        let Some(geo) = &self.geo else { return };
        let (a, b, _) = v.genre.axes();
        let doc = self.session.doc();
        let mut vus = HashSet::new();
        for f in &geo.formes {
            let Ref::Entite(id) = f.designe.objet else { continue };
            let coin = v.monde_vers_ecran(glam::Vec2::new(f.min[a], f.max[b]));
            if !v.rect.contains(coin) || !vus.insert(id) {
                continue;
            }
            let Some(e) = doc.entite(id) else { continue };
            let texte = e.nom_donne().unwrap_or(e.classe());
            let c = f.couleur;
            p.text(coin + egui::vec2(0.0, -2.0), egui::Align2::LEFT_BOTTOM, texte, egui::FontId::proportional(10.0), Color32::from_rgb(c[0], c[1], c[2]));
            if vus.len() >= 300 {
                break;
            }
        }
    }

    /// « Convert QuickHide Objects to Visgroup » : les objets masqués (H) vont dans un nouveau visgroup décoché.
    pub(super) fn masquage_en_visgroup(&mut self) {
        let objets: Vec<Ref> = self.filtres.masques.iter().copied().collect();
        if objets.is_empty() {
            self.statut = "rien de masqué (H)".into();
            return;
        }
        let n = self.session.doc().visgroups().len() + 1;
        self.executer(Commande::CreerVisgroup { nom: format!("masqués {n}"), objets, cache: true });
        self.reafficher();
    }

    /// Le sous-menu « Déplacer la sélection vers » : un visgroup existant, dont la sélection devient seule membre parmi
    /// les visgroups (retirée des autres).
    pub(super) fn menu_deplacer_vers_visgroup(&mut self, ui: &mut egui::Ui) {
        let sel = self.session.selection.clone();
        let vgs = self.session.doc().visgroups();
        ui.add_enabled_ui(!sel.is_empty() && !vgs.is_empty(), |ui| {
            ui.menu_button("Déplacer la sélection vers le visgroup", |ui| {
                for vg in &vgs {
                    if ui.button(&vg.nom).clicked() {
                        let membres = self.session.doc().membres_visgroups();
                        let mut commandes: Vec<Commande> = vgs
                            .iter()
                            .filter(|x| x.id != vg.id)
                            .filter_map(|x| {
                                let siens: Vec<Ref> = sel.iter().copied().filter(|r| membres.get(&x.id).map_or(false, |m| m.iter().any(|(y, _)| y == r))).collect();
                                (!siens.is_empty()).then(|| Commande::RetirerDuVisgroup { visgroup: x.id, objets: siens })
                            })
                            .collect();
                        commandes.push(Commande::AjouterAuVisgroup { visgroup: vg.id, objets: sel.clone() });
                        self.executer(Commande::Lot { nom: format!("déplacer la sélection vers « {} »", vg.nom), commandes });
                        ui.close();
                    }
                }
            });
        });
    }
}
