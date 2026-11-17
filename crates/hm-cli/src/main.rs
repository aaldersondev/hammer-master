//! `hm` : les opérations de Hammer Master en ligne de commande (essais, scripts, tests).
//!
//!     hm etat  <carte.vmf> [--gameinfo g.txt] [--contenu dossier]... [--pak carte.bsp]
//!     hm vue   <carte.vmf> --pos x,y,z (--vers x,y,z | --angles pitch,yaw) [--fov 75] [--taille 1280x720]
//!              [--outils] -o image.jpg
//!     hm tour  <carte.vmf> --dossier d [--groupes pieces,facades,reseau] [--zone "B2 rue"]
//!     hm zoom  <carte.vmf> --tour d --nom piece_12_a -o image.jpg
//!     hm lire  <carte.vmf> [--tour d] (--fichier lecture.json | --lectures dossier)
//!     hm verrou <carte.vmf>          code de sortie 0 si le tour de cette révision est lu, 2 sinon
//!     hm dessus <carte.vmf> [--centre x,y] [--demi 4000] [--zmin z --zmax z] -o image.jpg
//!     hm revision <carte.vmf>        révision du VMF, et celle qu'il porte s'il est estampillé
//!     hm estampiller <carte.vmf>     inscrit la révision dans la map (entité info_target hm_rev_…), sur place
//!     hm signalements <carte.vmf> [--dossier d]...                 liste des signalements faits en jeu
//!     hm signalement  <carte.vmf> (--id L07 -o image.jpg | --tous --sortie dossier) [--dossier d]...
//!     hm signalement-fermer <carte.vmf> --id L07 --etat corrige|regle|ecarte|ouvert [--note "…"]
//!     hm valider <carte.vmf> [--familles geometrie,assets,physique,budgets,joueur] [--regle motif]... [--filtre texte]
//!                [--max 5] [--json rapport.json] [--vues dossier [--par-regle 9]] [--config hammer-master.toml]
//!     hm vbsp  <carte.vmf>          vbsp seul sur une copie : fuite (code 2) et budgets exacts de cette révision
//!     hm nav   <carte.vmf> [--nav carte.nav]   accord entre le joueur simulé et le maillage de navigation du jeu
//!     hm amenagements <dossier>                liste les modèles d'aménagement et leur statut (brouillon, validé)
//!     hm amenagement <modele.toml> --vues dossier [--piece LxPxH]...   le modèle dans une pièce neutre, jugé par les règles
//!     hm amenagement-valider <modele.toml>     pose le statut « validé » ; dans un terminal, par l'utilisateur seulement
//!     hm construire <spec.yaml> [-o sortie.vmf] [--phase blockout|texture] [--modules dossier]   la spec du skill → VMF, plan, modèle sémantique
//!     hm comparer <carte.vmf> --pos x,y,z --vers x,y,z [-o comparaison.jpg] [--attente 20]
//!                                              la vue 3D de Hammer++ (installé, fermé) et celle de Hammer Master, côte à côte
//!     hm commande <carte.vmf> '{"commande":"deplacer","objets":[{"type":"solide","id":12}],"delta":[0,0,64]}'
//!                                              une commande du noyau (les mêmes que la fenêtre et le MCP), puis enregistre
//!     hm amenager <carte.vmf> --piece id --modele nom [--porte x-|x+|y-|y+] [--garder motif]... [--essai]
//!     hm difference <avant.vmf> <après.vmf> [--vues dossier] [--zones 4]   ce qui a changé, aux numéros près
//!     hm modele <carte.vmf> [--vues dossier]   le modèle sémantique (<carte>.sem.json) : pièces par rôle, accessibilité
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use glam::Vec3;
use hm_assets::Vfs;
use hm_render::{Camera, Gpu, Lighting, Renderer, View};
use hm_scene::{Scene, SceneOptions};

mod amenagement;
mod construire;
mod edition;
mod valider;

pub struct Args(pub Vec<String>);

impl Args {
    fn flag(&self, name: &str) -> bool {
        self.0.iter().any(|a| a == name)
    }
    /// Valeur de `--nom valeur` ou de `--nom=valeur` (forme à utiliser pour un nombre négatif).
    fn get(&self, name: &str) -> Option<&str> {
        self.all(name).into_iter().next()
    }
    fn all(&self, name: &str) -> Vec<&str> {
        let eq = format!("{name}=");
        let mut out = Vec::new();
        for (i, a) in self.0.iter().enumerate() {
            if let Some(v) = a.strip_prefix(&eq) {
                out.push(v);
            } else if a == name {
                if let Some(v) = self.0.get(i + 1) {
                    out.push(v.as_str());
                }
            }
        }
        out
    }
    fn floats(&self, name: &str) -> Option<Vec<f32>> {
        self.get(name).map(|v| v.split([',', 'x', ' ']).filter_map(|p| p.trim().parse().ok()).collect())
    }
    fn vec3(&self, name: &str) -> Result<Option<Vec3>> {
        match self.floats(name) {
            None => Ok(None),
            Some(v) if v.len() == 3 => Ok(Some(Vec3::new(v[0], v[1], v[2]))),
            Some(_) => bail!("{name} attend trois nombres séparés par des virgules"),
        }
    }
}

/// Le `gameinfo.txt` du projet : celui de `--gameinfo`, sinon `compile/gameinfo.txt` près du VMF, sinon celui du jeu.
fn gameinfo_de(vmf: &Path, args: &Args) -> PathBuf {
    if let Some(g) = args.get("--gameinfo") {
        return PathBuf::from(g);
    }
    let vmf = std::path::absolute(vmf).unwrap_or_else(|_| vmf.to_path_buf());
    let mut dir = vmf.parent();
    for _ in 0..3 {
        let Some(d) = dir else { break };
        for cand in ["compile/gameinfo.txt", "_compile/gameinfo.txt", "gameinfo.txt"] {
            if d.join(cand).is_file() {
                return d.join(cand);
            }
        }
        dir = d.parent();
    }
    hm_assets::jeu::gameinfo_gmod().unwrap_or_default()
}

/// Monte les fichiers du jeu : le `gameinfo.txt` donné, sinon celui du projet (`compile/gameinfo.txt` près du
/// VMF), sinon celui de Garry's Mod ; puis les dossiers `--contenu`.
fn monter(vmf: &Path, args: &Args) -> Result<Vfs> {
    let mut vfs = Vfs::new();
    // le contenu embarqué d'une map compilée passe avant tout : ce sont les fichiers de cette version-là
    for b in args.all("--pak") {
        vfs.add_bsp_pak(b)?;
    }
    for c in args.all("--contenu") {
        vfs.add_dir(c);
    }
    let mut gameinfo = args.get("--gameinfo").map(PathBuf::from);
    // chemin complet d'abord : un `carte.vmf` relatif n'a pas de dossier parent, et le projet ne serait pas trouvé
    let vmf = std::path::absolute(vmf).unwrap_or_else(|_| vmf.to_path_buf());
    if gameinfo.is_none() {
        let mut dir = vmf.parent();
        for _ in 0..3 {
            let Some(d) = dir else { break };
            for cand in ["compile/gameinfo.txt", "_compile/gameinfo.txt", "gameinfo.txt"] {
                if gameinfo.is_none() && d.join(cand).is_file() {
                    gameinfo = Some(d.join(cand));
                }
            }
            dir = d.parent();
        }
    }
    let gameinfo = gameinfo.unwrap_or_else(|| {
        eprintln!("attention : aucun gameinfo.txt de projet près de {} ; fichiers de Garry's Mod seuls", vmf.display());
        hm_assets::jeu::gameinfo_gmod().unwrap_or_default()
    });
    vfs.add_gameinfo(&gameinfo).with_context(|| format!("gameinfo {}", gameinfo.display()))?;
    Ok(vfs)
}

/// Champ de vision supposé quand un signalement ne le dit pas (ancien format) et que la configuration du jeu est
/// introuvable, en base 4/3.
const FOV_SUPPOSE: f32 = 90.0;

/// Signalements de la map : ceux des dossiers `--dossier`, sinon ceux du jeu installé.
fn signalements(carte: &Path, args: &Args) -> Result<Vec<hm_signal::Signalement>> {
    let nom = carte.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let mut dossiers: Vec<PathBuf> = args.all("--dossier").into_iter().map(PathBuf::from).collect();
    if dossiers.is_empty() {
        dossiers = hm_signal::dossiers_par_defaut(&nom);
    }
    hm_signal::lire(&dossiers)
}

fn main() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(carte)) = (raw.first().cloned(), raw.get(1).cloned()) else {
        bail!("usage : hm <comparer|etat|vue|dessus|tour|zoom|lire|verrou|valider|vbsp|nav|modele|modeles|construire|amenagement|amenagements|amenagement-valider|commande|amenager|difference|revision|estampiller|signalements|signalement|signalement-fermer> <carte.vmf> [options]");
    };
    let args = Args(raw);
    let carte = PathBuf::from(carte);
    // ---- modèles d'aménagement : le fichier du modèle, pas une map
    match cmd.as_str() {
        "construire" => return construire::lancer(&carte, &args),
        "amenagements" => return amenagement::lister(&carte),
        "amenagement-valider" => return amenagement::valider(&carte),
        "amenagement" => return amenagement::montrer(&carte, &args),
        _ => {}
    }
    // ---- Hammer++ et Hammer Master au même point de vue
    if cmd == "comparer" {
        let pos = args.vec3("--pos")?.ok_or_else(|| anyhow!("--pos x,y,z manque"))?;
        let vers = args.vec3("--vers")?.ok_or_else(|| anyhow!("--vers x,y,z manque"))?;
        let sortie = PathBuf::from(args.get("-o").unwrap_or("comparaison.jpg"));
        let attente = args.get("--attente").and_then(|v| v.parse().ok()).unwrap_or(20);
        let editeur = std::env::current_exe()?.with_file_name("hammer-master.exe");
        let d = |v: Vec3| [v.x as f64, v.y as f64, v.z as f64];
        let c = hm_compile::comparer::comparer(&carte, d(pos), d(vers), &editeur, &sortie, attente).map_err(|e| anyhow!(e))?;
        for l in &c.journal {
            println!("{l}");
        }
        println!("{}", c.image.display());
        return Ok(());
    }
    // ---- édition : commandes du noyau
    if edition::COMMANDES.contains(&cmd.as_str()) {
        return edition::lancer(&cmd, &carte, &args);
    }
    // ---- révision et estampille : le fichier seul
    if cmd == "revision" || cmd == "estampiller" {
        let octets = std::fs::read(&carte).with_context(|| format!("lecture de {}", carte.display()))?;
        let revision = hm_vmf::revision(&octets);
        if cmd == "estampiller" {
            match hm_vmf::estampiller(&octets).map_err(|e| anyhow!("{} : {e}", carte.display()))? {
                None => bail!("aucune entité ponctuelle où poser la révision : la map n'est pas estampillée"),
                Some(neuf) if neuf == octets => println!("déjà estampillée : révision {revision}"),
                Some(neuf) => {
                    std::fs::write(&carte, &neuf)?;
                    println!("estampillée : révision {revision} inscrite dans {} (entité info_target « {}{revision} »)", carte.display(), hm_vmf::PREFIXE_REVISION);
                }
            }
            return Ok(());
        }
        match hm_vmf::revision_inscrite(&octets) {
            Some(r) if r == revision => println!("révision {revision} ; estampille à jour"),
            Some(r) => println!("révision {revision} ; ATTENTION : la map porte l'estampille {r}, d'avant sa dernière modification"),
            None => println!("révision {revision} ; map non estampillée"),
        }
        return Ok(());
    }
    // ---- signalements en jeu : liste et états, sans charger la map
    if cmd == "signalements" || cmd == "signalement-fermer" {
        let revision = hm_vmf::revision(&std::fs::read(&carte).with_context(|| format!("lecture de {}", carte.display()))?);
        let suivi = hm_tour::suivi::dossier_suivi(&carte);
        let liste = signalements(&carte, &args)?;
        let mut etats = hm_signal::Etats::charger(&suivi);
        if cmd == "signalement-fermer" {
            let id = args.get("--id").ok_or_else(|| anyhow!("--id manque"))?;
            let s = liste.iter().find(|s| s.id.eq_ignore_ascii_case(id)).ok_or_else(|| anyhow!("signalement {id} introuvable"))?;
            let etat = args.get("--etat").ok_or_else(|| anyhow!("--etat manque ({})", hm_signal::ETATS.join(", ")))?;
            etats.fixer(s, etat, args.get("--note").unwrap_or(""), &hm_signal::maintenant())?;
            etats.enregistrer(&suivi)?;
            println!("{} : {etat}", s.id);
            return Ok(());
        }
        println!("{} signalement(s) pour {} ; révision ouverte {revision}", liste.len(), carte.file_stem().unwrap_or_default().to_string_lossy());
        for s in &liste {
            let e = etats.de(s);
            println!("{} · {} · {} · {}{}", s.id, s.quand, e.etat, s.texte, if e.note.is_empty() { String::new() } else { format!(" [{}]", e.note) });
            println!("      œil ({:.0} {:.0} {:.0}) · {} · {}", s.oeil[0], s.oeil[1], s.oeil[2], if s.capture.is_some() { "capture" } else { "sans capture" }, hm_signal::jugement_revision(s, &revision));
        }
        return Ok(());
    }
    // ---- règles de vérification : la scène sans la carte graphique, sauf si des vues sont demandées
    if cmd == "valider" && args.get("--vues").is_none() {
        let vfs = monter(&carte, &args)?;
        let scene = Scene::load(&carte, &vfs, &SceneOptions::default())?;
        let (r, _) = valider::rapport(&carte, &args, &vfs, &scene)?;
        valider::imprimer(&r, &args);
        if let Some(j) = args.get("--json") {
            valider::ecrire_json(&r, j)?;
        }
        if args.flag("--strict") && r.compte(hm_regles::Niveau::Erreur) > 0 {
            std::process::exit(2);
        }
        return Ok(());
    }
    // ---- commandes de suivi : elles n'ont pas besoin de charger la map
    if cmd == "verrou" || cmd == "lire" {
        let revision = hm_vmf::revision(&std::fs::read(&carte).with_context(|| format!("lecture de {}", carte.display()))?);
        let suivi = hm_tour::suivi::dossier_suivi(&carte);
        if cmd == "lire" {
            let tour = args.get("--tour").map(PathBuf::from).unwrap_or_else(|| suivi.join("tour").join(&revision));
            let mut registre = hm_tour::suivi::Registre::charger(&suivi);
            let mut fichiers: Vec<PathBuf> = args.all("--fichier").into_iter().map(PathBuf::from).collect();
            if let Some(d) = args.get("--lectures") {
                let mut l: Vec<PathBuf> = std::fs::read_dir(d)?.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "json")).collect();
                l.sort();
                fichiers.extend(l);
            }
            let (mut ok, mut vues, mut refus) = (0, 0, Vec::new());
            for f in &fichiers {
                let res = std::fs::read(f).map_err(anyhow::Error::from).and_then(|b| Ok(serde_json::from_slice::<hm_tour::suivi::LecturePlanche>(&b)?)).and_then(|l| hm_tour::suivi::enregistrer_lecture(&tour, &mut registre, &l));
                match res {
                    Ok(n) => {
                        ok += 1;
                        vues += n;
                    }
                    Err(e) => refus.push(format!("{} : {e}", f.file_name().unwrap_or_default().to_string_lossy())),
                }
            }
            registre.enregistrer(&suivi)?;
            println!("lectures enregistrées : {ok} planche(s), {vues} vue(s) ; refusées : {}", refus.len());
            for r in &refus {
                println!("  refus : {r}");
            }
        }
        if let Some(raison) = args.get("--forcer") {
            // Passage en force : réservé à l'utilisateur. Il faut un vrai terminal et une confirmation tapée ; un
            // appel par un outil automatique (sans terminal) est refusé.
            use std::io::{IsTerminal, Write};
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                println!("PASSAGE EN FORCE REFUSÉ : cette commande doit être lancée par l'utilisateur lui-même, dans un terminal.");
                std::process::exit(3);
            }
            if raison.trim().chars().count() < 8 {
                println!("PASSAGE EN FORCE REFUSÉ : donner une raison (--forcer \"pourquoi\").");
                std::process::exit(3);
            }
            print!("Compiler la révision {revision} SANS que son tour ait été lu ? Tapez les 6 premiers caractères de la révision pour confirmer : ");
            std::io::stdout().flush().ok();
            let mut ligne = String::new();
            std::io::stdin().read_line(&mut ligne).ok();
            if ligne.trim() != &revision[..6] {
                println!("Non confirmé : rien n'est changé.");
                std::process::exit(3);
            }
            hm_tour::suivi::forcer(&carte, &revision, raison)?;
            println!("Passage en force enregistré pour la révision {revision} (journal : passages_en_force.jsonl).");
        }
        let etat = hm_tour::suivi::etat_verrou_dans(&carte, &revision, args.get("--tour").map(Path::new));
        println!("verrou « tour lu » de la révision {} : {} — {}", etat.revision, if etat.ouvert { "OUVERT" } else { "FERMÉ" }, etat.raison);
        for p in etat.planches_a_lire.iter().take(12) {
            println!("  à lire : {p}");
        }
        std::process::exit(if etat.ouvert { 0 } else { 2 });
    }
    let t0 = Instant::now();
    let vfs = monter(&carte, &args)?;
    let scene = Scene::load(&carte, &vfs, &SceneOptions::default())?;
    let t_charge = t0.elapsed().as_secs_f32();
    println!("{}", scene.summary());
    if cmd == "etat" {
        for m in vfs.mounts() {
            println!("  monté : {}", m.label);
        }
        return Ok(());
    }
    if cmd == "espace" {
        let field = hm_espace::Field::build(&scene);
        println!("{}", field.summary());
        let mut rooms: Vec<&hm_espace::Region> = field.regions.iter().filter(|r| r.is_room()).collect();
        rooms.sort_by_key(|r| std::cmp::Reverse(r.cells));
        let hist = |lo: u32, hi: u32| rooms.iter().filter(|r| r.cells >= lo && r.cells < hi).count();
        println!("pièces par surface (cellules de 16²) : <40 : {}, 40-200 : {}, 200-1000 : {}, 1000-5000 : {}, >5000 : {}", hist(0, 40), hist(40, 200), hist(200, 1000), hist(1000, 5000), hist(5000, u32::MAX));
        for r in rooms.iter().take(8) {
            println!("  région {} : {} cellules, {:.0}×{:.0}, sol {:.0}, plafond {:.0}, centre ({:.0} {:.0} {:.0})", r.id, r.cells, r.max.x - r.min.x, r.max.y - r.min.y, r.floor, r.ceil, r.centre.x, r.centre.y, r.centre.z);
        }
        let mut toutes: Vec<&hm_espace::Region> = field.regions.iter().collect();
        toutes.sort_by_key(|r| std::cmp::Reverse(r.cells));
        println!("plus grandes régions, pièces ou non :");
        for r in toutes.iter().take(10) {
            println!("  région {} : {} cellules, {:.0}×{:.0}, sol {:.0}, couvert {:.2}, bords sur le vide {:.2}, pièce : {}", r.id, r.cells, r.max.x - r.min.x, r.max.y - r.min.y, r.floor, r.interior, r.drop_edges, r.is_room());
        }
        let mut par_niveau = std::collections::BTreeMap::new();
        for f in &field.facades {
            let e = par_niveau.entry((f.a.z / 500.0).floor() as i32 * 500).or_insert((0usize, 0f32));
            e.0 += 1;
            e.1 += f.a.distance(f.b);
        }
        println!("façades par niveau (z, nombre, longueur) : {par_niveau:?}");
        let long: f32 = field.facades.iter().map(|f| f.a.distance(f.b)).sum();
        println!("façades : {} tronçons, {:.0} unités au total", field.facades.len(), long);
        return Ok(());
    }
    let t1 = Instant::now();
    let renderer = Renderer::new(Gpu::new()?);
    let gs = renderer.upload(&scene);
    println!(
        "carte graphique : {} ; envoi {:.2} s ({:.0} Mo de textures) ; ouverture complète {:.2} s",
        renderer.gpu.info,
        t1.elapsed().as_secs_f32(),
        gs.texture_bytes as f64 / 1e6,
        t_charge + t1.elapsed().as_secs_f32()
    );
    if cmd == "nav" {
        // accord entre le joueur simulé et le maillage de navigation que le jeu a généré
        let nav = args.get("--nav").map(PathBuf::from).unwrap_or_else(|| carte.with_extension("nav"));
        let aires = hm_regles::joueur::lire_nav(&std::fs::read(&nav).with_context(|| format!("lecture de {}", nav.display()))?)?;
        let (config, _) = match args.get("--config") {
            Some(p) => (hm_regles::Config::lire(Path::new(p))?, None),
            None => hm_regles::Config::du_projet(&std::path::absolute(&carte)?)?,
        };
        let modele = hm_model::Modele::du_vmf(&std::path::absolute(&carte)?)?;
        let (bilan, _, sim) = hm_regles::joueur::simuler(&scene, &config, modele.as_ref());
        println!("{}", hm_regles::joueur::resume(&bilan));
        let sim = sim.ok_or_else(|| anyhow!("pas de point d'apparition"))?;
        let (n, oui, non, rien, ex) = hm_regles::joueur::accord_nav(&sim, &aires);
        println!("{} : {n} aires ; centre accessible au joueur simulé : {oui} ({:.1} %) ; inaccessible : {non} ; sans sol trouvé à 40 près : {rien}", nav.display(), oui as f64 / n.max(1) as f64 * 100.0);
        for p in ex {
            println!("  désaccord vers ({:.0} {:.0} {:.0})", p.x, p.y, p.z);
        }
        return Ok(());
    }
    if cmd == "vbsp" {
        // vbsp seul, dans un dossier à part : budgets exacts et fuite de cette révision
        let gameinfo = gameinfo_de(&carte, &args);
        let jeu = gameinfo.parent().unwrap_or(Path::new(".")).to_path_buf();
        let exe = PathBuf::from(args.get("--vbsp").map(String::from).or_else(|| std::env::var("HM_VBSP").ok()).unwrap_or_else(|| hm_assets::jeu::bin_gmod().map(|b| b.join("vbsp.exe").display().to_string()).unwrap_or_default()));
        let v = hm_regles::budgets::lancer_vbsp(&std::path::absolute(&carte)?, &scene.revision, &jeu, &exe)?;
        println!("vbsp : code {} en {:.0} s ; log {}", v.code, v.duree, v.log);
        for m in &v.messages {
            println!("  {m}");
        }
        match &v.fuite {
            Some(p) => println!("FUITE : chemin de {} points, sort vers {:?}", p.len(), p.last()),
            None => println!("pas de fuite"),
        }
        let (mut r, _) = valider::rapport(&carte, &args, &vfs, &scene)?;
        let avant = r.constats.len();
        hm_regles::budgets::constats_vbsp(&v, &mut r.constats);
        for c in &r.constats[avant..] {
            println!("[{}] {} · {}", c.niveau.nom(), c.regle, c.message);
        }
        if let Some(b) = &r.budgets {
            println!("{}", hm_regles::budgets::tableau(b));
        }
        std::process::exit(if v.fuite.is_some() || v.code != 0 { 2 } else { 0 });
    }
    if cmd == "modeles" {
        // cotes des modèles de la map dont le nom contient un des mots de --filtre (séparés par des virgules)
        let mots: Vec<String> = args.get("--filtre").unwrap_or("").split(',').map(|m| m.trim().to_lowercase()).filter(|m| !m.is_empty()).collect();
        let mut poses = vec![0usize; scene.models.len()];
        for p in &scene.props {
            poses[p.model as usize] += 1;
        }
        for (k, sm) in scene.models.iter().enumerate() {
            let court = sm.name.rsplit('/').next().unwrap_or(&sm.name);
            if !mots.is_empty() && !mots.iter().any(|m| court.contains(m.as_str())) {
                continue;
            }
            match &sm.model {
                Some(m) => println!("{:<34} x {:>6.1}..{:<6.1} y {:>6.1}..{:<6.1} z {:>6.1}..{:<6.1}  {} pose(s), {} convexe(s) de collision", court, m.bounds.0[0], m.bounds.1[0], m.bounds.0[1], m.bounds.1[1], m.bounds.0[2], m.bounds.1[2], poses[k], sm.collision.len()),
                None => println!("{court:<34} illisible"),
            }
        }
        return Ok(());
    }
    if cmd == "modele" {
        // le modèle sémantique de la map : ses pièces par rôle, et une vue d'une pièce de chaque rôle
        let complet = std::path::absolute(&carte)?;
        let m = hm_model::Modele::du_vmf(&complet)?.ok_or_else(|| anyhow!("pas de modèle sémantique : {} n'existe pas", hm_model::Modele::chemin(&complet).display()))?;
        println!("{} : {}\nsource : {}", hm_model::Modele::chemin(&complet).display(), m.resume(), m.source);
        let (config, _) = hm_regles::Config::du_projet(&complet)?;
        let (_, _, sim) = hm_regles::joueur::simuler(&scene, &config, Some(&m));
        let mut par_role: std::collections::BTreeMap<&str, Vec<&hm_model::Piece>> = Default::default();
        for p in &m.pieces {
            par_role.entry(p.role.as_str()).or_default().push(p);
        }
        println!("{:<16} {:>6} {:>10} {:>12} {:>8}", "rôle", "pièces", "accessible", "surface moy.", "vides");
        for (role, liste) in &par_role {
            let acc = liste.iter().filter(|p| sim.as_ref().and_then(|s| s.accessible(glam::Vec3::from(p.centre()), 40.0)).unwrap_or(false)).count();
            let surf = liste.iter().map(|p| p.surface()).sum::<f32>() / liste.len() as f32;
            println!("{:<16} {:>6} {:>10} {:>9.0} u² {:>8}", role, liste.len(), acc, surf, liste.iter().filter(|p| p.vide_expres).count());
        }
        if let Some(d) = args.get("--vues") {
            use hm_render::planche::{planche, Case, Police};
            let dossier = Path::new(d);
            std::fs::create_dir_all(dossier)?;
            let monde = hm_regles::Monde::construire(&scene, &config);
            let police = Police::systeme();
            let mut cases = Vec::new();
            for (role, liste) in &par_role {
                // la pièce médiane du rôle par la surface : ni la plus petite ni la plus grande
                let mut tri = liste.clone();
                tri.sort_by(|a, b| a.surface().total_cmp(&b.surface()));
                let p = tri[tri.len() / 2];
                let (lo, hi) = (glam::Vec3::new(p.emprise[0], p.emprise[1], p.sol), glam::Vec3::new(p.emprise[2], p.emprise[3], p.sol + p.hauteur.unwrap_or(128.0)));
                let c = hm_regles::Constat::lieu("sem.piece_inaccessible", lo, hi, format!("{} · {} pièce(s) de ce rôle · {:.0} × {:.0}{}", p.id, liste.len(), p.largeur(), p.profondeur(), if p.vide_expres { " · vide exprès" } else { "" }));
                let lignes = hm_tour::constat::legende(&format!("rôle « {role} »"), &c, 92);
                let image = match hm_tour::constat::vue_interieure(&renderer, &gs, &scene, &monde, lo, hi, 640, 360) {
                    Some(v) => v.image,
                    // aucun coin libre : la vue choisie comme pour un constat
                    None => hm_tour::constat::vue_constat(&renderer, &gs, &scene, &monde, &c, 640, 360).image,
                };
                cases.push(Case { image, legende: lignes });
            }
            for (n, lot) in cases.chunks(9).enumerate() {
                let f = dossier.join(format!("planche_pieces_par_role_{}.jpg", n + 1));
                std::fs::write(&f, planche(lot, 3, &format!("pièces du modèle sémantique, une par rôle · {} · révision {}", m.resume(), scene.revision), &police).to_jpeg(86)?)?;
                println!("planche : {}", f.display());
            }
        }
        return Ok(());
    }
    if cmd == "valider" {
        // avec --vues : le rapport, puis une vue par constat et des planches par règle
        let (r, config) = valider::rapport(&carte, &args, &vfs, &scene)?;
        valider::imprimer(&r, &args);
        if let Some(j) = args.get("--json") {
            valider::ecrire_json(&r, j)?;
        }
        valider::vues(&r, &config, &args, &scene, &renderer, &gs, Path::new(args.get("--vues").unwrap_or("vues_regles")))?;
        return Ok(());
    }
    if cmd == "tour" {
        let field = hm_espace::Field::build(&scene);
        println!("{}", field.summary());
        let groupes = args.get("--groupes").unwrap_or("pieces,facades,reseau");
        let mut vues = Vec::new();
        if groupes.contains("pieces") {
            vues.extend(hm_tour::vues_pieces(&field, &scene));
        }
        if groupes.contains("facades") {
            vues.extend(hm_tour::vues_facades(&field));
        }
        if groupes.contains("reseau") {
            vues.extend(hm_tour::vues_reseau(&field, &scene, hm_tour::MOTIFS_RESEAU));
        }
        if let Some(z) = args.get("--zone") {
            vues.retain(|v| v.zone.starts_with(z));
        }
        if let Some(n) = args.floats("--pas").and_then(|v| v.first().copied()) {
            // une vue sur n, pour un aperçu rapide
            let n = n.max(1.0) as usize;
            vues = vues.into_iter().enumerate().filter(|(i, _)| i % n == 0).map(|(_, v)| v).collect();
        }
        let mut par_groupe = std::collections::BTreeMap::new();
        for v in &vues {
            *par_groupe.entry(v.groupe).or_insert(0usize) += 1;
        }
        println!("vues : {par_groupe:?}");
        let suivi = hm_tour::suivi::dossier_suivi(&carte);
        let dossier = args.get("--dossier").map(PathBuf::from).unwrap_or_else(|| suivi.join("tour").join(&scene.revision));
        let registre = if args.flag("--sans-suivi") { None } else { Some(hm_tour::suivi::Registre::charger(&suivi)) };
        let t_sel = Instant::now();
        let sel = hm_tour::selectionner(&renderer, &gs, &scene, &field, vues);
        println!(
            "sélection : {} vues gardées sur {} candidates, dont {} compléments ; objets à voir {} dont vus {} ({:.1} %) ; {:.1} s",
            sel.vues.len(), sel.candidates, sel.complements, sel.a_voir, sel.vus, 100.0 * sel.vus as f32 / sel.a_voir.max(1) as f32, t_sel.elapsed().as_secs_f32()
        );
        std::fs::create_dir_all(&dossier)?;
        std::fs::write(dossier.join("non_vus.json"), serde_json::to_vec_pretty(&sel.non_vus)?)?;
        let vues = sel.vues;
        let bilan = hm_tour::rendre(&renderer, &gs, &scene, vues, &dossier, registre.as_ref())?;
        println!("tour : {} vues dont {} déjà lues (reprises), {} planches à lire, {:.1} s ; {}", bilan.vues.len(), bilan.deja_lues, bilan.planches.len(), bilan.secondes, dossier.display());
        return Ok(());
    }
    if cmd == "signalement" {
        // vue comparée : la capture du jeu à côté du rendu de l'outil, et l'objet que le joueur visait
        let liste = signalements(&carte, &args)?;
        let etats = hm_signal::Etats::charger(&hm_tour::suivi::dossier_suivi(&carte));
        let police = hm_render::planche::Police::systeme();
        let choisis: Vec<&hm_signal::Signalement> = if args.flag("--tous") {
            liste.iter().collect()
        } else {
            let id = args.get("--id").ok_or_else(|| anyhow!("--id L07 ou --tous manque"))?;
            vec![liste.iter().find(|s| s.id.eq_ignore_ascii_case(id)).ok_or_else(|| anyhow!("signalement {id} introuvable"))?]
        };
        let sortie = args.get("--sortie").map(PathBuf::from);
        if let Some(d) = &sortie {
            std::fs::create_dir_all(d)?;
        }
        let champ = hm_espace::Field::build(&scene);
        let mut bilan = Vec::new();
        for s in choisis {
            let rev = hm_signal::jugement_revision(s, &scene.revision);
            let legende = vec![format!("{} · {} · {} · « {} »", s.id, s.quand, etats.de(s).etat, s.texte), rev.clone()];
            let fov_defaut = args.floats("--fov-suppose").and_then(|v| v.first().copied()).or_else(hm_signal::fov_du_jeu).unwrap_or(FOV_SUPPOSE);
            let c = hm_signal::comparer(&renderer, &gs, &scene, s, &police, &legende, fov_defaut);
            let objet = hm_signal::decrire_objet(&scene, c.objet);
            let fichier = match (&sortie, args.get("-o")) {
                (Some(d), _) => d.join(format!("signalement_{}.jpg", s.id)),
                (None, Some(o)) => PathBuf::from(o),
                (None, None) => bail!("-o image.jpg ou --sortie dossier manque"),
            };
            std::fs::write(&fichier, c.image.to_jpeg(88)?)?;
            println!("{} · « {} »", s.id, s.texte);
            println!("  {rev}");
            // où se tient le joueur, et où est ce qu'il vise
            let (oeil, vise) = (Vec3::from(s.oeil), Vec3::from(s.vise));
            println!("  visé : {objet}");
            println!("  le joueur : {} ; le point visé : {}", champ.decrire_lieu(Vec3::from(s.pos) + Vec3::Z * 8.0), champ.decrire_lieu_vise(vise, oeil));
            println!("  distance œil → point visé : {:.0} en jeu, {} dans la map ouverte{}", c.distance_jeu, c.distance_outil.map_or("rien touché".to_string(), |d| format!("{d:.0}")), if c.fov_suppose { format!(" ; champ de vision supposé : {fov_defaut}, le réglage actuel du jeu (l'ancien format ne le relevait pas)") } else { String::new() });
            if let Some(d) = c.distance_outil.filter(|d| (d - c.distance_jeu).abs() > (0.1 * c.distance_jeu).max(24.0)) {
                println!("  la map ouverte diffère à cet endroit : en jeu le regard portait à {:.0}, il s'arrête ici à {d:.0}", c.distance_jeu);
            }
            println!("  image : {}", fichier.display());
            bilan.push(serde_json::json!({ "id": s.id, "texte": s.texte, "objet": c.objet, "vise": objet, "distance_jeu": c.distance_jeu, "distance_outil": c.distance_outil, "image": fichier }));
        }
        if let Some(d) = &sortie {
            std::fs::write(d.join("comparaisons.json"), serde_json::to_vec_pretty(&bilan)?)?;
        }
        return Ok(());
    }
    if cmd == "empreintes" {
        // complète un tour rendu avant l'ajout des empreintes : hm empreintes <carte> --tour <dossier>
        let dossier = PathBuf::from(args.get("--tour").ok_or_else(|| anyhow!("--tour dossier manque"))?);
        let mut tour: serde_json::Value = serde_json::from_slice(&std::fs::read(dossier.join("tour.json"))?)?;
        let mut identites = std::collections::BTreeMap::new();
        let mut n = 0;
        for v in tour["vues"].as_array_mut().ok_or_else(|| anyhow!("tour.json sans vues"))? {
            let f = |k: &str, i: usize| v[k][i].as_f64().unwrap_or(0.0) as f32;
            let eye = Vec3::new(f("oeil", 0), f("oeil", 1), f("oeil", 2));
            let dir = Vec3::new(f("regard", 0), f("regard", 1), f("regard", 2));
            let camera = Camera::look_at(eye, eye + dir * 100.0, v["fov_y"].as_f64().unwrap_or(75.0) as f32);
            v["empreinte"] = serde_json::Value::String(hm_tour::suivi::empreinte_camera(&renderer, &gs, &scene, camera, &mut identites));
            n += 1;
        }
        std::fs::write(dossier.join("tour.json"), serde_json::to_vec(&tour)?)?;
        println!("{n} empreintes écrites dans {}", dossier.join("tour.json").display());
        return Ok(());
    }
    let taille = args.floats("--taille").unwrap_or_else(|| vec![1280.0, 720.0]);
    let (w, h) = (taille[0] as u32, *taille.get(1).unwrap_or(&720.0) as u32);
    let light = Lighting::neutral(&scene);
    let out = args.get("-o").ok_or_else(|| anyhow!("-o image.jpg manque"))?;
    let view = match cmd.as_str() {
        "zoom" => {
            // une vue du tour, rendue seule en grand : hm zoom <carte> --tour <dossier> --nom <vue> -o image.jpg
            let dossier = PathBuf::from(args.get("--tour").ok_or_else(|| anyhow!("--tour dossier manque"))?);
            let nom = args.get("--nom").ok_or_else(|| anyhow!("--nom manque"))?;
            let tour: serde_json::Value = serde_json::from_slice(&std::fs::read(dossier.join("tour.json"))?)?;
            let v = tour["vues"].as_array().and_then(|a| a.iter().find(|v| v["nom"] == nom)).ok_or_else(|| anyhow!("vue {nom} absente de tour.json"))?;
            let f = |k: &str, i: usize| v[k][i].as_f64().unwrap_or(0.0) as f32;
            let eye = Vec3::new(f("oeil", 0), f("oeil", 1), f("oeil", 2));
            let dir = Vec3::new(f("regard", 0), f("regard", 1), f("regard", 2));
            let camera = Camera::look_at(eye, eye + dir * 100.0, v["fov_y"].as_f64().unwrap_or(75.0) as f32);
            println!("{}", v["legende"].as_array().map(|l| l.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" | ")).unwrap_or_default());
            View { camera, width: w, height: h, show_tools: args.flag("--outils"), clip: None, light }
        }
        "vue" => {
            let pos = args.vec3("--pos")?.ok_or_else(|| anyhow!("--pos x,y,z manque"))?;
            let fov = args.floats("--fov").and_then(|v| v.first().copied()).unwrap_or(hm_render::fov_y_from_game(90.0));
            let camera = if let Some(t) = args.vec3("--vers")? {
                Camera::look_at(pos, t, fov)
            } else if let Some(a) = args.floats("--angles") {
                Camera::from_angles(pos, a[0], *a.get(1).unwrap_or(&0.0), fov)
            } else {
                bail!("--vers x,y,z ou --angles pitch,yaw manque");
            };
            View { camera, width: w, height: h, show_tools: args.flag("--outils"), clip: None, light }
        }
        "dessus" => {
            let c = args.floats("--centre").unwrap_or_else(|| vec![(scene.min.x + scene.max.x) / 2.0, (scene.min.y + scene.max.y) / 2.0]);
            let demi = args.floats("--demi").and_then(|v| v.first().copied()).unwrap_or((scene.max.y - scene.min.y) / 2.0 * 1.02);
            let zmax = args.floats("--zmax").and_then(|v| v.first().copied()).unwrap_or(scene.max.z + 16.0);
            let zmin = args.floats("--zmin").and_then(|v| v.first().copied()).unwrap_or(scene.min.z - 16.0);
            let camera = Camera::top_down(Vec3::new(c[0], c[1], 0.0), demi, zmax, zmax - zmin);
            let clip = Some((Vec3::new(-1e9, -1e9, zmin), Vec3::new(1e9, 1e9, zmax)));
            let mut light = light;
            light.headlight = 0.0;
            View { camera, width: w, height: h, show_tools: args.flag("--outils"), clip, light }
        }
        other => bail!("commande inconnue : {other}"),
    };
    let mut temps = Vec::new();
    let mut frame = renderer.render(&gs, &view);
    temps.push(frame.ms);
    for _ in 0..3 {
        frame = renderer.render(&gs, &view);
        temps.push(frame.ms);
    }
    let jpg = frame.to_jpeg(88)?;
    std::fs::write(out, &jpg)?;
    let vus: std::collections::HashSet<u32> = frame.ids.iter().copied().filter(|&i| i != 0).collect();
    println!("vue {w}×{h} : {:?} ms ; {} objets visibles ; {} ({} Ko)", temps.iter().map(|t| t.round() as u32).collect::<Vec<_>>(), vus.len(), out, jpg.len() / 1024);
    Ok(())
}
