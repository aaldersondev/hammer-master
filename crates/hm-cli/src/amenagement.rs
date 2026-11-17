//! `hm amenagement`, `hm amenagements`, `hm amenagement-valider` : montrer, lister et valider les modèles
//! d'aménagement d'un projet.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use glam::Vec3;
use hm_model::amenagement::{piece_neutre, Amenagement, Statut, Validations};
use hm_render::planche::{planche, Case, Police};
use hm_render::{Camera, Gpu, Lighting, Renderer, View};
use hm_scene::{Scene, SceneOptions};

use crate::Args;

/// Liste les modèles d'un dossier avec leur statut.
pub fn lister(dossier: &Path) -> Result<()> {
    let v = Validations::charger(dossier);
    let mut fichiers: Vec<PathBuf> = std::fs::read_dir(dossier)?.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "toml")).collect();
    fichiers.sort();
    for f in fichiers {
        match Amenagement::lire(&f) {
            Ok(a) => println!("{:<24} {:<14} {:<10} pièce {:.0}-{:.0} × {:.0}-{:.0} · empreinte {}", a.nom, a.role, v.statut(&a).nom(), a.piece.largeur[0], a.piece.largeur[1], a.piece.profondeur[0], a.piece.profondeur[1], a.empreinte()),
            Err(e) => println!("{} : illisible ({e:#})", f.display()),
        }
    }
    Ok(())
}

/// Pose le statut « validé ». Réservé à l'utilisateur : il faut un vrai terminal et une confirmation tapée.
pub fn valider(fichier: &Path) -> Result<()> {
    use std::io::{IsTerminal, Write};
    let a = Amenagement::lire(fichier)?;
    let dossier = fichier.parent().unwrap_or(Path::new("."));
    let mut v = Validations::charger(dossier);
    if v.statut(&a) == Statut::Valide {
        println!("« {} » est déjà validé (empreinte {}).", a.nom, a.empreinte());
        return Ok(());
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        println!("VALIDATION REFUSÉE : cette commande doit être lancée par l'utilisateur lui-même, dans un terminal.");
        std::process::exit(3);
    }
    print!("Valider le modèle d'aménagement « {} » (rôle {}, empreinte {}) ? Tapez son nom pour confirmer : ", a.nom, a.role, a.empreinte());
    std::io::stdout().flush().ok();
    let mut ligne = String::new();
    std::io::stdin().read_line(&mut ligne).ok();
    if ligne.trim() != a.nom {
        println!("Non confirmé : rien n'est changé.");
        std::process::exit(3);
    }
    v.valider(&a, &hm_signal::maintenant());
    v.enregistrer(dossier)?;
    println!("« {} » est validé. Toute modification du fichier le repassera en brouillon.", a.nom);
    Ok(())
}

/// Tailles de pièce à montrer : celles de `--piece LxPxH`, sinon la plus petite et la plus grande que le modèle accepte.
fn tailles(a: &Amenagement, args: &Args) -> Vec<(f32, f32, f32)> {
    let h = a.piece.hauteur_min.max(128.0);
    let donnees: Vec<(f32, f32, f32)> = args.all("--piece").iter().filter_map(|t| {
        let v: Vec<f32> = t.split(['x', 'X', ',']).filter_map(|p| p.trim().parse().ok()).collect();
        (v.len() >= 2).then(|| (v[0], v[1], v.get(2).copied().unwrap_or(h)))
    }).collect();
    if !donnees.is_empty() {
        return donnees;
    }
    let p = &a.piece;
    let mut t = vec![(p.largeur[0], p.profondeur[0], h)];
    if p.largeur[1] > p.largeur[0] || p.profondeur[1] > p.profondeur[0] {
        t.push((p.largeur[1], p.profondeur[1], h));
    }
    t
}

/// Montre un modèle dans une pièce neutre : trois vues par taille de pièce, et ce que les règles en disent.
pub fn montrer(fichier: &Path, args: &Args) -> Result<()> {
    let a = Amenagement::lire(fichier)?;
    let dossier_modeles = fichier.parent().unwrap_or(Path::new("."));
    let statut = Validations::charger(dossier_modeles).statut(&a);
    println!("modèle « {} » · rôle {} · {} · empreinte {}", a.nom, a.role, statut.nom(), a.empreinte());
    let sortie = PathBuf::from(args.get("--vues").ok_or_else(|| anyhow!("--vues dossier manque"))?);
    std::fs::create_dir_all(&sortie)?;
    let vfs = crate::monter(fichier, args)?;
    let (config, _) = hm_regles::Config::du_projet(&std::path::absolute(fichier)?)?;
    let renderer = Renderer::new(Gpu::new()?);
    let police = Police::systeme();
    let travail = std::env::temp_dir().join(format!("hm_amenagement_{}", std::process::id()));
    std::fs::create_dir_all(&travail)?;
    let mut cases = Vec::new();
    let mut tout_bon = true;
    for (l, p, h) in tailles(&a, args) {
        let vmf = match piece_neutre(&a, l, p, h) {
            Ok(v) => v,
            Err(e) => {
                println!("pièce {l:.0} × {p:.0} × {h:.0} : {e}");
                tout_bon = false;
                continue;
            }
        };
        let chemin = travail.join("piece.vmf");
        std::fs::write(&chemin, &vmf)?;
        // la pièce, déclarée au modèle sémantique : les règles la jugent comme une pièce de ce rôle
        std::fs::write(travail.join("piece.sem.json"), format!(r#"{{"version":1,"carte":"piece","pieces":[{{"id":"piece_neutre","role":"{}","emprise":[0,0,{l},{p}],"sol":0,"hauteur":{h}}}]}}"#, a.role))?;
        let scene = Scene::load(&chemin, &vfs, &SceneOptions::default())?;
        if !scene.report.missing_models.is_empty() {
            bail!("modèle(s) introuvable(s) : {}", scene.report.missing_models.join(", "));
        }
        let r = hm_regles::valider(&scene, &vfs, &config, &[hm_regles::Famille::Physique, hm_regles::Famille::Joueur, hm_regles::Famille::Semantique]);
        let alertes: Vec<&hm_regles::Constat> = r.constats.iter().filter(|c| c.excepte.is_none() && c.niveau != hm_regles::Niveau::Information).collect();
        let (poses, coffrages) = a.disposer(l, p, h).map(|(q, b)| (q.len(), b.len()))?;
        let poses = if coffrages > 0 { format!("{poses} pose(s), {coffrages} coffrage(s)") } else { format!("{poses} pose(s)") };
        println!("pièce {l:.0} × {p:.0} × {h:.0} : {poses} ; règles : {}", if alertes.is_empty() { "aucune alerte".to_string() } else { format!("{} alerte(s)", alertes.len()) });
        for c in &alertes {
            println!("  [{}] {} · {}", c.niveau.nom(), c.regle, c.message);
            tout_bon = false;
        }
        let gs = renderer.upload(&scene);
        let lumiere = Lighting::neutral(&scene);
        let rendre = |camera: Camera, clip: Option<(Vec3, Vec3)>| {
            let f = renderer.render(&gs, &View { camera, width: 1280, height: 720, show_tools: false, clip, light: lumiere });
            hm_tour::reduire2(&f.rgba, 1280, 720)
        };
        let verdict = if alertes.is_empty() { "règles : aucune alerte".to_string() } else { format!("règles : {} alerte(s), la première : {}", alertes.len(), alertes[0].regle) };
        let titre = |vue: &str| vec![format!("{} · {} · pièce {l:.0} × {p:.0} × {h:.0} · {vue}", a.nom, statut.nom()), format!("{poses} · {verdict}")];
        cases.push(Case { image: rendre(Camera::look_at(Vec3::new(l * 0.5, 10.0, 64.0), Vec3::new(l * 0.5, p, 36.0), 88.0), None), legende: titre("depuis la porte") });
        // seconde vue : du coin de la pièce d'où le regard porte le plus loin (un coin pris par un meuble est écarté)
        let monde = hm_regles::Monde::construire(&scene, &config);
        let coin = match hm_tour::constat::vue_interieure(&renderer, &gs, &scene, &monde, Vec3::ZERO, Vec3::new(l, p, h), 640, 360) {
            Some(v) => v.image,
            None => rendre(Camera::look_at(Vec3::new(20.0, p - 20.0, 70.0), Vec3::new(l * 0.75, p * 0.15, 30.0), 88.0), None),
        };
        cases.push(Case { image: coin, legende: titre("depuis un coin") });
        let demi = (p + 48.0) * 0.5;
        let dessus = Camera::top_down(Vec3::new(l * 0.5, p * 0.5 - 8.0, 0.0), demi.max((l + 48.0) * 0.5 * 9.0 / 16.0), h - 2.0, h + 8.0);
        cases.push(Case { image: rendre(dessus, Some((Vec3::new(-1e9, -1e9, -32.0), Vec3::new(1e9, 1e9, h - 2.0)))), legende: titre("de dessus, la porte en bas") });
    }
    let _ = std::fs::remove_dir_all(&travail);
    if cases.is_empty() {
        bail!("aucune pièce n'a pu être montrée");
    }
    let f = sortie.join(format!("amenagement_{}.jpg", a.nom));
    std::fs::write(&f, planche(&cases, 3, &format!("modèle d'aménagement « {} » · rôle {} · {} · empreinte {}", a.nom, a.role, statut.nom(), a.empreinte()), &police).to_jpeg(88)?)?;
    println!("planche : {}", f.display());
    if !tout_bon {
        std::process::exit(2);
    }
    Ok(())
}
