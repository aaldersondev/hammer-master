//! Props : onglet « Modèles » (navigateur et aperçu tournant), skin et bodygroups dans les propriétés, pose sur la
//! surface visée par l'outil entité, pose au sol de la sélection. Les modifications passent par les commandes du
//! noyau (`creer_entite`, `cles`, `poser_au_sol`) ; la fenêtre ne fait que lire les modèles pour en connaître le bas.
use glam::{DVec3, Vec3};
use hm_doc::{Commande, Ref};
use hm_render::vivant::{Cibles, MondeGpu, Pose};
use hm_render::{Lighting, View};

use super::{Editeur, Onglet, Outil};
use crate::modeles::{ActionModele, InfoModele};

/// L'image tournante du navigateur de modèles.
pub(super) struct Apercu {
    cibles: Cibles,
    id: egui::TextureId,
    constantes: (wgpu::Buffer, wgpu::BindGroup),
    monde: Option<((String, u32, u32), MondeGpu, ([f32; 3], [f32; 3]))>,
}

const LARGEUR_APERCU: u32 = 400;
const HAUTEUR_APERCU: u32 = 300;

impl Editeur {
    /// La classe prend un modèle (clé `model` de type studio dans le FGD).
    pub(super) fn classe_a_modele(&self, classe: &str) -> bool {
        self.fgd.classe(classe).map_or(false, |c| c.proprietes.iter().any(|p| p.nom.eq_ignore_ascii_case("model") && p.genre.eq_ignore_ascii_case("studio")))
    }

    /// La clé qui porte les bodygroups pour cette classe : `body` si le FGD l'a, sinon `SetBodyGroup`, sinon aucune
    /// (prop_static : wiki Valve, « prop_static » : seuls les premiers sous-modèles sont montrés en jeu).
    pub(super) fn cle_body(&self, classe: &str) -> Option<&'static str> {
        let c = self.fgd.classe(classe)?;
        if c.proprietes.iter().any(|p| p.nom.eq_ignore_ascii_case("body")) {
            Some("body")
        } else if c.proprietes.iter().any(|p| p.nom.eq_ignore_ascii_case("SetBodyGroup")) {
            Some("SetBodyGroup")
        } else {
            None
        }
    }

    pub(super) fn info_modele(&mut self, nom: &str) -> Option<InfoModele> {
        let cle = hm_assets::model::normalize_model(nom);
        if !self.infos_modeles.contains_key(&cle) {
            let i = InfoModele::lire(&self.vfs, &cle).ok();
            self.infos_modeles.insert(cle.clone(), i);
        }
        self.infos_modeles[&cle].clone()
    }

    /// La distance de l'origine d'une entité à son bas : celui de son modèle (échelle comprise), sinon celui de sa
    /// boîte dans le FGD.
    pub(super) fn bas_de(&mut self, e: u64) -> f64 {
        -self.bornes_z(e).0
    }

    /// Bas et haut de l'entité par rapport à son origine (bornes de son modèle à son échelle, sinon taille du FGD).
    fn bornes_z(&mut self, e: u64) -> (f64, f64) {
        let Some(ent) = self.session.doc().entite(e) else { return (0.0, 0.0) };
        let classe = ent.classe().to_string();
        let modele = ent.get("model").filter(|m| m.to_ascii_lowercase().ends_with(".mdl")).map(String::from);
        let echelle = ent.get("modelscale").and_then(|v| v.trim().parse::<f64>().ok()).filter(|s| *s > 0.0).unwrap_or(1.0);
        match modele.and_then(|m| self.info_modele(&m)) {
            Some(i) => (i.bornes.0[2] as f64 * echelle, i.bornes.1[2] as f64 * echelle),
            None => self.fgd.classe(&classe).map_or((0.0, 0.0), |c| (c.taille().0[2] as f64, c.taille().1[2] as f64)),
        }
    }

    /// Les entités sélectionnées descendent sur la surface sous elles (`aligner` : leur haut suit la surface).
    pub(super) fn poser_au_sol_selection(&mut self, aligner: bool) {
        self.poser_selection(aligner, false);
    }

    /// Comme `poser_au_sol_selection`, vers le haut si `plafond` (« Move to Ceiling » de Hammer++).
    pub(super) fn poser_selection(&mut self, aligner: bool, plafond: bool) {
        let Ok((_, ents, _)) = hm_doc::resoudre(self.session.doc(), &self.session.selection) else { return };
        let ents: Vec<u64> = ents.into_iter().filter(|e| self.session.doc().entite(*e).map_or(false, |x| x.ponctuelle())).collect();
        if ents.is_empty() {
            self.statut = crate::langue::t("aucune entité ponctuelle sélectionnée").into();
            return;
        }
        let bas: Vec<f64> = ents.iter().map(|e| if plafond { self.bornes_z(*e).1 } else { self.bas_de(*e) }).collect();
        self.executer(Commande::PoserAuSol { entites: ents, bas, aligner, plafond });
    }

    /// Les clés d'une entité posée par l'outil : celles du FGD, le modèle choisi, son skin et ses bodygroups.
    pub(super) fn cles_entite_posee(&self, angles: Option<DVec3>) -> Vec<(String, String)> {
        let classe = self.classe_entite.clone();
        let mut cles = crate::proprietes::cles_par_defaut(&self.fgd, &classe);
        let mut mettre = |k: &str, v: String| match cles.iter_mut().find(|(c, _)| c.eq_ignore_ascii_case(k)) {
            Some(x) => x.1 = v,
            None => cles.push((k.to_string(), v)),
        };
        if let (Some((m, skin, body)), true) = (&self.modele_entite, self.classe_a_modele(&classe)) {
            mettre("model", m.clone());
            if *skin > 0 {
                mettre("skin", skin.to_string());
            }
            if let (Some(k), true) = (self.cle_body(&classe), *body > 0) {
                mettre(k, body.to_string());
            }
        }
        if let Some(a) = angles {
            mettre("angles", hm_doc::modele::texte_vec3(a));
        }
        cles
    }

    /// Clic de l'outil entité dans la vue 3D : l'entité est posée sur la surface visée (son bas sur la surface ; son
    /// haut suit la surface si « aligner » est coché). Rien de visé : devant la caméra.
    /// La surface visible sous le point d'écran `p` de la vue 3D `i` (rayon depuis la caméra).
    pub(super) fn impact_3d(&self, i: usize, p: egui::Pos2) -> Option<hm_doc::geo::Impact> {
        let v = &self.vues[i];
        let r = v.rect;
        let (x, y) = ((p.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (p.y - r.top()) / r.height() * 2.0);
        let (f, droite) = (v.cam.avant(), v.cam.droite());
        let haut = droite.cross(f);
        let t = (hm_render::fov_y_from_game(v.cam.fov).to_radians() * 0.5).tan();
        let dir = (f + droite * x * t * (r.width() / r.height()) + haut * y * t).normalize();
        let depart = v.cam.pos.as_dvec3();
        // seulement ce qui est montré
        let visibles: std::collections::HashSet<u64> = self.geo.as_ref().map(|g| g.formes.iter().filter_map(|f| f.designe.solide).collect()).unwrap_or_default();
        hm_doc::geo::rayon(self.session.doc(), depart, dir.as_dvec3(), &|s| !visibles.contains(&s))
    }

    pub(super) fn poser_entite_3d(&mut self, i: usize, p: egui::Pos2, ctx: &egui::Context) {
        let impact = self.impact_3d(i, p);
        let classe = self.classe_entite.clone();
        let bas = match (&self.modele_entite, self.classe_a_modele(&classe)) {
            (Some((m, ..)), true) => {
                let m = m.clone();
                self.info_modele(&m).map_or(0.0, |i| -i.bornes.0[2] as f64)
            }
            _ => self.fgd.classe(&classe).map_or(0.0, |c| -c.taille().0[2] as f64),
        };
        let (origine, angles) = match impact {
            Some(imp) => {
                let mut point = imp.point;
                if self.aimant && imp.normale.z.abs() > 0.999 {
                    point.x = crate::gestes::aimanter(point.x as f32, self.grille, true) as f64;
                    point.y = crate::gestes::aimanter(point.y as f32, self.grille, true) as f64;
                }
                let (o, a) = hm_doc::geo::pose_sur(point, imp.normale, bas, self.aligner_surface, DVec3::ZERO);
                (o, (self.aligner_surface && a != DVec3::ZERO).then_some(a))
            }
            None => (self.point_sous(i, p, ctx).as_dvec3(), None),
        };
        let cles = self.cles_entite_posee(angles);
        self.executer(Commande::CreerEntite { classe, origine: origine.to_array(), cles });
        self.onglet = Onglet::Proprietes;
    }

    /// Le skin et les bodygroups de l'entité sélectionnée, au-dessus de ses clés.
    pub(super) fn section_modele(&mut self, ui: &mut egui::Ui, e: u64) {
        let Some(ent) = self.session.doc().entite(e) else { return };
        let Some(m) = ent.get("model").filter(|m| m.to_ascii_lowercase().ends_with(".mdl")).map(String::from) else { return };
        let classe = ent.classe().to_string();
        let skin_actuel = ent.get("skin").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as u32;
        let cle_body = self.cle_body(&classe);
        let body_actuel = ent.get("body").or(ent.get("SetBodyGroup")).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as u32;
        let Some(info) = self.info_modele(&m) else {
            ui.colored_label(egui::Color32::from_rgb(255, 140, 100), tr!("modèle illisible : {m}", "unreadable model: {m}"));
            return;
        };
        let mut cles: Vec<(String, Option<String>)> = Vec::new();
        let mut skin_glisse: Option<u32> = None;
        ui.horizontal(|ui| {
            ui.strong(crate::langue::t("Modèle"));
            ui.label(m.strip_prefix("models/").unwrap_or(&m));
        });
        ui.horizontal(|ui| {
            ui.label("skin");
            let mut s = skin_actuel.min(info.skins as u32 - 1);
            if ui.add_enabled(info.skins > 1, egui::DragValue::new(&mut s).range(0..=(info.skins as u32 - 1))).changed() {
                skin_glisse = Some(s);
            }
            ui.weak(tr!("sur {}", "on {}", info.skins));
        });
        let mut choix = hm_assets::model::bodygroups(&info.parties, body_actuel);
        let mut change = false;
        let choisissables: Vec<(usize, hm_assets::model::Partie)> = info.choisissables().map(|(i, p)| (i, p.clone())).collect();
        if !choisissables.is_empty() {
            match cle_body {
                None => {
                    ui.weak(tr!("{} : pas de bodygroups (seul le premier sous-modèle est montré en jeu)", "{}: no bodygroups (only the first submodel is shown in game)", classe));
                }
                Some(_) => {
                    for (i, p) in &choisissables {
                        ui.horizontal(|ui| {
                            ui.label(&p.nom);
                            let mut c = choix[*i];
                            egui::ComboBox::from_id_salt(("body_ent", *i)).selected_text(p.sous_modeles.get(c).cloned().unwrap_or_default()).width(170.0).show_ui(ui, |ui| {
                                for (k, s) in p.sous_modeles.iter().enumerate() {
                                    ui.selectable_value(&mut c, k, if s.is_empty() { tr!("{k} (vide)", "{k} (empty)") } else { s.clone() });
                                }
                            });
                            if c != choix[*i] {
                                choix[*i] = c;
                                change = true;
                            }
                        });
                    }
                }
            }
        }
        if change {
            if let Some(k) = cle_body {
                cles.push((k.into(), Some(hm_assets::model::body_de(&info.parties, &choix).to_string())));
            }
        }
        ui.horizontal(|ui| {
            if ui.button(crate::langue::t("Poser au sol")).on_hover_text(crate::langue::t("descend la sélection sur la surface dessous")).clicked() {
                self.poser_au_sol_selection(false);
            }
            if ui.button(crate::langue::t("Poser aligné")).on_hover_text(crate::langue::t("descend la sélection et aligne son haut sur la surface")).clicked() {
                self.poser_au_sol_selection(true);
            }
            if ui.button(crate::langue::t("Voir dans Modèles")).clicked() {
                self.modeles.montrer(&self.vfs, &m, skin_actuel, body_actuel);
                self.onglet = Onglet::Modeles;
            }
        });
        ui.separator();
        // un glisser du skin : une seule étape d'historique
        if let Some(v) = skin_glisse {
            if let Err(err) = self.session.executer_continu(Commande::Cles { entite: e, cles: vec![("skin".into(), Some(v.to_string()))] }, &format!("skin{e}")) {
                self.erreur = Some(tr!("skin refusé : {err}", "skin refused: {err}"));
            }
        }
        if !cles.is_empty() {
            self.executer(Commande::Cles { entite: e, cles });
        }
    }

    pub(super) fn panneau_modeles(&mut self, ui: &mut egui::Ui) {
        let ents: Vec<u64> = self
            .session
            .selection
            .iter()
            .filter_map(|r| if let Ref::Entite(e) = r { Some(*e) } else { None })
            .filter(|e| self.session.doc().entite(*e).map_or(false, |x| self.classe_a_modele(x.classe())))
            .collect();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.aligner_surface, crate::langue::t("aligner sur la surface")).on_hover_text(crate::langue::t("outil entité dans la 3D : le haut du modèle suit la surface visée"));
            if let Some((m, ..)) = &self.modele_entite {
                ui.weak(tr!("outil : {}", "tool: {}", m.rsplit('/').next().unwrap_or(m)));
            }
        });
        let apercu = self.apercu.as_ref().map(|a| a.id);
        let gi = self.gameinfo.clone();
        let action = self.modeles.ui(ui, &self.vfs, &gi, apercu, ents.len());
        if self.modeles.apercu().is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        match action {
            Some(ActionModele::PourOutil(m, skin, body)) => {
                // une classe qui prend ce modèle : celle de l'outil si elle en prend un, sinon d'après le modèle
                // (un bodygroup choisi écarte prop_static, qui ne les prend pas : wiki Valve, « prop_static »)
                if !self.classe_a_modele(&self.classe_entite.clone()) {
                    let info = self.info_modele(&m);
                    self.classe_entite = match info {
                        Some(i) if i.prop_data => "prop_physics",
                        Some(i) if i.statique && body == 0 => "prop_static",
                        _ => "prop_dynamic",
                    }
                    .into();
                }
                let perdu = if body > 0 && self.cle_body(&self.classe_entite.clone()).is_none() { tr!(" ({} ne prend pas de bodygroups : choix ignoré)", " ({} takes no bodygroups: choice ignored)", self.classe_entite) } else { String::new() };
                self.statut = tr!("outil entité : {} avec {m}{perdu} — cliquer dans une vue", "entity tool: {} with {m}{perdu} — click in a view", self.classe_entite);
                self.modele_entite = Some((m, skin, body));
                self.outil = Outil::Entite;
            }
            Some(ActionModele::Appliquer(m, skin, body)) => {
                let mut cmds = Vec::new();
                for e in ents {
                    let classe = self.session.doc().entite(e).map(|x| x.classe().to_string()).unwrap_or_default();
                    let mut cles = vec![("model".to_string(), Some(m.clone())), ("skin".to_string(), Some(skin.to_string()))];
                    if let Some(k) = self.cle_body(&classe) {
                        cles.push((k.to_string(), Some(body.to_string())));
                    }
                    cmds.push(Commande::Cles { entite: e, cles });
                }
                self.executer(Commande::Lot { nom: tr!("modèle {m}", "model {m}"), commandes: cmds });
            }
            None => {}
        }
    }

    /// Dessine l'aperçu tournant du modèle choisi dans le navigateur (seulement quand l'onglet est ouvert).
    pub(super) fn dessiner_apercu(&mut self, temps: f64) {
        if self.onglet != Onglet::Modeles {
            return;
        }
        let Some(voulu) = self.modeles.apercu() else { return };
        let Some(g) = &mut self.gfx else { return };
        let device = g.r.gpu.device.clone();
        let mut enc = device.create_command_encoder(&Default::default());
        if self.apercu.is_none() {
            let cibles = Cibles::nouvelles(&device, LARGEUR_APERCU, HAUTEUR_APERCU);
            let id = g.egui.write().register_native_texture(&device, &cibles.vue_brute(), wgpu::FilterMode::Linear);
            self.apercu = Some(Apercu { cibles, id, constantes: g.r.constantes_vue(), monde: None });
        }
        let a = self.apercu.as_mut().unwrap();
        if a.monde.as_ref().map_or(true, |(k, ..)| *k != voulu) {
            let bornes = g.bib.modele_corps(&g.r, &self.vfs, &voulu.0, voulu.2).unwrap_or(([-16.0; 3], [16.0; 3]));
            let pose = Pose { modele: voulu.0.clone(), skin: voulu.1, corps: voulu.2, transform: glam::Mat4::IDENTITY, objet: 1, teinte: [1.0; 4], ombre: false };
            let monde = g.bib.monde(&g.r, &[], &[], &[pose]);
            a.monde = Some((voulu, monde, bornes));
        }
        let (_, monde, (lo, hi)) = a.monde.as_ref().unwrap();
        let (lo, hi) = (Vec3::from(*lo), Vec3::from(*hi));
        let c = (lo + hi) * 0.5;
        let rayon = ((hi - lo).length() * 0.5).max(4.0);
        let ang = temps as f32 * 0.6;
        let oeil = c + Vec3::new(ang.cos(), ang.sin(), 0.45).normalize() * rayon * 2.4;
        let mut cam = crate::vues::Cam3::viser(oeil, c);
        cam.fov = 50.0;
        let light = Lighting { sun_dir: Vec3::new(0.45, 0.35, 0.82).normalize(), sun_color: Vec3::new(0.92, 0.88, 0.80), ambient: Vec3::new(0.45, 0.47, 0.52), headlight: 0.35, sky: [0.16, 0.17, 0.19] };
        let mut camera = cam.camera();
        camera.near = (rayon * 0.02).max(0.5);
        camera.far = rayon * 20.0 + 1000.0;
        let view = View { camera, width: LARGEUR_APERCU, height: HAUTEUR_APERCU, show_tools: true, clip: None, light };
        g.r.dessiner_vivant(&g.bib, monde, &view, &a.cibles, &a.constantes.0, &a.constantes.1, hm_render::vivant::OptionsVue::default(), &mut enc);
        g.r.gpu.queue.submit([enc.finish()]);
    }
}
