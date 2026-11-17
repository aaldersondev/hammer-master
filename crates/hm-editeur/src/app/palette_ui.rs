//! Palette de commandes (Ctrl+Maj+P) : toutes les actions de la table des raccourcis, et les entités de la map par
//! nom ou classe, filtrées à la frappe (tous les mots tapés), les dernières employées en tête. Entrée lance l'action
//! (par le même chemin qu'un raccourci) ou sélectionne l'entité et cadre les vues dessus.
use hm_doc::Ref;

use super::Editeur;
use crate::raccourcis::ACTIONS;

#[derive(Default)]
pub struct Palette {
    pub ouverte: bool,
    texte: String,
    choisi: usize,
    /// Les dernières actions lancées, la plus récente d'abord.
    recents: Vec<&'static str>,
}

#[derive(Clone, Copy)]
enum Choix {
    Action(&'static str),
    Entite(u64),
}

impl Editeur {
    pub(super) fn ouvrir_palette(&mut self) {
        self.palette.ouverte = true;
        self.palette.texte.clear();
        self.palette.choisi = 0;
    }

    fn resultats_palette(&self) -> Vec<(Choix, String)> {
        let t = self.palette.texte.to_lowercase();
        let mots: Vec<&str> = t.split_whitespace().collect();
        let va = |s: &str| {
            let s = s.to_lowercase();
            mots.iter().all(|m| s.contains(m))
        };
        let mut v: Vec<(Choix, String)> = Vec::new();
        let mut actions: Vec<&(&'static str, &str, &[&str])> = ACTIONS.iter().filter(|(id, nom, _)| va(&format!("{nom} {id}"))).collect();
        // les récentes d'abord, dans leur ordre
        actions.sort_by_key(|(id, _, _)| self.palette.recents.iter().position(|r| r == id).unwrap_or(usize::MAX));
        for (id, nom, _) in actions {
            let touche = self.rc.texte(id);
            v.push((Choix::Action(id), if touche.is_empty() { nom.to_string() } else { format!("{nom}   ({touche})") }));
        }
        if !mots.is_empty() {
            for e in self.session.doc().entites() {
                let nom = e.get("targetname").unwrap_or("");
                if va(&format!("{nom} {}", e.classe())) {
                    v.push((Choix::Entite(e.id), format!("entité : {}{}", e.classe(), if nom.is_empty() { format!(" n° {}", e.id) } else { format!(" « {nom} »") })));
                }
                if v.len() > 200 {
                    break;
                }
            }
        }
        v
    }

    pub(super) fn palette(&mut self, ctx: &egui::Context) {
        if !self.palette.ouverte {
            return;
        }
        let res = self.resultats_palette();
        let (bas, haut, entree, echap) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if bas {
            self.palette.choisi = (self.palette.choisi + 1).min(res.len().saturating_sub(1));
        }
        if haut {
            self.palette.choisi = self.palette.choisi.saturating_sub(1);
        }
        let mut lancer = None;
        egui::Window::new("Palette de commandes").title_bar(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 60.0]).fixed_size([560.0, 0.0]).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.palette.texte).hint_text("commande, ou nom / classe d'une entité de la map").desired_width(f32::INFINITY));
            r.request_focus();
            if r.changed() {
                self.palette.choisi = 0;
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (k, (c, texte)) in res.iter().enumerate().take(60) {
                    let l = ui.selectable_label(k == self.palette.choisi, texte);
                    if k == self.palette.choisi {
                        l.scroll_to_me(None);
                    }
                    if l.clicked() {
                        lancer = Some(*c);
                    }
                }
                if res.is_empty() {
                    ui.weak("rien ne correspond");
                }
            });
        });
        if entree {
            lancer = res.get(self.palette.choisi).map(|(c, _)| *c);
        }
        if echap || lancer.is_some() {
            self.palette.ouverte = false;
        }
        match lancer {
            Some(Choix::Action(id)) => {
                self.palette.recents.retain(|r| *r != id);
                self.palette.recents.insert(0, id);
                self.declenchee = Some(id);
            }
            Some(Choix::Entite(e)) => self.aller_a(Ref::Entite(e), None),
            None => {}
        }
    }
}
