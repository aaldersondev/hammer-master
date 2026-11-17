//! La fenêtre de l'éditeur. Elle lit le document de la session et ne le modifie que par `Session::executer` :
//! les gestes deviennent des commandes du noyau, comme celles que le serveur MCP envoie.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind};
use glam::{Vec2, Vec3};
use hm_assets::Vfs;
use hm_doc::{Alignement, Commande, Justif, Ref, RefFace, ReglageFaces, Session};
use hm_render::lignes::{Lignes, Sommet};
use hm_render::vivant::{Bibliotheque, Cibles, MondeGpu};
use hm_render::{Gpu, Lighting, Renderer, View};

use crate::gestes::{self, Geste, ModePoignees};
use crate::raccourcis::Raccourcis;
use crate::recup::{self, Fiche, Recup};
use crate::scene::{self, Designe, Geometrie};
use crate::script::{Action, Script};
use crate::proprietes::Proprietes;
use crate::textures::Navigateur;
use crate::vues::{disposer, Genre, Vue};

mod barre_objets;
mod barres;
mod brosses_ui;
mod carte_ui;
mod instances_ui;
mod io_ui;
mod logique_ui;
mod navigateurs_ui;
mod affichage_ui;
mod aides;
mod decals_ui;
mod disp_ui;
mod ciel_ui;
mod direct_ui;
mod eclairage_ui;
mod fonds_ui;
mod amenagements_ui;
mod organisation;
mod palette_ui;
mod polygone_ui;
mod pont_ui;
mod prefabs_ui;
mod remarques_ui;
mod verification_ui;
mod semis_ui;
mod sommets_ui;
mod propper_ui;
mod props;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Onglet {
    Textures,
    Proprietes,
    Historique,
    Visgroups,
    Modeles,
    Prefabs,
    Verification,
    Remarques,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outil {
    Selection,
    Camera,
    Bloc,
    Entite,
    Texture,
    Decoupe,
    Sommets,
    Decal,
    Overlay,
    Sculpter,
    Loupe,
    Semer,
    Polygone,
}

impl Outil {
    pub fn nom(self) -> &'static str {
        match self {
            Outil::Selection => crate::langue::t("Sélection"),
            Outil::Camera => crate::langue::t("Caméra"),
            Outil::Bloc => crate::langue::t("Bloc"),
            Outil::Entite => crate::langue::t("Entité"),
            Outil::Texture => crate::langue::t("Texture"),
            Outil::Decoupe => crate::langue::t("Découpe"),
            Outil::Sommets => crate::langue::t("Sommets"),
            Outil::Decal => crate::langue::t("Décal"),
            Outil::Overlay => crate::langue::t("Overlay"),
            Outil::Sculpter => crate::langue::t("Sculpter"),
            Outil::Loupe => crate::langue::t("Loupe"),
            Outil::Semer => crate::langue::t("Semer"),
            Outil::Polygone => crate::langue::t("Polygone"),
        }
    }
}

struct Gfx {
    r: Renderer,
    bib: Bibliotheque,
    lignes: Lignes,
    monde: Option<MondeGpu>,
    egui: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
}

pub struct Editeur {
    pub session: Session,
    vfs: Vfs,
    pub gameinfo: PathBuf,
    rc: Raccourcis,
    gfx: Option<Gfx>,
    geo: Option<Geometrie>,
    version_geo: Option<u64>,
    /// Révision des filtres de visibilité de la dernière géométrie.
    revision_filtres: u64,
    /// Chemin de la map à la dernière géométrie (les instances se lisent relativement à lui).
    chemin_geo: Option<PathBuf>,
    /// Masquage rapide, visgroups automatiques, « tout montrer ».
    pub filtres: crate::visibilite::Filtres,
    /// Nom du prochain visgroup (« n objets » tant qu'on ne l'a pas tapé).
    vg_nom: String,
    vg_nom_auto: bool,
    /// Visgroup en cours de renommage : (numéro, nom tapé).
    vg_renommer: Option<(u64, String)>,
    /// Visgroup choisi dans la liste de la barre d'objets (pour Mark et les flèches).
    vg_choisi: Option<u64>,
    /// Le cordon choisi dans l'onglet Cordon, et celui qu'on renomme.
    cordon_choisi: usize,
    cordon_renommer: Option<(usize, String)>,
    /// Navigateur de modèles et son aperçu tournant.
    modeles: crate::modeles::NavModeles,
    apercu: Option<props::Apercu>,
    /// Modèle posé par l'outil entité : (chemin, skin, body).
    modele_entite: Option<(String, u32, u32)>,
    /// Outil entité dans la 3D : le haut du modèle suit la surface visée.
    aligner_surface: bool,
    infos_modeles: std::collections::HashMap<String, Option<crate::modeles::InfoModele>>,
    /// Découpe et primitives.
    brosses: brosses_ui::ReglagesBrosses,
    /// Outil sommets : solides édités, points choisis.
    sommets: sommets_ui::EtatSommets,
    /// Propriétés d'une entité : clés, sorties ou entrées.
    vue_proprietes: io_ui::VueProprietes,
    /// Aides visuelles des entités (traits, cordes, sphères, cônes, icônes).
    aides: aides::CacheAides,
    /// Pinceau des displacements.
    sculpture: disp_ui::ReglagesSculpture,
    /// Documents des fichiers d'instance lus (chemin absolu → document, ou rien s'il ne se lit pas).
    instances: std::cell::RefCell<std::collections::HashMap<PathBuf, Option<Arc<hm_doc::Document>>>>,
    /// Onglet des préfabriqués.
    prefabs: prefabs_ui::EtatPrefabs,
    /// Aperçu de l'éclairage de la map dans la vue 3D.
    eclairage: eclairage_ui::EtatEclairage,
    /// Ciel 2D et skybox 3D dans la vue 3D.
    ciel: ciel_ui::EtatCiel,
    /// Onglet « Vérification » (Alt+P).
    verification: verification_ui::EtatVerification,
    /// Palette de commandes (Ctrl+Maj+P).
    palette: palette_ui::Palette,
    /// Validation en direct.
    direct: direct_ui::Direct,
    /// Remarques faites en jeu.
    remarques: remarques_ui::EtatRemarques,
    /// Modèles d'aménagement (onglet Préfabs).
    amenagements: amenagements_ui::EtatAmenagements,
    /// Action lancée hors clavier (palette) : prise par les raccourcis à l'image suivante, comme une touche.
    declenchee: Option<&'static str>,
    /// Panneau de droite réduit à une colonne d'icônes.
    panneau_reduit: bool,
    /// Coller spécial demandé : le prochain collage ouvre son dialogue au lieu de coller.
    attente_coller_special: bool,
    /// Menu Carte : rapport d'entités, différence, unités.
    carte: carte_ui::EtatCarte,
    affichage: affichage_ui::EtatAffichage,
    /// Dernier export (fichier, sélection seule) pour « Exporter à nouveau ».
    dernier_export: Option<(PathBuf, bool)>,
    navigateurs: navigateurs_ui::EtatNavigateurs,
    /// Mode de la vue 3D (comme le menu View de Hammer) : 0 textures ombrées, 1 textures, 2 polygones pleins, 3 fil de fer.
    pub(crate) rendu_3d: u8,
    /// Aides des entités (traits, sphères, cônes), textures d'outils, icônes des entités dans la 3D.
    afficher_aides: bool,
    afficher_outils: bool,
    afficher_icones: bool,
    /// Statistiques de la vue 3D (images par seconde, géométrie), comme « Toggle Statistics » de Hammer++.
    afficher_stats: bool,
    /// Couleur à donner aux objets (Outils → Couleur des objets).
    couleur_objets: [u8; 3],
    /// Grille tracée sur les surfaces de la vue 3D (P), brouillard de l'env_fog_controller de la map.
    grille_3d: bool,
    apercu_brouillard: bool,
    fonds: fonds_ui::EtatFonds,
    /// Modèles en fil de fer (arêtes vives) dans les vues 2D, et leur cache par (modèle, corps).
    modeles_2d: bool,
    semis: semis_ui::EtatSemis,
    /// Manipulateur 3D dans le repère de l'entité (J), rotation autour de l'origine de l'entité (Ctrl+P).
    gizmo_local: bool,
    pivot_origine: bool,
    polygone: polygone_ui::EtatPolygone,
    logique: logique_ui::EtatLogique,
    propper: propper_ui::EtatPropper,
    barre_objets: barre_objets::EtatBarreObjets,
    /// Mode de sélection « Solides » : chaque solide seul, même dans une entité à brosses.
    selection_solides: bool,
    /// Sous-onglet des visgroups : 0 utilisateur, 1 automatiques, 2 cordon.
    sous_onglet_vg: u8,
    aretes_modeles: std::cell::RefCell<std::collections::HashMap<(String, u32), Option<Arc<Vec<[[f32; 3]; 2]>>>>>,
    /// Dialogues ouverts par les menus (Transformer, Aller à…, Informations).
    dialogues: barres::Dialogues,
    options: crate::options::Options,
    options_ouvertes: bool,
    options_a_enregistrer: bool,
    /// Comptée à chaque envoi de la géométrie à la carte graphique.
    revision_monde: u64,
    /// Les traits de la map (hors sélection) déjà envoyés à la carte graphique, pour (révision de la géométrie,
    /// sélection) : refaits seulement quand l'une change, pas à chaque image.
    cache_lignes_monde: Option<((u64, Vec<Ref>), hm_render::lignes::LotLignes)>,
    /// `HM_MESURES` : temps d'image et de reconstruction écrits sur la sortie d'erreur (n, somme, max des images).
    mesures: Option<(u32, f32, f32, f32, Option<std::time::Instant>)>,
    selection_geo: Vec<Ref>,
    vues: Vec<Vue>,
    sep: [f32; 2],
    agrandie: Option<usize>,
    active: usize,
    pub grille: f32,
    pub aimant: bool,
    grille_visible: bool,
    pub verrou_texture: bool,
    pub ignorer_groupes: bool,
    pub outil: Outil,
    statut: String,
    erreur: Option<String>,
    script: Option<Script>,
    captures: Arc<Mutex<Vec<(String, Arc<egui::ColorImage>)>>>,
    vue_souris: bool,
    /// Liste des objets sous le dernier clic, pour passer de l'un à l'autre (PgUp / PgDn, clics répétés).
    sous_clic: (Pos2, Vec<Designe>, usize),
    cadrer: bool,
    /// Geste de la souris en cours (aperçu, pas encore de commande).
    geste: Option<Geste>,
    mode_poignees: ModePoignees,
    /// Bloc tracé par l'outil bloc, pas encore créé (Entrée le crée, Échap l'abandonne).
    pub bloc: Option<(Vec3, Vec3)>,
    /// Épaisseur du dernier bloc sur l'axe de profondeur de la vue où il a été tracé.
    profondeur_bloc: (f32, f32),
    /// Matériau courant (outil bloc, Maj+T).
    pub materiau: String,
    recup: Recup,
    /// Sauvegardes d'une session plantée, proposées au démarrage.
    a_recuperer: Vec<Fiche>,
    /// La fenêtre demande à se fermer alors que la map n'est pas enregistrée.
    confirmer_fermeture: bool,
    fermer_sans_demander: bool,
    planter: bool,
    rs: Option<egui_wgpu::RenderState>,
    navigateur: Navigateur,
    pub onglet: Onglet,
    /// Faces choisies par l'outil texture.
    pub faces: Vec<RefFace>,
    pub fgd: hm_fgd::Fgd,
    proprietes: Proprietes,
    /// Classe posée par l'outil entité.
    pub classe_entite: String,
    /// Couleur et boîte de chaque classe (tirées du FGD, gardées).
    classes_vues: std::collections::HashMap<String, ([u8; 3], [f32; 3], [f32; 3])>,
    /// Modèle montré par le FGD de chaque classe, et s'il vient de `lightprop` (tangage inversé).
    modeles_classes: std::collections::HashMap<String, Option<(String, bool)>>,
    /// Icône (`iconsprite`) de chaque classe.
    icones_classes: std::collections::HashMap<String, Option<String>>,
    outil_precedent: Outil,
    outils: Option<hm_compile::Outils>,
    compil: Option<hm_compile::Compilation>,
    fenetre_compil: bool,
    /// Fenêtre « Propriétés de l'objet » (double clic, Alt+Entrée) : le nombre d'étapes de l'historique à son
    /// ouverture, pour « Annuler ».
    fenetre_proprietes: Option<usize>,
    nom_compile: String,
    /// Nom de la compilation en cours (`<map>_zone` pour une zone) : celui que le jeu lance.
    nom_en_cours: String,
    /// La dernière compilation finie : (réussie, secondes, erreur, BSP), pour le MCP.
    pub(crate) derniere_compil: Option<(bool, f32, Option<String>, Option<PathBuf>)>,
    /// Compilation rapide d'une zone : la boîte à compiler, prise par la prochaine compilation.
    pub(crate) zone_compil: Option<(glam::DVec3, glam::DVec3)>,
    pub(crate) mode_compile: hm_compile::Mode,
    pub(crate) copier_bsp: bool,
    /// Embarquer dans le .bsp les fichiers employés que les joueurs n'ont pas (bspzip).
    pub(crate) embarquer: bool,
    pub(crate) lancer_jeu: bool,
    /// Ne refaire que ce qui a changé depuis la compilation précédente (entités, lumières).
    pub(crate) compil_partielle: bool,
    /// Les étapes du lancement du jeu sur la map compilée (fil à part : il attend le menu du jeu).
    jeu_lance: Option<std::sync::mpsc::Receiver<String>>,
    /// Journal de la dernière compilation : (ligne, 0 = normale, 1 = étape, 2 = erreur).
    journal: Vec<(String, u8)>,
    journal_tout: bool,
    etape: String,
    /// Chemin de la dernière fuite trouvée par vbsp.
    fuite: Option<Vec<[f32; 3]>>,
    /// Portails de la dernière compilation (`.prt` de vbsp, comme « Load Portal File » de Hammer), et s'ils sont montrés.
    portails: Option<Vec<Vec<[f32; 3]>>>,
    portails_visibles: bool,
    editeur_raccourcis: crate::raccourcis::Editeur,
    epaisseur_murs: f64,
    faces_comme_une: bool,
    /// La dernière face cliquée de l'outil texture : source d'un enroulement (Alt+clic droit).
    face_prise: Option<RefFace>,
    tampon_axes: std::collections::HashMap<String, String>,
    /// Fenêtre « Remplacer » ouverte : (ancien, nouveau, dans la sélection seulement).
    remplacer: Option<(String, String, bool)>,
    /// Le lien avec Claude (serveur MCP) : demandes servies entre deux images.
    pont: Option<hm_doc::pont::Ecoute>,
    pont_lance: bool,
}

impl Editeur {
    pub fn new(carte: Option<PathBuf>, script: Option<String>) -> Editeur {
        let gameinfo = hm_assets::jeu::gameinfo_gmod().unwrap_or_default();
        // un script n'écrit jamais dans le dossier du jeu et ne lance pas le jeu
        let en_script = script.is_some();
        // la langue d'abord : les champs suivants ont déjà du texte affiché (source des raccourcis…)
        let options = crate::options::Options::charger().appliquer();
        let mut e = Editeur {
            session: Session::nouvelle(),
            vfs: Vfs::new(),
            gameinfo,
            rc: Raccourcis::charger(),
            gfx: None,
            geo: None,
            version_geo: None,
            revision_filtres: 0,
            chemin_geo: None,
            filtres: Default::default(),
            vg_nom: String::new(),
            vg_nom_auto: true,
            vg_renommer: None,
            vg_choisi: None,
            cordon_choisi: 0,
            cordon_renommer: None,
            modeles: crate::modeles::NavModeles::new(),
            apercu: None,
            modele_entite: None,
            aligner_surface: false,
            infos_modeles: Default::default(),
            brosses: Default::default(),
            sommets: Default::default(),
            vue_proprietes: Default::default(),
            aides: Default::default(),
            sculpture: Default::default(),
            instances: Default::default(),
            prefabs: Default::default(),
            eclairage: Default::default(),
            ciel: Default::default(),
            verification: Default::default(),
            palette: Default::default(),
            direct: Default::default(),
            remarques: Default::default(),
            amenagements: Default::default(),
            declenchee: None,
            panneau_reduit: false,
            attente_coller_special: false,
            carte: Default::default(),
            affichage: Default::default(),
            dernier_export: None,
            navigateurs: Default::default(),
            rendu_3d: 0,
            afficher_aides: true,
            afficher_outils: true,
            afficher_icones: true,
            afficher_stats: false,
            couleur_objets: [220, 120, 40],
            grille_3d: false,
            apercu_brouillard: false,
            fonds: Default::default(),
            modeles_2d: true,
            semis: Default::default(),
            gizmo_local: false,
            pivot_origine: false,
            polygone: Default::default(),
            logique: Default::default(),
            propper: Default::default(),
            barre_objets: Default::default(),
            selection_solides: false,
            sous_onglet_vg: 0,
            aretes_modeles: Default::default(),
            dialogues: Default::default(),
            options,
            options_ouvertes: false,
            options_a_enregistrer: false,
            revision_monde: 0,
            cache_lignes_monde: None,
            mesures: std::env::var_os("HM_MESURES").map(|_| (0, 0.0, 0.0, 0.0, None)),
            selection_geo: Vec::new(),
            vues: vec![Vue::nouvelle(Genre::Camera3D), Vue::nouvelle(Genre::Dessus), Vue::nouvelle(Genre::Face), Vue::nouvelle(Genre::Cote)],
            sep: [0.5, 0.5],
            agrandie: None,
            active: 0,
            grille: 64.0,
            aimant: true,
            grille_visible: true,
            verrou_texture: true,
            ignorer_groupes: false,
            outil: Outil::Selection,
            statut: String::new(),
            erreur: None,
            script: script.map(|s| Script::lire(&s)),
            captures: Arc::new(Mutex::new(Vec::new())),
            vue_souris: false,
            sous_clic: (Pos2::ZERO, Vec::new(), 0),
            cadrer: true,
            geste: None,
            mode_poignees: ModePoignees::Taille,
            bloc: None,
            profondeur_bloc: (0.0, 64.0),
            materiau: "DEV/DEV_MEASUREGENERIC01".into(),
            recup: Recup::new(recup::dossier_par_defaut()),
            a_recuperer: recup::a_recuperer(&recup::dossier_par_defaut()),
            confirmer_fermeture: false,
            fermer_sans_demander: false,
            planter: false,
            rs: None,
            navigateur: Navigateur::new(),
            onglet: Onglet::Textures,
            faces: Vec::new(),
            fgd: hm_fgd::Fgd::de_garrysmod(),
            proprietes: Proprietes::default(),
            classe_entite: "info_player_start".into(),
            classes_vues: std::collections::HashMap::new(),
            modeles_classes: std::collections::HashMap::new(),
            icones_classes: std::collections::HashMap::new(),
            outil_precedent: Outil::Selection,
            outils: hm_compile::Outils::detecter(),
            compil: None,
            fenetre_compil: false,
            fenetre_proprietes: None,
            nom_compile: "sans_titre".into(),
            nom_en_cours: String::new(),
            derniere_compil: None,
            zone_compil: None,
            mode_compile: hm_compile::Mode::Rapide,
            copier_bsp: !en_script,
            embarquer: true,
            lancer_jeu: !en_script,
            compil_partielle: true,
            jeu_lance: None,
            journal: Vec::new(),
            journal_tout: false,
            etape: String::new(),
            fuite: None,
            portails: None,
            portails_visibles: false,
            editeur_raccourcis: Default::default(),
            epaisseur_murs: 16.0,
            faces_comme_une: false,
            face_prise: None,
            tampon_axes: std::collections::HashMap::new(),
            remplacer: None,
            pont: None,
            pont_lance: false,
        };
        e.monter_jeu(None);
        e.recup.intervalle = std::time::Duration::from_secs(60 * e.options.sauvegarde_min as u64);
        e.navigateur.exclus = e.options.exclus.clone();
        if let Some(c) = carte {
            e.ouvrir(&c);
        }
        e.statut = tr!("raccourcis : {}", "shortcuts: {}", e.rc.source);
        e
    }

    /// Les fichiers du jeu : le `gameinfo.txt` d'un projet près de la map, sinon celui de Garry's Mod.
    fn monter_jeu(&mut self, carte: Option<&Path>) {
        let mut gi = None;
        if let Some(c) = carte.and_then(|c| std::path::absolute(c).ok()) {
            let mut d = c.parent().map(Path::to_path_buf);
            for _ in 0..3 {
                let Some(x) = d else { break };
                if let Some(g) = [x.join("compile/gameinfo.txt"), x.join("_compile/gameinfo.txt"), x.join("gameinfo.txt")].into_iter().find(|p| p.is_file()) {
                    gi = Some(g);
                    break;
                }
                d = x.parent().map(Path::to_path_buf);
            }
        }
        let gi = gi.unwrap_or_else(|| hm_assets::jeu::gameinfo_gmod().unwrap_or_default());
        if gi == self.gameinfo && !self.vfs.mounts().is_empty() {
            return;
        }
        let mut vfs = Vfs::new();
        if let Err(e) = vfs.add_gameinfo(&gi) {
            self.erreur = Some(tr!("fichiers du jeu introuvables ({}) : {e}", "game files not found ({}): {e}", gi.display()));
        }
        // le projecteur des light_spot tel que Hammer++ le montre (son modèle gris, teinté de `_light`), s'il est là
        if let Some(g) = hm_assets::jeu::garrysmod() {
            vfs.add_fichiers(g.join("bin/win64/hammerplusplus"), &scene::PROJECTEUR_HAMMERPP);
        }
        self.vfs = vfs;
        self.gameinfo = gi;
        self.navigateur.oublier();
        self.modeles.oublier();
        self.infos_modeles.clear();
        // les fichiers ont changé : matériaux et modèles sont à recharger
        if let Some(g) = &mut self.gfx {
            g.bib = Bibliotheque::nouvelle(&g.r);
            g.bib.outils_reels = true;
        }
        self.version_geo = None;
    }

    pub fn ouvrir(&mut self, p: &Path) {
        match Session::ouvrir(p) {
            Ok(s) => {
                let renum = s.rapport_lecture.renumerotes.len();
                self.session = s;
                self.filtres.masques.clear();
                self.instances.borrow_mut().clear();
                self.filtres.changer();
                self.monter_jeu(Some(p));
                self.version_geo = None;
                self.cadrer = true;
                self.nom_d_apres(p);
                self.fuite = None;
                let invalides = self.session.doc().solides_invalides().len();
                self.statut = tr!("ouvert : {}{}{}", "opened: {}{}{}",
                    p.display(),
                    if renum > 0 { tr!(" ({renum} numéro(s) en double renuméroté(s))", " ({renum} duplicate ID(s) renumbered)") } else { String::new() },
                    if invalides > 0 { tr!(" — {invalides} solide(s) invalide(s) : Outils > Sélectionner les solides invalides", " — {invalides} invalid solid(s): Tools > Select invalid solids") } else { String::new() }
                );
            }
            Err(e) => self.erreur = Some(tr!("ouverture impossible : {e}", "cannot open: {e}")),
        }
    }

    fn executer(&mut self, c: Commande) {
        let nom = c.nom();
        match self.session.executer(c) {
            Ok(r) => self.statut = if r.message.is_empty() { tr!("fait : {nom}", "done: {nom}") } else { tr!("{nom} : {}", "{nom}: {}", r.message) },
            Err(e) => self.erreur = Some(tr!("{nom} refusé : {e}", "{nom} refused: {e}")),
        }
    }

    fn titre(&self) -> String {
        let nom = self.session.chemin().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| crate::langue::t("sans titre").into());
        format!("{}{} — Hammer Master", nom, if self.session.modifiee() { " *" } else { "" })
    }

    // ------------------------------------------------------------------ carte graphique

    fn preparer_gfx(&mut self, frame: &eframe::Frame) {
        if self.gfx.is_some() {
            return;
        }
        let Some(rs) = frame.wgpu_render_state() else {
            self.erreur = Some(crate::langue::t("pas de carte graphique wgpu").into());
            return;
        };
        let info = rs.adapter.get_info();
        let gpu = Gpu::depuis(rs.device.clone(), rs.queue.clone(), format!("{} ({:?})", info.name, info.backend));
        let mut r = Renderer::new(gpu);
        if !self.options.filtrer_textures {
            r.regler_filtrage(false);
        }
        let mut bib = Bibliotheque::nouvelle(&r);
        bib.outils_reels = true;
        let lignes = Lignes::new(&rs.device);
        self.gfx = Some(Gfx { r, bib, lignes, monde: None, egui: rs.renderer.clone() });
        self.rs = Some(rs.clone());
    }

    /// Refait la géométrie quand le document ou la sélection a changé.
    fn mettre_a_jour_geo(&mut self) {
        self.preparer_sommets();
        let Some(g) = &mut self.gfx else { return };
        let v = self.session.version();
        let chemin = self.session.chemin().map(Path::to_path_buf);
        if chemin != self.chemin_geo {
            self.chemin_geo = chemin;
            self.instances.borrow_mut().clear();
            self.version_geo = None;
        }
        if self.version_geo != Some(v) || self.revision_filtres != self.filtres.revision {
            // couleur et taille de chaque classe : celles du FGD, gardées d'une image à l'autre
            for e in self.session.doc().entites() {
                let c = e.classe().to_string();
                if !self.classes_vues.contains_key(&c) {
                    let k = self.fgd.classe(&c);
                    let v = match &k {
                        Some(k) => {
                            let (a, b) = k.taille();
                            (k.couleur(), a, b)
                        }
                        None => (couleur_classe(&c), [-8.0; 3], [8.0; 3]),
                    };
                    self.modeles_classes.insert(c.clone(), k.as_ref().and_then(|k| k.modele().map(|m| (m.to_string(), k.modele_tangage_inverse()))));
                    self.icones_classes.insert(c.clone(), k.as_ref().and_then(|k| k.icone().map(String::from)));
                    self.classes_vues.insert(c, v);
                }
            }
            let table = &self.classes_vues;
            let classe = |c: &str| table.get(c).copied().unwrap_or(([220, 30, 220], [-8.0; 3], [8.0; 3]));
            let modeles = &self.modeles_classes;
            let modele_classe = |c: &str| modeles.get(c).cloned().flatten();
            let (icones, avec_icones) = (&self.icones_classes, self.afficher_icones);
            let icone_classe = |c: &str| icones.get(c).cloned().flatten().filter(|_| avec_icones);
            let (doc, filtres) = (self.session.doc(), &self.filtres);
            let cordon = doc.cordons().boites_actives();
            let visible = |d: &Designe, lo: glam::DVec3, hi: glam::DVec3| filtres.visible(doc, d, lo, hi, &cordon);
            // les fichiers d'instance, relatifs au fichier de la map (wiki Valve, func_instance), lus une fois
            let dossier = self.session.chemin().and_then(|c| c.parent()).map(Path::to_path_buf);
            let cache = &self.instances;
            let instance = |f: &str| -> Option<Arc<hm_doc::Document>> {
                let p = dossier.as_ref()?.join(f);
                if let Some(x) = cache.borrow().get(&p) {
                    return x.clone();
                }
                let d = std::fs::read(&p).ok().and_then(|b| hm_doc::Document::lire(&b).ok()).map(|(d, _)| Arc::new(d));
                cache.borrow_mut().insert(p, d.clone());
                d
            };
            let t0 = std::time::Instant::now();
            // les modèles en fil de fer dans les vues 2D (Maj+M) : leurs arêtes vives, lues une fois par modèle
            let (vfs, cache_aretes, en_2d) = (&self.vfs, &self.aretes_modeles, self.modeles_2d);
            let aretes_modele = |m: &str, corps: u32| -> Option<Arc<Vec<[[f32; 3]; 2]>>> {
                if !en_2d {
                    return None;
                }
                let cle = (m.to_ascii_lowercase(), corps);
                if let Some(x) = cache_aretes.borrow().get(&cle) {
                    return x.clone();
                }
                let a = hm_assets::model::load_model_body(vfs, &hm_assets::model::normalize_model(m), corps).ok().map(|x| Arc::new(hm_assets::aretes::aretes_vives(&x.vertices, &x.meshes, 35.0)));
                cache_aretes.borrow_mut().insert(cle, a.clone());
                a
            };
            let mut geo = scene::construire(doc, &g.r, &mut g.bib, &self.vfs, &classe, &modele_classe, &icone_classe, &visible, &instance, &aretes_modele);
            direct_ui::ajouter_calque(&self.direct, &mut geo, &g.r, &mut g.bib);
            let t1 = t0.elapsed().as_secs_f32() * 1000.0;
            g.monde = Some(g.bib.monde(&g.r, &geo.sommets, &geo.lots, &geo.poses));
            if self.mesures.is_some() {
                eprintln!("[mesure] géométrie : construite en {t1:.0} ms, envoyée en {:.0} ms ({} sommets)", t0.elapsed().as_secs_f32() * 1000.0 - t1, geo.sommets.len());
            }
            self.revision_monde += 1;
            self.geo = Some(geo);
            self.version_geo = Some(v);
            self.revision_filtres = self.filtres.revision;
            // un objet caché ne reste pas sélectionné
            if let Some(geo) = &self.geo {
                let vus_s: std::collections::HashSet<u64> = geo.formes.iter().filter_map(|f| f.designe.solide).collect();
                let vus_e: std::collections::HashSet<u64> = geo.formes.iter().filter_map(|f| if let Ref::Entite(e) = f.designe.objet { Some(e) } else { None }).collect();
                let doc = self.session.doc();
                let garder: Vec<Ref> = self
                    .session
                    .selection
                    .iter()
                    .copied()
                    .filter(|r| match hm_doc::resoudre(doc, &[*r]) {
                        Ok((s, e, _)) => s.iter().any(|x| vus_s.contains(x)) || e.iter().any(|x| vus_e.contains(x)),
                        Err(_) => false,
                    })
                    .collect();
                self.session.selection = garder;
            }
        }
        self.selection_geo = self.session.selection.clone();
        // cadrer une fois que les vues ont leur taille (pas à la première image)
        if self.cadrer && self.vues.iter().all(|v| v.rect.is_positive()) {
            if let Some(geo) = &self.geo {
                let (lo, hi) = (geo.min, geo.max);
                // la caméra enregistrée dans la map, comme Hammer : vue 3D à sa place, vues 2D centrées dessus au
                // zoom 0,25 de Hammer++ ; sans caméra, toute la map
                let camera = self.session.doc().camera_active().map(|(p, c)| (glam::DVec3::from(p).as_vec3(), glam::DVec3::from(c).as_vec3()));
                for vue in &mut self.vues {
                    vue.cadrer(lo, hi);
                    if let Some((p, c)) = camera {
                        vue.sur_camera(p, c, Some(0.25));
                    }
                }
                self.cadrer = false;
            }
        }
    }

    // ------------------------------------------------------------------ sélection

    /// Ce que sélectionne un clic sur cet objet : son groupe le plus haut, sauf si les groupes sont ignorés.
    fn a_selectionner(&self, d: Designe) -> Ref {
        if self.selection_solides {
            return d.solide.map_or(d.objet, Ref::Solide);
        }
        if self.ignorer_groupes {
            return match d.solide {
                Some(s) if matches!(d.objet, Ref::Solide(_)) => Ref::Solide(s),
                _ => d.objet,
            };
        }
        let doc = self.session.doc();
        let mut groupe = match d.objet {
            Ref::Solide(id) => doc.solide(id).map(|s| s.groupe()).unwrap_or(0),
            Ref::Entite(id) => doc.entite(id).map(|e| e.groupe()).unwrap_or(0),
            Ref::Groupe(id) => id,
        };
        if groupe == 0 {
            return d.objet;
        }
        // remonter les groupes imbriqués
        for _ in 0..64 {
            let parent = doc.groupe(groupe).and_then(|g| g.editeur.get("groupid")).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
            if parent == 0 || doc.groupe(parent).is_none() {
                break;
            }
            groupe = parent;
        }
        Ref::Groupe(groupe)
    }

    /// Masquage rapide de la sélection (H), ou de tout le reste (Ctrl+H) : un réglage de la vue, pas une édition.
    pub fn masquer(&mut self, le_reste: bool) {
        if self.session.selection.is_empty() {
            self.statut = crate::langue::t("rien de sélectionné à masquer").into();
            return;
        }
        let sel = self.session.selection.clone();
        let n = if le_reste { self.filtres.masquer_le_reste(self.session.doc(), &sel) } else { self.filtres.masquer(self.session.doc(), &sel) };
        if !le_reste {
            self.session.selection.clear();
        }
        self.statut = tr!("{n} objet(s) masqué(s) — U pour tout réafficher", "{n} object(s) hidden — U to show everything again");
    }

    /// Réaffiche les objets du masquage rapide (U).
    pub fn reafficher(&mut self) {
        let n = self.filtres.masques.len();
        self.filtres.masques.clear();
        self.instances.borrow_mut().clear();
        self.filtres.changer();
        self.statut = tr!("{n} objet(s) réaffiché(s)", "{n} object(s) shown again");
    }

    fn cliquer(&mut self, cands: Vec<Designe>, pos: Pos2, ajouter: bool) {
        // un clic au même endroit passe à l'objet suivant dessous
        let meme = (self.sous_clic.0 - pos).length() < 3.0 && self.sous_clic.1 == cands;
        let rang = if meme && !cands.is_empty() { (self.sous_clic.2 + 1) % cands.len() } else { 0 };
        self.sous_clic = (pos, cands.clone(), rang);
        let Some(d) = cands.get(rang).copied() else {
            if !ajouter {
                self.session.selection.clear();
            }
            return;
        };
        let r = self.a_selectionner(d);
        if !ajouter && self.session.selection == vec![r] {
            self.mode_poignees = match self.mode_poignees {
                ModePoignees::Taille => ModePoignees::Rotation,
                ModePoignees::Rotation => ModePoignees::Taille,
            };
            return;
        }
        self.mode_poignees = ModePoignees::Taille;
        if ajouter {
            if let Some(i) = self.session.selection.iter().position(|x| *x == r) {
                self.session.selection.remove(i);
            } else {
                self.session.selection.push(r);
            }
        } else {
            self.session.selection = vec![r];
        }
        self.statut = tr!("sélection : {} objet(s){}", "selection: {} object(s){}", self.session.selection.len(), if cands.len() > 1 { tr!(" — {} sous le curseur (PgUp/PgDn pour changer)", " — {} under the cursor (PgUp/PgDn to cycle)", cands.len()) } else { String::new() });
    }

    fn passer_sous_le_curseur(&mut self, sens: i32) {
        let (pos, cands, rang) = self.sous_clic.clone();
        if cands.len() < 2 {
            return;
        }
        let n = cands.len() as i32;
        let r = ((rang as i32 + sens).rem_euclid(n)) as usize;
        self.sous_clic = (pos, cands.clone(), r);
        let x = self.a_selectionner(cands[r]);
        self.session.selection = vec![x];
    }

    // ------------------------------------------------------------------ raccourcis

    fn raccourcis(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.editeur_raccourcis.ouvert {
            return;
        }
        let rc = self.rc.clone();
        let d = self.declenchee.take();
        let p = |a: &str| d == Some(a) || ctx.input(|i| rc.presse(a, i));
        if p("Palette_Commandes") {
            self.ouvrir_palette();
        }
        if p("Edit_Undo") {
            match self.session.annuler() {
                Some(n) => self.statut = tr!("annulé : {n}", "undone: {n}"),
                None => self.statut = crate::langue::t("rien à annuler").into(),
            }
        }
        if p("Edit_Redo") {
            match self.session.retablir() {
                Some(n) => self.statut = tr!("rétabli : {n}", "redone: {n}"),
                None => self.statut = crate::langue::t("rien à rétablir").into(),
            }
        }
        if p("File_Save") {
            self.enregistrer(false);
        }
        if p("File_Open") {
            self.dialogue_ouvrir();
        }
        if p("File_New") {
            self.session = Session::nouvelle();
            self.filtres.masques.clear();
            self.instances.borrow_mut().clear();
            self.filtres.changer();
            self.version_geo = None;
            self.cadrer = true;
        }
        if p("Map_Grid_Higher") {
            self.grille = (self.grille * 2.0).min(1024.0);
        }
        if p("Map_Grid_Lower") {
            self.grille = (self.grille / 2.0).max(1.0);
        }
        if p("Map_Snap_To_Grid") {
            self.aimant = !self.aimant;
            self.statut = tr!("aimantation : {}", "snapping: {}", if self.aimant { crate::langue::t("oui") } else { crate::langue::t("non") });
        }
        if p("View_Toggle_Grid") {
            self.grille_visible = !self.grille_visible;
        }
        if p("Tools_Toggle_Texture_Lock") {
            self.verrou_texture = !self.verrou_texture;
            self.statut = tr!("verrou de texture : {}", "texture lock: {}", if self.verrou_texture { crate::langue::t("oui") } else { crate::langue::t("non") });
        }
        if p("Edit_Toggle_Group_Ignore") {
            self.ignorer_groupes = !self.ignorer_groupes;
        }
        if p("View_Maximize_Restore_Active_View") {
            self.agrandie = if self.agrandie.is_some() { None } else { Some(self.active) };
        }
        if p("View_Autosize4") {
            self.sep = [0.5, 0.5];
        }
        if p("View_Clear_Selection") || (p("Tool_Exit") && !self.vue_souris) {
            if self.bloc.is_some() || self.geste.is_some() || self.brosses.decoupe.is_some() {
                self.bloc = None;
                self.geste = None;
                self.brosses.decoupe = None;
            } else {
                self.session.selection.clear();
            }
        }
        if p("Map_Check_For_Problems") {
            self.verifier_map();
        }
        if p("Compile_Zone") {
            self.compiler_zone();
        }
        if p("View_Walkable_Layer") {
            self.direct.calque = !self.direct.calque;
            self.direct.actif |= self.direct.calque;
            self.version_geo = None;
        }
        if p("View_Lighting_Preview") {
            self.eclairage.actif = !self.eclairage.actif;
        }
        if p("Tools_Create_Prefab") {
            self.enregistrer_prefab();
        }
        if p("Tools_Clipping") {
            self.basculer_decoupe();
        }
        if p("Tools_Vertex_Manipulation") {
            self.basculer_sommets();
        }
        if p("Tools_Split_Face") {
            self.couper_face_sommets();
        }
        if p("Tools_Merge") {
            self.fusionner();
        }
        if p("Tools_Carve") {
            self.creuser();
        }
        if p("View_Quickhide") {
            self.masquer(false);
        }
        if p("View_Quickhide_Unselected") {
            self.masquer(true);
        }
        if p("View_Quick_Unhide") {
            self.reafficher();
        }
        self.raccourcis_edition(ctx, &rc, d);
        self.raccourcis_menus(ctx, &rc, d);
        if p("Tool_Selection_Delete") && !self.session.selection.is_empty() {
            let objets = self.session.selection.clone();
            self.executer(Commande::Supprimer { objets });
        }
        if p("File_Run_Map") {
            self.fenetre_compil = true;
        }
        if p("Edit_Properties") {
            self.ouvrir_proprietes();
        }
        if p("Tool_Selection_Next_Hit") {
            self.passer_sous_le_curseur(1);
        }
        if p("Tool_Selection_Previous_Hit") {
            self.passer_sous_le_curseur(-1);
        }
        for (a, g) in [("View_2D_X_Y", Genre::Dessus), ("View_2D_X_Z", Genre::Cote), ("View_2D_Y_Z", Genre::Face)] {
            if p(a) {
                self.vues[self.active].genre = g;
                self.vues[self.active].cibles = None;
            }
        }
        if p("View2D_Next_Draw_Type") && self.vues[self.active].genre != Genre::Camera3D {
            let g = self.vues[self.active].genre.suivant_2d();
            self.vues[self.active].genre = g;
        }
        if p("View3D_Mouselook_Toggle") && self.vues[self.active].genre == Genre::Camera3D {
            self.basculer_vue_souris(ctx, !self.vue_souris);
        } else if p("Tool_Exit") && self.vue_souris {
            self.basculer_vue_souris(ctx, false);
        }
        if p("View_Center_On_Selection") || p("View_Center_3D_Views_On_Selection") {
            if let Some((lo, hi)) = hm_doc::boite_de(self.session.doc(), &self.session.selection) {
                let trois_d = p("View_Center_3D_Views_On_Selection");
                for v in &mut self.vues {
                    if (v.genre == Genre::Camera3D) == trois_d {
                        v.cadrer(lo.as_vec3(), hi.as_vec3());
                    }
                }
            }
        }
        for (a, o) in [("Tools_Pointer", Outil::Selection), ("Tools_Camera", Outil::Camera), ("Tools_Block", Outil::Bloc), ("Tools_Entity", Outil::Entite), ("Tools_Texture_Application", Outil::Texture), ("Tools_Apply_Decals", Outil::Decal), ("Tools_Overlay", Outil::Overlay)] {
            if p(a) {
                self.outil = o;
            }
        }
    }

    /// Vue souris : le curseur est capturé et caché, la souris tourne la caméra (comme Hammer).
    fn basculer_vue_souris(&mut self, ctx: &egui::Context, actif: bool) {
        self.vue_souris = actif;
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(if actif { egui::CursorGrab::Locked } else { egui::CursorGrab::None }));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(!actif));
        self.statut = if actif { crate::langue::t("vue souris : Échap ou la même touche pour en sortir").into() } else { String::new() };
    }


    /// Raccourcis qui modifient la map : décaler, copier, coller, grouper, miroir, caler sur la grille, bloc.
    fn raccourcis_edition(&mut self, ctx: &egui::Context, rc: &Raccourcis, d: Option<&str>) {
        let p = |a: &str| d == Some(a) || ctx.input(|i| rc.presse(a, i));
        let sel = self.session.selection.clone();
        let vue_genre = self.vues[self.active].genre;
        let (a, b, _) = vue_genre.axes();
        // point du monde sous le curseur dans la vue 2D active (pour coller)
        let w_curseur = ctx.pointer_hover_pos().filter(|p| vue_genre != Genre::Camera3D && self.vues[self.active].rect.contains(*p)).map(|p| self.vues[self.active].ecran_vers_monde(p));
        // presse-papiers : egui transforme Ctrl+C / X / V en événements
        let (copie, coupe, colle) = ctx.input(|i| {
            let mut c = (false, false, None);
            for e in &i.events {
                match e {
                    egui::Event::Copy => c.0 = true,
                    egui::Event::Cut => c.1 = true,
                    egui::Event::Paste(t) => c.2 = Some(t.clone()),
                    _ => {}
                }
            }
            c
        });
        if (copie || coupe) && !sel.is_empty() {
            match self.session.copier() {
                Ok(t) => {
                    ctx.copy_text(t);
                    self.statut = tr!("{} objet(s) copié(s)", "{} object(s) copied", sel.len());
                    if coupe {
                        self.executer(Commande::Supprimer { objets: sel.clone() });
                    }
                }
                Err(e) => self.erreur = Some(e),
            }
        }
        if let Some(t) = colle {
            if std::mem::take(&mut self.attente_coller_special) {
                self.ouvrir_coller_special(t);
            } else if t.contains("world") {
                // collé sous le curseur, à la grille, dans la vue active (profondeur gardée)
                let mut decalage = [self.grille as f64, self.grille as f64, 0.0];
                if let (Ok((frag, _)), Some(w)) = (hm_doc::Document::lire(t.as_bytes()), w_curseur) {
                    let tous: Vec<Ref> = frag.solides_monde().map(|s| Ref::Solide(s.id)).chain(frag.entites().map(|e| Ref::Entite(e.id))).collect();
                    if let Some((lo, hi)) = hm_doc::boite_de(&frag, &tous) {
                        let c = (lo + hi) * 0.5;
                        decalage = [0.0; 3];
                        decalage[a] = gestes::aimanter(w.x - c[a] as f32, self.grille, self.aimant) as f64;
                        decalage[b] = gestes::aimanter(w.y - c[b] as f32, self.grille, self.aimant) as f64;
                    }
                }
                self.executer(Commande::Coller { vmf: t, decalage });
            }
        }
        // décaler aux flèches dans la vue 2D active (Maj : cloner puis décaler)
        if vue_genre != Genre::Camera3D && !sel.is_empty() && self.options.fleches {
            let (shift, fleches) = ctx.input(|i| {
                let mut d = [0.0f32; 2];
                for e in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                        if modifiers.command || modifiers.alt {
                            continue;
                        }
                        match key {
                            egui::Key::ArrowLeft => d[0] -= 1.0,
                            egui::Key::ArrowRight => d[0] += 1.0,
                            egui::Key::ArrowUp => d[1] += 1.0,
                            egui::Key::ArrowDown => d[1] -= 1.0,
                            _ => {}
                        }
                    }
                }
                (i.modifiers.shift, d)
            });
            if fleches != [0.0; 2] {
                let mut delta = [0.0f64; 3];
                delta[a] = (fleches[0] * self.grille) as f64;
                delta[b] = (fleches[1] * self.grille) as f64;
                if shift {
                    self.executer(Commande::Dupliquer { objets: sel.clone(), delta });
                } else {
                    self.executer(Commande::Deplacer { objets: sel.clone(), delta, verrou_texture: self.verrou_texture });
                }
            }
        }
        if !sel.is_empty() {
            if p("Tools_Snap_Selected_To_Grid") {
                if let Some((lo, _)) = hm_doc::boite_de(self.session.doc(), &sel) {
                    let g = self.grille as f64;
                    let d = (lo / g).round() * g - lo;
                    self.executer(Commande::Deplacer { objets: sel.clone(), delta: d.to_array(), verrou_texture: self.verrou_texture });
                }
            }
            for (action, horizontal) in [("Edit_Flip_Horizontal", true), ("Edit_Flip_Vertical", false)] {
                if p(action) && vue_genre != Genre::Camera3D {
                    if let Some((lo, hi)) = hm_doc::boite_de(self.session.doc(), &sel) {
                        let axe = if horizontal { a } else { b };
                        self.executer(Commande::Miroir { objets: sel.clone(), centre: ((lo + hi) * 0.5).to_array(), axe });
                    }
                }
            }
            if p("Tools_Group") {
                self.executer(Commande::Grouper { objets: sel.clone() });
            }
            if p("Tools_Ungroup") {
                let groupes: Vec<u64> = sel.iter().filter_map(|r| if let Ref::Groupe(g) = r { Some(*g) } else { None }).collect();
                if !groupes.is_empty() {
                    self.executer(Commande::Degrouper { groupes });
                }
            }
            if p("Edit_To_Entity") {
                // comme Hammer : func_detail, puis les propriétés pour choisir la classe (bouton « … » du champ Classe)
                self.executer(Commande::LierAEntite { objets: sel.clone(), classe: "func_detail".into() });
                self.onglet = Onglet::Proprietes;
                self.vue_proprietes = io_ui::VueProprietes::Cles;
                self.statut = crate::langue::t("lié à func_detail : choisir la classe avec « … » à côté de Classe").into();
            }
            if p("Edit_To_World") {
                let entites: Vec<u64> = sel.iter().filter_map(|r| if let Ref::Entite(e) = r { Some(*e) } else { None }).filter(|e| self.session.doc().entite(*e).map_or(false, |x| !x.ponctuelle())).collect();
                if !entites.is_empty() {
                    self.executer(Commande::RemettreAuMonde { entites });
                }
            }
            if p("Edit_Apply_Texture") {
                self.appliquer_materiau();
            }
        }
        // l'outil bloc : Entrée crée le bloc tracé
        if p("Tool_Enter") {
            if self.outil == Outil::Decoupe && self.brosses.decoupe.is_some() {
                self.valider_decoupe();
            } else if self.bloc.is_some() {
                self.creer_bloc_trace();
            } else if self.outil == Outil::Polygone {
                self.creer_polygone();
            }
        }
    }


    /// Le panneau de droite : textures, propriétés, historique.
    fn panneau_droit(&mut self, ui: &mut egui::Ui) {
        // à la ligne plutôt que d'élargir le panneau
        self.barre_objets(ui);
        self.onglets(ui);
        ui.separator();
        match self.onglet {
            Onglet::Textures => {
                if self.outil == Outil::Texture {
                    self.panneau_faces(ui);
                    ui.separator();
                }
                let courant = self.materiau.clone();
                let rs = self.rs.clone();
                let gi = self.gameinfo.clone();
                if let Some(c) = self.navigateur.ui(ui, &self.vfs, &gi, rs.as_ref(), &courant) {
                    self.traiter_choix_texture(c);
                }
            }
            Onglet::Proprietes => self.panneau_proprietes(ui),
            Onglet::Visgroups => self.panneau_visgroups(ui),
            Onglet::Modeles => self.panneau_modeles(ui),
            Onglet::Prefabs => self.panneau_prefabs(ui),
            Onglet::Verification => self.panneau_verification(ui),
            Onglet::Remarques => self.panneau_remarques(ui),
            Onglet::Historique => {
                let faits: Vec<String> = self.session.historique().faits().iter().map(|c| c.nom.clone()).collect();
                let annules: Vec<String> = self.session.historique().annules().iter().rev().map(|c| c.nom.clone()).collect();
                ui.label(tr!("{} étape(s) ; cliquer une ligne pour revenir à ce point", "{} step(s); click a line to go back to that point", faits.len()));
                let mut aller = None;
                egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                    if ui.selectable_label(faits.is_empty(), crate::langue::t("(ouverture de la map)")).clicked() {
                        aller = Some(0);
                    }
                    for (i, n) in faits.iter().enumerate() {
                        if ui.selectable_label(i + 1 == faits.len(), format!("{:>3}  {n}", i + 1)).clicked() {
                            aller = Some(i + 1);
                        }
                    }
                    for (k, n) in annules.iter().enumerate() {
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{:>3}  {n}", faits.len() + k + 1)).weak()).frame(false)).on_hover_text(crate::langue::t("annulé : cliquer pour rétablir jusqu'ici")).clicked() {
                            aller = Some(faits.len() + k + 1);
                        }
                    }
                });
                if let Some(n) = aller {
                    self.session.aller_a(n);
                    self.statut = tr!("historique : étape {n}", "history: step {n}");
                }
            }
        }
    }

    /// La fenêtre d'application de texture (comme « Face Edit » de Hammer) : les réglages de la première face choisie,
    /// appliqués à toutes les faces choisies.
    fn panneau_faces(&mut self, ui: &mut egui::Ui) {
        self.faces.retain(|f| self.session.doc().solide(f.solide).map_or(false, |s| s.faces.iter().any(|x| x.id == f.face)));
        ui.strong(tr!("Faces : {} choisie(s)", "Faces: {} chosen", self.faces.len()));
        ui.label(egui::RichText::new(crate::langue::t("clic : choisir et prendre la texture · Ctrl : ajouter · Maj : tout le solide · clic droit : appliquer la courante · Alt+clic droit : enrouler depuis la face prise")).small().weak());
        self.section_disp(ui);
        let Some(f) = self.faces.first().and_then(|r| self.session.doc().solide(r.solide).and_then(|s| s.faces.iter().find(|x| x.id == r.face))).cloned() else {
            return;
        };
        let faces = self.faces.clone();
        let cle_faces = format!("{:?}", faces);
        let mut regler: Option<(ReglageFaces, &'static str)> = None;
        let mut fini_glisser = false;
        ui.label(tr!("matériau : {}", "material: {}", f.materiau));
        egui::Grid::new("face_edit").num_columns(3).spacing([6.0, 4.0]).show(ui, |ui| {
            let mut champ = |ui: &mut egui::Ui, nom: &str, val: f64, vitesse: f64, cle: &'static str, faire: &dyn Fn(f64) -> ReglageFaces| {
                let mut v = val;
                let r = ui.add(egui::DragValue::new(&mut v).speed(vitesse).max_decimals(4)).on_hover_text(tr!("{nom} : glisser, molette ou taper une valeur", "{nom}: drag, scroll or type a value"));
                if r.changed() && v != val {
                    regler = Some((faire(v), cle));
                }
                if r.drag_stopped() || r.lost_focus() {
                    fini_glisser = true;
                }
            };
            ui.label(crate::langue::t("échelle"));
            champ(ui, crate::langue::t("échelle X"), f.u.echelle, 0.01, "echelle_u", &|v| ReglageFaces { echelle_u: Some(v), ..Default::default() });
            champ(ui, crate::langue::t("échelle Y"), f.v.echelle, 0.01, "echelle_v", &|v| ReglageFaces { echelle_v: Some(v), ..Default::default() });
            ui.end_row();
            ui.label(crate::langue::t("décalage"));
            champ(ui, crate::langue::t("décalage X"), f.u.decalage, 1.0, "decalage_u", &|v| ReglageFaces { decalage_u: Some(v), ..Default::default() });
            champ(ui, crate::langue::t("décalage Y"), f.v.decalage, 1.0, "decalage_v", &|v| ReglageFaces { decalage_v: Some(v), ..Default::default() });
            ui.end_row();
            ui.label("rotation");
            champ(ui, "rotation", f.rotation, 1.0, "rotation", &|v| ReglageFaces { rotation: Some(v), ..Default::default() });
            ui.label("");
            ui.end_row();
            ui.label(crate::langue::t("lumière"));
            champ(ui, crate::langue::t("unités par luxel (16 par défaut ; moins = plus fin et plus cher)"), f.echelle_lumiere, 1.0, "lumiere", &|v| ReglageFaces { echelle_lumiere: Some(v.round().max(1.0)), ..Default::default() });
            ui.label(crate::langue::t("unités / luxel"));
            ui.end_row();
        });
        if let Some((r, cle)) = regler {
            let c = Commande::ReglerFaces { faces: faces.clone(), reglage: r };
            if let Err(e) = self.session.executer_continu(c, &format!("{cle}{cle_faces}")) {
                self.erreur = Some(e);
            }
        }
        if fini_glisser {
            // la prochaine modification ouvre une nouvelle étape
            self.session.couper_continu();
        }
        ui.horizontal(|ui| {
            ui.label(crate::langue::t("justifier"));
            let mut j = None;
            for (t, x, aide) in [("G", Justif::Gauche, crate::langue::t("gauche")), ("D", Justif::Droite, crate::langue::t("droite")), ("H", Justif::Haut, crate::langue::t("haut")), ("B", Justif::Bas, crate::langue::t("bas")), ("C", Justif::Centre, crate::langue::t("centre")), (crate::langue::t("Ajuster"), Justif::Ajuster, crate::langue::t("une répétition exacte sur la face"))] {
                if ui.small_button(t).on_hover_text(aide).clicked() {
                    j = Some(x);
                }
            }
            ui.checkbox(&mut self.faces_comme_une, crate::langue::t("comme une seule"));
            if let Some(justif) = j {
                let tailles = self.tailles_textures(&faces);
                self.executer(Commande::JustifierFaces { faces: faces.clone(), justif, ensemble: self.faces_comme_une, tailles });
            }
        });
        ui.horizontal(|ui| {
            ui.label(crate::langue::t("aligner sur"));
            if ui.small_button(crate::langue::t("le monde")).clicked() {
                self.executer(Commande::AlignerFaces { faces: faces.clone(), sur: Alignement::Monde });
            }
            if ui.small_button(crate::langue::t("la face")).clicked() {
                self.executer(Commande::AlignerFaces { faces: faces.clone(), sur: Alignement::Face });
            }
            if ui.small_button("Nodraw").on_hover_text(crate::langue::t("tools/toolsnodraw sur les faces choisies, sans changer la texture courante")).clicked() {
                self.executer(Commande::MateriauFaces { faces: faces.clone(), materiau: "TOOLS/TOOLSNODRAW".into() });
            }
            if ui.small_button(crate::langue::t("Remplacer…")).clicked() {
                self.remplacer = Some((f.materiau.clone(), self.materiau.clone(), false));
            }
        });
        // groupes de lissage (masque de bits du VMF)
        let masque: u32 = f.lissage.trim().parse().unwrap_or(0);
        ui.label(crate::langue::t("lissage"));
        let mut nouveau = masque;
        egui::Grid::new("lissage").spacing([2.0, 2.0]).show(ui, |ui| {
            for g in 0..24u32 {
                let mut b = masque & (1 << g) != 0;
                if ui.toggle_value(&mut b, format!("{:>2}", g + 1)).changed() {
                    nouveau = if b { masque | (1 << g) } else { masque & !(1 << g) };
                }
                if g % 8 == 7 {
                    ui.end_row();
                }
            }
        });
        if nouveau != masque {
            self.executer(Commande::ReglerFaces { faces: faces.clone(), reglage: ReglageFaces { lissage: Some(nouveau.to_string()), ..Default::default() } });
        }
        // axes U et V (comme les « UV vectors » de Hammer++)
        ui.collapsing(crate::langue::t("axes U et V"), |ui| {
            for (k, axe) in [("U", f.u.dir), ("V", f.v.dir)] {
                ui.horizontal(|ui| {
                    ui.label(k);
                    let cle = format!("axe{k}");
                    let mut t = self.tampon_axes.get(&cle).cloned().unwrap_or_else(|| format!("{} {} {}", hm_doc::kv::nombre(axe.x), hm_doc::kv::nombre(axe.y), hm_doc::kv::nombre(axe.z)));
                    let r = ui.text_edit_singleline(&mut t);
                    if r.changed() {
                        self.tampon_axes.insert(cle.clone(), t.clone());
                    }
                    if r.lost_focus() {
                        self.tampon_axes.remove(&cle);
                        let v = hm_doc::kv::nombres(&t);
                        if v.len() == 3 && glam::DVec3::new(v[0], v[1], v[2]).length() > 1e-6 {
                            let a = [v[0], v[1], v[2]];
                            let r = if k == "U" { ReglageFaces { axe_u: Some(a), ..Default::default() } } else { ReglageFaces { axe_v: Some(a), ..Default::default() } };
                            self.executer(Commande::ReglerFaces { faces: faces.clone(), reglage: r });
                        }
                    }
                });
            }
        });
        if ui.button(tr!("Appliquer {} aux faces choisies", "Apply {} to the chosen faces", self.materiau)).clicked() {
            let m = self.materiau.clone();
            self.executer(Commande::MateriauFaces { faces, materiau: m });
        }
    }

    /// Taille en texels de la texture de chaque matériau de ces faces (pour justifier).
    fn tailles_textures(&mut self, faces: &[RefFace]) -> Vec<(String, [f64; 2])> {
        let mut out: Vec<(String, [f64; 2])> = Vec::new();
        let Some(g) = &mut self.gfx else { return out };
        for rf in faces {
            if let Some(m) = self.session.doc().solide(rf.solide).and_then(|s| s.faces.iter().find(|x| x.id == rf.face)).map(|f| f.materiau.clone()) {
                if !out.iter().any(|(x, _)| x.eq_ignore_ascii_case(&m)) {
                    let i = g.bib.materiau(&g.r, &self.vfs, &m);
                    out.push((m, [i.taille.0 as f64, i.taille.1 as f64]));
                }
            }
        }
        out
    }

    /// La fenêtre « Remplacer » : un matériau par un autre, dans la sélection ou dans toute la map.
    fn fenetre_remplacer(&mut self, ctx: &egui::Context) {
        let Some((mut ancien, mut nouveau, mut selection)) = self.remplacer.clone() else { return };
        let mut ouvert = true;
        let mut faire = false;
        egui::Window::new(crate::langue::t("Remplacer un matériau")).open(&mut ouvert).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("remplacer").num_columns(2).show(ui, |ui| {
                ui.label(crate::langue::t("chercher"));
                ui.text_edit_singleline(&mut ancien);
                ui.end_row();
                ui.label(crate::langue::t("remplacer par"));
                ui.text_edit_singleline(&mut nouveau);
                ui.end_row();
            });
            ui.checkbox(&mut selection, crate::langue::t("dans la sélection seulement"));
            faire = ui.button(crate::langue::t("Remplacer")).clicked();
        });
        self.remplacer = Some((ancien.clone(), nouveau.clone(), selection));
        if faire && !ancien.trim().is_empty() && !nouveau.trim().is_empty() {
            let objets = selection.then(|| self.session.selection.clone());
            self.executer(Commande::RemplacerMateriau { ancien: ancien.trim().into(), nouveau: nouveau.trim().into(), objets });
            self.remplacer = None;
        }
        if !ouvert {
            self.remplacer = None;
        }
    }

    /// Le matériau courant sur la sélection (faces choisies avec l'outil texture, sinon objets sélectionnés).
    fn appliquer_materiau(&mut self) {
        let m = self.materiau.clone();
        if self.outil == Outil::Texture && !self.faces.is_empty() {
            let faces = self.faces.clone();
            self.executer(Commande::MateriauFaces { faces, materiau: m });
        } else if !self.session.selection.is_empty() {
            let objets = self.session.selection.clone();
            self.executer(Commande::MateriauObjets { objets, materiau: m });
        }
    }

    /// La face d'un solide sous un pixel de la vue 3D : le solide vient de l'image d'identifiants, la face du rayon.
    fn face_sous(&self, i: usize, p: Pos2, ctx: &egui::Context) -> Option<RefFace> {
        let g = self.gfx.as_ref()?;
        let (c, _) = self.vues[i].cibles.as_ref()?;
        let ppp = ctx.pixels_per_point();
        let r = self.vues[i].rect;
        let id = g.r.id_sous(c, ((p.x - r.left()) * ppp) as u32, ((p.y - r.top()) * ppp) as u32);
        let d = self.geo.as_ref()?.objets.get(id as usize).copied().flatten()?;
        let solide = self.session.doc().solide(d.solide?)?;
        // rayon depuis l'œil à travers le pixel
        let v = &self.vues[i];
        let (x, y) = ((p.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (p.y - r.top()) / r.height() * 2.0);
        let f = v.cam.avant();
        let droite = v.cam.droite();
        let haut = droite.cross(f);
        let t = (hm_render::fov_y_from_game(v.cam.fov).to_radians() * 0.5).tan();
        let dir = (f + droite * x * t * (r.width() / r.height()) + haut * y * t).normalize().as_dvec3();
        let o = v.cam.pos.as_dvec3();
        let mut meilleur: Option<(f64, u64)> = None;
        for face in &solide.faces {
            let n = face.normale();
            let dn = n.dot(dir);
            if dn >= -1e-9 {
                continue;
            }
            // l'entrée dans le convexe : le plus lointain des plans qu'on traverse en entrant
            let tt = (face.plan[0] - o).dot(n) / dn;
            if meilleur.map_or(true, |(m, _)| tt > m) {
                meilleur = Some((tt, face.id));
            }
        }
        meilleur.map(|(_, face)| RefFace { solide: solide.id, face })
    }

    fn clic_texture(&mut self, i: usize, p: Pos2, ctx: &egui::Context, droit: bool) {
        let Some(rf) = self.face_sous(i, p, ctx) else { return };
        if droit {
            let alt = ctx.input(|x| x.modifiers.alt);
            match (alt, self.face_prise) {
                // Alt+clic droit : la texture de la face prise, enroulée sur celle-ci (continue au pli)
                (true, Some(src)) if src != rf => self.executer(Commande::EnroulerFaces { source: src, cibles: vec![rf] }),
                _ => {
                    let m = self.materiau.clone();
                    self.executer(Commande::MateriauFaces { faces: vec![rf], materiau: m });
                }
            }
            return;
        }
        // prendre la texture de la face (mode « Lift+Select » de Hammer)
        if let Some(m) = self.session.doc().solide(rf.solide).and_then(|s| s.faces.iter().find(|x| x.id == rf.face)).map(|f| f.materiau.clone()) {
            self.materiau = m.clone();
            self.navigateur.noter_recent(&m);
        }
        self.face_prise = Some(rf);
        let (ctrl, shift) = ctx.input(|x| (x.modifiers.command, x.modifiers.shift));
        let toutes: Vec<RefFace> = if shift { self.session.doc().solide(rf.solide).map(|s| s.faces.iter().map(|f| RefFace { solide: s.id, face: f.id }).collect()).unwrap_or_default() } else { vec![rf] };
        if ctrl {
            for f in toutes {
                if let Some(k) = self.faces.iter().position(|x| *x == f) {
                    self.faces.remove(k);
                } else {
                    self.faces.push(f);
                }
            }
        } else {
            self.faces = toutes;
        }
        if let Some(m) = self.faces.first().and_then(|r| self.session.doc().solide(r.solide).and_then(|s| s.faces.iter().find(|x| x.id == r.face))).map(|f| f.materiau.clone()) {
            self.statut = tr!("face choisie : {m}", "chosen face: {m}");
        }
    }

    /// Ouvre la fenêtre « Propriétés de l'objet » sur la sélection.
    fn ouvrir_proprietes(&mut self) {
        if self.fenetre_proprietes.is_none() {
            self.fenetre_proprietes = Some(self.session.historique().faits().len());
        }
    }

    /// La fenêtre « Propriétés de l'objet » de Hammer : le panneau des propriétés de la sélection (clés, drapeaux,
    /// sorties, entrées), appliquées au fil de la saisie comme partout ici ; « Annuler » défait ce qui a été changé
    /// depuis son ouverture, « Fermer » le garde.
    fn fenetre_proprietes(&mut self, ctx: &egui::Context) {
        let Some(depart) = self.fenetre_proprietes else { return };
        if self.session.selection.is_empty() {
            self.fenetre_proprietes = None;
            return;
        }
        let titre = {
            let doc = self.session.doc();
            let ents: Vec<&hm_doc::Entite> = self.session.selection.iter().filter_map(|r| if let Ref::Entite(e) = r { doc.entite(*e) } else { None }).collect();
            match ents.as_slice() {
                [e] => match e.get("targetname").filter(|n| !n.is_empty()) {
                    Some(n) => tr!("Propriétés de l'objet : {n} - {}", "Object Properties: {n} - {}", e.classe()),
                    None => tr!("Propriétés de l'objet : {}", "Object Properties: {}", e.classe()),
                },
                [] => crate::langue::t("Propriétés de l'objet").to_string(),
                l => tr!("Propriétés de l'objet : {} entités", "Object Properties: {} entities", l.len()),
            }
        };
        let (mut ouverte, mut annuler, mut fermer) = (true, false, false);
        egui::Window::new(titre)
            .id(egui::Id::unique("fenetre_proprietes"))
            .open(&mut ouverte)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([520.0, 480.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| self.panneau_proprietes_contenu(ui));
                ui.separator();
                let changes = self.session.historique().faits().len().saturating_sub(depart);
                ui.horizontal(|ui| {
                    if ui.add_enabled(changes > 0, egui::Button::new(crate::langue::t("Annuler"))).on_hover_text(tr!("défaire les {changes} changement(s) faits depuis l'ouverture de la fenêtre", "undo the {changes} change(s) made since the window was opened")).clicked() {
                        annuler = true;
                    }
                    if ui.button(crate::langue::t("Fermer")).on_hover_text(crate::langue::t("garder les changements (déjà appliqués, annulables par Ctrl+Z)")).clicked() {
                        fermer = true;
                    }
                });
            });
        if annuler {
            while self.session.historique().faits().len() > depart {
                if self.session.annuler().is_none() {
                    break;
                }
            }
            self.statut = crate::langue::t("propriétés : changements annulés").into();
        }
        if !ouverte || annuler || fermer {
            self.fenetre_proprietes = None;
        }
    }

    fn panneau_proprietes(&mut self, ui: &mut egui::Ui) {
        if self.fenetre_proprietes.is_some() {
            ui.label(crate::langue::t("Les propriétés sont ouvertes dans leur fenêtre."));
            return;
        }
        self.panneau_proprietes_contenu(ui);
    }

    fn panneau_proprietes_contenu(&mut self, ui: &mut egui::Ui) {
        if self.outil == Outil::Entite {
            ui.strong(crate::langue::t("Outil entité"));
            ui.label(crate::langue::t("Clic dans la vue 3D : posée sur la surface visée. Clic dans une vue 2D : posée à la grille."));
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Classe"));
                ui.add(egui::TextEdit::singleline(&mut self.proprietes.filtre_classes).hint_text(crate::langue::t("chercher")).desired_width(160.0));
            });
            ui.label(tr!("courante : {}", "current: {}", self.classe_entite));
            let f = self.proprietes.filtre_classes.to_ascii_lowercase();
            let mut choisie = None;
            egui::ScrollArea::vertical().id_salt("classes").max_height(180.0).show(ui, |ui| {
                for c in self.fgd.classes(false) {
                    if f.split_whitespace().all(|w| c.to_ascii_lowercase().contains(w)) && ui.selectable_label(c == self.classe_entite, c).clicked() {
                        choisie = Some(c.to_string());
                    }
                }
            });
            if let Some(c) = choisie {
                self.classe_entite = c;
            }
            ui.separator();
        }
        let ents: Vec<u64> = self.session.selection.iter().filter_map(|r| if let Ref::Entite(e) = r { Some(*e) } else { None }).collect();
        if ents.is_empty() && !self.session.selection.is_empty() {
            ui.label(crate::langue::t("La sélection n'a pas d'entité : rien sélectionné montre les propriétés du monde."));
            return;
        }
        if ents.len() == 1 {
            self.onglets_proprietes(ui, ents[0]);
            match self.vue_proprietes {
                io_ui::VueProprietes::Sorties => {
                    self.panneau_sorties(ui, ents[0]);
                    return;
                }
                io_ui::VueProprietes::Entrees => {
                    self.panneau_entrees(ui, ents[0]);
                    return;
                }
                io_ui::VueProprietes::Cles => {}
            }
            self.section_instance(ui, ents[0]);
            self.section_modele(ui, ents[0]);
        }
        let faces: Vec<u64> = self.faces.iter().map(|f| f.face).collect();
        let cmds = self.proprietes.ui(ui, self.session.doc(), &self.fgd, &ents, &faces);
        match cmds.len() {
            0 => {}
            1 => self.executer(cmds.into_iter().next().unwrap()),
            _ => self.executer(Commande::Lot { nom: crate::langue::t("propriétés").into(), commandes: cmds }),
        }
    }


    /// La fenêtre « Compiler » (F9) et le journal en bas de la fenêtre.
    fn compilation(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        if let Some(rx) = self.jeu_lance.take() {
            let mut fini = false;
            loop {
                match rx.try_recv() {
                    Ok(m) => {
                        let grave = if m.contains("impossible") || m.contains("non envoyée") || m.starts_with("cannot ") || m.contains("not sent") { 2 } else { 1 };
                        self.statut = m.clone();
                        self.journal.push((m, grave));
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        fini = true;
                        break;
                    }
                }
            }
            if !fini {
                self.jeu_lance = Some(rx);
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        // messages arrivés
        if let Some(c) = &self.compil {
            let mut fini = None;
            while let Ok(m) = c.rx.try_recv() {
                match m {
                    hm_compile::Message::Etape(e) => {
                        self.journal.push((format!("—— {e} ——"), 1));
                        self.etape = e;
                    }
                    hm_compile::Message::Ligne(l) => {
                        let g = if hm_compile::grave(&l) { 2 } else { 0 };
                        self.journal.push((l, g));
                    }
                    hm_compile::Message::Fuite(p) => {
                        self.fuite = Some(p);
                        self.version_geo = None;
                    }
                    hm_compile::Message::Fin { reussi, bsp, secondes, erreur } => fini = Some((reussi, bsp, secondes, erreur)),
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            if let Some((reussi, bsp, secondes, erreur)) = fini {
                self.compil = None;
                self.derniere_compil = Some((reussi, secondes, erreur.clone(), bsp));
                self.portails = hm_compile::lire_portails(&hm_compile::dossier_compile(self.session.chemin()).join(format!("{}.prt", self.nom_en_cours)));
                if reussi {
                    self.journal.push((tr!("compilation réussie en {secondes:.1} s", "compiled successfully in {secondes:.1} s"), 1));
                    self.statut = tr!("compilée en {secondes:.1} s", "compiled in {secondes:.1} s");
                    if self.lancer_jeu {
                        if let Some(o) = &self.outils {
                            self.jeu_lance = Some(o.lancer_jeu(&self.nom_en_cours));
                        }
                    }
                } else {
                    let e = erreur.unwrap_or_else(|| crate::langue::t("échec").into());
                    self.journal.push((tr!("ÉCHEC : {e}", "FAILED: {e}"), 2));
                    self.erreur = Some(tr!("compilation : {e}", "compile: {e}"));
                }
            }
        }
        if self.fenetre_compil {
            let mut ouverte = true;
            let mut choix_exe: Option<(usize, Option<PathBuf>)> = None;
            egui::Window::new(crate::langue::t("Compiler")).open(&mut ouverte).collapsible(false).resizable(false).show(ctx, |ui| {
                match &self.outils {
                    None => {
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 100), crate::langue::t("Garry's Mod introuvable : pas de vbsp, vvis, vrad."));
                    }
                    Some(o) => {
                        ui.label(tr!("outils : {}", "tools: {}", o.source));
                        // l'exécutable de chaque étape (VVIS++, VRAD++… à la place de ceux du jeu), gardé pour la suite
                        egui::Grid::new("executables").num_columns(4).show(ui, |ui| {
                            for (i, (etape, chemin)) in [("vbsp", &o.vbsp), ("vvis", &o.vvis), ("vrad", &o.vrad)].into_iter().enumerate() {
                                ui.label(etape);
                                ui.label(chemin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).on_hover_text(chemin.display().to_string());
                                if ui.small_button(crate::langue::t("Choisir…")).clicked() {
                                    if let Some(p) = rfd::FileDialog::new().add_filter(crate::langue::t("Programme"), &["exe"]).set_title(tr!("Exécutable de {etape}", "{etape} executable")).pick_file() {
                                        choix_exe = Some((i, Some(p)));
                                    }
                                }
                                if ui.small_button(crate::langue::t("Défaut")).clicked() {
                                    choix_exe = Some((i, None));
                                }
                                ui.end_row();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(crate::langue::t("nom de la map"));
                            ui.text_edit_singleline(&mut self.nom_compile);
                        });
                        for m in [hm_compile::Mode::Geometrie, hm_compile::Mode::Rapide, hm_compile::Mode::Normal, hm_compile::Mode::Final] {
                            ui.radio_value(&mut self.mode_compile, m, m.nom());
                        }
                        ui.checkbox(&mut self.copier_bsp, tr!("copier le .bsp dans {}", "copy the .bsp to {}", o.dossier_maps.display()));
                        ui.checkbox(&mut self.embarquer, crate::langue::t("embarquer dans le .bsp ce que les joueurs n'ont pas")).on_hover_text(crate::langue::t("matériaux, textures et modèles employés qui ne viennent pas du jeu de base (addons, dossiers locaux, autres jeux), par bspzip"));
                        ui.checkbox(&mut self.lancer_jeu, crate::langue::t("lancer le jeu sur la map après"));
                        ui.checkbox(&mut self.compil_partielle, crate::langue::t("ne refaire que ce qui a changé")).on_hover_text(crate::langue::t("seulement des entités : vbsp -onlyents (quelques secondes) ; des lumières : -onlyents puis vrad ; le reste (solides, props, overlays, cubemaps…) : tout"));
                        // ce qui ne sera pas compilé, dit avant de lancer (le piège des visgroups oubliés)
                        let doc = self.session.doc();
                        let caches = doc.tous_les_solides().filter(|(p, s)| *p == hm_doc::Proprio::Monde && s.cache).count() + doc.entites().filter(|e| e.cache).count();
                        let orange = egui::Color32::from_rgb(255, 170, 60);
                        if caches > 0 {
                            ui.colored_label(orange, tr!("{caches} objet(s) dans des visgroups décochés ne seront pas compilés", "{caches} object(s) in unchecked visgroups will not be compiled"));
                        }
                        match doc.cordons().boites_actives().as_slice() {
                            [] => {}
                            [(a, b)] => {
                                ui.colored_label(orange, tr!("cordon actif : seul ce qui touche ({}, {}, {}) – ({}, {}, {}) est compilé", "active cordon: only what touches ({}, {}, {}) – ({}, {}, {}) is compiled", a.x, a.y, a.z, b.x, b.y, b.z));
                            }
                            v => {
                                ui.colored_label(orange, tr!("cordon actif : seul ce qui touche une de ses {} boîtes est compilé", "active cordon: only what touches one of its {} boxes is compiled", v.len()));
                            }
                        }
                        let pret = self.compil.is_none() && !self.nom_compile.trim().is_empty() && self.nom_compile.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
                        if ui.add_enabled(pret, egui::Button::new(crate::langue::t("Compiler"))).clicked() {
                            self.demarrer_compilation();
                            self.fenetre_compil = false;
                        }
                    }
                }
            });
            if let Some((i, p)) = choix_exe {
                self.choisir_executable(i, p);
            }
            if !ouverte {
                self.fenetre_compil = false;
            }
        }
        if self.compil.is_some() || !self.journal.is_empty() {
            egui::Panel::bottom("journal").resizable(true).default_size(180.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(crate::langue::t("Compilation"));
                    if self.compil.is_some() {
                        ui.spinner();
                        ui.label(tr!("en cours : {}", "running: {}", self.etape));
                        if ui.button(crate::langue::t("Arrêter")).clicked() {
                            if let Some(c) = &self.compil {
                                c.arreter();
                            }
                        }
                    }
                    if let Some(f) = &self.fuite {
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 100), tr!("fuite : {} points", "leak: {} points", f.len()));
                        if ui.button(crate::langue::t("Voir la fuite")).clicked() {
                            let lo = f.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(Vec3::from(*p)));
                            let hi = f.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(Vec3::from(*p)));
                            for v in &mut self.vues {
                                v.cadrer(lo - Vec3::splat(64.0), hi + Vec3::splat(64.0));
                            }
                        }
                        if ui.button(crate::langue::t("Oublier la fuite")).clicked() {
                            self.fuite = None;
                        }
                    }
                    if let Some(p) = &self.portails {
                        ui.checkbox(&mut self.portails_visibles, tr!("Portails ({})", "Portals ({})", p.len())).on_hover_text(crate::langue::t("les portails de vbsp, entre les visleafs : où poser des hints"));
                    }
                    ui.checkbox(&mut self.journal_tout, crate::langue::t("tout le journal"));
                    if ui.button(crate::langue::t("Fermer")).clicked() && self.compil.is_none() {
                        self.journal.clear();
                    }
                });
                egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                    for (l, g) in &self.journal {
                        if !self.journal_tout && *g == 0 {
                            continue;
                        }
                        let t = egui::RichText::new(l).monospace().small();
                        ui.label(match g {
                            2 => t.color(egui::Color32::from_rgb(255, 130, 110)),
                            1 => t.strong(),
                            _ => t,
                        });
                    }
                });
            });
        }
    }

    pub(crate) fn demarrer_compilation(&mut self) {
        let Some(o) = self.outils.clone() else { return };
        self.journal.clear();
        self.erreur = None;
        self.fuite = None;
        self.portails = None;
        self.version_geo = None;
        let dossier = hm_compile::dossier_compile(self.session.chemin());
        let zone = self.zone_compil.take();
        self.nom_en_cours = match zone {
            Some(_) => format!("{}_zone", self.nom_compile.trim()),
            None => self.nom_compile.trim().to_string(),
        };
        self.journal.push((tr!("map « {} », compilation {}, dans {}", "map \"{}\", {} compile, in {}", self.nom_en_cours, self.mode_compile.nom(), dossier.display()), 1));
        if let Some((lo, hi)) = zone {
            self.journal.push((tr!("zone seule : ({:.0}, {:.0}, {:.0}) – ({:.0}, {:.0}, {:.0}), comme sous un cordon ; le cordon de la map n'est pas touché", "zone only: ({:.0}, {:.0}, {:.0}) – ({:.0}, {:.0}, {:.0}), as under a cordon; the map's cordon is not touched", lo.x, lo.y, lo.z, hi.x, hi.y, hi.z), 1));
        }
        // une zone sert à essayer vite : rien à embarquer
        let fichiers = if self.embarquer && zone.is_none() {
            let config = self.session.chemin().and_then(|c| hm_regles::Config::du_projet(c).ok()).map(|(c, _)| c).unwrap_or_default();
            hm_edit::embarque::fichiers(self.session.doc(), &self.vfs, &config)
        } else {
            Vec::new()
        };
        if !fichiers.is_empty() {
            self.journal.push((tr!("{} fichier(s) à embarquer : {}", "{} file(s) to embed: {}", fichiers.len(), fichiers.iter().map(|(r, _)| r.as_str()).collect::<Vec<_>>().join(", ")), 1));
        }
        let vmf = match zone {
            Some((lo, hi)) => self.session.doc().pour_compilation_zone(&o.texture_cordon, self.session.chemin(), &dossier, lo.to_array(), hi.to_array()),
            None => self.session.doc().pour_compilation_dans(&o.texture_cordon, self.session.chemin(), &dossier),
        };
        // ne refaire que ce qui a changé depuis la compilation précédente de ce nom, dans ce mode
        let portee = match hm_compile::precedente(&dossier, &self.nom_en_cours, self.mode_compile) {
            Some(avant) if self.compil_partielle => hm_compile::portee(&avant, &vmf),
            _ => hm_compile::Portee::Tout,
        };
        self.compil = Some(hm_compile::lancer_avec(vmf, &self.nom_en_cours, &dossier, &o, self.mode_compile, self.copier_bsp, fichiers, portee));
    }

    /// Compilation rapide d'une zone : la boîte de la sélection (et 64 autour), seule, en mode rapide.
    fn compiler_zone(&mut self) {
        if self.compil.is_some() {
            return;
        }
        let Some((lo, hi)) = hm_doc::boite_de(self.session.doc(), &self.session.selection) else {
            self.statut = crate::langue::t("compiler une zone : rien de sélectionné").into();
            return;
        };
        let m = glam::DVec3::splat(64.0);
        self.zone_compil = Some((lo - m, hi + m));
        self.mode_compile = hm_compile::Mode::Rapide;
        self.demarrer_compilation();
    }

    /// Pose une entité de la classe courante, avec les valeurs par défaut de son FGD.
    fn poser_entite(&mut self, origine: Vec3) {
        let classe = self.classe_entite.clone();
        let cles = self.cles_entite_posee(None);
        self.executer(Commande::CreerEntite { classe, origine: origine.as_dvec3().to_array(), cles });
        self.onglet = Onglet::Proprietes;
    }

    /// Le point de la surface sous un pixel de la vue 3D (rayon contre le solide désigné), sinon devant la caméra.
    fn point_sous(&self, i: usize, p: Pos2, ctx: &egui::Context) -> Vec3 {
        let v = &self.vues[i];
        let r = v.rect;
        let (x, y) = ((p.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (p.y - r.top()) / r.height() * 2.0);
        let f = v.cam.avant();
        let droite = v.cam.droite();
        let haut = droite.cross(f);
        let t = (hm_render::fov_y_from_game(v.cam.fov).to_radians() * 0.5).tan();
        let dir = (f + droite * x * t * (r.width() / r.height()) + haut * y * t).normalize();
        if let Some(rf) = self.face_sous(i, p, ctx) {
            if let Some(face) = self.session.doc().solide(rf.solide).and_then(|s| s.faces.iter().find(|x| x.id == rf.face)) {
                let n = face.normale().as_vec3();
                let dn = n.dot(dir);
                if dn.abs() > 1e-6 {
                    let tt = (face.plan[0].as_vec3() - v.cam.pos).dot(n) / dn;
                    if tt > 0.0 {
                        return v.cam.pos + dir * tt;
                    }
                }
            }
        }
        v.cam.pos + dir * 256.0
    }


    /// Boîte que les poignées manipulent : le bloc en cours de tracé, sinon la sélection.
    fn boite_poignees(&self) -> Option<(Vec3, Vec3)> {
        if self.outil == Outil::Bloc {
            return self.bloc;
        }
        if self.outil != Outil::Selection {
            return None;
        }
        hm_doc::boite_de(self.session.doc(), &self.session.selection).map(|(a, b)| (a.as_vec3(), b.as_vec3()))
    }

    /// Les gestes du bouton gauche dans une vue 2D.
    fn gestes_2d(&mut self, i: usize, rep: &egui::Response, ctx: &egui::Context) {
        if self.outil == Outil::Decoupe {
            self.gestes_decoupe(i, rep, ctx);
            return;
        }
        if self.outil == Outil::Sommets {
            self.gestes_sommets(i, rep, ctx);
            return;
        }
        let v = &self.vues[i];
        let genre = v.genre;
        let (a, b, c) = genre.axes();
        let alt = ctx.input(|x| x.modifiers.alt);
        let aimant = self.aimant && !alt;
        let pas = self.grille;
        if rep.drag_started_by(egui::PointerButton::Primary) {
            let Some(depart) = ctx.input(|x| x.pointer.press_origin()) else { return };
            let w0 = v.ecran_vers_monde(depart);
            let shift = ctx.input(|x| x.modifiers.shift);
            let mut g = None;
            if let Some((lo, hi)) = self.boite_poignees() {
                let r = gestes::rect_ecran(v, lo, hi);
                let mode = if self.outil == Outil::Bloc { ModePoignees::Taille } else { self.mode_poignees };
                // un clic dans l'objet le déplace, même petit : ses poignées, posées autour, ne doivent pas le cacher
                if self.outil == Outil::Selection && r.contains(depart) {
                    g = Some(Geste::Deplacer { vue: i, debut: w0, delta: Vec3::ZERO, cloner: shift });
                }
                for (cote, pos) in gestes::poignees(r, mode).into_iter().filter(|_| g.is_none()) {
                    if (gestes::ecarter(r, pos, cote) - depart).length() <= 7.0 {
                        g = Some(match mode {
                            ModePoignees::Taille => Geste::Taille { cote, boite0: (lo, hi), boite: (lo, hi) },
                            ModePoignees::Rotation => {
                                // pivot (Ctrl+P) : l'origine de l'entité seule sélectionnée plutôt que le milieu de la boîte
                                let origine = match (self.pivot_origine, self.session.selection.as_slice()) {
                                    (true, [Ref::Entite(e)]) => self.session.doc().entite(*e).and_then(|e| e.origine()).map(|o| o.as_vec3()),
                                    _ => None,
                                };
                                let centre = origine.unwrap_or((lo + hi) * 0.5);
                                Geste::Tourner { vue: i, centre, depart: (w0.y - centre[b]).atan2(w0.x - centre[a]), angle: 0.0 }
                            }
                        });
                        break;
                    }
                }
                if g.is_none() && self.outil == Outil::Selection && r.expand(2.0).contains(depart) {
                    g = Some(Geste::Deplacer { vue: i, debut: w0, delta: Vec3::ZERO, cloner: shift });
                }
            }
            if g.is_none() {
                g = Some(match self.outil {
                    Outil::Bloc => Geste::Bloc { vue: i, debut: w0, fin: w0 },
                    _ => Geste::Cadre { vue: i, debut: w0, fin: w0 },
                });
            }
            self.geste = g;
        }
        if rep.dragged_by(egui::PointerButton::Primary) {
            let Some(pos) = rep.interact_pointer_pos() else { return };
            let w = v.ecran_vers_monde(pos);
            let shift = ctx.input(|x| x.modifiers.shift);
            match &mut self.geste {
                Some(Geste::Cadre { fin, .. }) | Some(Geste::Bloc { fin, .. }) => *fin = w,
                Some(Geste::Deplacer { debut, delta, .. }) => {
                    let d = w - *debut;
                    *delta = Vec3::ZERO;
                    delta[a] = gestes::aimanter(d.x, pas, aimant);
                    delta[b] = gestes::aimanter(d.y, pas, aimant);
                }
                Some(Geste::Taille { cote, boite0, boite, .. }) => *boite = gestes::tirer(*boite0, genre, *cote, w, pas, aimant),
                Some(Geste::Tourner { centre, depart, angle, .. }) => {
                    let ang = (w.y - centre[b]).atan2(w.x - centre[a]);
                    let deg = (ang - *depart).to_degrees();
                    // par pas de 15° (Hammer), libre avec Maj
                    *angle = if shift { deg } else { (deg / 15.0).round() * 15.0 };
                }
                _ => {}
            }
        }
        if rep.drag_stopped_by(egui::PointerButton::Primary) {
            let Some(g) = self.geste.take() else { return };
            let sel = self.session.selection.clone();
            match g {
                Geste::Cadre { debut, fin, .. } => {
                    let (lo, hi) = (debut.min(fin), debut.max(fin));
                    let entierement = ctx.input(|x| x.modifiers.shift);
                    let mut choix: Vec<Ref> = Vec::new();
                    if let Some(geo) = &self.geo {
                        for f in &geo.formes {
                            let (fl, fh) = (Vec2::new(f.min[a], f.min[b]), Vec2::new(f.max[a], f.max[b]));
                            let touche = fl.x <= hi.x && fh.x >= lo.x && fl.y <= hi.y && fh.y >= lo.y;
                            let dedans = fl.x >= lo.x && fh.x <= hi.x && fl.y >= lo.y && fh.y <= hi.y;
                            if (entierement && dedans) || (!entierement && touche) {
                                let r = self.a_selectionner(f.designe);
                                if !choix.contains(&r) {
                                    choix.push(r);
                                }
                            }
                        }
                    }
                    if ctx.input(|x| x.modifiers.command) {
                        for r in choix {
                            if !self.session.selection.contains(&r) {
                                self.session.selection.push(r);
                            }
                        }
                    } else {
                        self.session.selection = choix;
                    }
                    self.mode_poignees = ModePoignees::Taille;
                    self.statut = tr!("sélection : {} objet(s)", "selection: {} object(s)", self.session.selection.len());
                }
                Geste::Bloc { debut, fin, .. } => {
                    let (mut lo, mut hi) = (Vec3::ZERO, Vec3::ZERO);
                    let (x0, x1) = (gestes::aimanter(debut.x.min(fin.x), pas, aimant), gestes::aimanter(debut.x.max(fin.x), pas, aimant));
                    let (y0, y1) = (gestes::aimanter(debut.y.min(fin.y), pas, aimant), gestes::aimanter(debut.y.max(fin.y), pas, aimant));
                    if x1 - x0 >= 1.0 && y1 - y0 >= 1.0 {
                        lo[a] = x0;
                        hi[a] = x1;
                        lo[b] = y0;
                        hi[b] = y1;
                        lo[c] = self.profondeur_bloc.0;
                        hi[c] = self.profondeur_bloc.1;
                        self.bloc = Some((lo, hi));
                        self.statut = crate::langue::t("bloc tracé : ajuster ses poignées dans les autres vues, Entrée pour le créer, Échap pour l'abandonner").into();
                    }
                }
                Geste::Deplacer { delta, cloner, .. } => {
                    if delta != Vec3::ZERO {
                        let d = delta.as_dvec3().to_array();
                        if cloner {
                            self.executer(Commande::Dupliquer { objets: sel, delta: d });
                        } else {
                            self.executer(Commande::Deplacer { objets: sel, delta: d, verrou_texture: self.verrou_texture });
                        }
                    }
                }
                Geste::Taille { boite0, boite, .. } => {
                    if self.outil == Outil::Bloc {
                        self.bloc = Some(boite);
                        self.profondeur_bloc = (boite.0[c], boite.1[c]);
                    } else if boite != boite0 {
                        self.executer(Commande::Redimensionner { objets: sel, de: [boite0.0.as_dvec3().to_array(), boite0.1.as_dvec3().to_array()], vers: [boite.0.as_dvec3().to_array(), boite.1.as_dvec3().to_array()], verrou_texture: self.verrou_texture });
                    }
                }
                Geste::Tourner { vue, centre, angle, .. } => {
                    if angle != 0.0 {
                        let axe = gestes::axe_rotation(self.vues[vue].genre).to_array();
                        self.executer(Commande::Tourner { objets: sel, centre: centre.as_dvec3().to_array(), axe, degres: angle as f64, verrou_texture: self.verrou_texture });
                    }
                }
                Geste::Fleche { .. } => {}
            }
        }
    }

    /// Le manipulateur 3D : trois flèches au centre de la sélection ; en tirer une déplace le long de son axe.
    /// Les axes du manipulateur : ceux du monde, ou (repère local, J) ceux de l'entité seule sélectionnée.
    fn axes_gizmo(&self) -> [Vec3; 3] {
        if self.gizmo_local {
            if let [Ref::Entite(e)] = self.session.selection.as_slice() {
                if let Some(a) = self.session.doc().entite(*e).map(|e| e.angles()) {
                    let m = hm_geom::angles_to_mat(a.x, a.y, a.z);
                    return [m.x_axis.as_vec3(), m.y_axis.as_vec3(), m.z_axis.as_vec3()];
                }
            }
        }
        [Vec3::X, Vec3::Y, Vec3::Z]
    }

    fn fleches_3d(&self, i: usize, ctx: &egui::Context) -> Option<(Pos2, [Pos2; 3])> {
        if self.outil != Outil::Selection || self.session.selection.is_empty() {
            return None;
        }
        let (lo, hi) = hm_doc::boite_de(self.session.doc(), &self.session.selection)?;
        let v = &self.vues[i];
        let c = ((lo + hi) * 0.5).as_vec3();
        let (w, h) = (v.rect.width(), v.rect.height());
        let cam = v.camera();
        let proj = |p: Vec3| cam.project(p, w as u32, h as u32).map(|(x, y)| Pos2::new(v.rect.left() + x, v.rect.top() + y));
        let o = proj(c)?;
        // longueur à l'écran constante : un pas d'axe vaut 70 points
        let dist = (c - v.cam.pos).length().max(1.0);
        let l = dist * 0.12;
        let _ = ctx;
        let ax = self.axes_gizmo();
        Some((o, [proj(c + ax[0] * l)?, proj(c + ax[1] * l)?, proj(c + ax[2] * l)?]))
    }

    fn gestes_3d(&mut self, i: usize, rep: &egui::Response, ctx: &egui::Context) -> bool {
        let Some((o, bouts)) = self.fleches_3d(i, ctx) else { return false };
        if rep.drag_started_by(egui::PointerButton::Primary) {
            let Some(p) = ctx.input(|x| x.pointer.press_origin()) else { return false };
            for (axe, bout) in bouts.iter().enumerate() {
                if distance_segment(p, o, *bout) < 6.0 {
                    self.geste = Some(Geste::Fleche { axe, vecteur: self.axes_gizmo()[axe], depart: p, delta: 0.0 });
                    return true;
                }
            }
        }
        if let Some(Geste::Fleche { axe, depart, delta, .. }) = &mut self.geste {
            if rep.dragged_by(egui::PointerButton::Primary) {
                if let Some(p) = rep.interact_pointer_pos() {
                    let bout = bouts[*axe];
                    let dir = bout - o;
                    let l2 = dir.length_sq().max(1.0);
                    // unités du monde par point d'écran le long de la flèche
                    let (lo, hi) = hm_doc::boite_de(self.session.doc(), &self.session.selection).unwrap_or_default();
                    let c = ((lo + hi) * 0.5).as_vec3();
                    let l = (c - self.vues[i].cam.pos).length().max(1.0) * 0.12;
                    let t = (p - *depart).dot(dir) / l2 * l;
                    let alt = ctx.input(|x| x.modifiers.alt);
                    *delta = gestes::aimanter(t, self.grille, self.aimant && !alt);
                }
            }
            if rep.drag_stopped_by(egui::PointerButton::Primary) {
                let Some(Geste::Fleche { vecteur, delta, .. }) = self.geste.take() else { return true };
                if delta != 0.0 {
                    let d = (vecteur * delta).as_dvec3().to_array();
                    let sel = self.session.selection.clone();
                    self.executer(Commande::Deplacer { objets: sel, delta: d, verrou_texture: self.verrou_texture });
                }
            }
            return true;
        }
        false
    }

    /// Ce qui se dessine par-dessus les vues : poignées, cadre, bloc en cours, flèches 3D.
    fn dessiner_dessus(&self, ctx: &egui::Context, peintre: &egui::Painter) {
        let jaune = Color32::from_rgb(255, 220, 60);
        for (i, v) in self.vues.iter().enumerate() {
            if !v.rect.is_positive() {
                continue;
            }
            let p = peintre.with_clip_rect(v.rect);
            if v.genre == Genre::Camera3D {
                if self.afficher_stats {
                    let dt = ctx.input(|x| x.stable_dt).max(1e-4);
                    let (s, t, f) = self.geo.as_ref().map_or((0, 0, 0), |g| (g.sommets.len(), g.lots.iter().map(|l| l.indices.len() / 3).sum::<usize>(), g.formes.len()));
                    let texte = tr!("{:.0} images/s ({:.1} ms)\n{s} sommets, {t} triangles\n{f} objets dessinés\ncaméra ({:.0} {:.0} {:.0})", "{:.0} fps ({:.1} ms)\n{s} vertices, {t} triangles\n{f} objects drawn\ncamera ({:.0} {:.0} {:.0})", 1.0 / dt, dt * 1000.0, v.cam.pos.x, v.cam.pos.y, v.cam.pos.z);
                    p.text(v.rect.left_bottom() + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, texte, egui::FontId::monospace(11.0), Color32::from_rgb(255, 230, 120));
                }
                if let Some((o, bouts)) = self.fleches_3d(i, ctx) {
                    for (k, b) in bouts.iter().enumerate() {
                        let c = [Color32::from_rgb(230, 60, 60), Color32::from_rgb(60, 210, 60), Color32::from_rgb(70, 120, 255)][k];
                        let actif = matches!(self.geste, Some(Geste::Fleche { axe, .. }) if axe == k);
                        p.line_segment([o, *b], Stroke::new(if actif { 4.0 } else { 2.5 }, c));
                        p.circle_filled(*b, 4.0, c);
                    }
                }
                continue;
            }
            self.dessiner_fond(i, &p);
            self.dessiner_polygone(i, &p);
            self.dessiner_noms(i, &p);
            self.dessiner_poignees_sommets(i, &p);
            // poignées de la sélection ou du bloc en cours
            let boite = match &self.geste {
                Some(Geste::Taille { boite, .. }) => Some(*boite),
                _ => self.boite_poignees(),
            };
            if let Some((lo, hi)) = boite {
                let r = gestes::rect_ecran(v, lo, hi);
                let mode = if self.outil == Outil::Bloc { ModePoignees::Taille } else { self.mode_poignees };
                if self.outil == Outil::Bloc {
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, jaune), StrokeKind::Middle);
                }
                for (cote, pos) in gestes::poignees(r, mode) {
                    let q = gestes::ecarter(r, pos, cote);
                    match mode {
                        ModePoignees::Taille => {
                            p.rect_filled(Rect::from_center_size(q, egui::vec2(6.0, 6.0)), 0.0, Color32::WHITE);
                            p.rect_stroke(Rect::from_center_size(q, egui::vec2(6.0, 6.0)), 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);
                        }
                        ModePoignees::Rotation => {
                            p.circle_filled(q, 4.0, Color32::WHITE);
                            p.circle_stroke(q, 4.0, Stroke::new(1.0, Color32::BLACK));
                        }
                    }
                }
                // cotes du bloc ou de la sélection
                let (a, b, _) = v.genre.axes();
                let txt = format!("{} × {}", carte_ui::longueur((hi[a] - lo[a]) as f64, self.carte.unites), carte_ui::longueur((hi[b] - lo[b]) as f64, self.carte.unites));
                p.text(r.center_top() - egui::vec2(0.0, 10.0), egui::Align2::CENTER_BOTTOM, txt, egui::FontId::monospace(11.0), Color32::from_gray(220));
            }
            match &self.geste {
                Some(Geste::Cadre { vue, debut, fin }) if *vue == i => {
                    let r = Rect::from_two_pos(v.monde_vers_ecran(*debut), v.monde_vers_ecran(*fin));
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, jaune), StrokeKind::Middle);
                }
                Some(Geste::Bloc { vue, debut, fin }) if *vue == i => {
                    let r = Rect::from_two_pos(v.monde_vers_ecran(*debut), v.monde_vers_ecran(*fin));
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, jaune), StrokeKind::Middle);
                }
                Some(Geste::Tourner { vue, angle, .. }) if *vue == i => {
                    if let Some(h) = ctx.pointer_hover_pos() {
                        p.text(h + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, format!("{angle:.0}°"), egui::FontId::monospace(12.0), jaune);
                    }
                }
                Some(Geste::Deplacer { vue, delta, .. }) if *vue == i => {
                    if let Some(h) = ctx.pointer_hover_pos() {
                        p.text(h + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, format!("{:.0} {:.0} {:.0}", delta.x, delta.y, delta.z), egui::FontId::monospace(12.0), jaune);
                    }
                }
                _ => {}
            }
        }
    }

    fn dialogue_ouvrir(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter(crate::langue::t("Map Hammer"), &["vmf"]).pick_file() {
            self.ouvrir(&p);
        }
    }

    /// L'exécutable de l'étape `i` (0 vbsp, 1 vvis, 2 vrad), ou celui par défaut avec `None` ; gardé dans
    /// `hm_compile::fichier_choix()`.
    fn choisir_executable(&mut self, i: usize, p: Option<PathBuf>) {
        let (Some(o), Some(defaut)) = (self.outils.as_mut(), hm_compile::Outils::detecter_sans_choix()) else { return };
        let champ = [&mut o.vbsp, &mut o.vvis, &mut o.vrad][i].clone();
        let nouveau = p.unwrap_or_else(|| [&defaut.vbsp, &defaut.vvis, &defaut.vrad][i].clone());
        if champ == nouveau {
            return;
        }
        *[&mut o.vbsp, &mut o.vvis, &mut o.vrad].into_iter().nth(i).unwrap() = nouveau;
        let f = hm_compile::fichier_choix();
        let ecrit = f.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&f, o.texte_choix(&defaut)));
        match ecrit {
            Ok(()) => self.statut = tr!("exécutables gardés dans {}", "executables kept in {}", f.display()),
            Err(e) => self.erreur = Some(tr!("choix des exécutables non enregistré : {e}", "executable choice not saved: {e}")),
        }
    }

    fn enregistrer(&mut self, sous: bool) {
        let chemin = if sous || self.session.chemin().is_none() {
            match rfd::FileDialog::new().add_filter(crate::langue::t("Map Hammer"), &["vmf"]).save_file() {
                Some(p) => Some(p),
                None => return,
            }
        } else {
            None
        };
        match self.session.enregistrer(chemin.as_deref()) {
            Ok(p) => {
                self.statut = tr!("enregistré : {}", "saved: {}", p.display());
                self.nom_d_apres(&p);
            }
            Err(e) => self.erreur = Some(tr!("enregistrement impossible : {e}", "cannot save: {e}")),
        }
    }

    /// La map compilée porte le nom du fichier VMF (à l'ouverture, à l'enregistrement sous un autre nom).
    pub(super) fn nom_d_apres(&mut self, p: &Path) {
        if let Some(n) = p.file_stem() {
            self.nom_compile = n.to_string_lossy().to_string();
        }
        crate::recents::ajouter(p);
    }

    // ------------------------------------------------------------------ vues

    fn interagir_vue(&mut self, i: usize, rep: &egui::Response, ctx: &egui::Context) {
        let survol = rep.hovered();
        if survol {
            self.active = i;
        }
        let genre = self.vues[i].genre;
        let (dt, shift) = ctx.input(|x| (x.stable_dt.min(0.1), x.modifiers.shift));
        if genre == Genre::Camera3D {
            // espace tenu : regarder à la souris tant qu'elle est tenue (« Mouselook Hold » de Hammer++)
            let tenue = survol && ctx.input(|x| self.rc.tenu("View3D_Mouselook_Hold", x));
            let v = &mut self.vues[i];
            // regarder : bouton droit tenu, ou la vue souris basculée
            let regarde = rep.dragged_by(egui::PointerButton::Secondary) || (self.vue_souris && self.active == i) || tenue;
            if regarde {
                // curseur capturé : le mouvement brut de la souris
                let d = if self.vue_souris && !rep.dragged_by(egui::PointerButton::Secondary) {
                    ctx.input(|x| x.pointer.motion().unwrap_or(x.pointer.delta()))
                } else if tenue && !rep.dragged_by(egui::PointerButton::Secondary) {
                    ctx.input(|x| x.pointer.delta())
                } else {
                    rep.drag_delta()
                };
                let (k, sens_y) = (self.options.sensibilite, if self.options.inverser_y { -1.0 } else { 1.0 });
                v.cam.lacet -= d.x * k;
                v.cam.tangage = (v.cam.tangage + d.y * k * sens_y).clamp(-89.0, 89.0);
            }
            if rep.dragged_by(egui::PointerButton::Middle) {
                let d = rep.drag_delta();
                let droite = v.cam.droite();
                v.cam.pos += droite * d.x * 2.0 - Vec3::Z * d.y * 2.0;
            }
            if survol || regarde {
                let rc = &self.rc;
                let vitesse = if shift { self.options.vitesse_rapide } else { self.options.vitesse } * dt;
                let (av, ar, ga, dr) = ctx.input(|x| (rc.tenu("View3D_Move_Forward", x), rc.tenu("View3D_Move_Backward", x), rc.tenu("View3D_Move_Left", x) || rc.tenu("View3D_Strafe_Left", x), rc.tenu("View3D_Move_Right", x) || rc.tenu("View3D_Strafe_Right", x)));
                let (haut, bas) = ctx.input(|x| (rc.tenu("View3D_Strafe_Up", x), rc.tenu("View3D_Strafe_Down", x)));
                let ctrl = ctx.input(|x| x.modifiers.command);
                if !ctrl {
                    let (f, r) = (v.cam.avant(), v.cam.droite());
                    let mut m = Vec3::ZERO;
                    if av {
                        m += f;
                    }
                    if ar {
                        m -= f;
                    }
                    if ga {
                        m -= r;
                    }
                    if dr {
                        m += r;
                    }
                    if haut {
                        m += Vec3::Z;
                    }
                    if bas {
                        m -= Vec3::Z;
                    }
                    v.cam.pos += m * vitesse;
                    if m != Vec3::ZERO {
                        ctx.request_repaint();
                    }
                }
                let molette = ctx.input(|x| x.smooth_scroll_delta.y);
                if survol && molette != 0.0 {
                    v.cam.pos += v.cam.avant() * molette * 2.0;
                }
            }
            if self.outil == Outil::Entite && rep.clicked() {
                if let Some(p) = rep.interact_pointer_pos() {
                    self.poser_entite_3d(i, p, ctx);
                }
            }
            if self.outil == Outil::Sculpter {
                let ctrl = ctx.input(|x| x.modifiers.command);
                self.gestes_sculpture(i, rep, ctrl);
            }
            if self.outil == Outil::Semer {
                self.gestes_semis(i, rep, ctx);
            }
            if matches!(self.outil, Outil::Decal | Outil::Overlay) && rep.clicked() {
                if let Some(p) = rep.interact_pointer_pos() {
                    self.clic_decal_overlay(i, p);
                }
            }
            if self.outil == Outil::Texture {
                if let Some(p) = rep.interact_pointer_pos() {
                    if rep.clicked() {
                        self.clic_texture(i, p, ctx, false);
                    } else if rep.secondary_clicked() {
                        self.clic_texture(i, p, ctx, true);
                    }
                }
            }
            let fleche = self.gestes_3d(i, rep, ctx);
            if rep.clicked() && self.outil == Outil::Selection && !fleche {
                if let (Some(p), Some(g)) = (rep.interact_pointer_pos(), &self.gfx) {
                    if let Some((c, _)) = &self.vues[i].cibles {
                        let ppp = ctx.pixels_per_point();
                        let r = self.vues[i].rect;
                        let (x, y) = (((p.x - r.left()) * ppp) as u32, ((p.y - r.top()) * ppp) as u32);
                        let id = g.r.id_sous(c, x, y);
                        let d = self.geo.as_ref().and_then(|geo| geo.objets.get(id as usize).copied().flatten());
                        let ajouter = ctx.input(|x| x.modifiers.command);
                        self.cliquer(d.into_iter().collect(), p, ajouter);
                    }
                }
            }
        } else {
            let v = &mut self.vues[i];
            // déplacer la vue : bouton du milieu, ou espace + gauche
            let espace = ctx.input(|x| x.key_down(egui::Key::Space));
            if rep.dragged_by(egui::PointerButton::Middle) || (espace && rep.dragged_by(egui::PointerButton::Primary)) || rep.dragged_by(egui::PointerButton::Secondary) {
                let d = rep.drag_delta();
                v.centre -= Vec2::new(d.x, -d.y) / v.zoom;
            }
            // la loupe : clic gauche rapproche, clic droit éloigne, autour du point cliqué
            if self.outil == Outil::Loupe {
                if let Some(p) = rep.interact_pointer_pos() {
                    if rep.clicked() {
                        v.zoomer(2.0, p);
                    } else if rep.secondary_clicked() {
                        v.zoomer(0.5, p);
                    }
                }
            }
            if survol {
                let molette = ctx.input(|x| x.smooth_scroll_delta.y);
                if molette != 0.0 {
                    if let Some(p) = rep.hover_pos() {
                        let f = (1.0015f32).powf(molette);
                        let tous = ctx.input(|x| x.modifiers.command);
                        if tous {
                            let c = v.ecran_vers_monde(p);
                            for w in &mut self.vues {
                                if w.genre != Genre::Camera3D {
                                    w.zoom = (w.zoom * f).clamp(0.002, 64.0);
                                    let _ = c;
                                }
                            }
                        } else {
                            v.zoomer(f, p);
                        }
                    }
                }
            }
            if !espace {
                self.gestes_2d(i, rep, ctx);
                self.clic_polygone(i, rep);
            }
            if self.outil == Outil::Entite && rep.clicked() && !espace {
                if let Some(p) = rep.interact_pointer_pos() {
                    let v = &self.vues[i];
                    let w = v.ecran_vers_monde(p);
                    let (a, b, c) = genre.axes();
                    let mut o = Vec3::ZERO;
                    o[a] = gestes::aimanter(w.x, self.grille, self.aimant);
                    o[b] = gestes::aimanter(w.y, self.grille, self.aimant);
                    // profondeur : le centre de la sélection (l'objet sur lequel on travaille), sinon le sol du dernier bloc
                    o[c] = hm_doc::boite_de(self.session.doc(), &self.session.selection).map(|(lo, hi)| ((lo[c] + hi[c]) * 0.5) as f32).unwrap_or(self.profondeur_bloc.0);
                    self.poser_entite(o);
                }
            }
            if rep.clicked() && self.outil == Outil::Selection && !espace {
                if let (Some(p), Some(geo)) = (rep.interact_pointer_pos(), &self.geo) {
                    let v = &self.vues[i];
                    let w = v.ecran_vers_monde(p);
                    let (a, b, _) = genre.axes();
                    let cands = scene::sous_le_point_2d(&geo.formes, a, b, [w.x, w.y], 4.0 / v.zoom);
                    let ajouter = ctx.input(|x| x.modifiers.command);
                    self.cliquer(cands, p, ajouter);
                }
            }
        }
    }

    /// Dessine les vues dans leurs textures et les envoie à la carte graphique.
    fn dessiner_vues(&mut self, ctx: &egui::Context) {
        let boite_bloc = match &self.geste {
            Some(Geste::Taille { boite, .. }) if self.outil == Outil::Bloc => Some(*boite),
            _ => self.bloc,
        };
        let mut apercu_b = self.apercu_brosses(boite_bloc);
        apercu_b.extend(self.apercu_sommets());
        // les aides des entités (liens, cordes, sphères) : cachées par ce qui est devant dans la vue 3D, comme Hammer
        let lignes_aides = if self.afficher_aides { self.lignes_aides() } else { Vec::new() };
        apercu_b.extend(self.lignes_liens());
        apercu_b.extend(self.apercu_sculpture());
        apercu_b.extend(self.apercu_semis());
        apercu_b.extend(self.lignes_validation());
        apercu_b.extend(self.lignes_remarques());
        let eclairage = self.eclairage_courant();
        let brouillard = if self.apercu_brouillard { self.brouillard_de_la_map() } else { None };
        let skybox = if self.ciel.montrer { self.skybox_courante() } else { None };
        let skyname = self.session.doc().monde().get("skyname").unwrap_or("").to_string();
        // pendant un geste, les ombres gardent la géométrie d'avant (sauf celles des lumières qui bougent)
        if self.geste.is_none() {
            self.eclairage.version_ombres = self.revision_monde;
        }
        let Some(g) = &mut self.gfx else { return };
        let Some(geo) = &self.geo else { return };
        let ppp = ctx.pixels_per_point();
        let device = g.r.gpu.device.clone();
        let mut enc = device.create_command_encoder(&Default::default());
        if let (Some(e), Some(monde)) = (&eclairage, &g.monde) {
            g.r.preparer_eclairage(&g.bib, monde, self.eclairage.version_ombres, e, &mut enc);
        }
        let (sel_s, sel_e) = {
            let (s, e, _) = hm_doc::resoudre(self.session.doc(), &self.session.selection).unwrap_or_default();
            (s.into_iter().collect::<std::collections::HashSet<u64>>(), e.into_iter().collect::<std::collections::HashSet<u64>>())
        };
        let choisi = |d: &Designe| match d.objet {
            Ref::Entite(e) => sel_e.contains(&e),
            _ => d.solide.map_or(false, |s| sel_s.contains(&s)),
        };
        const ROUGE: [u8; 4] = [255, 40, 40, 255];
        let lignes_sel = scene::lignes(&geo.formes, |f| choisi(&f.designe).then_some(ROUGE));
        // aperçu du geste en cours : la sélection transformée, ou le bloc tracé, en jaune
        const JAUNE: [u8; 4] = [255, 220, 60, 255];
        let mut lignes_apercu: Vec<Sommet> = Vec::new();
        if let Some(f) = self.geste.as_ref().and_then(|g| g.affine(&self.vues)) {
            if self.outil == Outil::Selection {
                for forme in geo.formes.iter().filter(|x| choisi(&x.designe)) {
                    for ar in &forme.aretes {
                        for p in ar {
                            lignes_apercu.push(Sommet { pos: f.point(p.as_dvec3()).as_vec3().to_array(), couleur: JAUNE });
                        }
                    }
                }
            }
        }
        // faces choisies par l'outil texture
        if self.outil == Outil::Texture {
            for rf in &self.faces {
                if let Some(s) = self.session.doc().solide(rf.solide) {
                    let polys = s.polygones();
                    if let Some((k, _)) = s.faces.iter().enumerate().find(|(_, f)| f.id == rf.face) {
                        let poly = &polys[k];
                        for j in 0..poly.len() {
                            for q in [poly[j], poly[(j + 1) % poly.len()]] {
                                lignes_apercu.push(Sommet { pos: q.as_vec3().to_array(), couleur: [255, 60, 60, 255] });
                            }
                        }
                    }
                }
            }
        }
        lignes_apercu.extend(apercu_b);
        // cordon actif : ses boîtes en trait rouge orangé (rouge épais dans Hammer)
        for (a, b) in self.session.doc().cordons().boites_actives() {
            for ar in scene::aretes_boite(a.as_vec3(), b.as_vec3()) {
                lignes_apercu.push(Sommet { pos: ar[0].to_array(), couleur: [255, 90, 40, 255] });
                lignes_apercu.push(Sommet { pos: ar[1].to_array(), couleur: [255, 90, 40, 255] });
            }
        }
        if let Some(p) = self.portails.as_ref().filter(|_| self.portails_visibles) {
            for poly in p {
                for k in 0..poly.len() {
                    lignes_apercu.push(Sommet { pos: poly[k], couleur: [60, 220, 255, 255] });
                    lignes_apercu.push(Sommet { pos: poly[(k + 1) % poly.len()], couleur: [60, 220, 255, 255] });
                }
            }
        }
        if let Some(f) = &self.fuite {
            for w in f.windows(2) {
                lignes_apercu.push(Sommet { pos: w[0], couleur: [255, 40, 220, 255] });
                lignes_apercu.push(Sommet { pos: w[1], couleur: [255, 40, 220, 255] });
            }
        }
        let bloc = match &self.geste {
            Some(Geste::Taille { boite, .. }) if self.outil == Outil::Bloc => Some(*boite),
            _ => self.bloc,
        };
        if let Some((lo, hi)) = bloc {
            for ar in scene::aretes_boite(lo, hi) {
                lignes_apercu.push(Sommet { pos: ar[0].to_array(), couleur: JAUNE });
                lignes_apercu.push(Sommet { pos: ar[1].to_array(), couleur: JAUNE });
            }
        }
        let cle_monde = (self.revision_monde, self.session.selection.clone());
        if self.cache_lignes_monde.as_ref().map_or(true, |(k, _)| *k != cle_monde) {
            let sommets = scene::lignes(&geo.formes, |f| (!choisi(&f.designe)).then_some(f.couleur));
            self.cache_lignes_monde = Some((cle_monde, g.lignes.lot(&device, &sommets, glam::Mat4::IDENTITY)));
        }
        let lignes_monde = &self.cache_lignes_monde.as_ref().unwrap().1;
        for v in self.vues.iter_mut() {
            if !v.rect.is_positive() {
                continue;
            }
            let (w, h) = ((v.rect.width() * ppp).round().max(8.0) as u32, (v.rect.height() * ppp).round().max(8.0) as u32);
            let refaire = v.cibles.as_ref().map_or(true, |(c, _)| c.largeur != w || c.hauteur != h);
            if refaire {
                let c = Cibles::nouvelles(&device, w, h);
                let vue = c.vue_brute();
                let id = match v.cibles.take() {
                    Some((_, id)) => {
                        g.egui.write().update_egui_texture_from_wgpu_texture(&device, &vue, wgpu::FilterMode::Nearest, id);
                        id
                    }
                    None => g.egui.write().register_native_texture(&device, &vue, wgpu::FilterMode::Nearest),
                };
                v.cibles = Some((c, id));
            }
            if v.constantes.is_none() {
                v.constantes = Some(g.r.constantes_vue());
            }
            let (cibles, _) = v.cibles.as_ref().unwrap();
            match v.genre {
                Genre::Camera3D => {
                    let Some(monde) = &g.monde else { continue };
                    let (b, bg) = v.constantes.as_ref().unwrap();
                    let light = Lighting { sun_dir: Vec3::new(0.45, 0.35, 0.82).normalize(), sun_color: Vec3::new(0.92, 0.88, 0.80), ambient: Vec3::new(0.40, 0.42, 0.47), headlight: 0.22, sky: if self.ciel.montrer { [0.50, 0.62, 0.78] } else { [0.0; 3] } };
                    let mut camera = v.camera();
                    camera.far = self.options.distance;
                    let view = View { camera, width: w, height: h, show_tools: self.afficher_outils, clip: None, light };
                    let avec_ciel = self.ciel.montrer;
                    if avec_ciel {
                        let ciel = ciel_ui::ciel_rendu(&g.r, &mut g.bib, &self.vfs, &skyname, skybox);
                        if v.constantes_ciel.is_none() {
                            v.constantes_ciel = Some([g.r.constantes_vue(), g.r.constantes_vue()]);
                        }
                        g.r.dessiner_ciel(&g.bib, monde, &view, cibles, &ciel, v.constantes_ciel.as_ref().unwrap(), eclairage.is_some(), &mut enc);
                    }
                    g.r.dessiner_vivant(&g.bib, monde, &view, cibles, b, bg, hm_render::vivant::OptionsVue { eclaire: eclairage.is_some(), ciel: avec_ciel, rendu: (self.rendu_3d as u32).min(2), grille: if self.grille_3d { self.grille } else { 0.0 }, brouillard, distance_modeles: self.options.distance_modeles, sans_voile: true }, &mut enc);
                    let vp = view.camera.view_proj(w as f32 / h as f32);
                    if self.rendu_3d == 3 {
                        // fil de fer : les arêtes sans profondeur sur fond noir ; la passe d'avant a rempli les numéros
                        // d'objets, le clic choisit donc toujours ce qui est devant
                        let (l_monde, lot, ap, aides) = (lignes_monde.pour_vue(&g.lignes, &device, vp), g.lignes.lot(&device, &lignes_sel, vp), g.lignes.lot(&device, &lignes_apercu, vp), g.lignes.lot(&device, &lignes_aides, vp));
                        g.lignes.dessiner_2d(&mut enc, &cibles.couleur, [0.0, 0.0, 0.0], &[&l_monde, &aides, &lot, &ap]);
                    } else if !lignes_sel.is_empty() || !lignes_apercu.is_empty() || !lignes_aides.is_empty() {
                        let lot = g.lignes.lot(&device, &lignes_sel, vp);
                        let ap = g.lignes.lot(&device, &lignes_apercu, vp);
                        let aides = g.lignes.lot(&device, &lignes_aides, vp);
                        g.lignes.dessiner_sur_3d(&mut enc, cibles, &[&aides], &[&lot, &ap]);
                    }
                }
                _ => {
                    let vp = v.projection_2d();
                    let grille: Vec<Sommet> = if self.grille_visible { v.grille(self.grille, (self.options.surligner_64, self.options.surligner_n, self.options.cacher_petite_grille, self.options.grille_pointillee)) } else { Vec::new() };
                    let l_grille = g.lignes.lot(&device, &grille, vp);
                    let l_monde = lignes_monde.pour_vue(&g.lignes, &device, vp);
                    let l_sel = g.lignes.lot(&device, &lignes_sel, vp);
                    let l_ap = g.lignes.lot(&device, &lignes_apercu, vp);
                    let l_aides = g.lignes.lot(&device, &lignes_aides, vp);
                    g.lignes.dessiner_2d(&mut enc, &cibles.couleur, [0.0, 0.0, 0.0], &[&l_grille, &l_monde, &l_aides, &l_sel, &l_ap]);
                }
            }
        }
        g.r.gpu.queue.submit([enc.finish()]);
    }

    // ------------------------------------------------------------------ script

    fn avancer_script(&mut self, ctx: &egui::Context) {
        let rects: Vec<Rect> = self.vues.iter().map(|v| v.rect).collect();
        // poignées de chaque vue 2D (côté, position à l'écran) et point du monde → écran, pour les positions symboliques
        let mut poignees: Vec<Vec<([i8; 2], Pos2)>> = Vec::new();
        let mut vers_ecran: Vec<Option<(usize, usize, Vec2, f32, Rect)>> = Vec::new();
        let boite = self.boite_poignees();
        for v in &self.vues {
            let mut l = Vec::new();
            if let (Some((lo, hi)), true) = (boite, v.genre != Genre::Camera3D) {
                let r = gestes::rect_ecran(v, lo, hi);
                let mode = if self.outil == Outil::Bloc { ModePoignees::Taille } else { self.mode_poignees };
                l = gestes::poignees(r, mode).into_iter().map(|(c, p)| (c, gestes::ecarter(r, p, c))).collect();
            }
            poignees.push(l);
            let (a, b, _) = v.genre.axes();
            vers_ecran.push((v.genre != Genre::Camera3D).then_some((a, b, v.centre, v.zoom, v.rect)));
        }
        let cameras: Vec<Option<(hm_render::Camera, Rect)>> = self.vues.iter().map(|v| (v.genre == Genre::Camera3D).then(|| (v.camera(), v.rect))).collect();
        let panneau = rects.iter().map(|r| r.right()).fold(0.0f32, f32::max) + 4.0;
        let pos = move |m: &str| -> Option<Pos2> {
            let (tete, f) = m.split_once(':')?;
            let nombres: Vec<f32> = f.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            match tete.chars().next()? {
                // v<n>:fx,fy — fraction de la vue n
                'v' => {
                    let r = rects.get(tete[1..].parse::<usize>().ok()?)?;
                    Some(Pos2::new(r.left() + r.width() * nombres.first()?, r.top() + r.height() * nombres.get(1)?))
                }
                // h<n>:cx,cy — poignée de côté (cx, cy) dans la vue n
                'h' => {
                    let l = poignees.get(tete[1..].parse::<usize>().ok()?)?;
                    let c = [*nombres.first()? as i8, *nombres.get(1)? as i8];
                    l.iter().find(|(x, _)| *x == c).map(|(_, p)| *p)
                }
                // w<n>:x,y,z — point du monde vu dans la vue n (2D, ou 3D par sa caméra)
                'w' => {
                    let n = tete[1..].parse::<usize>().ok()?;
                    let w = [*nombres.first()?, *nombres.get(1)?, *nombres.get(2)?];
                    if let Some((c, r)) = cameras.get(n)?.as_ref() {
                        let (x, y) = c.project(Vec3::from(w), r.width() as u32, r.height() as u32)?;
                        return Some(r.min + egui::vec2(x, y));
                    }
                    let (a, b, centre, zoom, r) = vers_ecran.get(n)?.clone()?;
                    Some(Pos2::new(r.center().x + (w[a] - centre.x) * zoom, r.center().y - (w[b] - centre.y) * zoom))
                }
                // d:x,y — dans le panneau de droite, depuis son bord gauche
                'd' => Some(Pos2::new(panneau + nombres.first()?, *nombres.get(1)?)),
                _ => None,
            }
        };
        // captures arrivées
        let arrivees: Vec<(String, Arc<egui::ColorImage>)> = std::mem::take(&mut *self.captures.lock().unwrap());
        for (f, img) in arrivees {
            let (w, h) = (img.size[0] as u32, img.size[1] as u32);
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(b) = image::RgbaImage::from_raw(w, h, rgba) {
                if let Err(e) = b.save(&f) {
                    eprintln!("capture {f} : {e}");
                }
            }
            if let Some(s) = &mut self.script {
                s.capture = None;
            }
        }
        let Some(s) = &mut self.script else { return };
        if let Some(c) = &s.capture {
            s.capture_delai = s.capture_delai.saturating_sub(1);
            if s.capture_delai == 0 {
                eprintln!("capture {c} : rien reçu");
                s.capture = None;
            }
            ctx.request_repaint();
            return;
        }
        let action = s.suivante(&pos);
        ctx.request_repaint();
        match action {
            Some(Action::Ouvrir(p)) => self.ouvrir(Path::new(&p)),
            Some(a @ (Action::Capture(_) | Action::Capture3d(_))) => {
                // `capture3d` : seulement la vue 3D (pour `comparer`, à côté de celle de Hammer++)
                let (f, cadre) = match a {
                    Action::Capture3d(f) => (f, self.vues.iter().find(|v| v.genre == Genre::Camera3D).map(|v| v.rect)),
                    Action::Capture(f) => (f, None),
                    _ => unreachable!(),
                };
                let s = self.script.as_mut().unwrap();
                s.capture = Some(f.clone());
                s.capture_delai = 120;
                let (boite, ppp) = (self.captures.clone(), ctx.pixels_per_point());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                    let img = match cadre {
                        Some(r) => Arc::new(img.region(&r, Some(ppp))),
                        None => img,
                    };
                    boite.lock().unwrap().push((f, img))
                })));
            }
            Some(Action::Commande(j)) => match serde_json::from_str::<Commande>(&j) {
                Ok(c) => self.executer(c),
                Err(e) => self.erreur = Some(tr!("commande de script illisible : {e}", "unreadable script command: {e}")),
            },
            Some(Action::Quitter) => {
                self.fermer_sans_demander = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close)
            }
            Some(Action::Planter) => self.planter = true,
            Some(Action::Compiler(a)) => {
                let mut mots = a.split_whitespace();
                self.mode_compile = match mots.next() {
                    Some("geometrie") => hm_compile::Mode::Geometrie,
                    Some("normale") => hm_compile::Mode::Normal,
                    Some("finale") => hm_compile::Mode::Final,
                    _ => hm_compile::Mode::Rapide,
                };
                if let Some(n) = mots.next() {
                    self.nom_compile = n.to_string();
                }
                self.lancer_jeu = false;
                self.copier_bsp = false;
                self.demarrer_compilation();
            }
            Some(Action::Couper) => std::process::exit(3),
            Some(Action::Cadrer) => self.cadrer = true,
            Some(Action::Camera(p)) => {
                let (pos, cible) = (Vec3::new(p[0], p[1], p[2]), Vec3::new(p[3], p[4], p[5]));
                for v in &mut self.vues {
                    v.sur_camera(pos, cible, None);
                }
            }
            Some(Action::Fond(f)) => self.charger_fond(ctx, 0, PathBuf::from(f)),
            Some(Action::Enregistrer(f)) => match self.session.enregistrer(Some(Path::new(&f))) {
                Ok(p) => {
                    self.statut = tr!("enregistré : {}", "saved: {}", p.display());
                    self.nom_d_apres(&p);
                }
                Err(e) => self.erreur = Some(e),
            },
            None => {}
        }
    }
}

/// Couleur d'une classe d'entité dans les vues (en attendant les couleurs des FGD).
pub fn couleur_classe(c: &str) -> [u8; 3] {
    let c = c.to_ascii_lowercase();
    if c.starts_with("light") {
        [255, 255, 0]
    } else if c.starts_with("info_player") {
        [0, 255, 0]
    } else if c.starts_with("trigger") {
        [255, 128, 0]
    } else if c.starts_with("func_detail") {
        [0, 180, 255]
    } else if c.starts_with("prop_") {
        [255, 0, 255]
    } else {
        [220, 30, 220]
    }
}

impl eframe::App for Editeur {
    /// Appelé même fenêtre réduite, quand `ui` ne l'est plus : un script (captures comprises) s'arrêterait net si la
    /// fenêtre était réduite (Win+D) ; elle est restaurée aussitôt (reproduit le 15/11 : 4 captures sur 10).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.script.is_some() && ctx.input(|i| i.viewport().minimized == Some(true)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(s) = &mut self.script {
            if let Some(img) = s.images.pop_front() {
                raw.events.push(egui::Event::ModifiersChanged(img.modificateurs));
                raw.events.extend(img.evenements);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // une erreur interne ne ferme pas la fenêtre : elle est rattrapée, la map est sauvée aussitôt
        let t0 = std::time::Instant::now();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.ui_interne(ui, frame)));
        // temps de l'interface (processeur) et intervalle entre deux images (avec la carte graphique et l'affichage)
        if let Some((n, somme, max, intervalle, avant)) = &mut self.mesures {
            let ms = t0.elapsed().as_secs_f32() * 1000.0;
            let iv = avant.map_or(0.0, |a| t0.duration_since(a).as_secs_f32() * 1000.0);
            *avant = Some(t0);
            (*n, *somme, *max, *intervalle) = (*n + 1, *somme + ms, max.max(ms), intervalle.max(iv));
            if *n == 60 {
                eprintln!("[mesure] 60 images : {:.1} ms en moyenne, {max:.1} ms au plus ; intervalle le plus long {intervalle:.1} ms", *somme / 60.0);
                (*n, *somme, *max, *intervalle) = (0, 0.0, 0.0, 0.0);
            }
        }
        if let Err(e) = r {
            let quoi = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_else(|| crate::langue::t("erreur inconnue").into());
            self.geste = None;
            let sauve = self.sauver_auto(true).or_else(|| self.recup.copie());
            self.erreur = Some(tr!("erreur interne rattrapée ({quoi}) ; {}", "internal error caught ({quoi}); {}", match sauve {
                Some(p) => tr!("travail sauvé dans {}", "work saved in {}", p.display()),
                None => crate::langue::t("rien à sauver").into(),
            }));
        }
        self.sauver_auto(false);
    }

    fn on_exit(&mut self) {
        // fermeture normale : la sauvegarde automatique ne sert plus si tout est enregistré ou abandonné
        if !self.session.modifiee() || self.fermer_sans_demander {
            self.recup.effacer();
        }
    }
}

impl Editeur {
    fn sauver_auto(&mut self, tout_de_suite: bool) -> Option<PathBuf> {
        let s = &self.session;
        match self.recup.sauver(|| s.doc().ecrire(), s.chemin(), s.version(), s.modifiee(), tout_de_suite) {
            Ok(p) => p,
            Err(e) => {
                self.erreur = Some(tr!("sauvegarde automatique impossible : {e}", "autosave failed: {e}"));
                None
            }
        }
    }

    /// Fenêtres de dialogue : récupération au démarrage, confirmation de fermeture.
    fn dialogues(&mut self, ctx: &egui::Context) {
        if !self.a_recuperer.is_empty() {
            let mut choix: Option<(usize, bool)> = None;
            egui::Window::new(crate::langue::t("Récupération")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
                ui.label(crate::langue::t("L'éditeur ne s'est pas fermé normalement. Ces maps avaient des modifications non enregistrées :"));
                for (i, f) in self.a_recuperer.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let nom = f.origine.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| crate::langue::t("map sans titre").into());
                        ui.label(tr!("{nom} — {} modification(s), {}", "{nom} — {} change(s), {}", f.version, recup::depuis(f.quand)));
                        if ui.button(crate::langue::t("Récupérer")).clicked() {
                            choix = Some((i, true));
                        }
                        if ui.button(crate::langue::t("Abandonner")).clicked() {
                            choix = Some((i, false));
                        }
                    });
                }
            });
            if let Some((i, garder)) = choix {
                let f = self.a_recuperer.remove(i);
                if garder {
                    match std::fs::read(&f.vmf).map_err(|e| e.to_string()).and_then(|o| Session::recuperee(&o, f.origine.clone())) {
                        Ok(s) => {
                            self.session = s;
                            self.filtres.masques.clear();
                            self.instances.borrow_mut().clear();
                            self.filtres.changer();
                            self.monter_jeu(f.origine.as_deref());
                            self.version_geo = None;
                            self.cadrer = true;
                            self.statut = crate::langue::t("map récupérée : elle n'est pas encore enregistrée").into();
                            recup::oublier(&f);
                        }
                        Err(e) => self.erreur = Some(tr!("récupération impossible : {e}", "recovery failed: {e}")),
                    }
                } else {
                    recup::oublier(&f);
                }
            }
        }
        // fermeture : demander si la map n'est pas enregistrée
        if ctx.input(|i| i.viewport().close_requested()) && self.session.modifiee() && !self.fermer_sans_demander {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirmer_fermeture = true;
        }
        if self.confirmer_fermeture {
            egui::Window::new(crate::langue::t("Fermer")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
                ui.label(crate::langue::t("La map a des modifications non enregistrées."));
                ui.horizontal(|ui| {
                    if ui.button(crate::langue::t("Enregistrer et fermer")).clicked() {
                        self.enregistrer(false);
                        if !self.session.modifiee() {
                            self.fermer_sans_demander = true;
                            ui.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        self.confirmer_fermeture = false;
                    }
                    if ui.button(crate::langue::t("Fermer sans enregistrer")).clicked() {
                        self.fermer_sans_demander = true;
                        self.confirmer_fermeture = false;
                        ui.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button(crate::langue::t("Annuler")).clicked() {
                        self.confirmer_fermeture = false;
                    }
                });
            });
        }
    }

    fn ui_interne(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.planter) {
            panic!("erreur provoquée par le script");
        }
        self.servir_pont(&ctx);
        self.dialogues(&ctx);
        // l'outil entité montre ses classes, l'outil texture le navigateur
        if self.outil != self.outil_precedent {
            match self.outil {
                Outil::Entite => self.onglet = Onglet::Proprietes,
                Outil::Texture => self.onglet = Onglet::Textures,
                _ => {}
            }
            self.outil_precedent = self.outil;
        }
        self.preparer_gfx(frame);
        self.avancer_script(&ctx);
        self.raccourcis(&ctx);
        self.palette(&ctx);
        self.mettre_a_jour_geo();
        self.validation_directe(&ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.titre()));

        egui::Panel::top("menu").show(ui, |ui| self.menus(ui));
        egui::Panel::top("barre").show(ui, |ui| self.barre_outils(ui));
        egui::Panel::top("bandeau_outil").show(ui, |ui| self.bandeau_outil(ui));
        for v in &mut self.vues {
            v.cam.fov = self.options.fov;
        }

        egui::Panel::bottom("etat").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr!("grille {} (aimant {})", "grid {} (snap {})", self.grille, if self.aimant { crate::langue::t("oui") } else { crate::langue::t("non") }));
                ui.separator();
                ui.label(tr!("outil : {}", "tool: {}", self.outil.nom()));
                ui.separator();
                if let Some(t) = self.etat_eclairage() {
                    ui.label(t);
                    ui.separator();
                }
                self.etat_validation(ui);
                if let Some(t) = self.etat_selection() {
                    ui.label(t);
                    ui.separator();
                }
                let (ns, ne) = self.session.doc().nombre_objets();
                ui.label(tr!("{ns} solides, {ne} entités", "{ns} solids, {ne} entities"));
                ui.separator();
                if let Some(p) = ctx.pointer_hover_pos() {
                    let v = &self.vues[self.active];
                    if v.genre != Genre::Camera3D && v.rect.contains(p) {
                        let w = v.ecran_vers_monde(p);
                        let (a, b, _) = v.genre.axes();
                        ui.label(format!("{} {:.0}  {} {:.0}", ["x", "y", "z"][a], w.x, ["x", "y", "z"][b], w.y));
                        ui.separator();
                    } else if v.genre == Genre::Camera3D {
                        let c = v.cam.pos;
                        ui.label(tr!("caméra ({:.0} {:.0} {:.0})", "camera ({:.0} {:.0} {:.0})", c.x, c.y, c.z));
                        ui.separator();
                    }
                }
                if let Some(e) = &self.erreur {
                    ui.colored_label(Color32::from_rgb(255, 120, 100), e);
                } else {
                    ui.label(&self.statut);
                }
            });
        });

        self.compilation(&ctx, ui);
        self.fenetre_proprietes(&ctx);
        self.fenetre_remplacer(&ctx);
        self.dialogues_menus(&ctx);
        self.fenetres_carte(&ctx);
        self.fenetres_navigateurs(&ctx);
        self.fenetre_fonds(&ctx);
        self.fenetre_logique(&ctx);
        self.fenetre_propper(&ctx);
        self.fenetres_textures(&ctx);
        self.fenetre_options(&ctx);
        if self.editeur_raccourcis.ouvert && self.editeur_raccourcis.ui(&ctx, &mut self.rc) {
            match self.rc.enregistrer(&Raccourcis::fichier_utilisateur()) {
                Ok(()) => self.statut = crate::langue::t("raccourcis enregistrés").into(),
                Err(e) => self.erreur = Some(e),
            }
        }

        if self.panneau_reduit {
            egui::Panel::right("panneau_reduit").resizable(false).exact_size(46.0).show(ui, |ui| self.onglets_reduits(ui));
        } else {
            egui::Panel::right("panneau").resizable(true).default_size(380.0).min_size(300.0).max_size(520.0).show(ui, |ui| self.panneau_droit(ui));
        }

        egui::Panel::left("outils").resizable(false).exact_size(46.0).show(ui, |ui| self.panneau_outils(ui));

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let zone = ui.available_rect_before_wrap();
            // séparateurs déplaçables
            if self.agrandie.is_none() {
                let x = zone.left() + zone.width() * self.sep[0];
                let y = zone.top() + zone.height() * self.sep[1];
                let sv = ui.interact(Rect::from_min_max(Pos2::new(x - 3.0, zone.top()), Pos2::new(x + 3.0, zone.bottom())), ui.scope_id().with("sep_v"), Sense::drag());
                let sh = ui.interact(Rect::from_min_max(Pos2::new(zone.left(), y - 3.0), Pos2::new(zone.right(), y + 3.0)), ui.scope_id().with("sep_h"), Sense::drag());
                if sv.dragged() {
                    self.sep[0] = ((x + sv.drag_delta().x - zone.left()) / zone.width()).clamp(0.1, 0.9);
                }
                if sh.dragged() {
                    self.sep[1] = ((y + sh.drag_delta().y - zone.top()) / zone.height()).clamp(0.1, 0.9);
                }
                if sv.hovered() || sv.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if sh.hovered() || sh.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
            }
            let rects = disposer(zone, self.sep, self.agrandie);
            for (i, r) in rects.iter().enumerate() {
                self.vues[i].rect = *r;
            }
            for i in 0..4 {
                let r = rects[i];
                if !r.is_positive() {
                    continue;
                }
                let rep = ui.interact(r, ui.scope_id().with(("vue", i)), Sense::click_and_drag());
                self.interagir_vue(i, &rep, &ctx);
                // double clic sur un objet (le premier clic l'a sélectionné) : ses propriétés, comme Hammer
                if rep.double_clicked() && self.outil == Outil::Selection && !self.session.selection.is_empty() {
                    self.ouvrir_proprietes();
                }
            }
        });

        self.dessiner_vues(&ctx);
        let temps = ctx.input(|i| i.time);
        self.dessiner_apercu(temps);
        // images des vues et ce qui se dessine par-dessus
        // couche du fond : les fenêtres de dialogue passent devant les vues
        let peintre = ctx.layer_painter(egui::LayerId::background());
        for (i, v) in self.vues.iter().enumerate() {
            if !v.rect.is_positive() {
                continue;
            }
            if let Some((_, id)) = &v.cibles {
                peintre.image(*id, v.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            let actif = i == self.active;
            peintre.rect_stroke(v.rect, 0.0, Stroke::new(1.0, if actif { Color32::from_rgb(255, 200, 60) } else { Color32::from_gray(70) }), StrokeKind::Inside);
            if v.genre == Genre::Camera3D && self.vue_souris {
                peintre.text(v.rect.left_top() + egui::vec2(140.0, 6.0), egui::Align2::LEFT_TOP, crate::langue::t("vue souris"), egui::FontId::proportional(13.0), Color32::from_rgb(255, 220, 60));
            }
        }
        // le titre de chaque vue choisit son type (comme le menu Affichage de Hammer)
        let mut changer: Option<(usize, Genre)> = None;
        let (rendu, mut mode) = (self.rendu_3d, None);
        for (i, v) in self.vues.iter().enumerate() {
            if !v.rect.is_positive() {
                continue;
            }
            egui::Area::new(egui::Id::unique(("titre_vue", i))).fixed_pos(v.rect.left_top() + egui::vec2(4.0, 3.0)).order(egui::Order::Background).show(&ctx, |ui| {
                let titre = if v.genre == Genre::Camera3D { crate::langue::t(MODES_3D[rendu as usize]) } else { v.genre.nom() };
                egui::ComboBox::from_id_salt(("genre_vue", i)).selected_text(titre).width(175.0).show_ui(ui, |ui| {
                    for (k, n) in MODES_3D.iter().enumerate() {
                        if ui.selectable_label(v.genre == Genre::Camera3D && rendu as usize == k, crate::langue::t(n)).clicked() {
                            changer = Some((i, Genre::Camera3D));
                            mode = Some(k as u8);
                        }
                    }
                    ui.separator();
                    for g in [Genre::Dessus, Genre::Face, Genre::Cote] {
                        if ui.selectable_label(v.genre == g, g.nom()).clicked() && v.genre != g {
                            changer = Some((i, g));
                        }
                    }
                });
            });
        }
        if let Some(m) = mode {
            self.rendu_3d = m;
        }
        if let Some((i, g)) = changer.filter(|(i, g)| self.vues[*i].genre != *g) {
            self.vues[i].genre = g;
            if let Some(geo) = &self.geo {
                let (lo, hi) = (geo.min, geo.max);
                self.vues[i].cadrer(lo, hi);
            }
        }
        self.dessiner_dessus(&ctx, &peintre);
        if self.script.as_ref().map_or(false, |s| s.fini()) {
            self.script = None;
        }
    }
}

/// Les modes de la vue 3D, dans l'ordre de `rendu_3d`.
const MODES_3D: [&str; 4] = ["3D textures ombrées", "3D textures", "3D polygones pleins", "3D fil de fer"];

/// Distance d'un point à un segment, à l'écran.
fn distance_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}
