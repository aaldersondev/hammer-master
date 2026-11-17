//! Outils de brosses dans la fenêtre : découpe (Maj+X : tracer une ligne dans une vue 2D, Maj+X change ce qu'on
//! garde, Entrée coupe), primitives de l'outil bloc (aperçu de la forme dans la boîte tracée), fusion, creusement,
//! solides invalides. Les calculs sont ceux du noyau (`hm_doc::brosses`) ; la fenêtre ne fait que les montrer et
//! envoyer les commandes.
use glam::{DVec3, Vec2, Vec3};
use hm_doc::brosses::{self, Arche, Gabarit, Plan};
use hm_doc::{Commande, Forme, Garder, Ref};
use hm_render::lignes::Sommet;

use super::{Editeur, Outil};

/// La forme que trace l'outil bloc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormeBloc {
    Bloc,
    Cylindre,
    Pointe,
    Sphere,
    Coin,
    Arche,
    Escalier,
}

impl FormeBloc {
    pub const TOUTES: [FormeBloc; 7] = [FormeBloc::Bloc, FormeBloc::Cylindre, FormeBloc::Pointe, FormeBloc::Sphere, FormeBloc::Coin, FormeBloc::Arche, FormeBloc::Escalier];

    pub fn nom(self) -> &'static str {
        match self {
            FormeBloc::Bloc => "bloc",
            FormeBloc::Cylindre => "cylindre",
            FormeBloc::Pointe => "pointe",
            FormeBloc::Sphere => "sphère",
            FormeBloc::Coin => "coin",
            FormeBloc::Arche => "arche",
            FormeBloc::Escalier => "escalier",
        }
    }
}

/// Réglages des outils de brosses.
pub struct ReglagesBrosses {
    /// Ligne de coupe tracée : (vue, point a, point b), points du monde dans les axes de la vue.
    pub decoupe: Option<(usize, Vec3, Vec3)>,
    pub garder: Garder,
    pub forme: FormeBloc,
    pub cotes: usize,
    pub arche: Arche,
    pub marches: usize,
    pub sens: u8,
}

impl Default for ReglagesBrosses {
    fn default() -> Self {
        ReglagesBrosses {
            decoupe: None,
            garder: Garder::LesDeux,
            forme: FormeBloc::Bloc,
            cotes: 8,
            arche: Arche { largeur_mur: 32.0, cotes: 8, arc: 360.0, depart: 0.0, hauteur_ajoutee: 0.0 },
            marches: 8,
            sens: 0,
        }
    }
}

impl ReglagesBrosses {
    pub fn forme(&self) -> Option<Forme> {
        Some(match self.forme {
            FormeBloc::Bloc => return None,
            FormeBloc::Cylindre => Forme::Cylindre { cotes: self.cotes },
            FormeBloc::Pointe => Forme::Pointe { cotes: self.cotes },
            FormeBloc::Sphere => Forme::Sphere { cotes: self.cotes },
            FormeBloc::Coin => Forme::Coin,
            FormeBloc::Arche => Forme::Arche(self.arche),
            FormeBloc::Escalier => Forme::Escalier { marches: self.marches, sens: self.sens },
        })
    }

    pub fn nom_garder(&self) -> &'static str {
        match self.garder {
            Garder::LesDeux => "les deux côtés",
            Garder::Devant => "devant (côté gauche de la ligne)",
            Garder::Derriere => "derrière (côté droit de la ligne)",
        }
    }
}

/// Les arêtes d'un solide fait de plans (aperçu), en sommets de lignes.
fn aretes_de(plans: &[Plan], couleur: [u8; 4], out: &mut Vec<Sommet>) {
    let g: Vec<Gabarit> = plans.iter().map(|p| Gabarit::neuf(*p, "")).collect();
    let ed = hm_doc::kv::Bloc::nouveau("editor");
    if let Some(s) = brosses::solide_de(0, &g, &mut || 0, &ed, false) {
        aretes_solide(&s, couleur, out);
    }
}

fn aretes_solide(s: &hm_doc::Solide, couleur: [u8; 4], out: &mut Vec<Sommet>) {
    for poly in s.polygones() {
        for k in 0..poly.len() {
            out.push(Sommet { pos: poly[k].as_vec3().to_array(), couleur });
            out.push(Sommet { pos: poly[(k + 1) % poly.len()].as_vec3().to_array(), couleur });
        }
    }
}

impl Editeur {
    /// Le plan de coupe de la ligne tracée : vertical à la vue, normale vers la gauche de la ligne (a → b).
    fn plan_decoupe(&self) -> Option<[[f64; 3]; 3]> {
        let (vue, a, b) = self.brosses.decoupe?;
        let (ia, ib, c) = self.vues.get(vue)?.genre.axes();
        if (b - a).length() < 1e-3 {
            return None;
        }
        let mut e = DVec3::ZERO;
        e[c] = 64.0;
        let (a, b) = (a.as_dvec3(), b.as_dvec3());
        // « devant » : à gauche de la ligne a → b telle qu'on la voit (axe horizontal ia, vertical ib)
        let d = b - a;
        let mut voulue = DVec3::ZERO;
        voulue[ia] = -d[ib];
        voulue[ib] = d[ia];
        let p = [a.to_array(), b.to_array(), (b + e).to_array()];
        let n = Plan::de_points(p.map(DVec3::from))?.n;
        Some(if n.dot(voulue) >= 0.0 { p } else { [b.to_array(), a.to_array(), (a + e).to_array()] })
    }

    /// Les solides concernés par la découpe : ceux de la sélection.
    fn solides_selection(&self) -> Vec<u64> {
        let doc = self.session.doc();
        let Ok((mut s, e, _)) = hm_doc::resoudre(doc, &self.session.selection) else { return Vec::new() };
        for id in e {
            if let Some(ent) = doc.entite(id) {
                s.extend(ent.solides.iter().map(|x| x.id));
            }
        }
        s
    }

    /// Glisser dans une vue 2D avec l'outil de découpe : trace la ligne de coupe.
    pub(super) fn gestes_decoupe(&mut self, i: usize, rep: &egui::Response, ctx: &egui::Context) {
        let v = &self.vues[i];
        let (a, b, _) = v.genre.axes();
        let aimant = self.aimant && !ctx.input(|x| x.modifiers.alt);
        let point = |w: Vec2| {
            let mut p = Vec3::ZERO;
            p[a] = crate::gestes::aimanter(w.x, self.grille, aimant);
            p[b] = crate::gestes::aimanter(w.y, self.grille, aimant);
            p
        };
        if rep.drag_started_by(egui::PointerButton::Primary) {
            if let Some(d) = ctx.input(|x| x.pointer.press_origin()) {
                let p = point(v.ecran_vers_monde(d));
                self.brosses.decoupe = Some((i, p, p));
            }
        }
        if rep.dragged_by(egui::PointerButton::Primary) {
            if let (Some(pos), Some((vue, _, fin))) = (rep.interact_pointer_pos(), self.brosses.decoupe.as_mut()) {
                if *vue == i {
                    *fin = point(v.ecran_vers_monde(pos));
                }
            }
        }
        if rep.drag_stopped() {
            if let Some((_, a, b)) = self.brosses.decoupe {
                if (b - a).length() < 1e-3 {
                    self.brosses.decoupe = None;
                } else {
                    self.statut = format!("découpe : on garde {} — Maj+X pour changer, Entrée pour couper, Échap pour abandonner", self.brosses.nom_garder());
                }
            }
        }
    }

    /// Maj+X : choisit l'outil de découpe, puis change ce qu'on garde (les deux, devant, derrière), comme Hammer.
    pub(super) fn basculer_decoupe(&mut self) {
        if self.outil != Outil::Decoupe {
            self.outil = Outil::Decoupe;
        } else {
            self.brosses.garder = match self.brosses.garder {
                Garder::LesDeux => Garder::Devant,
                Garder::Devant => Garder::Derriere,
                Garder::Derriere => Garder::LesDeux,
            };
        }
        self.statut = format!("découpe : on garde {}", self.brosses.nom_garder());
    }

    /// Entrée avec l'outil de découpe : la sélection est coupée.
    pub(super) fn valider_decoupe(&mut self) {
        let Some(plan) = self.plan_decoupe() else { return };
        if self.session.selection.is_empty() {
            self.statut = "découpe : rien de sélectionné".into();
            return;
        }
        let objets = self.session.selection.clone();
        let materiau = self.materiau.clone();
        self.executer(Commande::Decouper { objets, plan, garder: self.brosses.garder, materiau });
        self.brosses.decoupe = None;
    }

    /// Entrée avec l'outil bloc : le bloc tracé, ou la primitive choisie.
    pub(super) fn creer_bloc_trace(&mut self) {
        let Some((lo, hi)) = self.bloc.take() else { return };
        let m = self.materiau.clone();
        let (min, max) = (lo.as_dvec3().to_array(), hi.as_dvec3().to_array());
        match self.brosses.forme() {
            None => self.executer(Commande::CreerBloc { min, max, materiau: m, classe: None }),
            Some(forme) => self.executer(Commande::Primitive { forme, min, max, materiau: m, classe: None }),
        }
    }

    /// Ctrl+Maj+M : fusionne la sélection en un solide.
    pub(super) fn fusionner(&mut self) {
        let objets = self.session.selection.clone();
        self.executer(Commande::Fusionner { objets });
    }

    /// Ctrl+Maj+C : creuse les solides que touche la sélection.
    pub(super) fn creuser(&mut self) {
        if self.session.selection.is_empty() {
            self.statut = "creuser : sélectionner le solide qui creuse".into();
            return;
        }
        let outils = self.session.selection.clone();
        self.executer(Commande::Creuser { outils, cibles: None });
    }

    pub(super) fn selectionner_invalides(&mut self) {
        let inv = self.session.doc().solides_invalides();
        let doc = self.session.doc();
        let mut sel: Vec<Ref> = Vec::new();
        for id in &inv {
            let r = match doc.proprio(*id) {
                Some(hm_doc::Proprio::Entite(e)) => Ref::Entite(e),
                _ => Ref::Solide(*id),
            };
            if !sel.contains(&r) {
                sel.push(r);
            }
        }
        self.statut = if inv.is_empty() { "aucun solide invalide".into() } else { format!("{} solide(s) invalide(s) sélectionné(s) : vbsp les rejette", inv.len()) };
        self.session.selection = sel;
    }

    /// Aperçu des outils de brosses : les morceaux de la découpe (gardés en jaune, jetés en rouge sombre), la ligne
    /// de coupe, et la primitive dans la boîte de l'outil bloc.
    pub(super) fn apercu_brosses(&self, boite_bloc: Option<(Vec3, Vec3)>) -> Vec<Sommet> {
        let mut out = Vec::new();
        if self.outil == Outil::Decoupe {
            if let Some((_, a, b)) = self.brosses.decoupe {
                out.push(Sommet { pos: a.to_array(), couleur: [80, 220, 255, 255] });
                out.push(Sommet { pos: b.to_array(), couleur: [80, 220, 255, 255] });
            }
            if let Some(p) = self.plan_decoupe().and_then(|p| Plan::de_points(p.map(DVec3::from))) {
                let texture = Gabarit::neuf(p, "").texture;
                for id in self.solides_selection() {
                    let Some(s) = self.session.doc().solide(id) else { continue };
                    if !brosses::coupe(s, &p) {
                        continue;
                    }
                    let (derriere, devant) = brosses::couper(s, &p, &texture);
                    let ed = hm_doc::kv::Bloc::nouveau("editor");
                    for (g, garde) in [(derriere, self.brosses.garder != Garder::Devant), (devant, self.brosses.garder != Garder::Derriere)] {
                        if let Some(x) = brosses::solide_de(0, &g, &mut || 0, &ed, false) {
                            aretes_solide(&x, if garde { [255, 220, 60, 255] } else { [150, 50, 50, 255] }, &mut out);
                        }
                    }
                }
            }
        }
        if self.outil == Outil::Bloc {
            if let (Some((lo, hi)), Some(f)) = (boite_bloc, self.brosses.forme()) {
                if (hi - lo).min_element() >= 1.0 {
                    if let Ok(pieces) = f.pieces(lo.as_dvec3(), hi.as_dvec3()) {
                        for p in pieces {
                            aretes_de(&p, [255, 255, 255, 255], &mut out);
                        }
                    }
                }
            }
        }
        out
    }

    /// Sous les outils, à gauche : la forme de l'outil bloc et ses réglages, ou le mode de la découpe.
    pub(super) fn reglages_outil(&mut self, ui: &mut egui::Ui) {
        let r = &mut self.brosses;
        match self.outil {
            Outil::Bloc => {
                ui.label("Forme");
                egui::ComboBox::from_id_salt("forme_bloc").width(70.0).selected_text(r.forme.nom()).show_ui(ui, |ui| {
                    for f in FormeBloc::TOUTES {
                        ui.selectable_value(&mut r.forme, f, f.nom());
                    }
                });
                let champ = |ui: &mut egui::Ui, nom: &str, v: &mut f64, lo: f64, hi: f64| {
                    ui.label(egui::RichText::new(nom).small());
                    ui.add(egui::DragValue::new(v).range(lo..=hi).speed(1.0));
                };
                match r.forme {
                    FormeBloc::Cylindre | FormeBloc::Pointe | FormeBloc::Sphere => {
                        ui.label(egui::RichText::new("côtés").small());
                        ui.add(egui::DragValue::new(&mut r.cotes).range(3..=64));
                    }
                    FormeBloc::Escalier => {
                        ui.label(egui::RichText::new("marches").small());
                        ui.add(egui::DragValue::new(&mut r.marches).range(1..=256));
                        ui.label(egui::RichText::new("monte vers").small());
                        egui::ComboBox::from_id_salt("sens_escalier").width(70.0).selected_text(["+X", "+Y", "−X", "−Y"][r.sens as usize % 4]).show_ui(ui, |ui| {
                            for (k, n) in ["+X", "+Y", "−X", "−Y"].iter().enumerate() {
                                ui.selectable_value(&mut r.sens, k as u8, *n);
                            }
                        });
                    }
                    FormeBloc::Arche => {
                        ui.label(egui::RichText::new("côtés").small());
                        ui.add(egui::DragValue::new(&mut r.arche.cotes).range(3..=100));
                        champ(ui, "arc °", &mut r.arche.arc, 8.0, 360.0);
                        champ(ui, "départ °", &mut r.arche.depart, -360.0, 360.0);
                        champ(ui, "mur", &mut r.arche.largeur_mur, 1.0, 4096.0);
                        champ(ui, "+ hauteur", &mut r.arche.hauteur_ajoutee, -1024.0, 1024.0);
                    }
                    _ => {}
                }
                if r.forme != FormeBloc::Bloc {
                    ui.label(egui::RichText::new("Entrée : créer").small().weak());
                }
            }
            Outil::Decoupe => {
                ui.label("Garder");
                for (g, n) in [(Garder::LesDeux, "les deux"), (Garder::Devant, "devant"), (Garder::Derriere, "derrière")] {
                    ui.radio_value(&mut r.garder, g, n);
                }
                ui.label(egui::RichText::new("tracer dans une vue 2D, Entrée : couper").small().weak());
            }
            _ => {}
        }
    }
}
