//! Validation en direct : 0,8 s après la dernière modification (jamais pendant un geste), une copie de la map part
//! vers un fil de travail qui a ses propres fichiers du jeu et le FGD ; il rend les problèmes (comme Alt+P) et les
//! budgets. La barre d'état donne le nombre d'erreurs et le budget le plus plein ; un clic montre la liste dans
//! l'onglet Vérification, et tant qu'il est ouvert les objets en erreur sont entourés de rouge dans les vues (pas
//! en dehors : Hammer n'en dessine pas, et ces cadres traversent les murs ; signalé à l'usage le 15/11).
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use hm_render::lignes::Sommet;
use hm_regles::budgets::Budgets;

use super::{Editeur, Onglet};
use hm_edit::verification::{self, Probleme};

struct Travail {
    cle: u64,
    doc: std::sync::Arc<hm_doc::Document>,
    chemin: Option<PathBuf>,
    log: Option<PathBuf>,
    gameinfo: PathBuf,
    calque: bool,
}

pub struct Resultat {
    /// La révision de la géométrie validée (`revision_monde`).
    pub cle: u64,
    pub problemes: Vec<Probleme>,
    pub budgets: Option<Budgets>,
    pub erreur: Option<String>,
    pub duree: f32,
    pub calque: Option<verification::Calque>,
}

pub struct Direct {
    pub actif: bool,
    /// Calque de praticabilité demandé (joueur simulé à chaque validation).
    pub calque: bool,
    fil: Option<(Sender<Travail>, Receiver<Resultat>)>,
    en_cours: bool,
    /// Révision vue, et depuis quand.
    vue: Option<(u64, Instant)>,
    envoyee: Option<u64>,
    pub resultat: Option<Resultat>,
    /// Boîtes des objets en faute, pour (résultat, géométrie) : refaites seulement quand l'un change.
    lignes: Option<(u64, u64, Vec<Sommet>)>,
}

impl Default for Direct {
    fn default() -> Self {
        Direct { actif: true, calque: false, fil: None, en_cours: false, vue: None, envoyee: None, resultat: None, lignes: None }
    }
}

fn lancer_fil(fgd: hm_fgd::Fgd) -> (Sender<Travail>, Receiver<Resultat>) {
    let (tx, rx_t) = channel::<Travail>();
    let (tx_r, rx) = channel::<Resultat>();
    std::thread::spawn(move || {
        let mut vfs: Option<(PathBuf, hm_assets::Vfs)> = None;
        while let Ok(mut t) = rx_t.recv() {
            // seule la dernière copie compte
            while let Ok(n) = rx_t.try_recv() {
                t = n;
            }
            if vfs.as_ref().map_or(true, |(g, _)| *g != t.gameinfo) {
                let mut v = hm_assets::Vfs::new();
                let _ = v.add_gameinfo(&t.gameinfo);
                vfs = Some((t.gameinfo.clone(), v));
            }
            let t0 = Instant::now();
            let (problemes, budgets, erreur, calque) = verification::tout(&t.doc, &fgd, &vfs.as_ref().unwrap().1, t.chemin.as_deref(), t.log.as_deref(), t.calque);
            if tx_r.send(Resultat { cle: t.cle, problemes, budgets, erreur, duree: t0.elapsed().as_secs_f32(), calque }).is_err() {
                break;
            }
        }
    });
    (tx, rx)
}

impl Editeur {
    /// Le journal de la dernière compilation de cette map, s'il existe (limites des budgets).
    pub(super) fn log_compile(&self) -> Option<PathBuf> {
        Some(hm_compile::dossier_compile(self.session.chemin()).join(format!("{}.log", self.nom_compile.trim()))).filter(|p| p.is_file())
    }

    /// À appeler à chaque image.
    pub(super) fn validation_directe(&mut self, ctx: &egui::Context) {
        if !self.direct.actif {
            return;
        }
        // la version du document (et la map ouverte, et le calque demandé) : pas la géométrie, que l'arrivée du calque
        // fait reconstruire
        let cle = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.session.version(), self.session.chemin(), self.direct.calque).hash(&mut h);
            h.finish()
        };
        if self.direct.vue.map_or(true, |(c, _)| c != cle) {
            self.direct.vue = Some((cle, Instant::now()));
        }
        if let Some((_, rx)) = &self.direct.fil {
            while let Ok(r) = rx.try_recv() {
                if self.mesures.is_some() {
                    let mut par: std::collections::BTreeMap<&str, usize> = Default::default();
                    for p in &r.problemes {
                        *par.entry(p.genre.as_str()).or_default() += 1;
                    }
                    eprintln!("[mesure] validation en direct : {:.2} s, {} problème(s) : {par:?}", r.duree, r.problemes.len());
                }
                self.direct.en_cours = false;
                // le calque change la géométrie dessinée
                if r.calque.is_some() || self.direct.resultat.as_ref().map_or(false, |x| x.calque.is_some()) {
                    self.version_geo = None;
                }
                self.direct.resultat = Some(r);
            }
        }
        let (c, depuis) = self.direct.vue.unwrap();
        if !self.direct.en_cours && self.direct.envoyee != Some(c) && self.geste.is_none() && self.revision_monde > 0 {
            if depuis.elapsed() >= Duration::from_millis(800) {
                if self.direct.fil.is_none() {
                    self.direct.fil = Some(lancer_fil(self.fgd.clone()));
                }
                let t = Travail { cle: c, doc: self.session.doc_partage(), chemin: self.session.chemin().map(PathBuf::from), log: self.log_compile(), gameinfo: self.gameinfo.clone(), calque: self.direct.calque };
                if self.direct.fil.as_ref().unwrap().0.send(t).is_ok() {
                    self.direct.en_cours = true;
                    self.direct.envoyee = Some(c);
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        if self.direct.en_cours {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    /// Les boîtes des objets en erreur, pour les vues.
    pub(super) fn lignes_validation(&mut self) -> Vec<Sommet> {
        let ouvert = self.onglet == Onglet::Verification && !self.panneau_reduit;
        let Some(r) = self.direct.resultat.as_ref().filter(|_| self.direct.actif && ouvert) else { return Vec::new() };
        if let Some((c, g, l)) = &self.direct.lignes {
            if *c == r.cle && *g == self.revision_monde {
                return l.clone();
            }
        }
        // la boîte de chaque objet, en un passage sur les formes
        let mut boites: std::collections::HashMap<hm_doc::Ref, (glam::Vec3, glam::Vec3)> = Default::default();
        for f in self.geo.iter().flat_map(|g| &g.formes) {
            for k in [Some(f.designe.objet), f.designe.solide.map(hm_doc::Ref::Solide)].into_iter().flatten() {
                let b = boites.entry(k).or_insert((f.min, f.max));
                *b = (b.0.min(f.min), b.1.max(f.max));
            }
        }
        let mut v = Vec::new();
        // les erreurs seulement : une grosse map a des centaines d'avertissements, qui noieraient les vues
        for p in r.problemes.iter().filter(|p| p.grave >= 2) {
            let Some((lo, hi)) = p.objet.and_then(|o| boites.get(&o)).map(|(a, b)| (a.as_dvec3(), b.as_dvec3())) else { continue };
            let couleur = [255, 60, 60, 255];
            let m = glam::DVec3::splat(2.0);
            for a in crate::scene::aretes_boite((lo - m).as_vec3(), (hi + m).as_vec3()) {
                v.push(Sommet { pos: a[0].to_array(), couleur });
                v.push(Sommet { pos: a[1].to_array(), couleur });
            }
        }
        self.direct.lignes = Some((r.cle, self.revision_monde, v.clone()));
        v
    }

    /// La barre d'état : problèmes et budget le plus plein ; un clic montre la liste.
    pub(super) fn etat_validation(&mut self, ui: &mut egui::Ui) {
        if !self.direct.actif {
            return;
        }
        let Some(r) = &self.direct.resultat else {
            if self.direct.en_cours {
                ui.weak("validation…");
                ui.separator();
            }
            return;
        };
        let (e, a) = (r.problemes.iter().filter(|p| p.grave >= 2).count(), r.problemes.iter().filter(|p| p.grave == 1).count());
        let couleur = if e > 0 { egui::Color32::from_rgb(255, 120, 100) } else if a > 0 { egui::Color32::from_rgb(240, 200, 90) } else { ui.visuals().weak_text_color() };
        let texte = format!("{e} erreur(s), {a} avertissement(s){}", if self.direct.en_cours { " …" } else { "" });
        let clic = ui.add(egui::Label::new(egui::RichText::new(texte).color(couleur)).sense(egui::Sense::click())).on_hover_text(format!("validation en direct ({:.1} s) : cliquer pour la liste", r.duree)).clicked();
        if let Some(b) = &r.budgets {
            let plein = b.lignes.iter().filter_map(|l| l.limite.filter(|m| *m > 0).map(|m| (l.valeur as f64 / m as f64, &l.nom))).max_by(|x, y| x.0.total_cmp(&y.0));
            ui.separator();
            match plein {
                Some((f, nom)) => ui.label(format!("budget le plus plein : {nom} {:.0} %", f * 100.0)),
                None => ui.weak("budgets : limites après une compilation complète"),
            };
        }
        ui.separator();
        if clic {
            let (p, b, err) = (r.problemes.clone(), r.budgets.clone(), r.erreur.clone());
            self.montrer_verification(p, b, err);
            self.montrer_onglet(Onglet::Verification);
        }
    }
}

/// Les triangles du calque de praticabilité, ajoutés à la géométrie (vert : atteint, rouge : praticable mais
/// inatteignable depuis un point d'apparition).
pub(super) fn ajouter_calque(direct: &Direct, geo: &mut crate::scene::Geometrie, r: &hm_render::Renderer, bib: &mut hm_render::vivant::Bibliotheque) {
    let Some(c) = direct.resultat.as_ref().and_then(|x| x.calque.as_ref()).filter(|_| direct.actif && direct.calque) else { return };
    for atteint in [true, false] {
        let m = bib.couleur(r, if atteint { [40, 230, 80, 170] } else { [240, 40, 40, 160] });
        let mut idx = Vec::new();
        for (lo, hi, _) in c.bandes.iter().filter(|b| b.2 == atteint) {
            let b = geo.sommets.len() as u32;
            for (x, y) in [(lo[0], lo[1]), (lo[0], hi[1]), (hi[0], hi[1]), (hi[0], lo[1])] {
                geo.sommets.push(hm_render::vivant::WVertex { pos: [x, y, lo[2]], normal: [0.0, 0.0, 1.0], uv: [0.0, 0.0], object: 0, alpha: 0.0 });
            }
            idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        geo.lots.push(hm_render::vivant::LotMonde { materiau: m, outil: false, ombre: false, indices: idx });
    }
}
