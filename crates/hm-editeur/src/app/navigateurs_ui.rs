//! Navigateurs de sons et de particules (« Sound Browser », Maj+F12 ; « Particle Browser », Maj+F11 de Hammer++) :
//! la liste se lit une fois dans les fichiers montés du jeu, se filtre à la frappe, et le choix va dans une clé des
//! entités sélectionnées (`message` d'un ambient_generic, `effect_name` d'un info_particle_system). Un son s'écoute :
//! le fichier est copié hors des VPK puis joué par le lecteur multimédia de Windows (wav, mp3).
use hm_doc::Commande;

use super::Editeur;

#[derive(Default)]
pub struct EtatNavigateurs {
    pub sons: bool,
    pub particules: bool,
    /// (nom du son ou chemin du fichier, fichier joué) ; lus à la première ouverture.
    liste_sons: Option<Vec<(String, Option<String>)>>,
    /// (système, fichier .pcf).
    liste_particules: Option<Vec<(String, String)>>,
    filtre_sons: String,
    filtre_particules: String,
    choisi_son: Option<usize>,
    choisi_particule: Option<usize>,
    cle_son: String,
    lecteur: Option<std::process::Child>,
}

impl Editeur {
    fn lire_sons(&self) -> Vec<(String, Option<String>)> {
        let mut v: Vec<(String, Option<String>)> = Vec::new();
        if let Some(m) = self.vfs.read("scripts/game_sounds_manifest.txt") {
            for f in hm_assets::sons::fichiers_du_manifeste(&String::from_utf8_lossy(&m.bytes)) {
                if let Some(x) = self.vfs.read(&f) {
                    v.extend(hm_assets::sons::entrees_soundscript(&String::from_utf8_lossy(&x.bytes)));
                }
            }
        }
        for ext in ["wav", "mp3", "ogg"] {
            for (f, _) in self.vfs.list_dans("sound/", ext) {
                let rel = f.trim_start_matches("sound/").to_string();
                v.push((rel.clone(), Some(rel)));
            }
        }
        v
    }

    fn lire_particules(&self) -> Vec<(String, String)> {
        let mut v = Vec::new();
        for (f, _) in self.vfs.list_dans("particles/", "pcf") {
            if let Some(x) = self.vfs.read(&f) {
                v.extend(hm_assets::particules::systemes(&x.bytes).unwrap_or_default().into_iter().map(|n| (n, f.clone())));
            }
        }
        v.sort();
        v.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
        v
    }

    /// Joue `sound/<rel>` (copié dans le dossier temporaire) ; arrête le son d'avant.
    fn ecouter(&mut self, rel: &str) {
        if let Some(mut c) = self.navigateurs.lecteur.take() {
            let _ = c.kill();
        }
        let Some(f) = self.vfs.read(&format!("sound/{rel}")) else {
            self.erreur = Some(format!("sound/{rel} introuvable dans les fichiers du jeu"));
            return;
        };
        let ext = rel.rsplit('.').next().unwrap_or("wav");
        let p = std::env::temp_dir().join(format!("hammer_master_son.{ext}"));
        if std::fs::write(&p, &f.bytes).is_err() {
            return;
        }
        // le lecteur multimédia de Windows (WPF) lit wav et mp3 ; le processus s'arrête à la fin du son
        let script = format!(
            "Add-Type -AssemblyName PresentationCore; $p = New-Object System.Windows.Media.MediaPlayer; $p.Open([uri]'{}'); $p.Play(); $t = 0; while (-not $p.NaturalDuration.HasTimeSpan -and $t -lt 50) {{ Start-Sleep -Milliseconds 100; $t++ }}; if ($p.NaturalDuration.HasTimeSpan) {{ Start-Sleep -Milliseconds ([int]$p.NaturalDuration.TimeSpan.TotalMilliseconds + 200) }}",
            p.display().to_string().replace('\'', "''")
        );
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        match c.spawn() {
            Ok(x) => self.navigateurs.lecteur = Some(x),
            Err(e) => self.erreur = Some(format!("lecture du son impossible : {e}")),
        }
    }

    /// `cle` = `valeur` sur les entités sélectionnées, en une étape.
    fn poser_cle(&mut self, cle: &str, valeur: &str) {
        let Ok((_, ents, _)) = hm_doc::resoudre(self.session.doc(), &self.session.selection) else { return };
        if ents.is_empty() {
            self.statut = format!("aucune entité sélectionnée pour {cle} = {valeur}");
            return;
        }
        let commandes = ents.iter().map(|e| Commande::Cles { entite: *e, cles: vec![(cle.to_string(), Some(valeur.to_string()))] }).collect();
        self.executer(Commande::Lot { nom: format!("{cle} = {valeur}"), commandes });
    }

    pub(super) fn fenetres_navigateurs(&mut self, ctx: &egui::Context) {
        if self.navigateurs.sons {
            if self.navigateurs.liste_sons.is_none() {
                self.navigateurs.liste_sons = Some(self.lire_sons());
                self.navigateurs.cle_son = "message".into();
            }
            let (mut ouvert, mut action) = (true, None::<(u8, usize)>);
            let n = &mut self.navigateurs;
            let liste = n.liste_sons.as_ref().unwrap();
            let f = n.filtre_sons.to_lowercase();
            let mots: Vec<&str> = f.split_whitespace().collect();
            let vus: Vec<usize> = (0..liste.len()).filter(|i| mots.iter().all(|m| liste[*i].0.to_lowercase().contains(m))).collect();
            super::barres::fenetre(ctx, "Navigateur de sons").open(&mut ouvert).resizable(true).default_size([480.0, 460.0]).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Chercher");
                    ui.add(egui::TextEdit::singleline(&mut n.filtre_sons).hint_text("weapon pistol, door…").desired_width(220.0));
                    ui.label(format!("{} / {}", vus.len(), liste.len()));
                });
                ui.separator();
                let h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
                egui::ScrollArea::vertical().max_height(ui.available_height() - 40.0).auto_shrink(false).show_rows(ui, h, vus.len(), |ui, rangs| {
                    for &i in &vus[rangs] {
                        let r = ui.selectable_label(n.choisi_son == Some(i), &liste[i].0);
                        if r.clicked() {
                            n.choisi_son = Some(i);
                        }
                        if r.double_clicked() {
                            action = Some((0, i));
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let c = n.choisi_son;
                    if ui.add_enabled(c.map_or(false, |i| liste[i].1.is_some()), egui::Button::new("Écouter")).clicked() {
                        action = c.map(|i| (0, i));
                    }
                    if ui.button("Arrêter").clicked() {
                        action = Some((2, 0));
                    }
                    ui.separator();
                    ui.label("clé");
                    ui.add(egui::TextEdit::singleline(&mut n.cle_son).desired_width(80.0));
                    if ui.add_enabled(c.is_some(), egui::Button::new("Poser sur la sélection")).clicked() {
                        action = c.map(|i| (1, i));
                    }
                });
            });
            self.navigateurs.sons = ouvert;
            let liste = self.navigateurs.liste_sons.as_ref().unwrap();
            match action {
                Some((0, i)) => {
                    if let Some(w) = liste[i].1.clone() {
                        self.ecouter(&w);
                    }
                }
                Some((1, i)) => {
                    let (nom, cle) = (liste[i].0.clone(), self.navigateurs.cle_son.trim().to_string());
                    self.poser_cle(&cle, &nom);
                }
                Some(_) => {
                    if let Some(mut c) = self.navigateurs.lecteur.take() {
                        let _ = c.kill();
                    }
                }
                None => {}
            }
        }
        if self.navigateurs.particules {
            if self.navigateurs.liste_particules.is_none() {
                self.navigateurs.liste_particules = Some(self.lire_particules());
            }
            let (mut ouvert, mut poser) = (true, None::<String>);
            let n = &mut self.navigateurs;
            let liste = n.liste_particules.as_ref().unwrap();
            let f = n.filtre_particules.to_lowercase();
            let mots: Vec<&str> = f.split_whitespace().collect();
            let vus: Vec<usize> = (0..liste.len()).filter(|i| mots.iter().all(|m| liste[*i].0.to_lowercase().contains(m) || liste[*i].1.to_lowercase().contains(m))).collect();
            super::barres::fenetre(ctx, "Navigateur de particules").open(&mut ouvert).resizable(true).default_size([480.0, 460.0]).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Chercher");
                    ui.add(egui::TextEdit::singleline(&mut n.filtre_particules).hint_text("fire, smoke, explosion…").desired_width(220.0));
                    ui.label(format!("{} / {}", vus.len(), liste.len()));
                });
                ui.separator();
                let h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
                egui::ScrollArea::vertical().max_height(ui.available_height() - 40.0).auto_shrink(false).show_rows(ui, h, vus.len(), |ui, rangs| {
                    for &i in &vus[rangs] {
                        let (nom, fichier) = &liste[i];
                        let r = ui.selectable_label(n.choisi_particule == Some(i), format!("{nom}   ({})", fichier.trim_start_matches("particles/")));
                        if r.clicked() {
                            n.choisi_particule = Some(i);
                        }
                        if r.double_clicked() {
                            poser = Some(nom.clone());
                        }
                    }
                });
                ui.separator();
                if ui.add_enabled(n.choisi_particule.is_some(), egui::Button::new("Poser sur la sélection (effect_name)")).clicked() {
                    poser = n.choisi_particule.map(|i| liste[i].0.clone());
                }
            });
            self.navigateurs.particules = ouvert;
            if let Some(nom) = poser {
                self.poser_cle("effect_name", &nom);
            }
        }
    }
}

impl Drop for EtatNavigateurs {
    fn drop(&mut self) {
        if let Some(mut c) = self.lecteur.take() {
            let _ = c.kill();
        }
    }
}

