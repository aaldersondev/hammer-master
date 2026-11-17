//! Fenêtre de propriétés d'une entité : chaque clé de sa classe dans le FGD, avec le champ qui convient à son type
//! (texte, nombre, choix, case, drapeaux, couleur), puis les clés que le FGD ne connaît pas. Une modification devient
//! une commande `Cles` du noyau quand le champ est validé.
use std::collections::HashMap;

use hm_doc::{Commande, Document, Entite};
use hm_fgd::{Fgd, Propriete};

#[derive(Default)]
pub struct Proprietes {
    /// Texte en cours de saisie, par (entité, clé).
    tampons: HashMap<(u64, String), String>,
    nouvelle_cle: (String, String),
    pub filtre_classes: String,
}

/// Valeur d'une clé pour un ensemble d'entités : commune, ou différente d'une entité à l'autre.
fn valeur(ents: &[&Entite], cle: &str) -> Option<String> {
    let mut v: Option<Option<&str>> = None;
    for e in ents {
        let x = e.get(cle);
        match v {
            None => v = Some(x),
            Some(y) if y != x => return None,
            _ => {}
        }
    }
    v.flatten().map(String::from).or(Some(String::new()))
}

impl Proprietes {
    /// Le panneau pour les entités `ids` (le monde si la liste est vide). Rend les commandes à exécuter. `faces` : les
    /// numéros des faces choisies à l'outil texture (pour les clés `sidelist`).
    pub fn ui(&mut self, ui: &mut egui::Ui, doc: &Document, fgd: &Fgd, ids: &[u64], faces: &[u64]) -> Vec<Commande> {
        let mut out = Vec::new();
        let monde = ids.is_empty();
        let ents: Vec<&Entite> = if monde { vec![doc.monde()] } else { ids.iter().filter_map(|i| doc.entite(*i)).collect() };
        if ents.is_empty() {
            return out;
        }
        let ids: Vec<u64> = ents.iter().map(|e| e.id).collect();
        let cle_tampon = ids[0];
        let classe_nom = valeur(&ents, "classname").unwrap_or_default();
        let classe = fgd.classe(&classe_nom);
        // fabrique la commande pour toutes les entités choisies
        let poser = |out: &mut Vec<Commande>, k: &str, v: Option<String>| {
            for id in &ids {
                out.push(Commande::Cles { entite: *id, cles: vec![(k.to_string(), v.clone())] });
            }
        };
        ui.horizontal(|ui| {
            ui.strong(if monde { "Monde (worldspawn)".to_string() } else if ents.len() > 1 { format!("{} entités", ents.len()) } else { format!("Entité n° {}", ents[0].id) });
            if classe_nom.is_empty() {
                ui.label("(classes différentes)");
            }
        });
        if !monde {
            ui.horizontal(|ui| {
                ui.label("Classe");
                let mut c = self.tampons.get(&(cle_tampon, "classname".into())).cloned().unwrap_or_else(|| classe_nom.clone());
                let r = ui.add(egui::TextEdit::singleline(&mut c).desired_width(200.0));
                if r.changed() {
                    self.tampons.insert((cle_tampon, "classname".into()), c.clone());
                }
                let mut nouvelle = None;
                // validé par Entrée seulement : cliquer « … » ne doit pas valider un nom à moitié tapé
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && c != classe_nom && !c.trim().is_empty() {
                    nouvelle = Some(c.trim().to_string());
                }
                // les classes du même genre (à brosses ou ponctuelles), filtrées par ce qui est tapé
                let a_brosses = !ents[0].solides.is_empty();
                ui.menu_button("…", |ui| {
                    // tant qu'on n'a rien tapé d'autre que la classe actuelle : toutes les classes
                    let f = c.to_ascii_lowercase();
                    let tape = f != classe_nom.to_ascii_lowercase();
                    let mut liste: Vec<&str> = fgd.classes(a_brosses).into_iter().filter(|k| !tape || k.to_ascii_lowercase().contains(&f)).collect();
                    liste.sort_by_key(|k| k.to_ascii_lowercase());
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for k in liste {
                            if ui.button(k).clicked() {
                                nouvelle = Some(k.to_string());
                                ui.close();
                            }
                        }
                    });
                });
                if let Some(n) = nouvelle {
                    poser(&mut out, "classname", Some(n.clone()));
                    // les clés par défaut de la nouvelle classe qui manquent (comme Hammer au changement de classe)
                    for (k, v) in cles_par_defaut(fgd, &n) {
                        if ents.iter().all(|e| e.get(&k).is_none()) {
                            poser(&mut out, &k, Some(v));
                        }
                    }
                    self.tampons.remove(&(cle_tampon, "classname".into()));
                }
            });
        }
        match &classe {
            Some(c) if !c.description.is_empty() => {
                ui.add(egui::Label::new(egui::RichText::new(c.description.replace("\\n", "\n")).small().weak()).wrap());
            }
            None if !classe_nom.is_empty() => {
                ui.colored_label(egui::Color32::from_rgb(255, 170, 80), format!("« {classe_nom} » n'est dans aucun FGD chargé"));
            }
            _ => {}
        }
        ui.separator();
        let mut connues: Vec<String> = Vec::new();
        egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, true]).max_height(ui.available_height() - 140.0).show(ui, |ui| {
            egui::Grid::new("cles").num_columns(2).striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
                if let Some(c) = &classe {
                    for p in &c.proprietes {
                        connues.push(p.nom.to_ascii_lowercase());
                        if p.nom.eq_ignore_ascii_case("classname") {
                            continue;
                        }
                        let actuel = valeur(&ents, &p.nom);
                        let rep = ui.label(if p.libelle.is_empty() { p.nom.clone() } else { p.libelle.clone() });
                        let aide = format!("{} ({})\n{}{}", p.nom, p.genre, p.description.replace("\\n", "\n"), p.defaut.as_ref().map(|d| format!("\npar défaut : {d}")).unwrap_or_default());
                        rep.on_hover_text(aide);
                        self.champ(ui, p, actuel, cle_tampon, faces, &mut |k, v| poser(&mut out, k, v));
                        ui.end_row();
                    }
                }
                // les clés que le FGD ne décrit pas
                let mut autres: Vec<(String, String)> = Vec::new();
                for (k, _) in &ents[0].cles {
                    let kb = k.to_ascii_lowercase();
                    if kb == "classname" || connues.contains(&kb) || autres.iter().any(|(x, _)| x.eq_ignore_ascii_case(k)) {
                        continue;
                    }
                    autres.push((k.clone(), valeur(&ents, k).unwrap_or_default()));
                }
                for (k, v) in autres {
                    ui.label(egui::RichText::new(&k).italics());
                    ui.horizontal(|ui| {
                        let cle = (cle_tampon, k.clone());
                        let mut t = self.tampons.get(&cle).cloned().unwrap_or_else(|| v.clone());
                        let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(170.0));
                        if r.changed() {
                            self.tampons.insert(cle.clone(), t.clone());
                        }
                        if r.lost_focus() && t != v {
                            poser(&mut out, &k, Some(t));
                            self.tampons.remove(&cle);
                        }
                        if ui.small_button("x").on_hover_text("retirer la clé").clicked() {
                            poser(&mut out, &k, None);
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(s) = ents[0].sorties.as_ref().filter(|s| !s.is_empty()) {
                ui.separator();
                ui.strong(format!("Sorties ({})", s.len()));
                for (k, v) in s {
                    ui.label(egui::RichText::new(format!("{k} > {}", v.replace('\u{1b}', ","))).monospace().small());
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouvelle_cle.0).hint_text("clé").desired_width(90.0));
            ui.add(egui::TextEdit::singleline(&mut self.nouvelle_cle.1).hint_text("valeur").desired_width(130.0));
            if ui.button("Ajouter").clicked() && !self.nouvelle_cle.0.trim().is_empty() {
                let (k, v) = std::mem::take(&mut self.nouvelle_cle);
                poser(&mut out, k.trim(), Some(v));
            }
        });
        out
    }

    /// Le champ d'une clé, selon son type.
    fn champ(&mut self, ui: &mut egui::Ui, p: &Propriete, actuel: Option<String>, cle_tampon: u64, faces: &[u64], poser: &mut dyn FnMut(&str, Option<String>)) {
        let differentes = actuel.is_none();
        let actuel = actuel.unwrap_or_default();
        let affiche = if actuel.is_empty() && !differentes { p.defaut.clone().unwrap_or_default() } else { actuel.clone() };
        match p.genre.as_str() {
            "choices" if !p.choix.is_empty() => {
                let libelle = p.choix.iter().find(|c| c.valeur == affiche).map(|c| c.libelle.clone()).unwrap_or_else(|| if differentes { "(différentes)".into() } else { affiche.clone() });
                // libellé court : la liste ne doit pas élargir le panneau
                let court: String = if libelle.chars().count() > 26 { libelle.chars().take(25).collect::<String>() + "…" } else { libelle };
                egui::ComboBox::from_id_salt(("choix", &p.nom)).selected_text(court).width(190.0).show_ui(ui, |ui| {
                    for c in &p.choix {
                        if ui.selectable_label(c.valeur == affiche, format!("{} : {}", c.valeur, c.libelle)).clicked() && c.valeur != actuel {
                            poser(&p.nom, Some(c.valeur.clone()));
                        }
                    }
                });
            }
            "boolean" => {
                let mut b = affiche.trim() == "1";
                if ui.checkbox(&mut b, if differentes { "(différentes)" } else { "" }).changed() {
                    poser(&p.nom, Some(if b { "1".into() } else { "0".into() }));
                }
            }
            "flags" => {
                let v: i64 = affiche.trim().parse().unwrap_or(0);
                ui.vertical(|ui| {
                    for c in &p.choix {
                        let bit: i64 = c.valeur.trim().parse().unwrap_or(0);
                        let mut b = v & bit != 0;
                        if ui.checkbox(&mut b, &c.libelle).changed() {
                            let n = if b { v | bit } else { v & !bit };
                            poser(&p.nom, Some(n.to_string()));
                        }
                    }
                });
            }
            "color255" | "color1" => {
                let n: Vec<f32> = affiche.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                let echelle = if p.genre == "color1" { 1.0 } else { 255.0 };
                ui.horizontal(|ui| {
                    if n.len() >= 3 {
                        let mut c = [n[0] / echelle, n[1] / echelle, n[2] / echelle];
                        if ui.color_edit_button_rgb(&mut c).changed() {
                            let mut t = format!("{} {} {}", (c[0] * echelle).round(), (c[1] * echelle).round(), (c[2] * echelle).round());
                            if p.genre == "color1" {
                                t = format!("{:.3} {:.3} {:.3}", c[0], c[1], c[2]);
                            }
                            if let Some(l) = n.get(3) {
                                t += &format!(" {l}");
                            }
                            poser(&p.nom, Some(t));
                        }
                    }
                    self.texte(ui, p, &affiche, &actuel, cle_tampon, 140.0, poser);
                });
            }
            // comme « Pick » dans Hammer : les faces choisies à l'outil texture, par leur numéro
            "sidelist" => {
                ui.horizontal(|ui| {
                    self.texte(ui, p, &affiche, &actuel, cle_tampon, 120.0, poser);
                    if ui.add_enabled(!faces.is_empty(), egui::Button::new(format!("faces choisies ({})", faces.len()))).on_hover_text("choisir les faces à l'outil texture (Ctrl+clic pour en ajouter), l'entité restant sélectionnée").clicked() {
                        poser(&p.nom, Some(faces.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(" ")));
                    }
                });
            }
            _ => self.texte(ui, p, &affiche, &actuel, cle_tampon, 200.0, poser),
        }
    }

    fn texte(&mut self, ui: &mut egui::Ui, p: &Propriete, affiche: &str, actuel: &str, cle_tampon: u64, largeur: f32, poser: &mut dyn FnMut(&str, Option<String>)) {
        let cle = (cle_tampon, p.nom.clone());
        let mut t = self.tampons.get(&cle).cloned().unwrap_or_else(|| affiche.to_string());
        let r = ui.add_enabled(!p.lecture_seule, egui::TextEdit::singleline(&mut t).desired_width(largeur));
        if r.changed() {
            self.tampons.insert(cle.clone(), t.clone());
        }
        if r.lost_focus() {
            self.tampons.remove(&cle);
            if t != actuel && !(actuel.is_empty() && Some(t.as_str()) == p.defaut.as_deref()) {
                let ok = match p.genre.as_str() {
                    "integer" => t.trim().is_empty() || t.trim().parse::<i64>().is_ok(),
                    "float" => t.trim().is_empty() || t.trim().parse::<f64>().is_ok(),
                    _ => true,
                };
                if ok {
                    poser(&p.nom, Some(t));
                }
            }
        }
    }
}

/// Les clés d'une entité neuve de cette classe : les valeurs par défaut du FGD (comme Hammer les écrit).
pub fn cles_par_defaut(fgd: &Fgd, classe: &str) -> Vec<(String, String)> {
    let Some(c) = fgd.classe(classe) else { return Vec::new() };
    c.proprietes
        .iter()
        .filter(|p| !matches!(p.nom.to_ascii_lowercase().as_str(), "classname" | "origin" | "targetname" | "parentname"))
        .filter_map(|p| {
            if p.genre == "flags" {
                let v: i64 = p.choix.iter().filter(|c| c.defaut).filter_map(|c| c.valeur.trim().parse::<i64>().ok()).sum();
                return Some((p.nom.clone(), v.to_string()));
            }
            p.defaut.clone().map(|d| (p.nom.clone(), d))
        })
        .collect()
}
