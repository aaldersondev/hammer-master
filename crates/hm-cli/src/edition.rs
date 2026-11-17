//! `hm commande`, `amenager`, `difference` : modifier une map par les commandes du noyau (`hm-doc`), comme le font la
//! fenêtre et le serveur MCP.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use glam::Vec3;
use hm_doc::{Commande, Session};
use hm_edit::difference::{comparer, Difference};
use hm_edit::mesure;
use hm_render::planche::{planche, Case, Police};
use hm_render::{Camera, Gpu, Lighting, Renderer, View};
use hm_scene::{Scene, SceneOptions};

use crate::Args;

pub const COMMANDES: [&str; 3] = ["commande", "amenager", "difference"];

/// Exécute une commande du noyau sur le fichier et l'enregistre.
fn executer(carte: &Path, c: Commande) -> Result<()> {
    let mut s = Session::ouvrir(carte).map_err(|e| anyhow!(e))?;
    let nom = c.nom();
    let r = s.executer(c).map_err(|e| anyhow!(e))?;
    s.enregistrer(None).map_err(|e| anyhow!(e))?;
    println!("fait : {nom}");
    if !r.crees.is_empty() {
        println!("créé : {}", serde_json::to_string(&r.crees)?);
    }
    if !r.message.is_empty() {
        println!("{}", r.message);
    }
    println!("révision de la map : {}", hm_vmf::revision(&std::fs::read(carte)?));
    Ok(())
}

pub fn lancer(cmd: &str, carte: &Path, args: &Args) -> Result<()> {
    match cmd {
        "commande" => {
            // la commande en JSON : en argument, ou dans un fichier par --json
            let texte = match args.get("--json") {
                Some(f) => std::fs::read_to_string(f)?,
                None => args.0.get(2).filter(|a| !a.starts_with("--")).cloned().ok_or_else(|| anyhow!("usage : hm commande <carte.vmf> '<commande en JSON>' (ou --json fichier)"))?,
            };
            let c: Commande = serde_json::from_str(&texte).map_err(|e| anyhow!("commande illisible : {e}"))?;
            executer(carte, c)
        }
        "amenager" => amenager(carte, args),
        "difference" => difference(carte, args),
        _ => bail!("commande d'édition inconnue : {cmd}"),
    }
}

fn amenager(carte: &Path, args: &Args) -> Result<()> {
    let piece = args.get("--piece").ok_or_else(|| anyhow!("--piece manque : l'identifiant de la pièce dans <carte>.sem.json"))?;
    let modele = args.get("--modele").ok_or_else(|| anyhow!("--modele manque : le nom du modèle d'aménagement"))?;
    let complet = std::path::absolute(carte)?;
    let sem = hm_model::Modele::du_vmf(&complet)?.ok_or_else(|| anyhow!("{} n'a pas de modèle sémantique (<carte>.sem.json)", carte.display()))?;
    let vfs = crate::monter(carte, args)?;
    let scene = Scene::load(carte, &vfs, &SceneOptions::default())?;
    let (config, _) = hm_regles::Config::du_projet(&complet)?;
    let monde = hm_regles::Monde::construire(&scene, &config);
    let garder: Vec<String> = args.all("--garder").iter().map(|s| s.to_string()).collect();
    let dossier = hm_regles::semantique::dossier_amenagements(&complet);
    let plan = mesure::preparer(&scene, &monde, &sem, &dossier, piece, modele, args.get("--porte"), &garder)?;
    println!("{}", plan.decrire());
    if let Some(b) = plan.bloque() {
        bail!("aménagement refusé : {b}");
    }
    if args.flag("--essai") {
        println!("essai : rien n'est écrit");
        return Ok(());
    }
    let s = Session::ouvrir(carte).map_err(|e| anyhow!(e))?;
    let c = hm_edit::commande_amenagement(&plan, &dossier, s.doc())?;
    drop(s);
    executer(carte, c)
}

fn imprimer_difference(d: &Difference, max: usize) {
    println!("{}", d.resume());
    let lister = |titre: &str, lignes: Vec<String>| {
        if lignes.is_empty() {
            return;
        }
        println!("{titre} ({}) :", lignes.len());
        for l in lignes.iter().take(max) {
            println!("  {l}");
        }
        if lignes.len() > max {
            println!("  … et {} autre(s)", lignes.len() - max);
        }
    };
    lister("ajouté", d.ajoutes.iter().map(|e| e.decrire()).collect());
    lister("retiré", d.retires.iter().map(|e| e.decrire()).collect());
    lister("déplacé", d.deplaces.iter().map(|(e, v)| format!("{} → ({:.0} {:.0} {:.0})", e.decrire(), v[0], v[1], v[2])).collect());
    lister("modifié", d.modifies.iter().map(|e| e.decrire()).collect());
    lister("solide du monde ajouté", d.solides_ajoutes.iter().map(|c| format!("vers ({:.0} {:.0} {:.0})", c[0], c[1], c[2])).collect());
    lister("solide du monde retiré", d.solides_retires.iter().map(|c| format!("vers ({:.0} {:.0} {:.0})", c[0], c[1], c[2])).collect());
}

/// `hm difference <avant.vmf> <après.vmf> [--vues dossier]` : ce qui a changé, et les zones touchées avant / après.
fn difference(avant: &Path, args: &Args) -> Result<()> {
    let apres = PathBuf::from(args.0.get(2).filter(|a| !a.starts_with("--")).ok_or_else(|| anyhow!("usage : hm difference <avant.vmf> <après.vmf> [--vues dossier]"))?);
    let (oa, ob) = (std::fs::read(avant)?, std::fs::read(&apres)?);
    println!("avant : {} (révision {})\naprès : {} (révision {})", avant.display(), hm_vmf::revision(&oa), apres.display(), hm_vmf::revision(&ob));
    let d = comparer(&oa, &ob)?;
    let max = args.get("--max").and_then(|v| v.parse().ok()).unwrap_or(12);
    imprimer_difference(&d, max);
    let sem = hm_model::Modele::du_vmf(&std::path::absolute(&apres)?)?;
    let zones = d.zones(sem.as_ref());
    if !zones.is_empty() {
        println!("zones touchées ({}) :", zones.len());
        for z in zones.iter().take(max) {
            println!("  {} : {} changement(s)", z.nom, z.n);
        }
    }
    let Some(sortie) = args.get("--vues") else { return Ok(()) };
    if zones.is_empty() {
        return Ok(());
    }
    let sortie = PathBuf::from(sortie);
    std::fs::create_dir_all(&sortie)?;
    let renderer = Renderer::new(Gpu::new()?);
    let police = Police::systeme();
    let (config, _) = hm_regles::Config::du_projet(&std::path::absolute(&apres)?)?;
    let vfs = crate::monter(&apres, args)?;
    let charge = |p: &Path| -> Result<Scene> { Ok(Scene::load(p, &vfs, &SceneOptions::default())?) };
    let (sa, sb) = (charge(avant)?, charge(&apres)?);
    let (ga, gb) = (renderer.upload(&sa), renderer.upload(&sb));
    let monde = hm_regles::Monde::construire(&sb, &config);
    let limite: usize = args.get("--zones").and_then(|v| v.parse().ok()).unwrap_or(4);
    let mut cases = Vec::new();
    for z in zones.iter().take(limite) {
        let (lo, hi) = (Vec3::from(z.min), Vec3::from(z.max));
        // l'œil est choisi sur la map d'après (un coin libre de la zone) et repris tel quel sur celle d'avant
        let vue = hm_tour::constat::vue_interieure(&renderer, &gb, &sb, &monde, lo, hi, 640, 360);
        let (oeil, vise) = match &vue {
            Some(v) => (v.oeil, v.cible),
            None => (Vec3::new(lo.x + 16.0, lo.y + 16.0, lo.z + 64.0), Vec3::new(hi.x, hi.y, lo.z + 40.0)),
        };
        let de_coin = |gs, scene: &Scene| {
            let f = renderer.render(gs, &View { camera: Camera::look_at(oeil, vise, 80.0), width: 1280, height: 720, show_tools: false, clip: None, light: Lighting::neutral(scene) });
            hm_tour::reduire2(&f.rgba, 1280, 720)
        };
        let (l, p, h) = (hi.x - lo.x, hi.y - lo.y, hi.z - lo.z);
        let de_dessus = |gs, scene: &Scene| {
            let demi = ((p + 48.0) * 0.5).max((l + 48.0) * 0.5 * 9.0 / 16.0);
            let camera = Camera::top_down(Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z), demi, hi.z - 2.0, hi.z + 8.0);
            let f = renderer.render(gs, &View { camera, width: 1280, height: 720, show_tools: false, clip: Some((Vec3::new(-1e9, -1e9, lo.z - 32.0), Vec3::new(1e9, 1e9, lo.z + h - 2.0))), light: Lighting::neutral(scene) });
            hm_tour::reduire2(&f.rgba, 1280, 720)
        };
        let titre = |quand: &str, vue: &str| vec![format!("{} · {quand} · {vue}", z.nom), format!("{} changement(s) dans la zone", z.n)];
        cases.push(Case { image: de_coin(&ga, &sa), legende: titre("AVANT", "depuis un coin") });
        cases.push(Case { image: de_coin(&gb, &sb), legende: titre("APRÈS", "depuis un coin") });
        cases.push(Case { image: de_dessus(&ga, &sa), legende: titre("AVANT", "de dessus, nord en haut") });
        cases.push(Case { image: de_dessus(&gb, &sb), legende: titre("APRÈS", "de dessus, nord en haut") });
    }
    let f = sortie.join("difference.jpg");
    std::fs::write(&f, planche(&cases, 2, &format!("différence · avant : révision {} · après : révision {} · {}", hm_vmf::revision(&oa), hm_vmf::revision(&ob), d.resume()), &police).to_jpeg(88)?)?;
    println!("planche : {}", f.display());
    Ok(())
}
