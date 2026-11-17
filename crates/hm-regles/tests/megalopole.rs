//! Les quatre défauts de référence : signalés par des règles sur le VMF du 10/11/2026, absents de la map actuelle.
//! Et les mesures de la phase 3 sur la vraie map : comptages exacts égaux au log, accord avec le `.nav` du jeu.
//! Les contrôles sont sautés si les fichiers du projet ne sont pas sur ce PC.
use std::path::{Path, PathBuf};

use hm_assets::Vfs;
use hm_regles::{valider, Config, Famille, Rapport};
use hm_scene::{Scene, SceneOptions};

fn mega() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Desktop").join("rp_megalopole")
}

fn ouvrir(vmf: &Path, pak: Option<&Path>) -> Option<(Scene, Vfs)> {
    if !vmf.is_file() {
        println!("{} absent : contrôle sauté", vmf.display());
        return None;
    }
    let mut vfs = Vfs::new();
    if let Some(p) = pak.filter(|p| p.is_file()) {
        vfs.add_bsp_pak(p).unwrap();
    }
    vfs.add_gameinfo(mega().join("compile/gameinfo.txt")).unwrap();
    let scene = Scene::load(vmf, &vfs, &SceneOptions::default()).unwrap();
    Some((scene, vfs))
}

fn ids(r: &Rapport, regle: &str) -> Vec<u64> {
    let mut v: Vec<u64> = r.de(regle).iter().map(|c| c.vmf_id).collect();
    v.sort();
    v
}

/// Le VMF archivé du 10/11/2026 : auvent des stations qui flotte, rails qui traversent le mur est de la gare,
/// viaduc du M1 coupé net à trois bouts, 13 salles de WC sans cabines.
#[test]
fn les_quatre_defauts_du_10_11_sont_signales_par_des_regles() {
    let Some((scene, vfs)) = ouvrir(&mega().join("megastructures_10-11/avant/avant_megastructures.vmf"), Some(&mega().join("maps/rp_megalopole_v1_10-11_avant_megastructures.bsp.bak"))) else { return };
    if scene.revision != "9c654d33437db142" {
        println!("le VMF du 10/11 a changé : contrôle sauté");
        return;
    }
    let r = valider(&scene, &vfs, &Config::default(), &[Famille::Physique]);
    // 1. l'auvent : les trois dalles de 12 d'épaisseur des stations aériennes (solides 6492, 6517, 6544)
    let flotte = ids(&r, "phys.flotte");
    for dalle in [6492, 6517, 6544] {
        assert!(flotte.contains(&dalle), "la dalle d'auvent {dalle} n'est pas signalée ; signalés : {flotte:?}");
    }
    // 2. les rails : voie_ballast n° 1088 et 1097 à travers le mur est de la gare (solide 749)
    let traverse = r.de("phys.traverse");
    for rail in [1088u64, 1097] {
        let c = traverse.iter().find(|c| c.vmf_id == rail).unwrap_or_else(|| panic!("rail {rail} non signalé"));
        assert!(c.message.contains("n° 749") && c.mesure >= 64.0, "{}", c.message);
    }
    // 3. le viaduc : ses trois bouts libres (Bas-Fonds, Docks, Gare), et eux seuls
    assert_eq!(ids(&r, "phys.reseau_coupe"), vec![14445, 14540, 14590]);
    // 4. les WC : 13 salles
    assert_eq!(r.de("phys.wc_sans_cloison").len(), 13);
    assert!(r.de("phys.wc_sans_cloison").iter().all(|c| c.mesure >= 2.0));
}

/// Contre-épreuve : sur la map actuelle, aucune de ces quatre règles ne se déclenche sur ces objets.
#[test]
fn la_map_actuelle_n_a_plus_ces_quatre_defauts() {
    let vmf = mega().join("out/rp_megalopole_v1.vmf");
    let Some((scene, vfs)) = ouvrir(&vmf, None) else { return };
    if scene.revision != "951d5486688f642f" {
        println!("la map a changé (révision {}) : contrôle sauté", scene.revision);
        return;
    }
    let (config, fichier) = Config::du_projet(&vmf).unwrap();
    assert!(fichier.is_some(), "hammer-master.toml du projet introuvable");
    let r = valider(&scene, &vfs, &config, &[Famille::Physique, Famille::Budgets, Famille::Assets]);
    assert!(r.de("phys.wc_sans_cloison").is_empty());
    assert!(r.de("phys.reseau_coupe").is_empty());
    assert!(!r.de("phys.traverse").iter().any(|c| c.modele.contains("voie_")), "une voie traverse encore un mur");
    assert!(!r.constats.iter().any(|c| c.regle == "phys.flotte" && c.materiau.contains("shop_frame")), "un auvent flotte encore");
    // le drone est suspendu exprès : déclaré dans le projet, il ne sort pas
    assert!(!r.constats.iter().any(|c| c.modele.contains("drone")));
    // comptages exacts : ceux du dernier log de compilation de cette révision
    let b = r.budgets.as_ref().unwrap();
    for (nom, attendu) in [("brushes", 7783u64), ("models", 201), ("static prop count", 15801), ("cubemapsamples", 301), ("dispinfos", 320), ("areaportals", 134)] {
        let l = b.lignes.iter().find(|l| l.nom == nom).unwrap();
        assert_eq!(l.valeur, attendu, "{nom}");
        if let Some(log) = l.log {
            assert_eq!(l.valeur, log, "{nom} : compté {} mais {} au log", l.valeur, log);
        }
    }
    assert!(r.de("budget.serre").iter().any(|c| c.message.contains("props statiques")));
    // fichiers : rien d'introuvable, rien qui dépende d'un autre jeu ; les modèles de Counter-Strike viennent de
    // l'archive livrée avec Garry's Mod
    assert!(!r.constats.iter().any(|c| c.regle.starts_with("asset.")), "{:?}", r.constats.iter().filter(|c| c.regle.starts_with("asset.")).map(|c| &c.message).collect::<Vec<_>>());
    let cs = r.provenances.iter().find(|p| p.0.contains("content_cstrike")).expect("montage content_cstrike");
    assert_eq!((cs.1, cs.3, cs.4), (hm_regles::assets::Provenance::Jeu, 9, 202));
}

/// Le joueur simulé contre le maillage de navigation généré par le jeu : presque toutes les aires du `.nav` doivent
/// être accessibles.
#[test]
fn le_joueur_simule_s_accorde_avec_le_nav_du_jeu() {
    let vmf = mega().join("out/rp_megalopole_v1.vmf");
    let nav = mega().join("maps/rp_megalopole_v1.nav");
    let Some((scene, _)) = ouvrir(&vmf, None) else { return };
    let Ok(octets) = std::fs::read(&nav) else { return };
    if scene.revision != "951d5486688f642f" {
        return;
    }
    let aires = hm_regles::joueur::lire_nav(&octets).unwrap();
    assert!(aires.len() > 30_000, "{} aires", aires.len());
    let (config, _) = Config::du_projet(&vmf).unwrap();
    let (bilan, _, sim) = hm_regles::joueur::simuler(&scene, &config, None);
    assert_eq!(bilan.apparitions, 24);
    let (n, oui, _, _, _) = hm_regles::joueur::accord_nav(&sim.unwrap(), &aires);
    assert!(oui as f64 / n as f64 >= 0.98, "accord de {:.1} %", oui as f64 / n as f64 * 100.0);
}

/// Les 435 pièces que le générateur déclare sont toutes accessibles ; sans les liaisons d'ascenseur du modèle, les
/// étages des tours ne le sont plus (contre-épreuve : la règle voit bien quelque chose).
#[test]
fn les_pieces_declarees_de_megalopole_sont_accessibles() {
    let vmf = mega().join("out/rp_megalopole_v1.vmf");
    let Some((scene, _)) = ouvrir(&vmf, None) else { return };
    let Ok(Some(modele)) = hm_model::Modele::du_vmf(&vmf) else {
        println!("pas de modèle sémantique : contrôle sauté");
        return;
    };
    if scene.revision != "951d5486688f642f" {
        return;
    }
    assert_eq!(modele.pieces.len(), 435);
    let (config, _) = Config::du_projet(&vmf).unwrap();
    let (bilan, constats, _) = hm_regles::joueur::simuler(&scene, &config, Some(&modele));
    assert_eq!((bilan.pieces, bilan.pieces_inaccessibles, bilan.pieces_sans_sol), (435, 0, 0), "{:?}", constats.iter().filter(|c| c.regle.starts_with("sem.")).map(|c| &c.message).take(5).collect::<Vec<_>>());
    let mut sans = modele.clone();
    sans.liaisons.clear();
    let (bilan, constats, _) = hm_regles::joueur::simuler(&scene, &config, Some(&sans));
    assert!(bilan.pieces_inaccessibles >= 100, "{} pièces inaccessibles sans ascenseurs", bilan.pieces_inaccessibles);
    assert!(constats.iter().any(|c| c.regle == "sem.piece_inaccessible" && c.message.contains("tour_nacre")));
}

/// Trois signalements faits en jeu le 11/11/2026 au soir, retrouvés par les règles du lot B sur le VMF de ce soir-là
/// (`refonte_11-11/avant_zones.vmf`, jugé avec le modèle sémantique d'aujourd'hui) : L01 « il faut créer des portes de
/// bureaux », L06 « panneau qui dépasse », L09 « toilette et baignoire trop proches ». Sur la map actuelle, où ils
/// ont été corrigés le 12/11, aucun des trois ne sort.
#[test]
fn trois_signalements_du_11_11_retrouves_par_les_regles() {
    let ancien = mega().join("refonte_11-11/avant_zones.vmf");
    let actuel = mega().join("out/rp_megalopole_v1.vmf");
    if !ancien.is_file() || !hm_model::Modele::chemin(&actuel).is_file() {
        println!("fichiers du projet absents : contrôle sauté");
        return;
    }
    // le VMF du 11/11 et le modèle d'aujourd'hui, côte à côte dans un dossier à part
    let d = std::env::temp_dir().join(format!("hm_regles_1111_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(&ancien, d.join("soir.vmf")).unwrap();
    std::fs::copy(hm_model::Modele::chemin(&actuel), d.join("soir.sem.json")).unwrap();
    let pres = |r: &Rapport, regle: &str, p: [f32; 3], rayon: f32| -> Vec<String> { r.de(regle).iter().filter(|c| (0..3).map(|k| (c.pos[k] - p[k]).powi(2)).sum::<f32>().sqrt() < rayon).map(|c| c.message.clone()).collect() };
    let (l01, l06, l09) = ([-3152.0, 13974.0, 12191.0], [-10120.0, 1221.8, 284.4], [11677.0, 4484.0, 607.0]);
    {
        let (scene, vfs) = ouvrir(&d.join("soir.vmf"), None).unwrap();
        if scene.revision != "5b1ff5e2f3f1b1a4" {
            println!("révision du VMF du 11/11 : {}", scene.revision);
        }
        let r = valider(&scene, &vfs, &Config::default(), &[Famille::Semantique, Famille::Physique]);
        assert!(pres(&r, "sem.baie_sans_porte", l01, 700.0).iter().any(|m| m.contains("tour_nacre.office")), "L01 non retrouvé");
        assert!(pres(&r, "phys.porte_a_faux", l06, 400.0).iter().any(|m| m.contains("sign_cybercafe")), "L06 non retrouvé");
        assert!(pres(&r, "sem.degagement", l09, 300.0).iter().any(|m| m.contains("toilettes.mdl") && m.contains("baignoire.mdl")), "L09 non retrouvé");
    }
    let _ = std::fs::remove_dir_all(&d);
    let Some((scene, vfs)) = ouvrir(&actuel, None) else { return };
    if scene.revision != "951d5486688f642f" {
        return;
    }
    let (config, _) = Config::du_projet(&actuel).unwrap();
    let r = valider(&scene, &vfs, &config, &[Famille::Semantique, Famille::Physique]);
    assert!(pres(&r, "sem.baie_sans_porte", l01, 700.0).is_empty());
    assert!(pres(&r, "phys.porte_a_faux", l06, 400.0).is_empty());
    assert!(pres(&r, "sem.degagement", l09, 300.0).is_empty());
    assert!(r.de("sem.degagement").is_empty());
    // douze des treize pièces « wc » sont trop grandes pour leur rôle ; les cellules ont leur couchette et leur
    // cuvette (vocabulaire du projet : poste_cellule), et la pharmacie est exceptée avec sa raison ; les trois terminus
    // déclarés du M1 sont posés ; les bouts de voie au sol ne sont pas déclarés
    assert_eq!(r.de("sem.role_trop_grand").len(), 12);
    assert!(r.de("sem.role_trop_grand").iter().all(|c| c.message.contains("(wc,")));
    assert!(r.de("sem.role_incomplet").is_empty());
    assert_eq!(r.constats.iter().filter(|c| c.regle == "sem.role_incomplet" && c.excepte.is_some()).count(), 1);
    assert!(!r.de("sem.voie_sans_extremite").iter().any(|c| c.message.contains("terminus déclaré")));
    assert!(!r.de("sem.voie_sans_extremite").is_empty());
}

/// Le 14/11/2026, trois signalements faits en jeu parlaient de scintillement : les gradins du Jardin (003), les
/// portes de l'hôpital (005), « beaucoup » dans le H-7 (012). La règle `phys.zfighting_modele` en est née. La map
/// d'avant ces corrections est rejouée avec ses modèles d'alors, pris dans le BSP gardé ce jour-là : la règle y
/// retrouve la planche des gradins dans le plan de la joue de la rampe, et une boîte du H-7 dans le plan d'un pylône.
/// Sur la map d'aujourd'hui, ces deux-là n'y sont plus. Contrôle sauté si les fichiers gardés ont été supprimés.
#[test]
fn le_scintillement_signale_le_14_11_est_retrouve_par_la_regle() {
    let gardes = mega().join("hammer_master_essais/lot_f");
    let Some((scene, vfs)) = ouvrir(&gardes.join("avant_stations_python.vmf"), Some(&gardes.join("rp_megalopole_v1_avant_lot_f.bsp"))) else { return };
    if !gardes.join("rp_megalopole_v1_avant_lot_f.bsp").is_file() || scene.revision != "4bf2bd7daec356ec" {
        println!("map ou BSP d'avant le 14/11 absents ou changés : contrôle sauté");
        return;
    }
    let r = valider(&scene, &vfs, &Config::default(), &[Famille::Physique]);
    let z = r.de("phys.zfighting_modele");
    let gradins: Vec<_> = z.iter().filter(|c| c.modele.contains("gradins_1536_384_128") && c.message.contains("kj_bois") && c.message.contains("func_detail")).collect();
    assert!(!gradins.is_empty(), "003 : la planche des gradins contre la joue de la rampe n'est pas retrouvée");
    assert!(gradins.iter().all(|c| c.normale.is_some() && !c.autres_vmf.is_empty()), "le constat ne dit ni le plan ni l'autre objet");
    let boites: f32 = z.iter().filter(|c| c.modele.contains("h7_boites_a") && c.message.contains("k_beton") && c.message.contains("func_detail")).map(|c| c.mesure).sum();
    assert!(boites > 90_000.0, "012 : les boîtes du H-7 dans le plan des pylônes ne sont pas retrouvées ({boites} unités carrées)");
    // le dos d'un modèle contre son mur est dans le même plan qu'une face du pylône voisin, tourné comme elle vers
    // le plein : personne ne le voit, il n'est pas signalé (c'était 5 millions d'unités carrées de fausses alertes)
    let total: f32 = z.iter().filter(|c| c.modele.contains("/h7_")).map(|c| c.mesure).sum();
    assert!(total < 1_500_000.0, "des faces adossées à du plein sont signalées ({total} unités carrées au H-7)");
    // à l'intérieur d'un modèle : les terrasses du H-7 ont deux matériaux dans un même plan
    assert!(r.de("asset.modele_scintille").iter().any(|c| c.modele.contains("h7_terrasse_a")), "défaut interne des terrasses non retrouvé");

    let actuel = mega().join("out/rp_megalopole_v1.vmf");
    let Some((scene, vfs)) = ouvrir(&actuel, None) else { return };
    let r = valider(&scene, &vfs, &Config::default(), &[Famille::Physique]);
    let z = r.de("phys.zfighting_modele");
    assert!(!z.iter().any(|c| c.modele.contains("gradins_1536_384_128")), "les gradins scintillent encore");
    assert!(!z.iter().any(|c| c.modele.contains("h7_boites_a") && c.message.contains("k_beton") && c.message.contains("func_detail")), "les boîtes du H-7 sont encore dans le plan des pylônes");
}
