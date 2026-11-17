//! Navigateur de textures : tous les matériaux du jeu et du projet, recherche instantanée, vignettes chargées à la
//! demande (seulement celles qu'on voit), récents.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;

use hm_assets::{load_material, load_texture, PixelFormat, Vfs};

/// Ce que l'utilisateur a fait dans le navigateur.
pub enum Choix {
    /// Le matériau devient le matériau courant.
    Choisir(String),
    /// Et il est appliqué à la sélection (double clic).
    Appliquer(String),
    /// Fenêtre du navigateur : sélectionner les objets qui l'emploient (« Mark »), le remplacer (« Replace »),
    /// montrer son VMT (« Open Source »), relire les matériaux (« Reload »).
    Marquer(String),
    Remplacer(String),
    Source(String),
    Recharger,
}

/// Ce que la fenêtre du navigateur sait d'un matériau (lu en tâche de fond, pour ses filtres).
#[derive(Clone, Debug, Default)]
pub struct InfoMat {
    pub translucide: bool,
    pub autoeclaire: bool,
    pub envmask: bool,
    /// Sa texture de base ne se trouve pas.
    pub erreur: bool,
    /// `%keywords` du VMT, en minuscules.
    pub mots: String,
}

/// Les réglages de la fenêtre du navigateur (comme celle de Hammer).
pub struct ReglagesFenetre {
    pub ouverte: bool,
    pub cote: f32,
    pub mots_cles: String,
    pub employes: bool,
    pub opaques: bool,
    pub translucides: bool,
    pub autoeclaires: bool,
    pub envmask: bool,
    pub erreurs: bool,
    pub alpha: bool,
}

impl Default for ReglagesFenetre {
    fn default() -> ReglagesFenetre {
        ReglagesFenetre { ouverte: false, cote: 128.0, mots_cles: String::new(), employes: false, opaques: true, translucides: true, autoeclaires: true, envmask: true, erreurs: true, alpha: true }
    }
}

pub struct Navigateur {
    tous: Option<Vec<String>>,
    reception: Option<mpsc::Receiver<Vec<String>>>,
    pub filtre: String,
    pub sans_modeles: bool,
    /// Débuts de chemins exclus quand `sans_modeles` est coché (Outils → Options).
    pub exclus: Vec<String>,
    vignettes: HashMap<String, Option<(egui::TextureId, [u32; 2])>>,
    pub recents: Vec<String>,
    pub favoris: HashSet<String>,
    seulement_favoris: bool,
    infos: Option<HashMap<String, InfoMat>>,
    reception_infos: Option<mpsc::Receiver<HashMap<String, InfoMat>>>,
    pub fenetre: ReglagesFenetre,
}

impl Navigateur {
    pub fn new() -> Navigateur {
        Navigateur { tous: None, reception: None, filtre: String::new(), sans_modeles: true, exclus: Vec::new(), vignettes: HashMap::new(), recents: Vec::new(), favoris: HashSet::new(), seulement_favoris: false, infos: None, reception_infos: None, fenetre: ReglagesFenetre::default() }
    }

    /// La liste se refait (fichiers du jeu changés).
    pub fn oublier(&mut self) {
        self.tous = None;
        self.reception = None;
        self.vignettes.clear();
        self.infos = None;
        self.reception_infos = None;
    }

    /// Reçoit la liste et les infos quand la tâche de fond les a finies ; lance les infos après la liste.
    fn recevoir(&mut self, gameinfo: &std::path::Path) {
        self.lister(gameinfo.to_path_buf());
        if let Some(rx) = &self.reception {
            if let Ok(l) = rx.try_recv() {
                self.tous = Some(l);
                self.reception = None;
            }
        }
        if let (Some(tous), None, None) = (&self.tous, &self.infos, &self.reception_infos) {
            let (tx, rx) = mpsc::channel();
            let (tous, gameinfo) = (tous.clone(), gameinfo.to_path_buf());
            std::thread::spawn(move || {
                let mut vfs = Vfs::new();
                let _ = vfs.add_gameinfo(&gameinfo);
                let infos = tous
                    .into_iter()
                    .map(|m| {
                        let mat = load_material(&vfs, &m);
                        let texte = vfs.read(&format!("materials/{m}.vmt")).map(|f| String::from_utf8_lossy(&f.bytes).to_lowercase()).unwrap_or_default();
                        let mots = texte.split_once("%keywords").map(|(_, r)| r.trim_start().trim_start_matches('"').split(['"', '\n']).next().unwrap_or("").trim().to_string()).unwrap_or_default();
                        let erreur = mat.base.as_ref().map_or(true, |b| vfs.locate(&format!("materials/{b}.vtf")).is_none());
                        (m, InfoMat { translucide: mat.translucent || mat.alphatest, autoeclaire: mat.selfillum, envmask: texte.contains("$envmapmask"), erreur, mots })
                    })
                    .collect();
                let _ = tx.send(infos);
            });
            self.reception_infos = Some(rx);
        }
        if let Some(rx) = &self.reception_infos {
            if let Ok(i) = rx.try_recv() {
                self.infos = Some(i);
                self.reception_infos = None;
            }
        }
    }

    /// Lance la liste des matériaux en tâche de fond (parcourir les dossiers et archives prend une seconde ou deux).
    fn lister(&mut self, gameinfo: PathBuf) {
        if self.tous.is_some() || self.reception.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut vfs = Vfs::new();
            let _ = vfs.add_gameinfo(&gameinfo);
            let mut l: Vec<String> = vfs.list_dans("materials", ".vmt").into_iter().filter_map(|(p, _)| Some(p.strip_prefix("materials/")?.strip_suffix(".vmt")?.to_string())).collect();
            l.sort();
            l.dedup();
            let _ = tx.send(l);
        });
        self.reception = Some(rx);
    }

    pub fn noter_recent(&mut self, m: &str) {
        let m = m.to_ascii_lowercase();
        self.recents.retain(|x| *x != m);
        self.recents.insert(0, m);
        self.recents.truncate(24);
    }

    /// La vignette d'un matériau : sa texture de base, petite, envoyée telle quelle à la carte graphique.
    pub fn vignette(&mut self, m: &str, vfs: &Vfs, rs: &egui_wgpu::RenderState) -> Option<(egui::TextureId, [u32; 2])> {
        if let Some(v) = self.vignettes.get(m) {
            return *v;
        }
        let v = (|| {
            let mat = load_material(vfs, m);
            let base = mat.base?;
            let t = load_texture(vfs, &base, 128)?;
            let bc = rs.device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
            // vue « brute » (sans sRGB) : l'interface affiche les octets tels qu'ils sont
            let format = match t.format {
                PixelFormat::Bc1 if bc => wgpu::TextureFormat::Bc1RgbaUnorm,
                PixelFormat::Bc2 if bc => wgpu::TextureFormat::Bc2RgbaUnorm,
                PixelFormat::Bc3 if bc => wgpu::TextureFormat::Bc3RgbaUnorm,
                PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
                _ => return None,
            };
            use wgpu::util::DeviceExt;
            let tex = rs.device.create_texture_with_data(
                &rs.queue,
                &wgpu::TextureDescriptor {
                    label: Some("vignette"),
                    size: wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
                    mip_level_count: t.mips.len() as u32,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &t.mips.concat(),
            );
            let id = rs.renderer.write().register_native_texture(&rs.device, &tex.create_view(&Default::default()), wgpu::FilterMode::Linear);
            Some((id, [t.full_width, t.full_height]))
        })();
        self.vignettes.insert(m.to_string(), v);
        v
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, vfs: &Vfs, gameinfo: &std::path::Path, rs: Option<&egui_wgpu::RenderState>, courant: &str) -> Option<Choix> {
        self.recevoir(gameinfo);
        let mut choix = None;
        ui.horizontal(|ui| {
            ui.label(crate::langue::t("Chercher"));
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("brick, concrete, tools/…").desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.sans_modeles, crate::langue::t("sans les dossiers exclus")).on_hover_text(tr!("exclus (Outils → Options) : {}", "excluded (Tools → Options): {}", self.exclus.join(", ")));
            ui.checkbox(&mut self.seulement_favoris, crate::langue::t("favoris"));
        });
        ui.label(tr!("courante : {courant}", "current: {courant}"));
        // ligne toujours présente : la grille ne bouge pas quand la première texture devient récente
        ui.horizontal(|ui| {
            ui.label(crate::langue::t("récentes :"));
            if self.recents.is_empty() {
                ui.weak(crate::langue::t("aucune"));
            }
            for r in self.recents.clone().iter().take(5) {
                if ui.small_button(r.rsplit('/').next().unwrap_or(r)).on_hover_text(r).clicked() {
                    choix = Some(Choix::Choisir(r.clone()));
                }
            }
        });
        ui.separator();
        let Some(tous) = &self.tous else {
            ui.spinner();
            ui.label(crate::langue::t("liste des matériaux en cours…"));
            ui.ctx().request_repaint();
            return choix;
        };
        let mots: Vec<String> = self.filtre.to_ascii_lowercase().split_whitespace().map(String::from).collect();
        let liste: Vec<&String> = tous
            .iter()
            .filter(|m| !self.sans_modeles || !self.exclus.iter().map(|x| x.trim()).filter(|x| !x.is_empty()).any(|x| m.starts_with(x)))
            .filter(|m| !self.seulement_favoris || self.favoris.contains(*m))
            .filter(|m| mots.iter().all(|w| m.contains(w.as_str())))
            .collect();
        ui.label(tr!("{} matériau(x)", "{} material(s)", liste.len()));
        let largeur = ui.available_width();
        let cote = 84.0;
        let par_ligne = ((largeur / (cote + 6.0)).floor() as usize).max(1);
        let lignes = liste.len().div_ceil(par_ligne);
        let liste: Vec<String> = liste.into_iter().cloned().collect();
        // charge au plus quelques vignettes par image, pour rester fluide
        let mut budget = 12;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, cote + 20.0, lignes, |ui, rangs| {
            for r in rangs {
                ui.horizontal(|ui| {
                    for m in liste.iter().skip(r * par_ligne).take(par_ligne) {
                        ui.vertical(|ui| {
                            ui.set_width(cote);
                            let deja = self.vignettes.contains_key(m);
                            let v = match rs {
                                Some(rs) if deja || budget > 0 => {
                                    if !deja {
                                        budget -= 1;
                                    }
                                    self.vignette(m, vfs, rs)
                                }
                                _ => None,
                            };
                            let (rect, rep) = ui.allocate_exact_size(egui::vec2(cote, cote), egui::Sense::click());
                            match v {
                                Some((id, _)) => {
                                    ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                }
                                None => {
                                    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                                    if !deja {
                                        ui.ctx().request_repaint();
                                    }
                                }
                            }
                            if m.eq_ignore_ascii_case(courant) {
                                ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 60)), egui::StrokeKind::Inside);
                            }
                            let nom = m.rsplit('/').next().unwrap_or(m);
                            let taille = v.map(|(_, t)| format!(" ({}×{})", t[0], t[1])).unwrap_or_default();
                            let rep = rep.on_hover_text(tr!("{m}{taille}\ndouble clic : appliquer à la sélection ; clic droit : favori", "{m}{taille}\ndouble click: apply to the selection; right click: favorite"));
                            if rep.double_clicked() {
                                choix = Some(Choix::Appliquer(m.clone()));
                            } else if rep.clicked() {
                                choix = Some(Choix::Choisir(m.clone()));
                            }
                            if rep.secondary_clicked() && !self.favoris.remove(m) {
                                self.favoris.insert(m.clone());
                            }
                            let etoile = if self.favoris.contains(m) { "* " } else { "" };
                            ui.add(egui::Label::new(egui::RichText::new(format!("{etoile}{nom}")).small()).truncate());
                        });
                    }
                });
            }
        });
        choix
    }

    /// La fenêtre du navigateur de textures (« Texture Browser » de Hammer) : grande grille aux vignettes de la
    /// taille choisie, filtre, mots-clés (`%keywords`), seulement les textures employées par la map, filtres
    /// opaques / translucides / autoéclairés / masque d'envmap / textures manquantes, fond en damier pour voir l'alpha.
    pub fn ui_fenetre(&mut self, ctx: &egui::Context, vfs: &Vfs, gameinfo: &std::path::Path, rs: Option<&egui_wgpu::RenderState>, courant: &str, employes: &HashSet<String>) -> Option<Choix> {
        if !self.fenetre.ouverte {
            return None;
        }
        self.recevoir(gameinfo);
        let mut choix = None;
        let mut ouverte = true;
        egui::Window::new(crate::langue::t("Navigateur de textures")).open(&mut ouverte).default_size([920.0, 640.0]).resizable(true).show(ctx, |ui| {
            let f = &mut self.fenetre;
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Taille"));
                egui::ComboBox::from_id_salt("taille_vignettes").width(80.0).selected_text(format!("{0}×{0}", f.cote as u32)).show_ui(ui, |ui| {
                    for t in [64.0, 128.0, 256.0, 512.0] {
                        ui.selectable_value(&mut f.cote, t, format!("{0}×{0}", t as u32));
                    }
                });
                ui.label(crate::langue::t("Filtre"));
                ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("grass, brick…").desired_width(180.0));
                ui.label(crate::langue::t("Mots-clés"));
                ui.add(egui::TextEdit::singleline(&mut f.mots_cles).desired_width(120.0));
                ui.checkbox(&mut f.employes, crate::langue::t("Seulement celles de la map"));
                ui.checkbox(&mut self.sans_modeles, crate::langue::t("Sans les dossiers exclus"));
            });
            ui.horizontal(|ui| {
                for (v, n) in [(&mut f.opaques, crate::langue::t("Opaques")), (&mut f.translucides, crate::langue::t("Translucides")), (&mut f.autoeclaires, crate::langue::t("Autoéclairées")), (&mut f.envmask, crate::langue::t("Masque d'envmap")), (&mut f.erreurs, crate::langue::t("Introuvables")), (&mut f.alpha, crate::langue::t("Alpha (damier)"))] {
                    ui.checkbox(v, n);
                }
                ui.separator();
                if ui.button(crate::langue::t("Marquer")).on_hover_text(crate::langue::t("sélectionne les objets qui emploient la texture courante")).clicked() {
                    choix = Some(Choix::Marquer(courant.to_string()));
                }
                if ui.button(crate::langue::t("Remplacer…")).clicked() {
                    choix = Some(Choix::Remplacer(courant.to_string()));
                }
                if ui.button(crate::langue::t("Source")).on_hover_text(crate::langue::t("le VMT de la texture courante")).clicked() {
                    choix = Some(Choix::Source(courant.to_string()));
                }
                if ui.button(crate::langue::t("Recharger")).on_hover_text(crate::langue::t("relit la liste et les fichiers des matériaux")).clicked() {
                    choix = Some(Choix::Recharger);
                }
            });
            let taille = self.vignettes.get(&courant.to_ascii_lowercase()).copied().flatten().map(|(_, t)| format!("  {}×{}", t[0], t[1])).unwrap_or_default();
            ui.label(tr!("courante : {courant}{taille}{}", "current: {courant}{taille}{}", if self.infos.is_none() { crate::langue::t("   (propriétés des matériaux en cours de lecture : filtres pas encore actifs)") } else { "" }));
            ui.separator();
            let Some(tous) = &self.tous else {
                ui.spinner();
                ui.ctx().request_repaint();
                return;
            };
            let f = &self.fenetre;
            let mots: Vec<String> = self.filtre.to_ascii_lowercase().split_whitespace().map(String::from).collect();
            let cles: Vec<String> = f.mots_cles.to_ascii_lowercase().split_whitespace().map(String::from).collect();
            let infos = self.infos.as_ref();
            let liste: Vec<String> = tous
                .iter()
                .filter(|m| !self.sans_modeles || !self.exclus.iter().map(|x| x.trim()).filter(|x| !x.is_empty()).any(|x| m.starts_with(x)))
                .filter(|m| mots.iter().all(|w| m.contains(w.as_str())))
                .filter(|m| !f.employes || employes.contains(*m))
                .filter(|m| match infos.and_then(|i| i.get(*m)) {
                    None => cles.is_empty(),
                    Some(i) => (if i.translucide { f.translucides } else { f.opaques }) && (f.autoeclaires || !i.autoeclaire) && (f.envmask || !i.envmask) && (f.erreurs || !i.erreur) && cles.iter().all(|c| i.mots.contains(c.as_str())),
                })
                .cloned()
                .collect();
            ui.label(tr!("{} matériau(x)", "{} material(s)", liste.len()));
            let cote = f.cote.min(ui.available_width() - 20.0).max(48.0);
            let alpha = f.alpha;
            let par_ligne = ((ui.available_width() / (cote + 6.0)).floor() as usize).max(1);
            let lignes = liste.len().div_ceil(par_ligne);
            let mut budget = 16;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, cote + 22.0, lignes, |ui, rangs| {
                for r in rangs {
                    ui.horizontal(|ui| {
                        for m in liste.iter().skip(r * par_ligne).take(par_ligne) {
                            ui.vertical(|ui| {
                                ui.set_width(cote);
                                let deja = self.vignettes.contains_key(m);
                                let v = match rs {
                                    Some(rs) if deja || budget > 0 => {
                                        if !deja {
                                            budget -= 1;
                                        }
                                        self.vignette(m, vfs, rs)
                                    }
                                    _ => None,
                                };
                                let (rect, rep) = ui.allocate_exact_size(egui::vec2(cote, cote), egui::Sense::click());
                                let p = ui.painter();
                                if alpha {
                                    // damier sous la vignette : l'alpha se voit
                                    let n = 8;
                                    let c = cote / n as f32;
                                    for i in 0..n {
                                        for j in 0..n {
                                            let g = if (i + j) % 2 == 0 { 70 } else { 110 };
                                            p.rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(i as f32 * c, j as f32 * c), egui::vec2(c, c)), 0.0, egui::Color32::from_gray(g));
                                        }
                                    }
                                } else {
                                    p.rect_filled(rect, 0.0, egui::Color32::from_gray(30));
                                }
                                match v {
                                    Some((id, _)) => {
                                        p.image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                    }
                                    None if !deja => ui.ctx().request_repaint(),
                                    None => {
                                        p.text(rect.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(cote * 0.3), egui::Color32::from_rgb(255, 80, 200));
                                    }
                                }
                                if m.eq_ignore_ascii_case(courant) {
                                    p.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 60)), egui::StrokeKind::Inside);
                                }
                                let taille = v.map(|(_, t)| format!(" ({}×{})", t[0], t[1])).unwrap_or_default();
                                let rep = rep.on_hover_text(tr!("{m}{taille}\nclic : texture courante ; double clic : appliquer à la sélection ; clic droit : favori", "{m}{taille}\nclick: current texture; double click: apply to the selection; right click: favorite"));
                                if rep.double_clicked() {
                                    choix = Some(Choix::Appliquer(m.clone()));
                                } else if rep.clicked() {
                                    choix = Some(Choix::Choisir(m.clone()));
                                }
                                if rep.secondary_clicked() && !self.favoris.remove(m) {
                                    self.favoris.insert(m.clone());
                                }
                                // le nom, sur fond bleu comme dans Hammer
                                let (r2, _) = ui.allocate_exact_size(egui::vec2(cote, 14.0), egui::Sense::hover());
                                ui.painter().rect_filled(r2, 0.0, egui::Color32::from_rgb(20, 40, 190));
                                let nom = if m.len() > (cote / 6.0) as usize { &m[m.len() - (cote / 6.0) as usize..] } else { m.as_str() };
                                ui.painter().text(r2.left_center() + egui::vec2(2.0, 0.0), egui::Align2::LEFT_CENTER, nom, egui::FontId::monospace(10.0), egui::Color32::WHITE);
                            });
                        }
                    });
                }
            });
        });
        self.fenetre.ouverte = ouverte;
        choix
    }
}
