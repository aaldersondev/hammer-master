//! Serveur MCP de Hammer Master (stdio). Il garde la map ouverte en mémoire et sur la carte graphique : une vue
//! coûte quelques millisecondes. Toute la logique est dans les briques ; ce fichier ne fait que les exposer.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use glam::Vec3;
use hm_assets::Vfs;
use hm_espace::Field;
use hm_render::{Camera, Frame, Gpu, GpuScene, Lighting, Renderer, View};
use hm_scene::{ObjKind, Scene, SceneOptions};
use hm_tour::suivi;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use serde::Deserialize;

struct Session {
    carte: PathBuf,
    /// Le document en cours d'édition (noyau) : toute modification passe par lui.
    doc: hm_doc::Session,
    scene: Scene,
    vfs: Vfs,
    /// Dossier du gameinfo.txt : le « jeu » à donner à vbsp.
    jeu: PathBuf,
    /// Dernier rapport des règles, pour `constat_vue`.
    rapport: Option<hm_regles::Rapport>,
    gs: GpuScene,
    field: Option<Field>,
    /// Dernière vue rendue : pour `sous_le_pixel`.
    derniere: Option<Frame>,
    /// La map est aussi ouverte dans la fenêtre de l'éditeur : les modifications s'y font (l'utilisateur les voit),
    /// `doc` en est une copie, refaite quand la fenêtre change.
    lien: Option<Lien>,
}

struct Lien {
    client: hm_doc::pont::Client,
    /// Version de la fenêtre au moment de la copie.
    version_copie: u64,
    /// Dernier état rendu par la fenêtre (modifiée, sélection).
    dernier: hm_doc::pont::Reponse,
}

#[derive(Default)]
struct Etat {
    renderer: Option<Arc<Renderer>>,
    session: Option<Session>,
    /// Les classes d'entités de Garry's Mod (FGD de Hammer++), lues à la première vérification.
    fgd: Option<Arc<hm_fgd::Fgd>>,
}

#[derive(Clone)]
struct Serveur {
    etat: Arc<Mutex<Etat>>,
    tool_router: ToolRouter<Self>,
}

fn texte(s: impl Into<String>) -> ContentBlock {
    ContentBlock::text(s.into())
}

fn image(jpeg: &[u8]) -> ContentBlock {
    ContentBlock::image(base64::engine::general_purpose::STANDARD.encode(jpeg), "image/jpeg")
}

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::from(a)
}

impl Serveur {
    fn new() -> Self {
        Self { etat: Arc::new(Mutex::new(Etat::default())), tool_router: Self::tool_router() }
    }

    /// Exécute une opération sur l'état ; une erreur devient un résultat d'outil en erreur, avec son message.
    fn faire(&self, f: impl FnOnce(&mut Etat) -> Result<Vec<ContentBlock>>) -> Result<CallToolResult, ErrorData> {
        let etat = self.etat.clone();
        let (note, res) = tokio::task::block_in_place(move || {
            let mut e = etat.lock().unwrap();
            let note = synchroniser(&mut e);
            (note, f(&mut e))
        });
        let note = note.map(|n| texte(format!("NOTE : {n}")));
        Ok(match res {
            Ok(blocs) => CallToolResult::success(note.into_iter().chain(blocs).collect()),
            Err(err) => CallToolResult::error(note.into_iter().chain([texte(format!("ERREUR : {err:#}"))]).collect()),
        })
    }
}

fn session(e: &mut Etat) -> Result<(&Renderer, &mut Session)> {
    let r = e.renderer.as_deref().ok_or_else(|| anyhow!("aucune map ouverte : appeler carte_ouvrir"))?;
    let s = e.session.as_mut().ok_or_else(|| anyhow!("aucune map ouverte : appeler carte_ouvrir"))?;
    Ok((r, s))
}

fn entete(s: &Session) -> String {
    let v = suivi::etat_verrou(&s.carte, &s.scene.revision);
    let modifiee = s.lien.as_ref().map_or(s.doc.modifiee(), |l| l.dernier.modifiee);
    let modif = if modifiee { " · modifiée, non enregistrée" } else { "" };
    let modif = if s.lien.is_some() { format!("{modif} · ouverte dans la fenêtre") } else { modif.to_string() };
    format!("[{} · révision {} · verrou « tour lu » : {}{modif}]", s.carte.file_name().unwrap_or_default().to_string_lossy(), s.scene.revision, if v.ouvert { "ouvert" } else { "fermé" })
}

fn field(s: &mut Session) -> &Field {
    if s.field.is_none() {
        s.field = Some(Field::build(&s.scene));
    }
    s.field.as_ref().unwrap()
}

/// Ce qu'on voit sur une vue : les objets qui occupent le plus de pixels.
fn resume_vue(scene: &Scene, frame: &Frame) -> String {
    let mut compte = std::collections::HashMap::new();
    for &id in &frame.ids {
        if id != 0 {
            *compte.entry(id).or_insert(0u32) += 1;
        }
    }
    let total = (frame.width * frame.height) as f32;
    let mut l: Vec<(u32, u32)> = compte.into_iter().collect();
    l.sort_by_key(|x| std::cmp::Reverse(x.1));
    let mut out = format!("{} objets visibles ; les plus présents :", l.len());
    for (id, n) in l.iter().take(10) {
        let o = &scene.objects[*id as usize];
        let quoi = if o.model.is_empty() { format!("solide {} ({})", o.class, o.material) } else { o.model.rsplit('/').next().unwrap_or("").to_string() };
        out += &format!("\n  objet {id} · {quoi} · {:.0} % de l'image", 100.0 * *n as f32 / total);
    }
    out
}

fn decrire(scene: &Scene, id: u32) -> String {
    let Some(o) = scene.objects.get(id as usize).filter(|_| id != 0) else { return "rien (ciel ou vide)".to_string() };
    let c = (o.min + o.max) * 0.5;
    let e = o.max - o.min;
    format!(
        "objet {id} · {:?} · classe {} · {}{}id VMF {} · centre ({:.0} {:.0} {:.0}) · encombrement {:.0} × {:.0} × {:.0}{}",
        o.kind,
        o.class,
        if o.model.is_empty() { format!("matériau {} · ", o.material) } else { format!("modèle {} · ", o.model) },
        if o.name.is_empty() { String::new() } else { format!("nom « {} » · ", o.name) },
        o.vmf_id,
        c.x,
        c.y,
        c.z,
        e.x,
        e.y,
        e.z,
        if o.hidden { " · masqué en jeu" } else { "" }
    )
}

#[derive(Deserialize, schemars::JsonSchema)]
struct OuvrirArgs {
    /// Chemin du fichier .vmf.
    vmf: String,
    /// gameinfo.txt dont les SearchPaths donnent les fichiers du jeu. Par défaut : `compile/gameinfo.txt` près du VMF, sinon celui de Garry's Mod.
    gameinfo: Option<String>,
    /// Dossiers de contenu à monter en priorité (materials/, models/).
    contenu: Option<Vec<String>>,
    /// Map compilée (.bsp) dont le contenu embarqué passe avant tout : pour revoir un ancien VMF avec ses fichiers d'alors.
    pak: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct VueArgs {
    /// Position de l'œil [x, y, z]. Avec `joueur`, position des pieds : l'œil est mis à 64 au-dessus du sol trouvé dessous.
    pos: [f32; 3],
    /// Point visé [x, y, z]. Sinon donner `angles`.
    vers: Option<[f32; 3]>,
    /// Angles du jeu [pitch, yaw] en degrés (pitch positif vers le bas).
    angles: Option<[f32; 2]>,
    /// Champ de vision du jeu (90 par défaut).
    fov: Option<f32>,
    /// Vue à hauteur de joueur depuis le sol sous `pos`.
    joueur: Option<bool>,
    /// Affiche les faces d'outil (nodraw, clip, déclencheurs, areaportals) en couleurs codées.
    outils: Option<bool>,
    /// Largeur de l'image (1280 par défaut, 1920 au plus).
    largeur: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct DessusArgs {
    /// Centre de la vue [x, y].
    centre: [f32; 2],
    /// Demi-hauteur de la zone vue, en unités.
    demi: f32,
    /// Ne garde que ce qui est entre ces deux hauteurs : mettre `zmax` sous le plafond de l'étage à voir.
    zmin: f32,
    zmax: f32,
    outils: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct HammerppArgs {
    /// Position de l'œil [x, y, z].
    pos: [f32; 3],
    /// Point visé [x, y, z].
    vers: [f32; 3],
    /// Secondes laissées à Hammer++ après l'ouverture, le temps que modèles et textures arrivent (20 par défaut).
    attente: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PixelArgs {
    /// Colonne et ligne dans la dernière image rendue par `vue` ou `vue_dessus`.
    x: u32,
    y: u32,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct RayonArgs {
    origine: [f32; 3],
    /// Point visé ; le rayon part de `origine` dans sa direction.
    vers: [f32; 3],
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ListerArgs {
    /// `pieces`, `objets`, `modeles`, `materiaux` ou `entites`.
    quoi: String,
    /// Texte à trouver dans le nom, la classe, le modèle ou le matériau.
    filtre: Option<String>,
    /// Boîte [xmin, ymin, zmin, xmax, ymax, zmax] où chercher.
    zone: Option<[f32; 6]>,
    /// Nombre maximal de lignes (40 par défaut).
    limite: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct InspecterArgs {
    /// Rang de l'objet (donné par `vue`, `sous_le_pixel`, `lister`).
    objet: Option<u32>,
    /// Ou `id` du solide ou de l'entité dans le VMF.
    vmf_id: Option<u64>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TourArgs {
    /// Groupes de vues : `pieces,facades,reseau` par défaut.
    groupes: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PageArgs {
    /// Nom d'une planche à lire. Sans nom : la prochaine planche non lue.
    planche: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct LuArgs {
    planche: String,
    /// Code de lecture écrit en haut à droite de l'image de la planche.
    code: String,
    /// Une entrée par case : ce qu'on voit sur la vue, et ce qui cloche.
    vues: Vec<LuCase>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct LuCase {
    /// Numéro de la case (0 à 8, de gauche à droite puis de haut en bas).
    case: usize,
    /// Ce qu'on voit sur la vue, en une phrase.
    vu: String,
    /// Ce qui cloche sur cette vue (liste vide s'il n'y a rien).
    #[serde(default)]
    defauts: Vec<LuDefaut>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct LuDefaut {
    /// 1 = saute aux yeux, 2 = gênant, 3 = détail.
    gravite: u8,
    /// `sûr` ou `à vérifier`.
    #[serde(default)]
    confiance: String,
    quoi: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct SignalementsArgs {
    /// Dossiers où lire les signalements (ceux d'un autre PC, par exemple). Par défaut : ceux du jeu installé,
    /// `garrysmod/data/hammer_master/<map>` et l'ancien `megalopole_defauts`.
    dossiers: Option<Vec<String>>,
    /// Ne lister que cet état : `ouvert`, `corrige`, `regle` ou `ecarte`.
    etat: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ComparerArgs {
    /// Identifiant du signalement, tel que le donne `signalements_lire` (`L07`, `003`).
    id: String,
    dossiers: Option<Vec<String>>,
    /// Champ de vision à supposer (base 4/3) quand le signalement ne le dit pas. Par défaut : le réglage du jeu.
    fov_suppose: Option<f32>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct FermerArgs {
    id: String,
    /// `corrige`, `regle` (une règle le surveille désormais), `ecarte`, ou `ouvert` pour le rouvrir.
    etat: String,
    /// Ce qui a été fait, en une phrase.
    note: Option<String>,
    dossiers: Option<Vec<String>>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ValiderArgs {
    /// Familles de règles : `geometrie`, `assets`, `physique`, `budgets`, `joueur`. Toutes par défaut.
    familles: Option<Vec<String>>,
    /// Ne lister que les règles dont l'identifiant suit ce motif (`phys.*`, `geo.zfighting`).
    regle: Option<String>,
    /// Ne lister que les constats dont le message contient ce texte.
    filtre: Option<String>,
    /// Constats listés par règle (6 par défaut).
    max: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ConstatArgs {
    /// Identifiant de la règle (`phys.flotte`).
    regle: String,
    /// Rang du constat dans cette règle, à partir de 1, dans l'ordre de `valider`.
    rang: usize,
    /// Le même filtre de texte que celui donné à `valider`, s'il y en avait un.
    filtre: Option<String>,
}

/// Les réglages du projet de la map ouverte.
fn config_de(s: &Session) -> Result<(hm_regles::Config, Option<PathBuf>)> {
    hm_regles::Config::du_projet(&std::path::absolute(&s.carte).unwrap_or_else(|_| s.carte.clone()))
}

/// Constats non exceptés d'une règle, dans l'ordre du rapport, après filtre de texte.
fn constats_de<'a>(r: &'a hm_regles::Rapport, regle: &str, filtre: Option<&str>) -> Vec<&'a hm_regles::Constat> {
    let f = filtre.map(|f| f.to_lowercase());
    r.constats.iter().filter(|c| c.excepte.is_none() && hm_regles::motif(regle, c.regle) && f.as_ref().map_or(true, |f| c.message.to_lowercase().contains(f))).collect()
}

/// Signalements de la map ouverte : ceux des dossiers donnés, sinon ceux du jeu installé.
fn signalements(carte: &std::path::Path, dossiers: Option<Vec<String>>) -> Result<Vec<hm_signal::Signalement>> {
    let nom = carte.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let d: Vec<PathBuf> = match dossiers {
        Some(d) if !d.is_empty() => d.into_iter().map(PathBuf::from).collect(),
        _ => hm_signal::dossiers_par_defaut(&nom),
    };
    hm_signal::lire(&d)
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AmenagerArgs {
    /// Identifiant de la pièce dans le modèle sémantique (`<carte>.sem.json`).
    piece: String,
    /// Nom du modèle d'aménagement (dossier `amenagements/` du projet).
    modele: String,
    /// Mur de la porte (`x-`, `x+`, `y-`, `y+`) si on veut l'imposer ; sinon il est cherché.
    porte: Option<String>,
    /// Mots de noms de modèles à laisser en place en plus du bâti et de l'éclairage.
    garder: Option<Vec<String>>,
    /// `true` : montrer ce qui serait fait sans rien écrire. À faire d'abord.
    essai: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ConstruireArgs {
    /// La spec YAML (format du skill gmod-map-maker : zones, ouvertures, éléments, entités).
    spec: String,
    /// Le VMF à écrire ; par défaut `<carte>.vmf` à côté de la spec.
    sortie: Option<String>,
    /// `blockout` (textures de mesure partout) ou `texture` ; par défaut celle de la spec.
    phase: Option<String>,
    /// Ouvrir la map construite dans la session (vrai par défaut).
    ouvrir: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct CommandeArgs {
    /// La commande, en JSON : `{"commande": "<nom>", …}`. Objets désignés par `{"type": "solide"|"entite"|"groupe", "id": n}`.
    commande: serde_json::Value,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AnnulerArgs {
    /// `true` : rétablir ce qui a été annulé au lieu d'annuler.
    retablir: Option<bool>,
    /// Nombre d'étapes (1 par défaut).
    n: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct EnregistrerArgs {
    /// Autre fichier où enregistrer (il devient celui de la session) ; par défaut celui de la map ouverte.
    chemin: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct DifferenceArgs {
    /// VMF d'avant ; par défaut le fichier de la map sur le disque (la différence montre alors ce qui n'est pas encore enregistré).
    avant: Option<String>,
    /// Lignes listées par rubrique (10 par défaut).
    max: Option<usize>,
}

/// Refait la scène d'après le document en mémoire après une modification : la carte graphique, et tout ce qui en
/// dépendait.
fn recharger(e: &mut Etat) -> Result<()> {
    let renderer = e.renderer.clone().ok_or_else(|| anyhow!("aucune map ouverte : appeler carte_ouvrir"))?;
    let s = e.session.as_mut().ok_or_else(|| anyhow!("aucune map ouverte : appeler carte_ouvrir"))?;
    s.scene = Scene::from_bytes(&s.carte, s.doc.doc().ecrire(), &s.vfs, &SceneOptions::default())?;
    s.gs = renderer.upload(&s.scene);
    s.rapport = None;
    s.field = None;
    s.derniere = None;
    Ok(())
}

/// Si la map a changé dans la fenêtre liée (souris de l'utilisateur, commande), la copie du MCP est refaite. Rend une
/// note si le lien est perdu : la session continue alors sur sa copie, sans lien.
fn synchroniser(e: &mut Etat) -> Option<String> {
    let s = e.session.as_mut()?;
    let l = s.lien.as_mut()?;
    let perdu = match l.client.demander(hm_doc::pont::Demande::Etat) {
        Err(err) => Some(format!("la fenêtre ne répond plus ({err})")),
        Ok(r) if r.carte.as_deref().and_then(|p| std::fs::canonicalize(p).ok()) != std::fs::canonicalize(&s.carte).ok() => Some("la fenêtre a ouvert une autre map".to_string()),
        Ok(r) => {
            let a_jour = r.version == l.version_copie;
            l.dernier = r;
            if a_jour {
                return None;
            }
            match recopier(e) {
                Ok(()) => return None,
                Err(err) => Some(format!("copie de la map de la fenêtre impossible ({err:#})")),
            }
        }
    };
    let s = e.session.as_mut()?;
    s.lien = None;
    perdu.map(|p| format!("{p} : le lien est coupé, la session continue sur la copie du MCP (rien n'y est plus vu en direct ; carte_ouvrir pour se relier)"))
}

/// Refait la copie du MCP d'après la map de la fenêtre.
fn recopier(e: &mut Etat) -> Result<()> {
    let (_, s) = session(e)?;
    let r = demander(s, hm_doc::pont::Demande::Document)?;
    s.doc = hm_doc::Session::depuis_octets(r.vmf.unwrap_or_default().as_bytes(), Some(s.carte.clone())).map_err(|err| anyhow!(err))?;
    if let Some(l) = &mut s.lien {
        l.version_copie = r.version;
    }
    recharger(e)
}

/// Une demande à la fenêtre liée ; son refus devient une erreur.
fn demander(s: &mut Session, d: hm_doc::pont::Demande) -> Result<hm_doc::pont::Reponse> {
    let l = s.lien.as_mut().ok_or_else(|| anyhow!("la map n'est pas ouverte dans la fenêtre"))?;
    let r = l.client.demander(d).map_err(|err| anyhow!("la fenêtre ne répond plus : {err}"))?;
    l.dernier = r.clone();
    if let Some(err) = &r.erreur {
        bail!("{err}");
    }
    Ok(r)
}

/// Exécute une commande du noyau sur la map ouverte (dans la fenêtre si elle y est), refait la scène, et dit ce qui
/// a été fait.
fn executer(e: &mut Etat, c: hm_doc::Commande) -> Result<Vec<ContentBlock>> {
    let nom = c.nom();
    if session(e)?.1.lien.is_some() {
        let avant = session(e)?.1.lien.as_ref().map_or(u64::MAX, |l| l.version_copie);
        let r = demander(session(e)?.1, hm_doc::pont::Demande::Commande { commande: c.clone() }).map_err(|err| anyhow!("refusé, rien n'a changé : {err}"))?;
        // la fenêtre n'a fait que cette commande depuis la copie : la refaire sur la copie évite de relire toute la map
        let s = session(e)?.1;
        if r.version == avant.wrapping_add(1) && s.doc.executer(c).is_ok() {
            if let Some(l) = &mut s.lien {
                l.version_copie = r.version;
            }
            recharger(e)?;
        } else {
            recopier(e)?;
        }
        let (_, s) = session(e)?;
        let mut t = format!("{}
fait dans la fenêtre : {nom} (étape {} de son historique)", entete(s), r.etapes);
        if !r.crees.is_empty() {
            t += &format!("
créé (sélectionné dans la fenêtre) : {}", serde_json::to_string(&r.crees)?);
        }
        if !r.message.is_empty() {
            t += &format!("
{}", r.message);
        }
        t += "
L'utilisateur le voit en direct et peut l'annuler (Ctrl+Z). Rien n'est écrit sur le disque avant `enregistrer`.";
        return Ok(vec![texte(t)]);
    }
    let r = {
        let (_, s) = session(e)?;
        s.doc.executer(c).map_err(|err| anyhow!("refusé, rien n'a changé : {err}"))?
    };
    recharger(e)?;
    let (_, s) = session(e)?;
    let mut t = format!("{}
fait : {nom} (étape {} de l'historique)", entete(s), s.doc.historique().faits().len());
    if !r.crees.is_empty() {
        t += &format!("
créé : {}", serde_json::to_string(&r.crees)?);
    }
    if !r.message.is_empty() {
        t += &format!("
{}", r.message);
    }
    t += "
Rien n'est écrit sur le disque avant `enregistrer`. Regarder le résultat avec `vue`, le juger avec `valider`.";
    Ok(vec![texte(t)])
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Rien {}

#[derive(Deserialize, schemars::JsonSchema)]
struct VerifierArgs {
    /// Nombre maximal de problèmes rendus (50 par défaut), les plus graves d'abord.
    limite: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct CompilerArgs {
    /// `geometrie` (vbsp seul), `rapide` (par défaut), `normale` ou `finale`.
    mode: Option<String>,
    /// Seulement cette boîte [xmin, ymin, zmin, xmax, ymax, zmax], comme « Compiler la zone de la sélection » (map `<nom>_zone`).
    zone: Option<[f64; 6]>,
    /// Copier le .bsp dans le dossier maps du jeu (oui par défaut).
    copier: Option<bool>,
    /// Embarquer dans le .bsp ce que les joueurs n'ont pas (non par défaut).
    embarquer: Option<bool>,
    /// Lancer le jeu sur la map après (non par défaut).
    lancer_jeu: Option<bool>,
    /// Ne refaire que ce qui a changé depuis la compilation précédente (oui par défaut).
    partielle: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AmenagerBoiteArgs {
    /// Nom du modèle d'aménagement (fichier `<nom>.toml` du dossier « amenagements » du projet).
    modele: String,
    /// La boîte de la pièce [xmin, ymin, zmin, xmax, ymax, zmax] ; sans elle, la boîte de la sélection de la fenêtre.
    zone: Option<[f64; 6]>,
    /// Le mur de la porte : `y-` (par défaut), `y+`, `x-` ou `x+`.
    porte: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PropperArgs {
    /// Le modèle à écrire, sous models/ et sans .mdl (« dossier/nom », minuscules, sans espace).
    modele: String,
    /// Les objets à transformer, comme pour `commande` ; sans eux, la sélection de la fenêtre.
    objets: Option<serde_json::Value>,
    /// Matière (`$surfaceprop`), « concrete » par défaut.
    surfaceprop: Option<String>,
    /// Masse, 500 par défaut.
    masse: Option<f64>,
    /// Collision : une pièce convexe par brosse (oui par défaut).
    collision: Option<bool>,
    /// Remplacer les brosses par le prop_static (oui par défaut), en une étape annulable.
    remplacer: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct MontrerArgs {
    /// Les objets, comme pour `commande` : [{"type": "solide"|"entite"|"groupe", "id": n}].
    objets: serde_json::Value,
}

#[tool_router]
impl Serveur {
    #[tool(description = "Ouvre une map (.vmf) : charge solides, modèles et textures réels et les envoie à la carte graphique. À appeler avant tout autre outil. Rend un résumé (comptes, assets introuvables, durées). Si la map est ouverte dans la fenêtre de l'éditeur Hammer Master, la session s'y lie : on part de sa version (modifications non enregistrées comprises), et `commande`, `annuler`, `enregistrer` se font dans la fenêtre, où l'utilisateur les voit en direct.")]
    async fn carte_ouvrir(&self, Parameters(a): Parameters<OuvrirArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let carte = PathBuf::from(&a.vmf);
            let mut vfs = Vfs::new();
            if let Some(p) = &a.pak {
                vfs.add_bsp_pak(p)?;
            }
            for c in a.contenu.iter().flatten() {
                vfs.add_dir(c);
            }
            let gameinfo = a.gameinfo.map(PathBuf::from).or_else(|| {
                let complet = std::path::absolute(&carte).ok()?;
                // comme l'outil en ligne : le dossier du VMF, puis jusqu'à deux dossiers au-dessus
                let mut dossier = complet.parent();
                for _ in 0..3 {
                    let d = dossier?;
                    if let Some(g) = [d.join("compile/gameinfo.txt"), d.join("_compile/gameinfo.txt"), d.join("gameinfo.txt")].into_iter().find(|p| p.is_file()) {
                        return Some(g);
                    }
                    dossier = d.parent();
                }
                None
            });
            let gameinfo = gameinfo.unwrap_or_else(|| hm_assets::jeu::gameinfo_gmod().unwrap_or_default());
            vfs.add_gameinfo(&gameinfo)?;
            // la map est ouverte dans la fenêtre : on travaille sur la sienne (modifications non enregistrées comprises)
            let mut lien = None;
            let doc = match hm_doc::pont::fenetre_de(&carte) {
                Some((mut client, _)) => {
                    let r = client.demander(hm_doc::pont::Demande::Document).map_err(|err| anyhow!("la fenêtre ne répond pas : {err}"))?;
                    let doc = hm_doc::Session::depuis_octets(r.vmf.clone().unwrap_or_default().as_bytes(), Some(carte.clone())).map_err(|e| anyhow!(e))?;
                    lien = Some(Lien { client, version_copie: r.version, dernier: r });
                    doc
                }
                None => hm_doc::Session::ouvrir(&carte).map_err(|e| anyhow!(e))?,
            };
            let scene = Scene::from_bytes(&carte, doc.doc().ecrire(), &vfs, &SceneOptions::default())?;
            if e.renderer.is_none() {
                e.renderer = Some(Arc::new(Renderer::new(Gpu::new()?)));
            }
            let renderer = e.renderer.clone().unwrap();
            let gs = renderer.upload(&scene);
            let resume = format!("{}\ncarte graphique : {} ; envoi {:.2} s ; fichiers du jeu : {}", scene.summary(), renderer.gpu.info, gs.upload_s, gameinfo.display());
            let jeu = gameinfo.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
            let renum = doc.rapport_lecture.renumerotes.len();
            let fenetre = lien.as_ref().map(|l| format!("\nliée à la fenêtre de Hammer Master (processus {}) où cette map est ouverte : `commande`, `annuler`, `enregistrer` s'y font, l'utilisateur les voit en direct et peut les annuler ; ses propres modifications sont reprises avant chaque outil", l.client.pid)).unwrap_or_default();
            e.session = Some(Session { carte, doc, scene, vfs, jeu, rapport: None, gs, field: None, derniere: None, lien });
            let renum = if renum > 0 { format!("\n{renum} numéro(s) en double renuméroté(s) à la lecture") } else { String::new() };
            Ok(vec![texte(format!("{}\n{resume}{renum}{fenetre}", entete(e.session.as_ref().unwrap())))])
        })
    }

    #[tool(description = "État de la session : map ouverte, révision, comptes, état du verrou « tour lu » et planches restant à lire ; si la map est ouverte dans la fenêtre de l'éditeur, ce que l'utilisateur y a sélectionné (objets à passer à `inspecter` ou à une commande).")]
    async fn carte_etat(&self, Parameters(_): Parameters<Rien>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (r, s) = session(e)?;
            let v = suivi::etat_verrou(&s.carte, &s.scene.revision);
            let fenetre = match &s.lien {
                Some(l) => format!("\nfenêtre liée (processus {}) ; sélection de l'utilisateur : {}", l.client.pid, if l.dernier.selection.is_empty() { "rien".to_string() } else { serde_json::to_string(&l.dernier.selection)? }),
                None => "\npas de fenêtre liée (la map n'est pas ouverte dans Hammer Master)".to_string(),
            };
            Ok(vec![texte(format!("{}\n{}\ncarte graphique : {}\nverrou : {}{}{fenetre}", entete(s), s.scene.summary(), r.gpu.info, v.raison, if v.planches_a_lire.is_empty() { String::new() } else { format!("\nprochaines planches à lire : {}", v.planches_a_lire.iter().take(8).cloned().collect::<Vec<_>>().join(", ")) }))])
        })
    }

    #[tool(description = "Rend une vue de la map avec ses vrais modèles et textures (lumière simplifiée : pas de lightmaps ni d'ombres). Rend l'image et la liste des objets les plus présents. Tout défaut de couleur ou de lumière vu ici se confirme en jeu.")]
    async fn vue(&self, Parameters(a): Parameters<VueArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (r, s) = session(e)?;
            let mut eye = v3(a.pos);
            if a.joueur.unwrap_or(false) {
                let f = field(s);
                let i = f.free_at(eye + Vec3::Z * 8.0).ok_or_else(|| anyhow!("aucun sol trouvé sous ce point"))?;
                eye = Vec3::new(eye.x, eye.y, f.free[i].floor as f32 + 64.0);
            }
            let fov = hm_render::fov_y_from_game(a.fov.unwrap_or(90.0).clamp(20.0, 150.0));
            let camera = match (a.vers, a.angles) {
                (Some(t), _) => Camera::look_at(eye, v3(t), fov),
                (None, Some(an)) => Camera::from_angles(eye, an[0], an[1], fov),
                _ => bail!("donner `vers` ou `angles`"),
            };
            let w = a.largeur.unwrap_or(1280).clamp(320, 1920);
            let frame = r.render(&s.gs, &View { camera, width: w, height: w * 9 / 16, show_tools: a.outils.unwrap_or(false), clip: None, light: Lighting::neutral(&s.scene) });
            let jpg = frame.to_jpeg(86)?;
            let t = format!("{}\nvue depuis ({:.0} {:.0} {:.0}), {} × {}, rendue en {:.0} ms\n{}", entete(s), eye.x, eye.y, eye.z, frame.width, frame.height, frame.ms, resume_vue(&s.scene, &frame));
            s.derniere = Some(frame);
            Ok(vec![texte(t), image(&jpg)])
        })
    }

    #[tool(description = "Vue de dessus orthographique, nord en haut, coupée entre deux hauteurs : pour voir le plan d'un étage, mettre zmax juste sous son plafond.")]
    async fn vue_dessus(&self, Parameters(a): Parameters<DessusArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (r, s) = session(e)?;
            let camera = Camera::top_down(Vec3::new(a.centre[0], a.centre[1], 0.0), a.demi.max(16.0), a.zmax, (a.zmax - a.zmin).max(1.0));
            let mut light = Lighting::neutral(&s.scene);
            light.headlight = 0.0;
            let frame = r.render(&s.gs, &View { camera, width: 1280, height: 1280, show_tools: a.outils.unwrap_or(false), clip: Some((Vec3::new(-1e9, -1e9, a.zmin), Vec3::new(1e9, 1e9, a.zmax))), light });
            let jpg = frame.to_jpeg(86)?;
            let t = format!("{}\nvue de dessus centrée ({:.0} {:.0}), {:.0} unités de côté, entre z {:.0} et {:.0} ; 1 pixel = {:.1} unités\n{}", entete(s), a.centre[0], a.centre[1], a.demi * 2.0, a.zmin, a.zmax, a.demi * 2.0 / 1280.0, resume_vue(&s.scene, &frame));
            s.derniere = Some(frame);
            Ok(vec![texte(t), image(&jpg)])
        })
    }

    #[tool(description = "Compare au même point de vue la vue 3D de Hammer++ (installé ; doit être fermé : il est ouvert sur une copie de la map, capturé, refermé, ses réglages remis tels quels) et celle de la fenêtre de Hammer Master (son mode script) : les deux côte à côte, Hammer++ à gauche. Prend le fichier de la map tel qu'il est sur le disque (pas les modifications non enregistrées). Compter une minute.")]
    async fn comparer(&self, Parameters(a): Parameters<HammerppArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let editeur = std::env::current_exe()?.with_file_name("hammer-master.exe");
            let sortie = std::env::temp_dir().join(format!("hm_comparaison_{}.jpg", std::process::id()));
            let d = |v: [f32; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
            let c = hm_compile::comparer::comparer(&s.carte, d(a.pos), d(a.vers), &editeur, &sortie, a.attente.unwrap_or(20)).map_err(|e| anyhow!(e))?;
            let jpg = std::fs::read(&c.image)?;
            let _ = std::fs::remove_file(&c.image);
            Ok(vec![texte(format!("{}
{}", entete(s), c.journal.join("
"))), image(&jpg)])
        })
    }

    #[tool(description = "Dit quel objet se trouve sous un pixel de la dernière image rendue (rang, classe, modèle ou matériau, id du VMF, position).")]
    async fn sous_le_pixel(&self, Parameters(a): Parameters<PixelArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let f = s.derniere.as_ref().ok_or_else(|| anyhow!("aucune vue rendue dans cette session"))?;
            if a.x >= f.width || a.y >= f.height {
                bail!("pixel hors de l'image ({} × {})", f.width, f.height);
            }
            Ok(vec![texte(decrire(&s.scene, f.id_at(a.x, a.y)))])
        })
    }

    #[tool(description = "Lance un rayon d'un point vers un autre et rend le premier obstacle : distance, point d'impact, objet touché. Les modèles sont approchés par leur boîte englobante.")]
    async fn rayon(&self, Parameters(a): Parameters<RayonArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let (o, t) = (v3(a.origine), v3(a.vers));
            let d = (t - o).normalize_or(Vec3::X);
            Ok(vec![texte(match s.scene.rayon(o, d) {
                Some((dist, id)) => {
                    let p = o + d * dist;
                    format!("impact à {:.1} unités, en ({:.1} {:.1} {:.1}) ; le point visé est à {:.1}\n{}", dist, p.x, p.y, p.z, o.distance(t), decrire(&s.scene, id))
                }
                None => "aucun obstacle sur ce rayon".to_string(),
            })])
        })
    }

    #[tool(description = "Liste des éléments de la map : `pieces` (régions trouvées seules : taille, sol, plafond), `objets` (solides et modèles posés), `modeles` (modèles et nombre d'exemplaires), `materiaux`, `entites` (entités ponctuelles). Filtre par texte et par boîte.")]
    async fn lister(&self, Parameters(a): Parameters<ListerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let lim = a.limite.unwrap_or(40).min(200);
            let f = a.filtre.clone().unwrap_or_default().to_ascii_lowercase();
            let dans = |p: Vec3| a.zone.map_or(true, |z| p.x >= z[0] && p.y >= z[1] && p.z >= z[2] && p.x <= z[3] && p.y <= z[4] && p.z <= z[5]);
            let mut lignes: Vec<String> = Vec::new();
            let total;
            match a.quoi.as_str() {
                "pieces" => {
                    let fld = field(s);
                    let mut r: Vec<&hm_espace::Region> = fld.regions.iter().filter(|r| r.is_room() && dans(r.centre)).collect();
                    r.sort_by_key(|r| std::cmp::Reverse(r.cells));
                    total = r.len();
                    lignes.extend(r.iter().take(lim).map(|r| format!("pièce {} · {:.0} × {:.0}, sol {:.0}, plafond {:.0} · centre ({:.0} {:.0} {:.0})", r.id, r.max.x - r.min.x, r.max.y - r.min.y, r.floor, r.ceil, r.centre.x, r.centre.y, r.centre.z)));
                }
                "objets" => {
                    let l: Vec<u32> = (1..s.scene.objects.len() as u32)
                        .filter(|&i| {
                            let o = &s.scene.objects[i as usize];
                            dans((o.min + o.max) * 0.5) && (f.is_empty() || o.class.contains(&f) || o.model.contains(&f) || o.material.contains(&f) || o.name.to_ascii_lowercase().contains(&f))
                        })
                        .collect();
                    total = l.len();
                    lignes.extend(l.iter().take(lim).map(|&i| decrire(&s.scene, i)));
                }
                "modeles" => {
                    let mut n = std::collections::BTreeMap::new();
                    for o in s.scene.objects.iter().filter(|o| o.kind == ObjKind::Prop && o.model.contains(&f) && dans((o.min + o.max) * 0.5)) {
                        *n.entry(o.model.as_str()).or_insert(0usize) += 1;
                    }
                    let mut l: Vec<_> = n.into_iter().collect();
                    l.sort_by_key(|x| std::cmp::Reverse(x.1));
                    total = l.len();
                    lignes.extend(l.iter().take(lim).map(|(m, c)| format!("{c} × {m}")));
                }
                "materiaux" => {
                    let l: Vec<_> = s.scene.materials.iter().filter(|m| m.name.contains(&f)).collect();
                    total = l.len();
                    lignes.extend(l.iter().take(lim).map(|m| format!("{} · {}{}", m.name, if m.found { m.shader.as_str() } else { "INTROUVABLE" }, m.base.as_deref().map(|b| format!(" · texture {b}")).unwrap_or_default())));
                }
                "entites" => {
                    let l: Vec<_> = s.scene.points.iter().filter(|p| dans(p.pos) && (f.is_empty() || p.class.contains(&f) || p.name.to_ascii_lowercase().contains(&f))).collect();
                    total = l.len();
                    lignes.extend(l.iter().take(lim).map(|p| format!("{} « {} » · id VMF {} · ({:.0} {:.0} {:.0})", p.class, p.name, p.vmf_id, p.pos.x, p.pos.y, p.pos.z)));
                }
                autre => bail!("`quoi` inconnu : {autre} (pieces, objets, modeles, materiaux, entites)"),
            }
            Ok(vec![texte(format!("{}\n{} résultat(s), {} affiché(s)\n{}", entete(s), total, lignes.len(), lignes.join("\n")))])
        })
    }

    #[tool(description = "Détail d'un objet : sa fiche, et pour une entité toutes ses clés du VMF.")]
    async fn inspecter(&self, Parameters(a): Parameters<InspecterArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let id = match (a.objet, a.vmf_id) {
                (Some(i), _) => i,
                (None, Some(v)) => s.scene.objects.iter().position(|o| o.vmf_id == v).ok_or_else(|| anyhow!("aucun objet d'id VMF {v}"))? as u32,
                _ => bail!("donner `objet` ou `vmf_id`"),
            };
            let mut t = decrire(&s.scene, id);
            if let Some(o) = s.scene.objects.get(id as usize) {
                if let Some(ent) = s.scene.vmf.entities().find(|en| en.id() == Some(o.vmf_id) && o.kind == ObjKind::Prop) {
                    t += "\nclés de l'entité :";
                    for p in &ent.pairs {
                        t += &format!("\n  {} = {}", p.key, p.value);
                    }
                }
            }
            Ok(vec![texte(t)])
        })
    }

    #[tool(description = "Lance le tour automatique de la révision ouverte : choisit seul les vues (pièces, façades de face et rasantes, bouts de réseau, compléments de couverture), écrit les planches, et rend le bilan : couverture mesurée, vues reprises d'une lecture précédente, planches à lire. Les planches se lisent ensuite une à une avec tour_page.")]
    async fn tour_lancer(&self, Parameters(a): Parameters<TourArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (r, s) = session(e)?;
            field(s);
            let fld = s.field.as_ref().unwrap();
            let groupes = a.groupes.unwrap_or_else(|| "pieces,facades,reseau".to_string());
            let mut vues = Vec::new();
            if groupes.contains("pieces") {
                vues.extend(hm_tour::vues_pieces(fld, &s.scene));
            }
            if groupes.contains("facades") {
                vues.extend(hm_tour::vues_facades(fld));
            }
            if groupes.contains("reseau") {
                vues.extend(hm_tour::vues_reseau(fld, &s.scene, hm_tour::MOTIFS_RESEAU));
            }
            let sel = hm_tour::selectionner(r, &s.gs, &s.scene, fld, vues);
            let base = suivi::dossier_suivi(&s.carte);
            let dossier = base.join("tour").join(&s.scene.revision);
            std::fs::create_dir_all(&dossier)?;
            std::fs::write(dossier.join("non_vus.json"), serde_json::to_vec_pretty(&sel.non_vus)?)?;
            let registre = suivi::Registre::charger(&base);
            let (cand, comp, a_voir, vus, non_vus) = (sel.candidates, sel.complements, sel.a_voir, sel.vus, sel.non_vus.len());
            let bilan = hm_tour::rendre(r, &s.gs, &s.scene, sel.vues, &dossier, Some(&registre))?;
            Ok(vec![texte(format!(
                "{}\n{}\ntour : {} vues gardées sur {cand} candidates, dont {comp} compléments\ncouverture : {vus} objets vus sur {a_voir} à voir ({:.1} %) ; {non_vus} jamais vus (liste : non_vus.json)\nlecture : {} vue(s) reprise(s) d'une lecture précédente, {} planche(s) à lire\ndossier : {}",
                entete(s), fld.summary(), bilan.vues.len(), 100.0 * vus as f32 / a_voir.max(1) as f32, bilan.deja_lues, bilan.planches.len(), dossier.display()
            ))])
        })
    }

    #[tool(description = "Rend une planche du tour à lire (9 vues légendées et son code de lecture). Sans nom : la prochaine planche non lue. Lire chaque vue, puis appeler tour_marquer_lu.")]
    async fn tour_page(&self, Parameters(a): Parameters<PageArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let v = suivi::etat_verrou(&s.carte, &s.scene.revision);
            let planche = match a.planche {
                Some(p) => p,
                None => v.planches_a_lire.first().cloned().ok_or_else(|| anyhow!("{}", if v.vues == 0 { v.raison.clone() } else { "toutes les planches de cette révision sont lues".to_string() }))?,
            };
            let dossier = suivi::dossier_suivi(&s.carte).join("tour").join(&s.scene.revision);
            let jpg = std::fs::read(dossier.join(&planche)).map_err(|_| anyhow!("planche {planche} introuvable dans {}", dossier.display()))?;
            let tour: serde_json::Value = serde_json::from_slice(&std::fs::read(dossier.join("tour.json"))?)?;
            let mut t = format!("{}\nplanche {planche} ({} planche(s) restent à lire)\ncases :", entete(s), v.planches_a_lire.len());
            for vue in tour["vues"].as_array().into_iter().flatten().filter(|x| x["planche"] == planche.as_str()) {
                t += &format!("\n  case {} · {} · {}", vue["case"], vue["nom"].as_str().unwrap_or(""), vue["legende"][0].as_str().unwrap_or(""));
            }
            Ok(vec![texte(t), image(&jpg)])
        })
    }

    #[tool(description = "Enregistre la lecture d'une planche. Exige le code de lecture écrit dans l'image, et pour chaque case une phrase qui dit ce qu'on voit, puis ce qui cloche (`defauts`). Refuse « rien à signaler » seul. Quand toutes les vues du tour sont lues, le verrou « tour lu » s'ouvre pour cette révision.")]
    async fn tour_marquer_lu(&self, Parameters(a): Parameters<LuArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let base = suivi::dossier_suivi(&s.carte);
            let dossier = base.join("tour").join(&s.scene.revision);
            let mut registre = suivi::Registre::charger(&base);
            let n = suivi::enregistrer_lecture(&dossier, &mut registre, &suivi::LecturePlanche {
                planche: a.planche.clone(),
                code: a.code,
                vues: a.vues.into_iter().map(|c| suivi::LectureCase { case: c.case, nom: String::new(), vu: c.vu, defauts: c.defauts.into_iter().map(|d| suivi::Defaut { gravite: d.gravite, confiance: d.confiance, quoi: d.quoi }).collect() }).collect(),
            })?;
            registre.enregistrer(&base)?;
            let v = suivi::etat_verrou(&s.carte, &s.scene.revision);
            Ok(vec![texte(format!("{}\nplanche {} : {n} vue(s) enregistrée(s) ; {}", entete(s), a.planche, v.raison))])
        })
    }

    #[tool(description = "Liste les signalements faits en jeu sur la map ouverte (`!defaut …` de l'addon hammer_master_signalement, et l'ancien format de rp_megalopole) : texte, date, état, position, et si le signalement a été fait sur la révision ouverte.")]
    async fn signalements_lire(&self, Parameters(a): Parameters<SignalementsArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let liste = signalements(&s.carte, a.dossiers)?;
            let etats = hm_signal::Etats::charger(&suivi::dossier_suivi(&s.carte));
            let mut t = format!("{}\n{} signalement(s)", entete(s), liste.len());
            for x in liste.iter().filter(|x| a.etat.as_deref().map_or(true, |f| etats.de(x).etat == f)) {
                let et = etats.de(x);
                t += &format!("\n{} · {} · {} · « {} »{}\n    œil ({:.0} {:.0} {:.0}) · {} · {}", x.id, x.quand, et.etat, x.texte, if et.note.is_empty() { String::new() } else { format!(" [{}]", et.note) }, x.oeil[0], x.oeil[1], x.oeil[2], if x.capture.is_some() { "capture" } else { "sans capture" }, hm_signal::jugement_revision(x, &s.scene.revision));
            }
            Ok(vec![texte(t)])
        })
    }

    #[tool(description = "Vue comparée d'un signalement : la capture faite en jeu et le rendu de la map ouverte côte à côte (même œil, même angle, même champ), l'objet du VMF que le joueur visait, et si la map a changé à cet endroit depuis. À regarder avant de corriger un défaut signalé, et après pour le fermer.")]
    async fn signalement_comparer(&self, Parameters(a): Parameters<ComparerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (r, s) = session(e)?;
            let liste = signalements(&s.carte, a.dossiers)?;
            let x = liste.iter().find(|x| x.id.eq_ignore_ascii_case(&a.id)).ok_or_else(|| anyhow!("signalement {} introuvable ({} lus)", a.id, liste.len()))?;
            let etats = hm_signal::Etats::charger(&suivi::dossier_suivi(&s.carte));
            let rev = hm_signal::jugement_revision(x, &s.scene.revision);
            let fov = a.fov_suppose.or_else(hm_signal::fov_du_jeu).unwrap_or(90.0);
            let legende = vec![format!("{} · {} · {} · « {} »", x.id, x.quand, etats.de(x).etat, x.texte), rev.clone()];
            let c = hm_signal::comparer(&r, &s.gs, &s.scene, x, &hm_render::planche::Police::systeme(), &legende, fov);
            let mut t = format!("{}\n{} · « {} »\n{rev}\nvisé : {}\ndistance œil → point visé : {:.0} en jeu, {} dans la map ouverte", entete(s), x.id, x.texte, hm_signal::decrire_objet(&s.scene, c.objet), c.distance_jeu, c.distance_outil.map_or("rien touché".to_string(), |d| format!("{d:.0}")));
            if let Some(d) = c.distance_outil.filter(|d| (d - c.distance_jeu).abs() > (0.1 * c.distance_jeu).max(24.0)) {
                t += &format!("\nla map ouverte diffère à cet endroit : en jeu le regard portait à {:.0}, il s'arrête ici à {d:.0}", c.distance_jeu);
            }
            if c.fov_suppose {
                t += &format!("\nchamp de vision supposé : {fov} (ce signalement ne le relevait pas)");
            }
            if let Some(o) = &x.objet {
                t += &format!("\nen jeu, entité visée : {} {}", o.classe, o.modele);
            }
            // où se tient le joueur, et où est ce qu'il vise
            let (oeil, vise, pieds) = (Vec3::from(x.oeil), Vec3::from(x.vise), Vec3::from(x.pos));
            let jpeg = c.image.to_jpeg(86)?;
            let champ = field(s);
            t += &format!("\nle joueur : {} ; le point visé : {}", champ.decrire_lieu(pieds + Vec3::Z * 8.0), champ.decrire_lieu_vise(vise, oeil));
            Ok(vec![texte(t), image(&jpeg)])
        })
    }

    #[tool(description = "Change l'état d'un signalement fait en jeu : `corrige`, `regle` (une règle le surveille désormais), `ecarte`, ou `ouvert` pour le rouvrir. Ne ferme un signalement qu'après avoir regardé sa vue comparée.")]
    async fn signalement_fermer(&self, Parameters(a): Parameters<FermerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let liste = signalements(&s.carte, a.dossiers)?;
            let x = liste.iter().find(|x| x.id.eq_ignore_ascii_case(&a.id)).ok_or_else(|| anyhow!("signalement {} introuvable", a.id))?;
            let base = suivi::dossier_suivi(&s.carte);
            let mut etats = hm_signal::Etats::charger(&base);
            etats.fixer(x, &a.etat, a.note.as_deref().unwrap_or(""), &hm_signal::maintenant())?;
            etats.enregistrer(&base)?;
            Ok(vec![texte(format!("{}\n{} · « {} » : {}", entete(s), x.id, x.texte, a.etat))])
        })
    }

    #[tool(description = "Lance les règles de vérification sur la map ouverte et rend le rapport : géométrie (solides invalides, hors grille, chevauchements, faces superposées), fichiers du jeu (introuvables, provenance), cohérence physique (ce qui flotte, ce qui est enfoui ou traverse un mur, WC sans cloison, portes, bouts de voie), budgets (comptages, limites du compilateur, lumière estimée) et joueur simulé (pièces inaccessibles, pièges). Les exceptions déclarées dans le hammer-master.toml du projet sont comptées à part. Chaque constat se regarde ensuite avec constat_vue.")]
    async fn valider(&self, Parameters(a): Parameters<ValiderArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let (config, fichier) = config_de(s)?;
            let familles: Vec<hm_regles::Famille> = match &a.familles {
                Some(l) => l.iter().map(|n| hm_regles::Famille::de(n).ok_or_else(|| anyhow!("famille inconnue « {n} » (geometrie, assets, physique, budgets, joueur, semantique)"))).collect::<Result<_>>()?,
                None => hm_regles::Famille::TOUTES.to_vec(),
            };
            let mut r = hm_regles::valider(&s.scene, &s.vfs, &config, &familles);
            r.config = fichier.map(|p| p.display().to_string());
            let max = a.max.unwrap_or(6);
            let mut t = format!("{}\n{} erreur(s), {} avertissement(s), {} information(s), {} constat(s) excepté(s) par le projet\n{}", entete(s), r.compte(hm_regles::Niveau::Erreur), r.compte(hm_regles::Niveau::Avertissement), r.compte(hm_regles::Niveau::Information), r.exceptes(), r.config.as_deref().map_or("réglages par défaut (pas de hammer-master.toml près du VMF)".to_string(), |c| format!("réglages du projet : {c}")));
            if let Some(m) = &r.modele {
                t += &format!("
modèle sémantique : {m}");
            }
            let motif = a.regle.as_deref().unwrap_or("*");
            for regle in hm_regles::catalogue() {
                if !hm_regles::motif(motif, regle.id) {
                    continue;
                }
                let liste = constats_de(&r, regle.id, a.filtre.as_deref());
                if liste.is_empty() {
                    continue;
                }
                t += &format!("\n\n[{}] {} · {} : {}", regle.niveau.nom(), regle.id, regle.titre, liste.len());
                for (k, c) in liste.iter().take(max).enumerate() {
                    t += &format!("\n  {}. {} · vers ({:.0} {:.0} {:.0})", k + 1, c.message, c.pos[0], c.pos[1], c.pos[2]);
                }
                if liste.len() > max {
                    t += &format!("\n  … et {} autre(s)", liste.len() - max);
                }
            }
            if !r.provenances.is_empty() {
                t += "\n\nprovenance des fichiers :";
                for (montage, p, mats, modeles, poses) in &r.provenances {
                    t += &format!("\n  {mats} matériau(x), {modeles} modèle(s) posés {poses} fois · {} · {montage}", p.nom());
                }
            }
            if let Some(b) = &r.budgets {
                t += &format!("\n\n{}", hm_regles::budgets::tableau(b));
            }
            if let Some(j) = &r.joueur {
                t += &format!("\n\n{}", hm_regles::joueur::resume(j));
            }
            s.rapport = Some(r);
            Ok(vec![texte(t)])
        })
    }

    #[tool(description = "Rend la vue d'un constat du dernier rapport de `valider` : l'œil est choisi seul, la boîte du constat est encadrée en jaune. À regarder avant de dire qu'un constat est un vrai défaut ou une fausse alerte.")]
    async fn constat_vue(&self, Parameters(a): Parameters<ConstatArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (r, s) = session(e)?;
            let (config, _) = config_de(s)?;
            let rapport = s.rapport.as_ref().ok_or_else(|| anyhow!("aucun rapport : appeler valider d'abord"))?;
            let liste = constats_de(rapport, &a.regle, a.filtre.as_deref());
            let c = liste.get(a.rang.saturating_sub(1)).ok_or_else(|| anyhow!("la règle {} n'a que {} constat(s)", a.regle, liste.len()))?;
            anyhow::ensure!(c.min != c.max, "ce constat n'a pas de lieu à montrer : {}", c.message);
            let monde = hm_regles::Monde::construire(&s.scene, &config);
            let v = hm_tour::constat::vue_constat(r, &s.gs, &s.scene, &monde, c, 960, 540);
            let nom = format!("{} n° {}", c.regle, a.rang);
            let img = hm_tour::constat::avec_legende(&v, &hm_tour::constat::legende(&nom, c, 130), &hm_render::planche::Police::systeme());
            let t = format!("{}\n{nom} · {}\n{}\nœil ({:.0} {:.0} {:.0}) vers ({:.0} {:.0} {:.0}) ; objet {} (id VMF {})", entete(s), hm_regles::regle(c.regle).map_or("", |x| x.titre), c.message, v.oeil.x, v.oeil.y, v.oeil.z, v.cible.x, v.cible.y, v.cible.z, c.objet, c.vmf_id);
            Ok(vec![texte(t), image(&img.to_jpeg(86)?)])
        })
    }

    #[tool(description = "Construit une map à partir d'une spec YAML (volumes d'air, ouvertures, éléments, entités ; format du skill gmod-map-maker) : écrit le VMF, son plan (.plan.json) et son modèle sémantique (.sem.json : pièces, rôles, objets nommés), copie les modules func_instance. Les murs, sols et plafonds sont déduits des volumes d'air : la map est scellée par construction. Avec une erreur dans la spec, rien n'est écrit et chaque erreur est dite. Écrit le même VMF que generer.py du skill, qu'il remplace. La map construite est ensuite ouverte dans la session : la regarder (vue), la juger (valider), chercher une fuite (budgets_vbsp).")]
    async fn construire(&self, Parameters(a): Parameters<ConstruireArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let spec = PathBuf::from(&a.spec);
            // les fichiers du jeu : le gameinfo du projet près de la spec, sinon celui de Garry's Mod
            let mut vfs = Vfs::new();
            let complet = std::path::absolute(&spec).unwrap_or_else(|_| spec.clone());
            let mut dossier = complet.parent();
            let mut gameinfo = None;
            for _ in 0..3 {
                let Some(d) = dossier else { break };
                gameinfo = [d.join("compile/gameinfo.txt"), d.join("_compile/gameinfo.txt"), d.join("gameinfo.txt")].into_iter().find(|p| p.is_file());
                if gameinfo.is_some() {
                    break;
                }
                dossier = d.parent();
            }
            vfs.add_gameinfo(&gameinfo.unwrap_or_else(|| hm_assets::jeu::gameinfo_gmod().unwrap_or_default()))?;
            let sortie = a.sortie.as_ref().map(PathBuf::from);
            let (c, sortie) = hm_edit::construction::construire_sur_disque(&spec, sortie.as_deref(), a.phase.as_deref(), None, &mut vfs)?;
            let bilan = hm_edit::construction::bilan(&c, &sortie);
            if !c.erreurs.is_empty() {
                bail!("{bilan}");
            }
            if a.ouvrir.unwrap_or(true) {
                let doc = hm_doc::Session::ouvrir(&sortie).map_err(|e| anyhow!(e))?;
                let scene = Scene::from_bytes(&sortie, doc.doc().ecrire(), &vfs, &SceneOptions::default())?;
                if e.renderer.is_none() {
                    e.renderer = Some(Arc::new(Renderer::new(Gpu::new()?)));
                }
                let gs = e.renderer.clone().unwrap().upload(&scene);
                let jeu = sortie.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
                e.session = Some(Session { carte: sortie.clone(), doc, scene, vfs, jeu, rapport: None, gs, field: None, derniere: None, lien: None });
                return Ok(vec![texte(format!("{}\n{bilan}\nmap ouverte dans la session", entete(e.session.as_ref().unwrap())))]);
            }
            Ok(vec![texte(bilan)])
        })
    }

    #[tool(description = "Modifie la map ouverte par une commande du noyau de l'éditeur (les mêmes que la fenêtre), annulable. Commandes : `creer_bloc` {min, max, materiau, classe?} ; `creer_entite` {classe, origine, cles?: [[nom, valeur]…]} ; `supprimer` {objets} ; `deplacer` {objets, delta, verrou_texture?} ; `tourner` {objets, centre, axe, degres} ; `redimensionner` {objets, de: [min, max], vers: [min, max]} ; `miroir` {objets, centre, axe: 0|1|2} ; `dupliquer` {objets, delta} ; `coller` {vmf, decalage} ; `materiau_faces` {faces: [{solide, face}…], materiau} ; `materiau_objets` {objets, materiau} ; `cles` {entite, cles: [[nom, valeur ou null]…]} (le monde est l'entité de son numéro) ; `grouper` {objets} ; `degrouper` {groupes} ; `lier_a_entite` {objets, classe} ; `remettre_au_monde` {entites} ; `evider` {objets, epaisseur} (tout solide devient des murs) ; `decouper` {objets, plan: [3 points], garder: derriere|devant|les_deux, materiau} ; `creuser` {outils, cibles?} ; `inserer_prefab` {vmf, origine} (jeton &i numéroté) ; `creer_disp` {faces, puissance: 2..4} ; `detruire_disp` {faces} ; `peindre_disp` {faces?, centre, rayon, pinceau: {mode: relever, hauteur} | {mode: lisser, force} | {mode: alpha, valeur, force}} ; `coudre_disp` {faces} ; `poser_decal` {point, materiau} ; `poser_overlay` {face: {solide, face}, point, materiau, taille: [l, h]} ; `sorties` {entite, sorties: [{sortie, cible, entree, parametre?, delai?, fois?}]} (remplace la liste) ; `modifier_sommets` {solide, operations: [{op: deplacer, points, delta} | {op: couper_face, a, b}]} (refusé si une face n'est plus plane) ; `fusionner` {objets} (enveloppe convexe) ; `primitive` {forme: {type: cylindre|pointe|sphere, cotes} | {type: coin} | {type: arche, largeur_mur, cotes, arc, depart, hauteur_ajoutee} | {type: escalier, marches, sens: 0..3}, min, max, materiau, classe?} ; `regler_faces` {faces, reglage: {echelle_u?, echelle_v?, decalage_u?, decalage_v?, rotation?, echelle_lumiere?, lissage?, axe_u?, axe_v?}} ; `justifier_faces` {faces, justif: gauche|droite|haut|bas|centre|ajuster, ensemble?, tailles: [[materiau, [l, h]]…]} ; `aligner_faces` {faces, sur: monde|face} ; `enrouler_faces` {source: {solide, face}, cibles} ; `remplacer_materiau` {ancien, nouveau, objets?} ; `poser_au_sol` {entites, bas: [distance de l'origine au bas du modèle…], aligner?} (sur la première brosse dessous ; aligner : l'axe haut suit la surface) ; `creer_visgroup` {nom, objets, cache?} ; `ajouter_au_visgroup` / `retirer_du_visgroup` {visgroup, objets} ; `montrer_visgroup` {visgroup, visible} (décoché : ses objets ne sont pas compilés) ; `renommer_visgroup` {visgroup, nom} ; `supprimer_visgroup` {visgroup} ; `deplacer_visgroup` {visgroup, monter} (une place parmi ceux de même parent) ; `cordon` {min, max, actif} (la première boîte du premier cordon ; actif : seul ce qui touche une boîte d'un cordon coché est compilé) ; `cordons` {actif, liste: [{nom, actif, boites: [[min, max]…]}…]} (remplace la liste) ; `lot` {nom, commandes} (une seule étape). Objets : [{\"type\": \"solide\"|\"entite\"|\"groupe\", \"id\": n}] (id VMF de `inspecter` ou `sous_le_pixel`). Une commande refusée (solide qui deviendrait invalide, objet introuvable) ne change rien. Rien n'est écrit sur le disque avant `enregistrer`.")]
    async fn commande(&self, Parameters(a): Parameters<CommandeArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let c: hm_doc::Commande = serde_json::from_value(a.commande).map_err(|err| anyhow!("commande illisible : {err}"))?;
            executer(e, c)
        })
    }

    #[tool(description = "Annule la dernière modification de la map ouverte (ou les n dernières), ou les rétablit avec `retablir: true`. L'historique est celui de la session, sans limite.")]
    async fn annuler(&self, Parameters(a): Parameters<AnnulerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let mut faits = Vec::new();
            if session(e)?.1.lien.is_some() {
                faits = demander(session(e)?.1, hm_doc::pont::Demande::Annuler { n: a.n.unwrap_or(1), retablir: a.retablir.unwrap_or(false) })?.faits;
                recopier(e)?;
            } else {
                let (_, s) = session(e)?;
                for _ in 0..a.n.unwrap_or(1).max(1) {
                    let r = if a.retablir.unwrap_or(false) { s.doc.retablir() } else { s.doc.annuler() };
                    match r {
                        Some(nom) => faits.push(nom),
                        None => break,
                    }
                }
            }
            if faits.is_empty() {
                bail!("rien à {}", if a.retablir.unwrap_or(false) { "rétablir" } else { "annuler" });
            }
            if session(e)?.1.lien.is_none() {
                recharger(e)?;
            }
            let (_, s) = session(e)?;
            let verbe = if a.retablir.unwrap_or(false) { "rétabli" } else { "annulé" };
            Ok(vec![texte(format!("{}\n{}", entete(s), faits.iter().map(|n| format!("{verbe} : {n}")).collect::<Vec<_>>().join("\n")))])
        })
    }

    #[tool(description = "Historique des modifications de la map ouverte depuis son ouverture : étapes faites (la dernière en bas) et étapes annulées qui peuvent être rétablies.")]
    async fn historique(&self, Parameters(_): Parameters<Rien>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            // celui de la fenêtre quand la map y est ouverte (les étapes de l'utilisateur et de Claude)
            let (faits, annules): (Vec<String>, Vec<String>) = if s.lien.is_some() {
                let r = demander(s, hm_doc::pont::Demande::Historique)?;
                (r.faits, r.annules)
            } else {
                let h = s.doc.historique();
                (h.faits().iter().map(|c| c.nom.clone()).collect(), h.annules().iter().rev().map(|c| c.nom.clone()).collect())
            };
            let mut t = format!("{}\n{} étape(s) faite(s)", entete(s), faits.len());
            for (i, c) in faits.iter().enumerate() {
                t += &format!("\n{:>4}  {c}", i + 1);
            }
            if !annules.is_empty() {
                t += &format!("\n{} étape(s) annulée(s), à rétablir :", annules.len());
                for c in &annules {
                    t += &format!("\n      {c}");
                }
            }
            Ok(vec![texte(t)])
        })
    }

    #[tool(description = "Montre des objets à l'utilisateur dans la fenêtre de l'éditeur : ils y sont sélectionnés et les quatre vues cadrées dessus. Seulement quand la map est ouverte dans la fenêtre. Objets : [{\"type\": \"solide\"|\"entite\"|\"groupe\", \"id\": n}].")]
    async fn fenetre_montrer(&self, Parameters(a): Parameters<MontrerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let objets: Vec<hm_doc::Ref> = serde_json::from_value(a.objets).map_err(|err| anyhow!("objets illisibles : {err}"))?;
            let (_, s) = session(e)?;
            let n = objets.len();
            demander(s, hm_doc::pont::Demande::Montrer { objets })?;
            Ok(vec![texte(format!("{}\n{n} objet(s) sélectionné(s) et cadré(s) dans la fenêtre", entete(s)))])
        })
    }

    #[tool(description = "Vérifie la map ouverte comme Alt+P dans la fenêtre de l'éditeur : contrôles de Hammer (entités sans cible, sorties vers rien, classes inconnues, solides invalides, hints…) et règles de Hammer Master, les plus graves d'abord ; pour chaque problème, l'objet en cause et, quand elle existe, la commande qui le corrige (à passer telle quelle à `commande`).")]
    async fn verifier(&self, Parameters(a): Parameters<VerifierArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let fgd = e.fgd.get_or_insert_with(|| Arc::new(hm_fgd::Fgd::de_garrysmod())).clone();
            let (_, s) = session(e)?;
            let log = s.carte.file_stem().map(|n| hm_compile::dossier_compile(Some(&s.carte)).join(format!("{}.log", n.to_string_lossy()))).filter(|p| p.is_file());
            let (v, _, erreur, _) = hm_edit::verification::tout(s.doc.doc(), &fgd, &s.vfs, Some(&s.carte), log.as_deref(), false);
            let compte = |g: u8| v.iter().filter(|p| p.grave == g).count();
            let mut t = format!("{}\n{} erreur(s), {} avertissement(s), {} information(s)", entete(s), compte(2), compte(1), compte(0));
            if let Some(err) = erreur {
                t += &format!("\n{err}");
            }
            for p in v.iter().take(a.limite.unwrap_or(50)) {
                let niveau = ["information", "avertissement", "erreur"][p.grave.min(2) as usize];
                t += &format!("\n[{niveau}] {} : {}", p.genre, p.message);
                if let Some(o) = &p.objet {
                    t += &format!(" ; objet {}", serde_json::to_string(o)?);
                }
                if let Some(c) = &p.correction {
                    t += &format!(" ; correction {}", serde_json::to_string(c)?);
                }
            }
            if v.len() > a.limite.unwrap_or(50) {
                t += &format!("\n… {} de plus (augmenter `limite`)", v.len() - a.limite.unwrap_or(50));
            }
            Ok(vec![texte(t)])
        })
    }

    #[tool(description = "Compile la map ouverte (vbsp, vvis, vrad, avec les exécutables choisis dans l'éditeur, par exemple VVIS++ et VRAD++) et attend la fin. Si la map est ouverte dans la fenêtre de l'éditeur, c'est la fenêtre qui compile : l'utilisateur voit le journal. Ne refait que ce qui a changé depuis la compilation précédente (seulement des entités : vbsp -onlyents ; des lumières : -onlyents puis vrad) sauf `partielle: false`. Rend le résultat, la durée, le BSP et la fin du journal ; une fuite est dite avec son chemin. `zone` : ne compiler que cette boîte, pour essayer vite. `lancer_jeu` : charger la map dans le jeu après (déjà ouvert : -hijack +map).")]
    async fn compiler(&self, Parameters(a): Parameters<CompilerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let mode = hm_compile::Mode::lire(a.mode.as_deref().unwrap_or("rapide")).ok_or_else(|| anyhow!("mode : geometrie, rapide, normale ou finale"))?;
            let zone = a.zone.map(|z| [[z[0], z[1], z[2]], [z[3], z[4], z[5]]]);
            let (copier, embarquer, lancer_jeu, partielle) = (a.copier.unwrap_or(true), a.embarquer.unwrap_or(false), a.lancer_jeu.unwrap_or(false), a.partielle.unwrap_or(true));
            let (_, s) = session(e)?;
            if s.lien.is_some() {
                demander(s, hm_doc::pont::Demande::Compiler { mode: a.mode.clone().unwrap_or_else(|| "rapide".into()), zone, copier, embarquer, lancer_jeu, partielle })?;
                let t0 = std::time::Instant::now();
                let fin = loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let r = demander(s, hm_doc::pont::Demande::Etat)?;
                    if !r.compilation.en_cours {
                        break r.compilation;
                    }
                    if t0.elapsed().as_secs() > 1800 {
                        bail!("compilation toujours en cours dans la fenêtre après 30 minutes");
                    }
                };
                let mut t = format!("{}\ncompilée dans la fenêtre ({}) : {}", entete(s), mode.nom(), match fin.reussi {
                    Some(true) => format!("réussie en {:.1} s", fin.secondes),
                    Some(false) => format!("ÉCHEC : {}", fin.erreur.clone().unwrap_or_default()),
                    None => "pas de résultat (compilation interrompue ?)".into(),
                });
                if let Some(b) = &fin.bsp {
                    t += &format!("\nBSP : {}", b.display());
                }
                t += &format!("\nfin du journal :\n{}", fin.journal.join("\n"));
                return Ok(vec![texte(t)]);
            }
            // sans fenêtre : le même déroulé, ici
            let o = hm_compile::Outils::detecter().ok_or_else(|| anyhow!("Garry's Mod introuvable : pas de vbsp, vvis, vrad"))?;
            let dossier = hm_compile::dossier_compile(Some(&s.carte));
            let base = s.carte.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "sans_titre".into());
            let nom = if zone.is_some() { format!("{base}_zone") } else { base };
            let doc = s.doc.doc();
            let vmf = match zone {
                Some([lo, hi]) => doc.pour_compilation_zone(&o.texture_cordon, Some(&s.carte), &dossier, lo, hi),
                None => doc.pour_compilation_dans(&o.texture_cordon, Some(&s.carte), &dossier),
            };
            let fichiers = if embarquer && zone.is_none() { hm_edit::embarque::fichiers(doc, &s.vfs, &config_de(s)?.0) } else { Vec::new() };
            let portee = match hm_compile::precedente(&dossier, &nom, mode) {
                Some(avant) if partielle => hm_compile::portee(&avant, &vmf),
                _ => hm_compile::Portee::Tout,
            };
            let msgs = hm_compile::attendre(&hm_compile::lancer_avec(vmf, &nom, &dossier, &o, mode, copier, fichiers, portee));
            let mut journal: Vec<String> = Vec::new();
            let mut resultat = String::from("pas de fin");
            for m in msgs {
                match m {
                    hm_compile::Message::Etape(x) => journal.push(format!("—— {x} ——")),
                    hm_compile::Message::Ligne(l) => journal.push(l),
                    hm_compile::Message::Fuite(p) => journal.push(format!("FUITE : {} points, de ({:.0} {:.0} {:.0})", p.len(), p.first().map_or(0.0, |q| q[0]), p.first().map_or(0.0, |q| q[1]), p.first().map_or(0.0, |q| q[2]))),
                    hm_compile::Message::Fin { reussi, bsp, secondes, erreur } => {
                        resultat = if reussi { format!("réussie en {secondes:.1} s ; BSP : {}", bsp.map(|b| b.display().to_string()).unwrap_or_default()) } else { format!("ÉCHEC : {}", erreur.unwrap_or_default()) };
                        if reussi && lancer_jeu {
                            let rx = o.lancer_jeu(&nom);
                            if let Ok(l) = rx.recv_timeout(std::time::Duration::from_secs(20)) {
                                journal.push(l);
                            }
                        }
                    }
                }
            }
            let fin: Vec<String> = journal.iter().rev().take(40).rev().cloned().collect();
            Ok(vec![texte(format!("{}\ncompilée ici ({}, {portee:?}) : {resultat}\nfin du journal :\n{}", entete(s), mode.nom(), fin.join("\n")))])
        })
    }

    #[tool(description = "Meuble une boîte avec un modèle d'aménagement du projet (dossier « amenagements », format TOML), comme « Aménager la boîte de la sélection » de l'onglet Préfabs : le modèle s'adapte à la boîte, vu depuis la porte, en une étape annulable ; refusé si la boîte sort de ses dimensions. Sans `zone` : la boîte de ce que l'utilisateur a sélectionné dans la fenêtre. Un modèle en brouillon se pose, mais ne jamais le valider à la place de l'utilisateur.")]
    async fn amenager_boite(&self, Parameters(a): Parameters<AmenagerBoiteArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let commande = {
                let (_, s) = session(e)?;
                let (lo, hi) = match a.zone {
                    Some(z) => ([z[0] as f32, z[1] as f32, z[2] as f32], [z[3] as f32, z[4] as f32, z[5] as f32]),
                    None => {
                        let sel = s.lien.as_ref().map(|l| l.dernier.selection.clone()).unwrap_or_default();
                        let (lo, hi) = hm_doc::boite_de(s.doc.doc(), &sel).ok_or_else(|| anyhow!("donner `zone`, ou sélectionner la pièce dans la fenêtre"))?;
                        (lo.as_vec3().to_array(), hi.as_vec3().to_array())
                    }
                };
                let dossier = hm_regles::semantique::dossier_amenagements(&std::path::absolute(&s.carte).unwrap_or_else(|_| s.carte.clone()));
                let (am, statut) = hm_model::amenagement::trouver(&dossier, &a.modele)?;
                let c = hm_edit::commande_amenagement_boite(&am, lo, hi, a.porte.as_deref().unwrap_or("y-"), s.doc.doc())?;
                (c, statut.nom().to_string(), dossier)
            };
            let (c, statut, dossier) = commande;
            let mut blocs = executer(e, c)?;
            blocs.insert(0, texte(format!("modèle « {} » ({statut}) de {}", a.modele, dossier.display())));
            Ok(blocs)
        })
    }

    #[tool(description = "Propper++ : transforme des brosses de la map ouverte en un modèle .mdl compilé par studiomdl (32 bits) dans le dossier du jeu (models/<modele>.mdl, matériaux de modèle VertexLitGeneric sous materials/models/<modele>/), comme la fenêtre Propper++ de l'éditeur. Faces enfouies et faces d'outil retirées, une pièce de collision convexe par brosse. Puis, sauf `remplacer: false`, le prop_static prend la place des brosses (et des entités à brosses visibles), en une étape annulable, dans la fenêtre si la map y est ouverte. Rend les comptes et la fin du journal de studiomdl.")]
    async fn propper(&self, Parameters(a): Parameters<PropperArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (p, jeu, sources) = {
                let (_, s) = session(e)?;
                let objets: Vec<hm_doc::Ref> = match a.objets {
                    Some(v) => serde_json::from_value(v).map_err(|err| anyhow!("objets illisibles : {err}"))?,
                    None => s.lien.as_ref().map(|l| l.dernier.selection.clone()).unwrap_or_default(),
                };
                if objets.is_empty() {
                    return Err(anyhow!("donner `objets`, ou sélectionner les brosses dans la fenêtre"));
                }
                let o = hm_edit::propper::OptionsPropper { modele: a.modele.clone(), surfaceprop: a.surfaceprop.clone().unwrap_or_else(|| "concrete".into()), masse: a.masse.unwrap_or(500.0), collision: a.collision.unwrap_or(true) };
                let p = hm_edit::propper::preparer(s.doc.doc(), &objets, &s.vfs, &o).map_err(|x| anyhow!(x))?;
                let jeu = hm_assets::jeu::gameinfo_gmod().and_then(|g| g.parent().map(std::path::PathBuf::from)).ok_or_else(|| anyhow!("Garry's Mod introuvable"))?;
                let carte = std::path::absolute(&s.carte).unwrap_or_else(|_| s.carte.clone());
                let sources = carte.parent().map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("modelsrc").join(&p.nom);
                (p, jeu, sources)
            };
            let qc = hm_edit::propper::ecrire(&p, &sources, &jeu).map_err(|x| anyhow!(x))?;
            let journal = hm_edit::propper::compiler(&qc, &jeu).map_err(|x| anyhow!(x))?;
            let modele = format!("models/{}.mdl", a.modele.trim().trim_end_matches(".mdl"));
            let mut blocs = vec![texte(format!(
                "compilé : {modele} — {} triangles, {} pièce(s) de collision, {} face(s) enfouie(s) retirée(s) ; sources : {}\n{}",
                p.triangles,
                p.pieces,
                p.faces_enfouies,
                sources.display(),
                journal.lines().filter(|l| l.contains("convex") || l.contains("WARNING")).collect::<Vec<_>>().join("\n")
            ))];
            if a.remplacer.unwrap_or(true) {
                let c = hm_doc::Commande::Lot { nom: format!("propper : {modele}"), commandes: vec![hm_doc::Commande::Supprimer { objets: p.utilises.clone() }, hm_doc::Commande::CreerEntite { classe: "prop_static".into(), origine: p.origine.to_array(), cles: vec![("model".into(), modele)] }] };
                blocs.extend(executer(e, c)?);
            }
            Ok(blocs)
        })
    }

    #[tool(description = "Enregistre la map ouverte dans son fichier (ou dans `chemin`). Le fichier précédent reste entier jusqu'à ce que le nouveau soit complet.")]
    async fn enregistrer(&self, Parameters(a): Parameters<EnregistrerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let p = if s.lien.is_some() {
                // la fenêtre enregistre sa map : un seul écrivain
                let r = demander(s, hm_doc::pont::Demande::Enregistrer { chemin: a.chemin.as_ref().map(PathBuf::from) })?;
                PathBuf::from(r.faits.first().cloned().unwrap_or_default())
            } else {
                s.doc.enregistrer(a.chemin.as_ref().map(std::path::Path::new)).map_err(|err| anyhow!(err))?
            };
            s.carte = p.clone();
            Ok(vec![texte(format!("{}\nenregistrée : {}", entete(s), p.display()))])
        })
    }

    #[tool(description = "Meuble une pièce du modèle sémantique d'après un modèle d'aménagement du projet. Mesure la pièce (mur de la porte, hauteur, matériau des murs), retire le mobilier qui s'y trouve (le bâti, l'éclairage et les portes des murs restent), pose le modèle, en une seule étape annulable. Refuse si la pièce est recoupée par une cloison ou si son mobilier est pris dans un modèle fusionné partagé. Commencer avec `essai: true` pour voir ce qui serait retiré et posé. Un modèle en brouillon se pose, mais la map ne passe plus la validation tant que l'utilisateur ne l'a pas validé : ne jamais tenter de le valider à sa place.")]
    async fn amenager(&self, Parameters(a): Parameters<AmenagerArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (plan, commande) = {
                let (_, s) = session(e)?;
                let complet = std::path::absolute(&s.carte).unwrap_or_else(|_| s.carte.clone());
                let sem = hm_model::Modele::du_vmf(&complet)?.ok_or_else(|| anyhow!("la map ouverte n'a pas de modèle sémantique (<carte>.sem.json)"))?;
                let (config, _) = config_de(s)?;
                let monde = hm_regles::Monde::construire(&s.scene, &config);
                let dossier = hm_regles::semantique::dossier_amenagements(&complet);
                let plan = hm_edit::mesure::preparer(&s.scene, &monde, &sem, &dossier, &a.piece, &a.modele, a.porte.as_deref(), a.garder.as_deref().unwrap_or(&[]))?;
                let c = hm_edit::commande_amenagement(&plan, &dossier, s.doc.doc())?;
                (plan, c)
            };
            if let Some(b) = plan.bloque() {
                bail!("{}\naménagement refusé : {b}", plan.decrire());
            }
            if a.essai.unwrap_or(false) {
                return Ok(vec![texte(format!("{}\nessai : rien n'est fait", plan.decrire()))]);
            }
            let mut blocs = executer(e, commande)?;
            blocs.insert(0, texte(plan.decrire()));
            Ok(blocs)
        })
    }

    #[tool(description = "Différence entre un VMF d'avant et la map ouverte (avec ses modifications non enregistrées), sans tenir compte des numéros : entités ajoutées, retirées, déplacées, modifiées, solides du monde ajoutés et retirés, et les zones touchées (par pièce du modèle sémantique). Sans `avant` : par rapport au fichier sur le disque, c'est-à-dire ce qui n'est pas encore enregistré.")]
    async fn difference(&self, Parameters(a): Parameters<DifferenceArgs>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let (avant, nom) = match &a.avant {
                Some(p) => (std::fs::read(p)?, p.clone()),
                None => (std::fs::read(&s.carte)?, format!("{} sur le disque", s.carte.display())),
            };
            let d = hm_edit::difference::comparer(&avant, &s.doc.doc().ecrire())?;
            let max = a.max.unwrap_or(10);
            let mut t = format!("{}\navant : {}\n{}", entete(s), nom, d.resume());
            let mut lister = |titre: &str, lignes: Vec<String>| {
                if !lignes.is_empty() {
                    t += &format!("\n{titre} ({}) :", lignes.len());
                    for l in lignes.iter().take(max) {
                        t += &format!("\n  {l}");
                    }
                    if lignes.len() > max {
                        t += &format!("\n  … et {} autre(s)", lignes.len() - max);
                    }
                }
            };
            lister("ajouté", d.ajoutes.iter().map(|x| x.decrire()).collect());
            lister("retiré", d.retires.iter().map(|x| x.decrire()).collect());
            lister("déplacé", d.deplaces.iter().map(|(x, v)| format!("{} → ({:.0} {:.0} {:.0})", x.decrire(), v[0], v[1], v[2])).collect());
            lister("modifié", d.modifies.iter().map(|x| x.decrire()).collect());
            let sem = hm_model::Modele::du_vmf(&std::path::absolute(&s.carte).unwrap_or_else(|_| s.carte.clone()))?;
            lister("zone touchée", d.zones(sem.as_ref()).iter().map(|z| format!("{} : {} changement(s), de ({:.0} {:.0} {:.0}) à ({:.0} {:.0} {:.0})", z.nom, z.n, z.min[0], z.min[1], z.min[2], z.max[0], z.max[1], z.max[2])).collect());
            Ok(vec![texte(t)])
        })
    }

    #[tool(description = "Lance vbsp seul sur une copie de la map ouverte, dans un dossier à part (une vingtaine de secondes sur une grande map) : dit s'il y a une fuite, avec l'endroit où elle sort, et rend les budgets exacts que seul le compilateur connaît (sommets, faces, feuilles, indices d'eau…). La map compilée du projet n'est pas touchée.")]
    async fn budgets_vbsp(&self, Parameters(_): Parameters<Rien>) -> Result<CallToolResult, ErrorData> {
        self.faire(|e| {
            let (_, s) = session(e)?;
            let exe = PathBuf::from(std::env::var("HM_VBSP").unwrap_or_else(|_| hm_assets::jeu::bin_gmod().map(|b| b.join("vbsp.exe").display().to_string()).unwrap_or_default()));
            let carte = std::path::absolute(&s.carte).unwrap_or_else(|_| s.carte.clone());
            let v = hm_regles::budgets::lancer_vbsp_sur(&carte, Some(&s.doc.doc().ecrire()), &s.scene.revision, &s.jeu, &exe)?;
            let mut t = format!("{}\nvbsp : code {} en {:.0} s ; {}", entete(s), v.code, v.duree, match &v.fuite {
                Some(p) => format!("FUITE : chemin de {} points, sort vers {:?}", p.len(), p.last().unwrap_or(&[0.0; 3])),
                None => "pas de fuite".to_string(),
            });
            for m in v.messages.iter().take(20) {
                t += &format!("\n  {m}");
            }
            let (config, _) = config_de(s)?;
            let r = hm_regles::valider(&s.scene, &s.vfs, &config, &[hm_regles::Famille::Budgets]);
            if let Some(b) = &r.budgets {
                t += &format!("\n\n{}", hm_regles::budgets::tableau(b));
            }
            for c in r.constats.iter().filter(|c| c.excepte.is_none()) {
                t += &format!("\n[{}] {} · {}", c.niveau.nom(), c.regle, c.message);
            }
            Ok(vec![texte(t)])
        })
    }
}

#[tool_handler]
impl ServerHandler for Serveur {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Hammer Master : voir et interroger une map Source / Garry's Mod (.vmf) sans la compiler. Commencer par carte_ouvrir. \
             Les vues montrent les vrais modèles et textures avec une lumière simplifiée. Avant toute compilation : tour_lancer, \
             puis lire chaque planche (tour_page, tour_marquer_lu) jusqu'à l'ouverture du verrou « tour lu ». \
             Les défauts que l'utilisateur signale en jeu (`!defaut …`) se lisent avec signalements_lire et se regardent avec signalement_comparer. \
             Si la map est ouverte dans la fenêtre de l'éditeur, carte_ouvrir s'y lie : les commandes s'y font sous les yeux de l'utilisateur \
             (une étape de son historique chacune), carte_etat donne ce qu'il a sélectionné, fenetre_montrer lui montre des objets, \
             compiler compile dans la fenêtre (l'utilisateur voit le journal). verifier fait comme Alt+P ; amenager_boite meuble une boîte.",
        )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let service = Serveur::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
