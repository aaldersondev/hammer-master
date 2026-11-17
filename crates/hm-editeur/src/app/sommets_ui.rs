//! Outil sommets (Maj+V) : les sommets (blancs) et milieux d'arêtes (jaunes) des solides sélectionnés, dans les vues
//! 2D. Clic : choisir (Ctrl : ajouter) ; glisser un point choisi : déplacer tous les points choisis, calés sur la
//! grille ; glisser dans le vide : cadre de choix ; Ctrl+F avec deux points d'une face : la couper ; Maj+V encore :
//! sommets et arêtes, sommets seuls, arêtes seules. Chaque glisser devient une commande `modifier_sommets` du noyau,
//! refusée (et dite) si une face n'est plus plane.
use egui::{Color32, Pos2, Rect, Stroke, StrokeKind};
use glam::{DVec3, Vec2, Vec3};
use hm_doc::sommets::{Maillage, OpSommet};
use hm_doc::{Commande, Ref};
use hm_render::lignes::Sommet;

use super::{Editeur, Outil};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeSommets {
    Tous,
    Sommets,
    Aretes,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Poignee {
    Sommet(usize),
    Arete(usize, usize),
}

pub struct SolideEdite {
    pub id: u64,
    pub maillage: Maillage,
    /// Opérations faites mais pas encore écrites (une face coupée qu'on n'a pas encore pliée).
    pub en_attente: Vec<OpSommet>,
}

pub struct EtatSommets {
    pub solides: Vec<SolideEdite>,
    pub choisis: Vec<(usize, Poignee)>,
    pub mode: ModeSommets,
    /// Glisser en cours : (vue, point saisi dans le monde, déplacement calé).
    pub glisse: Option<(usize, Vec3, Vec3)>,
    pub cadre: Option<(usize, Pos2, Pos2)>,
    /// La sélection et la version du document d'où viennent les maillages.
    source: (Vec<Ref>, u64),
}

impl Default for EtatSommets {
    fn default() -> Self {
        EtatSommets { solides: Vec::new(), choisis: Vec::new(), mode: ModeSommets::Tous, glisse: None, cadre: None, source: (Vec::new(), u64::MAX) }
    }
}

impl EtatSommets {
    fn position(&self, s: usize, p: Poignee) -> DVec3 {
        let m = &self.solides[s].maillage;
        match p {
            Poignee::Sommet(i) => m.sommets[i],
            Poignee::Arete(a, b) => (m.sommets[a] + m.sommets[b]) * 0.5,
        }
    }

    /// Toutes les poignées visibles dans le mode courant.
    fn poignees(&self) -> Vec<(usize, Poignee)> {
        let mut out = Vec::new();
        for (k, s) in self.solides.iter().enumerate() {
            if self.mode != ModeSommets::Aretes {
                out.extend((0..s.maillage.sommets.len()).map(|i| (k, Poignee::Sommet(i))));
            }
            if self.mode != ModeSommets::Sommets {
                out.extend(s.maillage.aretes().into_iter().map(|(a, b)| (k, Poignee::Arete(a, b))));
            }
        }
        out
    }
}

impl Editeur {
    /// Refait les maillages quand la sélection ou le document a changé.
    pub(super) fn preparer_sommets(&mut self) {
        if self.outil != Outil::Sommets {
            if !self.sommets.solides.is_empty() {
                if self.sommets.solides.iter().any(|s| !s.en_attente.is_empty()) {
                    self.statut = crate::langue::t("outil sommets quitté : la face coupée et non pliée est abandonnée").into();
                }
                self.sommets = Default::default();
            }
            return;
        }
        let source = (self.session.selection.clone(), self.session.version());
        if source == self.sommets.source {
            return;
        }
        let doc = self.session.doc();
        let Ok((mut ids, ents, _)) = hm_doc::resoudre(doc, &self.session.selection) else { return };
        for e in ents {
            if let Some(x) = doc.entite(e) {
                ids.extend(x.solides.iter().map(|s| s.id));
            }
        }
        // les points choisis sont retrouvés par leur position dans les nouveaux maillages
        let anciens: Vec<(u64, DVec3)> = self.sommets.choisis.iter().map(|(k, p)| (self.sommets.solides[*k].id, self.sommets.position(*k, *p))).collect();
        let solides: Vec<SolideEdite> = ids.iter().filter_map(|id| doc.solide(*id).map(|s| SolideEdite { id: *id, maillage: Maillage::de(s), en_attente: Vec::new() })).collect();
        let mode = self.sommets.mode;
        self.sommets = EtatSommets { solides, choisis: Vec::new(), mode, glisse: None, cadre: None, source };
        let poignees = self.sommets.poignees();
        for (id, pos) in anciens {
            if let Some(x) = poignees.iter().find(|(k, p)| self.sommets.solides[*k].id == id && (self.sommets.position(*k, *p) - pos).length() < 0.05) {
                self.sommets.choisis.push(*x);
            }
        }
    }

    /// Maj+V : choisit l'outil sommets, puis change de mode.
    pub(super) fn basculer_sommets(&mut self) {
        if self.outil != Outil::Sommets {
            self.outil = Outil::Sommets;
        } else {
            self.sommets.mode = match self.sommets.mode {
                ModeSommets::Tous => ModeSommets::Sommets,
                ModeSommets::Sommets => ModeSommets::Aretes,
                ModeSommets::Aretes => ModeSommets::Tous,
            };
            self.sommets.choisis.clear();
        }
        self.statut = tr!("sommets : {}", "vertices: {}",
            match self.sommets.mode {
                ModeSommets::Tous => crate::langue::t("sommets et arêtes"),
                ModeSommets::Sommets => crate::langue::t("sommets seuls"),
                ModeSommets::Aretes => crate::langue::t("arêtes seules"),
            }
        );
    }

    /// La poignée sous un point de l'écran d'une vue 2D.
    fn poignee_sous(&self, i: usize, p: Pos2) -> Option<(usize, Poignee)> {
        let v = &self.vues[i];
        let (a, b, _) = v.genre.axes();
        let mut meilleur: Option<((usize, Poignee), f32)> = None;
        for (k, q) in self.sommets.poignees() {
            let w = self.sommets.position(k, q);
            let e = v.monde_vers_ecran(Vec2::new(w[a] as f32, w[b] as f32));
            let d = (e - p).length();
            // un point déjà choisi passe devant
            let d = if self.sommets.choisis.contains(&(k, q)) { d - 2.0 } else { d };
            if d <= 6.0 && meilleur.map_or(true, |(_, m)| d < m) {
                meilleur = Some(((k, q), d));
            }
        }
        meilleur.map(|(x, _)| x)
    }

    pub(super) fn gestes_sommets(&mut self, i: usize, rep: &egui::Response, ctx: &egui::Context) {
        let (a, b, _) = self.vues[i].genre.axes();
        let ctrl = ctx.input(|x| x.modifiers.command);
        if rep.drag_started_by(egui::PointerButton::Primary) {
            let Some(d) = ctx.input(|x| x.pointer.press_origin()) else { return };
            match self.poignee_sous(i, d) {
                Some(h) => {
                    if !self.sommets.choisis.contains(&h) {
                        if !ctrl {
                            self.sommets.choisis.clear();
                        }
                        self.sommets.choisis.push(h);
                    }
                    let w = self.sommets.position(h.0, h.1).as_vec3();
                    self.sommets.glisse = Some((i, w, Vec3::ZERO));
                }
                None => self.sommets.cadre = Some((i, d, d)),
            }
        }
        if rep.dragged_by(egui::PointerButton::Primary) {
            let Some(pos) = rep.interact_pointer_pos() else { return };
            let wv = self.vues[i].ecran_vers_monde(pos);
            let aimant = self.aimant && !ctx.input(|x| x.modifiers.alt);
            if let Some((vue, saisi, delta)) = self.sommets.glisse.as_mut() {
                if *vue == i {
                    // le point saisi tombe sur la grille
                    let mut cible = *saisi;
                    cible[a] = crate::gestes::aimanter(wv.x, self.grille, aimant);
                    cible[b] = crate::gestes::aimanter(wv.y, self.grille, aimant);
                    *delta = cible - *saisi;
                }
            }
            if let Some((vue, _, fin)) = self.sommets.cadre.as_mut() {
                if *vue == i {
                    *fin = pos;
                }
            }
        }
        if rep.clicked() {
            if let Some(p) = rep.interact_pointer_pos() {
                match self.poignee_sous(i, p) {
                    Some(h) => {
                        if ctrl {
                            match self.sommets.choisis.iter().position(|x| *x == h) {
                                Some(k) => {
                                    self.sommets.choisis.remove(k);
                                }
                                None => self.sommets.choisis.push(h),
                            }
                        } else {
                            self.sommets.choisis = vec![h];
                        }
                    }
                    None if !ctrl => self.sommets.choisis.clear(),
                    None => {}
                }
            }
        }
        if rep.drag_stopped() {
            if let Some((vue, d0, d1)) = self.sommets.cadre.take() {
                let r = Rect::from_two_pos(d0, d1);
                let v = &self.vues[vue];
                let (a, b, _) = v.genre.axes();
                if !ctrl {
                    self.sommets.choisis.clear();
                }
                for (k, q) in self.sommets.poignees() {
                    let w = self.sommets.position(k, q);
                    if r.contains(v.monde_vers_ecran(Vec2::new(w[a] as f32, w[b] as f32))) && !self.sommets.choisis.contains(&(k, q)) {
                        self.sommets.choisis.push((k, q));
                    }
                }
            }
            if let Some((_, _, delta)) = self.sommets.glisse.take() {
                if delta.length() > 1e-4 {
                    self.deplacer_sommets(delta.as_dvec3());
                }
            }
        }
    }

    /// Écrit le déplacement des points choisis (avec ce qui attendait : une face coupée) : une commande par solide.
    /// « Vertex Scale » de Hammer++ (Alt+E) : les sommets choisis s'écartent du centre de leur groupe (facteur `k`),
    /// chaque solide en une commande, le tout en une étape.
    pub(super) fn echelle_sommets(&mut self, k: f64) {
        let tous: Vec<DVec3> = self.sommets.choisis.iter().map(|(x, p)| self.sommets.position(*x, *p)).collect();
        if tous.is_empty() {
            self.statut = crate::langue::t("aucun sommet choisi (outil sommets)").into();
            return;
        }
        let c = tous.iter().fold(DVec3::ZERO, |a, p| a + *p) / tous.len() as f64;
        let mut cmds = Vec::new();
        for (n, s) in self.sommets.solides.iter().enumerate() {
            let mut operations = s.en_attente.clone();
            for (x, p) in self.sommets.choisis.iter().filter(|(x, _)| *x == n) {
                let q = self.sommets.position(*x, *p);
                operations.push(OpSommet::Deplacer { points: vec![q.to_array()], delta: ((c + (q - c) * k) - q).to_array() });
            }
            if operations.len() > s.en_attente.len() {
                cmds.push(Commande::ModifierSommets { solide: s.id, operations });
            }
        }
        self.executer(Commande::Lot { nom: tr!("sommets à l'échelle ×{k}", "vertices scaled ×{k}"), commandes: cmds });
    }

    fn deplacer_sommets(&mut self, delta: DVec3) {
        let mut cmds = Vec::new();
        for (k, s) in self.sommets.solides.iter().enumerate() {
            let points: Vec<[f64; 3]> = self.sommets.choisis.iter().filter(|(x, _)| *x == k).map(|(x, p)| self.sommets.position(*x, *p).to_array()).collect();
            if points.is_empty() {
                continue;
            }
            let mut operations = s.en_attente.clone();
            operations.push(OpSommet::Deplacer { points, delta: delta.to_array() });
            cmds.push(Commande::ModifierSommets { solide: s.id, operations });
        }
        let c = match cmds.len() {
            0 => return,
            1 => cmds.pop().unwrap(),
            _ => Commande::Lot { nom: crate::langue::t("modifier des sommets").into(), commandes: cmds },
        };
        let nom = c.nom();
        match self.session.executer(c) {
            Ok(_) => {
                self.statut = tr!("fait : {nom}", "done: {nom}");
                // les maillages sont refaits depuis le document ; les points choisis suivent leur déplacement
                let decales: Vec<(u64, DVec3)> = self.sommets.choisis.iter().map(|(k, p)| (self.sommets.solides[*k].id, self.sommets.position(*k, *p) + delta)).collect();
                for s in &mut self.sommets.solides {
                    s.en_attente.clear();
                    if let Some(x) = self.session.doc().solide(s.id) {
                        s.maillage = Maillage::de(x);
                    }
                }
                self.sommets.source = (self.session.selection.clone(), self.session.version());
                let poignees = self.sommets.poignees();
                self.sommets.choisis = decales
                    .iter()
                    .filter_map(|(id, pos)| poignees.iter().find(|(k, p)| self.sommets.solides[*k].id == *id && (self.sommets.position(*k, *p) - *pos).length() < 0.05).copied())
                    .collect();
            }
            Err(e) => self.erreur = Some(tr!("sommets refusés : {e}", "vertices refused: {e}")),
        }
    }

    /// Ctrl+F : coupe la face qui porte les deux points choisis (sommets ou milieux d'arêtes) ; la coupe est écrite
    /// au prochain déplacement (une face coupée et pas pliée laisserait deux faces dans le même plan).
    pub(super) fn couper_face_sommets(&mut self) {
        if self.outil != Outil::Sommets {
            return;
        }
        let c = &self.sommets.choisis;
        if c.len() != 2 || c[0].0 != c[1].0 {
            self.statut = crate::langue::t("couper une face : choisir deux points (sommets ou milieux d'arêtes) d'une même face").into();
            return;
        }
        let k = c[0].0;
        let (pa, pb) = (self.sommets.position(k, c[0].1), self.sommets.position(k, c[1].1));
        let op = OpSommet::CouperFace { a: pa.to_array(), b: pb.to_array() };
        let s = &mut self.sommets.solides[k];
        let mut m = s.maillage.clone();
        match m.appliquer(&op) {
            Ok(()) => {
                s.maillage = m;
                s.en_attente.push(op);
                // les deux points restent choisis (ils sont devenus des sommets)
                let poignees = self.sommets.poignees();
                self.sommets.choisis = [pa, pb].iter().filter_map(|p| poignees.iter().find(|(x, q)| *x == k && matches!(q, Poignee::Sommet(_)) && (self.sommets.position(*x, *q) - *p).length() < 0.05).copied()).collect();
                self.statut = crate::langue::t("face coupée : déplacer les points de la coupe pour la plier (écrite au déplacement)").into();
            }
            Err(e) => self.erreur = Some(tr!("couper la face : {e}", "split the face: {e}")),
        }
    }

    /// Les arêtes des solides édités, avec le déplacement en cours sur les points choisis (aperçu).
    pub(super) fn apercu_sommets(&self) -> Vec<Sommet> {
        let mut out = Vec::new();
        if self.outil != Outil::Sommets {
            return out;
        }
        let delta = self.sommets.glisse.map_or(DVec3::ZERO, |(_, _, d)| d.as_dvec3());
        for (k, s) in self.sommets.solides.iter().enumerate() {
            let mut bouge = vec![false; s.maillage.sommets.len()];
            for (x, p) in &self.sommets.choisis {
                if *x == k {
                    match p {
                        Poignee::Sommet(i) => bouge[*i] = true,
                        Poignee::Arete(a, b) => {
                            bouge[*a] = true;
                            bouge[*b] = true;
                        }
                    }
                }
            }
            let pos = |i: usize| (s.maillage.sommets[i] + if bouge[i] { delta } else { DVec3::ZERO }).as_vec3().to_array();
            for (a, b) in s.maillage.aretes() {
                let c = if bouge[a] || bouge[b] { [255, 220, 60, 255] } else { [255, 255, 255, 255] };
                out.push(Sommet { pos: pos(a), couleur: c });
                out.push(Sommet { pos: pos(b), couleur: c });
            }
        }
        out
    }

    /// Les poignées dans une vue 2D (peintes par-dessus) et le cadre de choix.
    pub(super) fn dessiner_poignees_sommets(&self, i: usize, p: &egui::Painter) {
        if self.outil != Outil::Sommets {
            return;
        }
        let v = &self.vues[i];
        let (a, b, _) = v.genre.axes();
        let delta = self.sommets.glisse.map_or(DVec3::ZERO, |(_, _, d)| d.as_dvec3());
        // les points choisis en dernier : un point choisi caché sous un autre au même endroit reste visible
        let mut toutes = self.sommets.poignees();
        toutes.sort_by_key(|h| self.sommets.choisis.contains(h));
        for (k, q) in toutes {
            let choisi = self.sommets.choisis.contains(&(k, q));
            let w = self.sommets.position(k, q) + if choisi { delta } else { DVec3::ZERO };
            let e = v.monde_vers_ecran(Vec2::new(w[a] as f32, w[b] as f32));
            let fond = match (choisi, q) {
                (true, _) => Color32::from_rgb(255, 50, 50),
                (false, Poignee::Sommet(_)) => Color32::WHITE,
                (false, Poignee::Arete(..)) => Color32::from_rgb(255, 220, 60),
            };
            let r = Rect::from_center_size(e, egui::vec2(6.0, 6.0));
            p.rect_filled(r, 0.0, fond);
            p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);
        }
        if let Some((vue, d0, d1)) = self.sommets.cadre {
            if vue == i {
                p.rect_stroke(Rect::from_two_pos(d0, d1), 0.0, Stroke::new(1.0, Color32::from_rgb(255, 220, 60)), StrokeKind::Middle);
            }
        }
    }
}
