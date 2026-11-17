//! La vue logique (« 2D Logical » de Hammer) : les entités nommées ou reliées, en boîtes, et leurs sorties en
//! flèches « Sortie → Entrée ». Les boîtes sont rangées en colonnes : les sources à gauche, chaque cible une colonne
//! plus loin que ce qui la déclenche. Glisser déplace la vue, la molette zoome, un clic sélectionne l'entité et y
//! amène les autres vues.
use std::collections::HashMap;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use hm_doc::Ref;

use super::Editeur;

pub struct EtatLogique {
    pub ouverte: bool,
    decalage: Vec2,
    zoom: f32,
}

impl Default for EtatLogique {
    fn default() -> EtatLogique {
        EtatLogique { ouverte: false, decalage: Vec2::new(20.0, 20.0), zoom: 1.0 }
    }
}

struct Noeud {
    id: u64,
    titre: String,
    classe: String,
    colonne: usize,
    rang: usize,
}

/// Les liens (source, cible, texte) et les boîtes, rangées en colonnes.
fn graphe(doc: &hm_doc::Document) -> (Vec<Noeud>, Vec<(u64, u64, String)>) {
    let mut liens = Vec::new();
    for e in doc.entites() {
        for s in e.sorties_lues() {
            for c in doc.entites_nommees(&s.cible) {
                liens.push((e.id, c, format!("{} → {}", s.sortie, s.entree)));
            }
        }
    }
    let mut ids: Vec<u64> = doc.entites().filter(|e| e.nom_donne().is_some() || liens.iter().any(|(a, b, _)| *a == e.id || *b == e.id)).map(|e| e.id).collect();
    ids.sort();
    // colonne : un de plus que la plus grande colonne de ce qui la vise (au plus 32 passes : les boucles s'arrêtent)
    let mut col: HashMap<u64, usize> = ids.iter().map(|i| (*i, 0)).collect();
    for _ in 0..32 {
        let mut change = false;
        for (a, b, _) in &liens {
            let (ca, cb) = (col.get(a).copied().unwrap_or(0), col.get(b).copied().unwrap_or(0));
            if a != b && cb < ca + 1 && ca < 31 {
                col.insert(*b, ca + 1);
                change = true;
            }
        }
        if !change {
            break;
        }
    }
    let mut par_colonne: HashMap<usize, usize> = HashMap::new();
    let noeuds = ids
        .iter()
        .filter_map(|i| doc.entite(*i))
        .map(|e| {
            let c = col[&e.id];
            let r = par_colonne.entry(c).or_insert(0);
            *r += 1;
            Noeud { id: e.id, titre: e.nom_donne().unwrap_or(crate::langue::t("(sans nom)")).to_string(), classe: e.classe().to_string(), colonne: c, rang: *r - 1 }
        })
        .collect();
    (noeuds, liens)
}

impl Editeur {
    pub(super) fn fenetre_logique(&mut self, ctx: &egui::Context) {
        if !self.logique.ouverte {
            return;
        }
        let (noeuds, liens) = graphe(self.session.doc());
        let sel: Vec<u64> = self.session.selection.iter().filter_map(|r| if let Ref::Entite(e) = r { Some(*e) } else { None }).collect();
        let (mut ouvert, mut aller) = (true, None::<u64>);
        let l = &mut self.logique;
        super::barres::fenetre(ctx, crate::langue::t("Vue logique")).open(&mut ouvert).resizable(true).default_size([760.0, 480.0]).show(ctx, |ui| {
            ui.label(tr!("{} entité(s), {} lien(s) — glisser : déplacer, molette : zoomer, clic : sélectionner", "{} entity(ies), {} link(s) — drag: move, scroll: zoom, click: select", noeuds.len(), liens.len()));
            let (rect, rep) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
            if rep.dragged() {
                l.decalage += rep.drag_delta();
            }
            if rep.hovered() {
                let m = ui.input(|i| i.smooth_scroll_delta.y);
                if m != 0.0 {
                    l.zoom = (l.zoom * 1.0015f32.powf(m)).clamp(0.2, 3.0);
                }
            }
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 0.0, Color32::from_gray(18));
            let (w, h) = (170.0 * l.zoom, 40.0 * l.zoom);
            let place = |n: &Noeud| Rect::from_min_size(rect.min + l.decalage + Vec2::new(n.colonne as f32 * 340.0, n.rang as f32 * 64.0) * l.zoom, Vec2::new(w, h));
            let boites: HashMap<u64, Rect> = noeuds.iter().map(|n| (n.id, place(n))).collect();
            let texte = FontId::proportional(11.0 * l.zoom.max(0.6));
            for (a, b, t) in &liens {
                let (Some(ra), Some(rb)) = (boites.get(a), boites.get(b)) else { continue };
                let (de, vers) = (ra.right_center(), rb.left_center());
                let c = Color32::from_rgb(200, 120, 255);
                p.line_segment([de, vers], Stroke::new(1.5, c));
                let d = (vers - de).normalized();
                let n = Vec2::new(-d.y, d.x);
                p.add(egui::Shape::convex_polygon(vec![vers, vers - d * 9.0 + n * 4.0, vers - d * 9.0 - n * 4.0], c, Stroke::NONE));
                if l.zoom > 0.5 {
                    let milieu: Pos2 = de + (vers - de) * 0.5;
                    p.text(milieu, Align2::CENTER_BOTTOM, t, texte.clone(), Color32::from_rgb(220, 190, 255));
                }
            }
            for n in &noeuds {
                let r = boites[&n.id];
                let choisie = sel.contains(&n.id);
                p.rect_filled(r, 4.0, if choisie { Color32::from_rgb(110, 40, 40) } else { Color32::from_gray(45) });
                p.rect_stroke(r, 4.0, Stroke::new(1.0, if choisie { Color32::from_rgb(255, 80, 80) } else { Color32::from_gray(110) }), egui::StrokeKind::Inside);
                p.text(r.left_top() + Vec2::new(6.0, 4.0) * l.zoom, Align2::LEFT_TOP, &n.titre, texte.clone(), Color32::WHITE);
                p.text(r.left_bottom() + Vec2::new(6.0, -4.0) * l.zoom, Align2::LEFT_BOTTOM, &n.classe, texte.clone(), Color32::from_gray(170));
            }
            if rep.clicked() {
                if let Some(pos) = rep.interact_pointer_pos() {
                    aller = noeuds.iter().find(|n| boites[&n.id].contains(pos)).map(|n| n.id);
                }
            }
        });
        self.logique.ouverte = ouvert;
        if let Some(id) = aller {
            self.aller_a(Ref::Entite(id), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use hm_doc::io::Sortie;
    use hm_doc::{Commande, Ref, Session};

    /// bouton → relais → porte : trois colonnes ; une entité seule sans nom n'apparaît pas.
    #[test]
    fn colonnes_du_graphe() {
        let mut s = Session::nouvelle();
        let mut id = |s: &mut Session, nom: &str| {
            let cles = if nom.is_empty() { vec![] } else { vec![("targetname".to_string(), nom.to_string())] };
            match s.executer(Commande::CreerEntite { classe: "logic_relay".into(), origine: [0.0; 3], cles }).unwrap().crees[0] {
                Ref::Entite(i) => i,
                _ => unreachable!(),
            }
        };
        let (b, r, _p, _seule) = (id(&mut s, "bouton"), id(&mut s, "relais"), id(&mut s, "porte"), id(&mut s, ""));
        let vers = |c: &str| Sortie { sortie: "OnTrigger".into(), cible: c.into(), entree: "Trigger".into(), parametre: String::new(), delai: 0.0, fois: -1 };
        s.executer(Commande::Sorties { entite: b, sorties: vec![vers("relais")] }).unwrap();
        s.executer(Commande::Sorties { entite: r, sorties: vec![vers("porte")] }).unwrap();
        let (noeuds, liens) = super::graphe(s.doc());
        assert_eq!(liens.len(), 2);
        let col = |t: &str| noeuds.iter().find(|n| n.titre == t).unwrap().colonne;
        assert_eq!((col("bouton"), col("relais"), col("porte")), (0, 1, 2));
        assert_eq!(noeuds.len(), 3);
    }
}
