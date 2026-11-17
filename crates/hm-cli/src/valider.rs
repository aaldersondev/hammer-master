//! `hm valider` : lance les règles de vérification et écrit le rapport.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use hm_assets::Vfs;
use hm_regles::{Config, Constat, Famille, Niveau, Rapport};
use hm_scene::Scene;

use crate::Args;

/// Lit les réglages du projet (ou ceux de `--config`) et lance les familles demandées.
pub fn rapport(carte: &Path, args: &Args, vfs: &Vfs, scene: &Scene) -> Result<(Rapport, Config)> {
    let (config, fichier) = match args.get("--config") {
        Some(p) => (Config::lire(Path::new(p))?, Some(PathBuf::from(p))),
        None => Config::du_projet(&std::path::absolute(carte).unwrap_or_else(|_| carte.to_path_buf()))?,
    };
    let familles: Vec<Famille> = match args.get("--familles") {
        Some(l) => l.split(',').map(|n| Famille::de(n.trim()).ok_or_else(|| anyhow!("famille inconnue « {n} » (geometrie, assets, physique, budgets, joueur, semantique)"))).collect::<Result<_>>()?,
        None => Famille::TOUTES.to_vec(),
    };
    let mut r = hm_regles::valider(scene, vfs, &config, &familles);
    r.config = fichier.map(|p| p.display().to_string());
    Ok((r, config))
}

fn garde<'a>(r: &'a Rapport, args: &Args) -> Vec<&'a Constat> {
    let regles = args.all("--regle");
    let filtre = args.get("--filtre").map(|f| f.to_lowercase());
    r.constats.iter().filter(|c| regles.is_empty() || regles.iter().any(|x| hm_regles::motif(x, c.regle))).filter(|c| filtre.as_ref().map_or(true, |f| c.message.to_lowercase().contains(f))).collect()
}

/// Résumé lisible : par niveau, par règle, avec les premiers constats de chaque règle.
pub fn imprimer(r: &Rapport, args: &Args) {
    let max: usize = args.get("--max").and_then(|v| v.parse().ok()).unwrap_or(5);
    println!(
        "révision {} : {} erreur(s), {} avertissement(s), {} information(s){}",
        r.revision,
        r.compte(Niveau::Erreur),
        r.compte(Niveau::Avertissement),
        r.compte(Niveau::Information),
        if r.exceptes() > 0 { format!(", {} constat(s) excepté(s) par le projet", r.exceptes()) } else { String::new() }
    );
    if let Some(m) = &r.modele {
        println!("modèle sémantique : {m}");
    }
    match &r.config {
        Some(c) => println!("réglages du projet : {c}"),
        None => println!("réglages par défaut (pas de hammer-master.toml près du VMF)"),
    }
    println!("durées : {}", r.durees.iter().map(|(q, t)| format!("{q} {t:.2} s")).collect::<Vec<_>>().join(", "));
    let mut par_regle: BTreeMap<(Niveau, &str), Vec<&Constat>> = BTreeMap::new();
    for c in garde(r, args) {
        par_regle.entry((c.niveau, c.regle)).or_default().push(c);
    }
    for ((niveau, id), liste) in &par_regle {
        let actifs: Vec<&&Constat> = liste.iter().filter(|c| c.excepte.is_none()).collect();
        let titre = hm_regles::regle(id).map_or("", |x| x.titre);
        println!("\n[{}] {id} · {titre} : {}{}", niveau.nom(), actifs.len(), if actifs.len() < liste.len() { format!(" (+ {} excepté(s))", liste.len() - actifs.len()) } else { String::new() });
        for c in actifs.iter().take(max) {
            println!("  - {} · vers ({:.0} {:.0} {:.0})", c.message, c.pos[0], c.pos[1], c.pos[2]);
        }
        if actifs.len() > max {
            println!("  … et {} autre(s)", actifs.len() - max);
        }
        if args.flag("--exceptes") {
            for c in liste.iter().filter(|c| c.excepte.is_some()).take(max) {
                println!("  (excepté : {}) {}", c.excepte.as_deref().unwrap_or(""), c.message);
            }
        }
    }
    if !r.provenances.is_empty() {
        println!("\nprovenance des fichiers :");
        for (montage, p, mats, modeles, poses) in &r.provenances {
            println!("  {mats:4} matériau(x), {modeles:4} modèle(s) posés {poses:5} fois · {} · {montage}", p.nom());
        }
    }
    if let Some(b) = &r.budgets {
        println!("\n{}", hm_regles::budgets::tableau(b));
    }
    if let Some(j) = &r.joueur {
        println!("\n{}", hm_regles::joueur::resume(j));
    }
}

pub fn ecrire_json(r: &Rapport, chemin: &str) -> Result<()> {
    std::fs::write(chemin, serde_json::to_vec_pretty(r)?)?;
    println!("rapport complet : {chemin}");
    Ok(())
}

/// Rend une vue par constat (au plus `--par-regle` par règle) et les assemble en planches de neuf.
pub fn vues(r: &Rapport, config: &Config, args: &Args, scene: &Scene, renderer: &hm_render::Renderer, gs: &hm_render::GpuScene, dossier: &Path) -> Result<()> {
    use hm_render::planche::{planche, Case, Police};
    std::fs::create_dir_all(dossier)?;
    let par_regle: usize = args.get("--par-regle").and_then(|v| v.parse().ok()).unwrap_or(9);
    let monde = hm_regles::Monde::construire(scene, config);
    let police = Police::systeme();
    let mut groupes: BTreeMap<&str, Vec<&Constat>> = BTreeMap::new();
    for c in garde(r, args).into_iter().filter(|c| c.excepte.is_none() && c.min != c.max) {
        groupes.entry(c.regle).or_default().push(c);
    }
    let (w, h) = (640u32, 360u32);
    let mut index = Vec::new();
    for (regle, liste) in &groupes {
        let mut cases = Vec::new();
        for (k, c) in liste.iter().take(par_regle).enumerate() {
            let v = hm_tour::constat::vue_constat(renderer, gs, scene, &monde, c, w, h);
            let nom = format!("{}_{:02}", regle.replace('.', "_"), k + 1);
            let lignes = hm_tour::constat::legende(&nom, c, 92);
            std::fs::write(dossier.join(format!("{nom}.jpg")), hm_tour::constat::avec_legende(&v, &lignes, &police).to_jpeg(88)?)?;
            index.push(serde_json::json!({ "vue": nom, "regle": regle, "message": c.message, "oeil": [v.oeil.x, v.oeil.y, v.oeil.z], "cible": [v.cible.x, v.cible.y, v.cible.z], "objet": c.objet, "vmf_id": c.vmf_id }));
            let img = v.image;
            cases.push(Case { image: img, legende: lignes });
        }
        for (n, lot) in cases.chunks(9).enumerate() {
            let titre = format!("{regle} · {} · {} constat(s), {} montré(s) · révision {}", hm_regles::regle(regle).map_or("", |x| x.titre), liste.len(), liste.len().min(par_regle), r.revision);
            let p = planche(lot, 3, &titre, &police);
            let fichier = dossier.join(format!("planche_{}_{}.jpg", regle.replace('.', "_"), n + 1));
            std::fs::write(&fichier, p.to_jpeg(86)?)?;
            println!("planche : {}", fichier.display());
        }
    }
    std::fs::write(dossier.join("vues.json"), serde_json::to_vec_pretty(&index)?)?;
    Ok(())
}
