//! Le pont avec le serveur MCP (`hm_doc::pont`) : la fenêtre sert les demandes de Claude entre deux images, sur sa
//! propre session. Une commande de Claude est une étape de l'historique comme une autre (Ctrl+Z l'annule) ; ce
//! qu'elle crée est sélectionné, comme ce que crée la souris.
use glam::DVec3;
use hm_doc::pont::{self, Demande, Reponse};

use super::Editeur;

impl Editeur {
    pub(super) fn servir_pont(&mut self, ctx: &egui::Context) {
        if !self.pont_lance {
            self.pont_lance = true;
            let c = ctx.clone();
            match pont::ecouter(move || c.request_repaint()) {
                Ok(e) => self.pont = Some(e),
                Err(e) => self.statut = tr!("lien avec Claude (MCP) impossible : {e}", "link with Claude (MCP) failed: {e}"),
            }
        }
        // jamais au milieu d'un geste : la demande attend qu'il soit fini
        if self.geste.is_some() {
            return;
        }
        let Some(p) = &self.pont else { return };
        let recues: Vec<pont::Recue> = p.demandes.try_iter().collect();
        for (d, rep) in recues {
            let r = self.repondre(d);
            let _ = rep.send(r);
        }
    }

    fn repondre(&mut self, d: Demande) -> Reponse {
        let mut r = Reponse::default();
        match d {
            Demande::Etat => {}
            Demande::Document => r.vmf = Some(String::from_utf8_lossy(&self.session.doc().ecrire()).into_owned()),
            Demande::Commande { commande } => {
                let nom = commande.nom();
                match self.session.executer(commande) {
                    Ok(x) => {
                        self.statut = tr!("Claude : {nom}{}", "Claude: {nom}{}", if x.message.is_empty() { String::new() } else { format!(" ({})", x.message) });
                        if !x.crees.is_empty() {
                            self.session.selection = x.crees.clone();
                        }
                        (r.crees, r.message, r.faits) = (x.crees, x.message, vec![nom]);
                    }
                    Err(e) => r.erreur = Some(e),
                }
            }
            Demande::Annuler { n, retablir } => {
                for _ in 0..n.max(1) {
                    match if retablir { self.session.retablir() } else { self.session.annuler() } {
                        Some(nom) => r.faits.push(nom),
                        None => break,
                    }
                }
                if r.faits.is_empty() {
                    r.erreur = Some(tr!("rien à {}", "nothing to {}", if retablir { crate::langue::t("rétablir") } else { crate::langue::t("annuler") }));
                } else {
                    self.statut = tr!("Claude : {} {}", "Claude: {} {}", if retablir { crate::langue::t("rétabli") } else { crate::langue::t("annulé") }, r.faits.join(", "));
                }
            }
            Demande::Historique => {
                let h = self.session.historique();
                r.faits = h.faits().iter().map(|c| c.nom.clone()).collect();
                r.annules = h.annules().iter().rev().map(|c| c.nom.clone()).collect();
            }
            Demande::Enregistrer { chemin } => match self.session.enregistrer(chemin.as_deref()) {
                Ok(p) => {
                    self.statut = tr!("Claude : enregistré {}", "Claude: saved {}", p.display());
                    self.nom_d_apres(&p);
                    r.faits = vec![p.display().to_string()];
                }
                Err(e) => r.erreur = Some(e),
            },
            Demande::Compiler { mode, zone, copier, embarquer, lancer_jeu, partielle } => {
                if self.compil.is_some() {
                    r.erreur = Some(crate::langue::t("une compilation est déjà en cours dans la fenêtre").into());
                } else if self.outils.is_none() {
                    r.erreur = Some(crate::langue::t("Garry's Mod introuvable : pas de vbsp, vvis, vrad").into());
                } else {
                    match hm_compile::Mode::lire(&mode) {
                        Some(m) => {
                            (self.mode_compile, self.copier_bsp, self.embarquer, self.lancer_jeu, self.compil_partielle) = (m, copier, embarquer, lancer_jeu, partielle);
                            self.zone_compil = zone.map(|[lo, hi]| (DVec3::from(lo), DVec3::from(hi)));
                            self.derniere_compil = None;
                            self.statut = tr!("Claude : compilation {}", "Claude: compile {}", m.nom());
                            self.demarrer_compilation();
                        }
                        None => r.erreur = Some(tr!("mode inconnu « {mode} » : geometrie, rapide, normale ou finale", "unknown mode \"{mode}\": geometrie, rapide, normale or finale")),
                    }
                }
            }
            Demande::Montrer { objets } => {
                let boites: Vec<(DVec3, DVec3)> = objets.iter().filter_map(|o| self.boite_objet(*o)).collect();
                if boites.is_empty() {
                    r.erreur = Some(crate::langue::t("aucun de ces objets n'existe dans la map de la fenêtre").into());
                } else {
                    let lo = boites.iter().fold(DVec3::INFINITY, |a, b| a.min(b.0));
                    let hi = boites.iter().fold(DVec3::NEG_INFINITY, |a, b| a.max(b.1));
                    self.session.selection = objets;
                    self.cadrer_sur(lo, hi);
                }
            }
        }
        r.carte = self.session.chemin().map(|p| p.to_path_buf());
        r.version = self.session.version();
        r.modifiee = self.session.modifiee();
        r.selection = self.session.selection.clone();
        r.etapes = self.session.historique().faits().len();
        let fin = self.derniere_compil.clone();
        r.compilation = pont::EtatCompilation {
            en_cours: self.compil.is_some(),
            reussi: fin.as_ref().map(|f| f.0),
            secondes: fin.as_ref().map_or(0.0, |f| f.1),
            erreur: fin.as_ref().and_then(|f| f.2.clone()),
            bsp: fin.and_then(|f| f.3),
            journal: self.journal.iter().rev().take(40).rev().map(|(l, _)| l.clone()).collect(),
        };
        r
    }
}
