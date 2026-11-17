//! Vérifier la map (Alt+P, « Check for Problems » de Hammer) : les erreurs que liste le wiki Valve (« Hammer Check For
//! Problems Dialog » : solide invalide, entité à brosses vide, clé que la classe n'utilise pas, aucun
//! info_player_start), un hint mal fait, et les règles de `hm-regles` passées sur la map ouverte. Chaque problème
//! désigne son objet et, quand Hammer sait le corriger, la commande du noyau qui le corrige (annulable).
use std::path::Path;

use glam::DVec3;
use hm_doc::{Commande, Document, Ref, RefFace};
use hm_fgd::Fgd;

#[derive(Clone, Debug)]
pub struct Probleme {
    /// Le type de problème (titre commun à ceux du même type).
    pub genre: String,
    /// 0 information, 1 avertissement, 2 erreur.
    pub grave: u8,
    pub message: String,
    pub objet: Option<Ref>,
    pub boite: Option<(DVec3, DVec3)>,
    pub correction: Option<Commande>,
}

/// Le matériau existant le plus proche d'un matériau introuvable : le même nom de fichier dans un autre dossier,
/// sinon un nom de fichier qui commence par sa racine (sans son numéro final, `carpet_30` → `carpet`), puis le plus
/// proche par le chemin entier. `None` sans candidat : rien n'est inventé.
pub fn materiau_proche(manquant: &str, existants: &[String]) -> Option<String> {
    let manquant = manquant.to_ascii_lowercase();
    let fichier = |s: &str| s.rsplit('/').next().unwrap_or(s).to_string();
    let nom = fichier(&manquant);
    let racine = nom.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_' || c == '-').to_string();
    let rang = |e: &str| -> Option<u8> {
        let f = fichier(e);
        if f == nom {
            Some(0)
        } else if racine.len() >= 3 && f.starts_with(&racine) {
            Some(1)
        } else {
            None
        }
    };
    existants.iter().filter(|e| **e != manquant).filter_map(|e| rang(e).map(|r| (r, distance(&manquant, e), e))).min().map(|(_, _, e)| e.clone())
}

/// Distance d'édition (Levenshtein) entre deux noms.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut ligne: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut coin = ligne[0];
        ligne[0] = i + 1;
        for j in 0..b.len() {
            let haut = ligne[j + 1];
            ligne[j + 1] = (coin + (ca != b[j]) as usize).min(haut + 1).min(ligne[j] + 1);
            coin = haut;
        }
    }
    ligne[b.len()]
}

/// Les matériaux des fichiers du jeu (`chemin/sans/extension`), lus une fois par ensemble de points de montage.
fn materiaux_du_jeu(vfs: &hm_assets::Vfs) -> std::sync::Arc<Vec<String>> {
    static CACHE: std::sync::Mutex<Option<(String, std::sync::Arc<Vec<String>>)>> = std::sync::Mutex::new(None);
    let cle: String = vfs.mounts().iter().map(|m| m.label.as_str()).collect::<Vec<_>>().join("|");
    let mut c = CACHE.lock().unwrap();
    if let Some((_, l)) = c.as_ref().filter(|(k, _)| *k == cle) {
        return l.clone();
    }
    let l: std::sync::Arc<Vec<String>> = std::sync::Arc::new(vfs.list_dans("materials", ".vmt").into_iter().map(|(n, _)| n.trim_start_matches("materials/").trim_end_matches(".vmt").to_string()).collect());
    *c = Some((cle, l.clone()));
    l
}

fn p(genre: &str, grave: u8, message: String, objet: Option<Ref>, correction: Option<Commande>) -> Probleme {
    Probleme { genre: genre.into(), grave, message, objet, boite: None, correction }
}

fn nom(e: &hm_doc::Entite) -> String {
    match e.get("targetname").filter(|n| !n.is_empty()) {
        Some(n) => format!("{} « {n} »", e.classe()),
        None => format!("{} n° {}", e.classe(), e.id),
    }
}

/// Les contrôles de Hammer et le hint mal fait.
pub fn controles(doc: &Document, fgd: &Fgd) -> Vec<Probleme> {
    let mut v = Vec::new();
    for id in doc.solides_invalides() {
        v.push(p(hm_langue::t("Solide invalide"), 2, hm_langue::tr!("le solide {id} n'est pas convexe ou n'a pas de volume", "solid {id} is not convex or has no volume"), Some(Ref::Solide(id)), None));
    }
    for e in doc.entites() {
        let classe = fgd.classe(e.classe());
        if let Some(c) = &classe {
            // entité à brosses sans solide : Hammer la supprime
            if c.genre == hm_fgd::Genre::Solide && e.solides.is_empty() {
                v.push(p(hm_langue::t("Entité à brosses vide"), 2, hm_langue::tr!("{} n'a aucun solide", "{} has no solid", nom(e)), Some(Ref::Entite(e.id)), Some(Commande::Supprimer { objets: vec![Ref::Entite(e.id)] })));
            }
            // clés que la classe n'utilise pas : Hammer les retire
            for (k, _) in &e.cles {
                let kb = k.to_ascii_lowercase();
                if matches!(kb.as_str(), "classname" | "origin" | "id" | "hammerid") || c.propriete(k).is_some() {
                    continue;
                }
                // sans effet en jeu : une information, comme dans la liste de Hammer, pas une boîte dans les vues
                v.push(p(hm_langue::t("Clé inutilisée"), 0, hm_langue::tr!("{} : clé « {k} » inconnue de sa classe", "{}: key \"{k}\" unknown to its class", nom(e)), Some(Ref::Entite(e.id)), Some(Commande::Cles { entite: e.id, cles: vec![(k.clone(), None)] })));
            }
        }
    }
    // un point d'apparition, dans le cordon s'il est actif
    let cordon = doc.cordons().boites_actives();
    let depart = doc.entites().any(|e| {
        e.classe().eq_ignore_ascii_case("info_player_start") && !e.cache && e.origine().map_or(false, |o| cordon.is_empty() || hm_doc::visgroups::touche(&cordon, o, o))
    });
    if !depart {
        v.push(p(hm_langue::t("Aucun point d'apparition"), 2, hm_langue::tr!("aucun info_player_start{}", "no info_player_start{}", if cordon.is_empty() { "" } else { hm_langue::t(" dans le cordon") }), None, None));
    }
    // hint : seules ses faces hint coupent ; les autres doivent être en skip (wiki Valve, « Hint brush »)
    for (_, s) in doc.tous_les_solides() {
        let m = |f: &hm_doc::Face| f.materiau.to_ascii_lowercase().replace('\\', "/");
        if !s.faces.iter().any(|f| m(f) == "tools/toolshint") {
            continue;
        }
        let autres: Vec<RefFace> = s.faces.iter().filter(|f| !matches!(m(f).as_str(), "tools/toolshint" | "tools/toolsskip")).map(|f| RefFace { solide: s.id, face: f.id }).collect();
        if !autres.is_empty() {
            v.push(p(hm_langue::t("Hint mal fait"), 1, hm_langue::tr!("le hint {} a {} face(s) ni hint ni skip : elles coupent aussi la map", "hint {} has {} face(s) that are neither hint nor skip: they split the map too", s.id, autres.len()), Some(Ref::Solide(s.id)), Some(Commande::MateriauFaces { faces: autres, materiau: "TOOLS/TOOLSSKIP".into() })));
        }
    }
    v
}

/// Le calque de praticabilité : des bandes de sol où un joueur tient debout, (min, max, atteint depuis un point
/// d'apparition), cellules voisines de même état fusionnées le long des x.
#[derive(Clone, Debug, Default)]
pub struct Calque {
    pub bandes: Vec<([f32; 3], [f32; 3], bool)>,
}

/// Les bandes du calque tirées du joueur simulé de `hm-regles` (colonnes de `hm_espace::CELL`, intervalles libres).
pub fn calque_de(sim: &hm_regles::joueur::Simulation) -> Calque {
    let f = &sim.champ;
    let c = hm_espace::CELL;
    let mut bandes = Vec::new();
    for iy in 0..f.ny {
        // bandes ouvertes : (sol, atteint) → première colonne
        let mut ouvertes: std::collections::HashMap<(i16, bool), usize> = Default::default();
        for ix in 0..=f.nx {
            let ici: std::collections::HashSet<(i16, bool)> = if ix < f.nx {
                f.intervalles(iy * f.nx + ix).filter(|&j| f.free[j].floor > -16000 && f.free[j].ceil as i32 - f.free[j].floor as i32 >= 72).map(|j| (f.free[j].floor, sim.atteint[j])).collect()
            } else {
                Default::default()
            };
            ouvertes.retain(|k, debut| {
                if ici.contains(k) {
                    return true;
                }
                let z = k.0 as f32 + 1.0;
                bandes.push(([f.x0 + *debut as f32 * c, f.y0 + iy as f32 * c, z], [f.x0 + ix as f32 * c, f.y0 + (iy + 1) as f32 * c, z], k.1));
                false
            });
            for k in ici {
                ouvertes.entry(k).or_insert(ix);
            }
        }
    }
    Calque { bandes }
}

/// Les règles de `hm-regles` (géométrie, fichiers du jeu, physique, budgets) sur une copie de la map ouverte, et ses
/// budgets. `chemin` : le fichier de la map, pour la configuration du projet ; `log` : le journal de la dernière
/// compilation, d'où viennent les limites des budgets. `calque` : lance aussi le joueur simulé (ses constats et le
/// calque de praticabilité).
pub fn regles(doc: &Document, vfs: &hm_assets::Vfs, chemin: Option<&Path>, log: Option<&Path>, calque: bool) -> Result<(Vec<Probleme>, Option<hm_regles::budgets::Budgets>, Option<Calque>), String> {
    let dossier = std::env::temp_dir().join("HammerMaster");
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let copie = dossier.join(format!("verification_{}.vmf", std::process::id()));
    std::fs::write(&copie, doc.ecrire()).map_err(|e| e.to_string())?;
    let scene = hm_scene::Scene::load(&copie, vfs, &hm_scene::SceneOptions::default()).map_err(|e| format!("{e:#}"));
    let _ = std::fs::remove_file(&copie);
    let scene = scene?;
    let config = chemin.and_then(|c| hm_regles::Config::du_projet(c).ok()).map(|(c, _)| c).unwrap_or_default();
    let mut r = hm_regles::valider(&scene, vfs, &config, &[hm_regles::Famille::Geometrie, hm_regles::Famille::Assets, hm_regles::Famille::Physique, hm_regles::Famille::Budgets]);
    // la copie n'a pas de journal : limites prises dans celui de la dernière compilation, budgets revus avec elles
    if let (Some(b), Some(l)) = (&mut r.budgets, log) {
        hm_regles::budgets::avec_log(b, &l.with_extension("vmf"), "");
        r.constats.retain(|c| !c.regle.starts_with("budget."));
        hm_regles::budgets::verifier(b, &mut r.constats);
    }
    let budgets = r.budgets.take();
    let mut cal = None;
    if calque {
        let (_, constats, sim) = hm_regles::joueur::simuler(&scene, &config, None);
        r.constats.extend(constats);
        cal = sim.as_ref().map(calque_de);
    }
    let mut existants: Option<std::sync::Arc<Vec<String>>> = None;
    let problemes = r.constats
        .into_iter()
        .filter(|c| c.excepte.is_none())
        .map(|c| {
            // matériau introuvable : le remplacer par le plus proche des fichiers du jeu (« Corriger »)
            let mut message = c.message;
            let mut correction = None;
            if c.regle == "asset.materiau_introuvable" && !c.materiau.is_empty() {
                let l = existants.get_or_insert_with(|| materiaux_du_jeu(vfs));
                if let Some(n) = materiau_proche(&c.materiau, l) {
                    message = hm_langue::tr!("{message} ; remplacement proposé : {n}", "{message}; suggested replacement: {n}");
                    correction = Some(Commande::RemplacerMateriau { ancien: c.materiau.clone(), nouveau: n, objets: None });
                }
            }
            let id = c.vmf_id;
            let objet = if doc.entite(id).is_some() {
                Some(Ref::Entite(id))
            } else if doc.solide(id).is_some() {
                Some(Ref::Solide(id))
            } else {
                None
            };
            let titre = hm_regles::regle(c.regle).map_or(c.regle.to_string(), |r| hm_langue::t(r.titre).to_string());
            let grave = match c.niveau {
                hm_regles::Niveau::Erreur => 2,
                hm_regles::Niveau::Avertissement => 1,
                _ => 0,
            };
            let (lo, hi) = (DVec3::from(c.min.map(|x| x as f64)), DVec3::from(c.max.map(|x| x as f64)));
            Probleme { genre: titre, grave, message, objet, boite: (hi.cmpge(lo).all() && hi != lo).then_some((lo, hi)), correction }
        })
        .collect();
    Ok((problemes, budgets, cal))
}

/// Tout ce que vérifie Alt+P, dans l'ordre d'affichage (les plus graves d'abord).
pub fn tout(doc: &Document, fgd: &Fgd, vfs: &hm_assets::Vfs, chemin: Option<&Path>, log: Option<&Path>, calque: bool) -> (Vec<Probleme>, Option<hm_regles::budgets::Budgets>, Option<String>, Option<Calque>) {
    let mut v = controles(doc, fgd);
    let (b, erreur, cal) = match regles(doc, vfs, chemin, log, calque) {
        Ok((r, b, c)) => {
            v.extend(r);
            (b, None, c)
        }
        Err(e) => (None, Some(hm_langue::tr!("règles non passées : {e}", "rules not run: {e}")), None),
    };
    v.sort_by(|a, b| b.grave.cmp(&a.grave).then_with(|| a.genre.cmp(&b.genre)));
    (v, b, erreur, cal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hm_doc::Session;

    #[test]
    fn materiau_proche_d_un_introuvable() {
        let l: Vec<String> = ["agency/carpet/carpet_black01", "realworldtextures/newer/1/carpet_1_01", "brick/brickwall001a", "props/carpetfloor001a", "autre/dossier/ben_paintblue"].iter().map(|s| s.to_string()).collect();
        // même nom de fichier ailleurs : celui-là
        assert_eq!(materiau_proche("AJACKS/BEN_PAINTBLUE", &l).as_deref(), Some("autre/dossier/ben_paintblue"));
        // sinon la même racine (carpet), le chemin le plus proche ; « carpetfloor » commence aussi par « carpet »
        assert_eq!(materiau_proche("realworldtextures2/ground/carpet_30", &l).as_deref(), Some("realworldtextures/newer/1/carpet_1_01"));
        // rien qui ressemble : pas de proposition
        assert_eq!(materiau_proche("ajacks/zz_inconnu", &l), None);
    }

    #[test]
    fn calque_de_praticabilite() {
        // deux salles fermées côte à côte, un point d'apparition dans la première : son sol est atteint, celui de la
        // seconde non
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01";
        for x0 in [0.0, 1000.0] {
            for (lo, hi) in [([-16.0, -16.0, -16.0], [528.0, 528.0, 0.0]), ([-16.0, -16.0, 256.0], [528.0, 528.0, 272.0]), ([-16.0, -16.0, 0.0], [0.0, 528.0, 256.0]), ([512.0, -16.0, 0.0], [528.0, 528.0, 256.0]), ([0.0, -16.0, 0.0], [512.0, 0.0, 256.0]), ([0.0, 512.0, 0.0], [512.0, 528.0, 256.0])] {
                s.executer(Commande::CreerBloc { min: [lo[0] + x0, lo[1], lo[2]], max: [hi[0] + x0, hi[1], hi[2]], materiau: m.into(), classe: None }).unwrap();
            }
        }
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [256.0, 256.0, 1.0], cles: vec![] }).unwrap();
        let (_, _, cal) = regles(s.doc(), &hm_assets::Vfs::new(), None, None, true).unwrap();
        let cal = cal.expect("le calque");
        let aire = |atteint: bool, x0: f32| cal.bandes.iter().filter(|b| b.2 == atteint && b.0[0] >= x0 - 1.0 && b.1[0] <= x0 + 513.0 && b.0[2] < 10.0).map(|b| (b.1[0] - b.0[0]) * (b.1[1] - b.0[1])).sum::<f32>();
        let (a1, n1, a2, n2) = (aire(true, 0.0), aire(false, 0.0), aire(true, 1000.0), aire(false, 1000.0));
        // le sol de chaque salle (512 × 512, moins la marge du joueur aux murs) : tout atteint dans la première, rien dans la seconde
        assert!(a1 > 0.8 * 512.0 * 512.0 && n1 == 0.0, "première salle : atteint {a1}, non atteint {n1}");
        assert!(a2 == 0.0 && n2 > 0.8 * 512.0 * 512.0, "seconde salle : atteint {a2}, non atteint {n2}");
        // les cellules voisines sont fusionnées : bien moins de bandes que de cellules (32 × 32 par salle)
        assert!(cal.bandes.len() < 200, "{} bandes", cal.bandes.len());
    }

    #[test]
    fn controles_de_hammer() {
        let f = std::env::temp_dir().join(format!("hm_verif_{}.fgd", std::process::id()));
        std::fs::write(&f, "@PointClass = info_player_start : \"\" []\n@SolidClass = func_detail : \"\" []\n@PointClass = light : \"\" [ _light(color255) : \"\" : \"255 255 255 200\" ]\n").unwrap();
        let mut fgd = Fgd::default();
        fgd.charger(&f).unwrap();
        let _ = std::fs::remove_file(&f);
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        s.executer(Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: m.clone(), classe: None }).unwrap();
        s.executer(Commande::CreerEntite { classe: "light".into(), origine: [0.0, 0.0, 100.0], cles: vec![("_light".into(), "255 0 0 200".into()), ("brillance".into(), "3".into())] }).unwrap();
        s.executer(Commande::CreerEntite { classe: "func_detail".into(), origine: [0.0; 3], cles: vec![] }).unwrap();
        let hint = match s.executer(Commande::CreerBloc { min: [100.0, 0.0, 0.0], max: [101.0, 64.0, 64.0], materiau: "TOOLS/TOOLSHINT".into(), classe: None }).unwrap().crees[0] {
            Ref::Solide(i) => i,
            _ => unreachable!(),
        };
        // une face du hint en skip : il en reste 4 ni hint ni skip une fois deux passées en béton
        let faces: Vec<u64> = s.doc().solide(hint).unwrap().faces.iter().map(|f| f.id).collect();
        s.executer(Commande::MateriauFaces { faces: faces[..2].iter().map(|f| RefFace { solide: hint, face: *f }).collect(), materiau: m.clone() }).unwrap();
        let v = controles(s.doc(), &fgd);
        let genres: Vec<&str> = v.iter().map(|x| x.genre.as_str()).collect();
        assert!(genres.contains(&"Entité à brosses vide"), "{genres:?}");
        assert!(genres.contains(&"Aucun point d'apparition"));
        let cle = v.iter().find(|x| x.genre == "Clé inutilisée").expect("la clé inutilisée");
        assert!(cle.message.contains("brillance"));
        assert!(!v.iter().any(|x| x.message.contains("_light")), "une clé du FGD n'est pas inutilisée");
        let h = v.iter().find(|x| x.genre == "Hint mal fait").expect("le hint");
        assert!(h.message.contains("2 face(s)"), "{}", h.message);
        // les corrections, appliquées : il ne reste que le point d'apparition
        for x in &v {
            if let Some(c) = &x.correction {
                s.executer(c.clone()).unwrap();
            }
        }
        let v = controles(s.doc(), &fgd);
        assert_eq!(v.iter().map(|x| x.genre.as_str()).collect::<Vec<_>>(), ["Aucun point d'apparition"]);
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [32.0, 32.0, 65.0], cles: vec![] }).unwrap();
        assert!(controles(s.doc(), &fgd).is_empty());
    }
}
