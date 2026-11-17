//! Le registre des défauts connus (`regles/registre.toml`) tient debout : chaque règle citée existe, chaque test
//! cité existe, chaque défaut « couvert par une règle » a son test, et chaque règle du catalogue est citée par au
//! moins un test.
use std::path::{Path, PathBuf};

use hm_assets::Vfs;
use hm_regles::{budgets, catalogue, valider, Config, Famille};
use hm_scene::{Scene, SceneOptions};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entree {
    id: String,
    #[allow(dead_code)]
    origine: String,
    texte: String,
    couverture: String,
    #[serde(default)]
    regle: String,
    #[serde(default)]
    test: String,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct Registre {
    entree: Vec<Entree>,
}

fn racine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registre() -> Registre {
    toml::from_str(&std::fs::read_to_string(racine().join("regles/registre.toml")).unwrap()).unwrap()
}

#[test]
fn le_registre_tient_debout() {
    let r = registre();
    let regles: Vec<&str> = catalogue().iter().map(|x| x.id).collect();
    let mut ids = Vec::new();
    for e in &r.entree {
        assert!(!ids.contains(&e.id), "{} en double", e.id);
        ids.push(e.id.clone());
        assert!(!e.texte.is_empty());
        assert!(["regle", "partielle", "phase4", "hors_portee", "conception", "aucune"].contains(&e.couverture.as_str()), "{} : couverture « {} »", e.id, e.couverture);
        match e.couverture.as_str() {
            "regle" | "partielle" => {
                assert!(regles.contains(&e.regle.as_str()), "{} : règle inconnue « {} »", e.id, e.regle);
                assert!(!e.test.is_empty(), "{} : couvert par une règle mais sans test", e.id);
            }
            "aucune" | "phase4" | "hors_portee" => assert!(!e.note.is_empty() && e.regle.is_empty(), "{} : dire quelle règle manque, ou pourquoi l'outil ne peut pas le voir", e.id),
            _ => assert!(e.regle.is_empty(), "{}", e.id),
        }
        // le test cité existe : fichier, et fonction si elle est nommée
        if !e.test.is_empty() {
            let (fichier, fonction) = e.test.split_once("::").unwrap_or((&e.test, ""));
            let texte = std::fs::read_to_string(racine().join(fichier)).unwrap_or_else(|_| panic!("{} : fichier de test {fichier} introuvable", e.id));
            assert!(fonction.is_empty() || texte.contains(&format!("fn {fonction}(")), "{} : pas de test « {fonction} » dans {fichier}", e.id);
        }
    }
    // les 18 signalements et les quatre défauts de référence y sont
    for i in 1..=18 {
        assert!(ids.contains(&format!("L{i:02}")), "L{i:02} manque");
    }
    for h in ["H1", "H2", "H3", "H4"] {
        assert!(r.entree.iter().any(|e| e.id == h && e.couverture == "regle"));
    }
}

#[test]
fn chaque_regle_du_catalogue_est_citee_par_un_test() {
    let mut textes = String::new();
    for d in ["crates/hm-regles/tests", "tests"] {
        for f in std::fs::read_dir(racine().join(d)).unwrap().flatten() {
            // ce fichier-ci nomme les règles sans test : il ne compte pas comme un test d'elles
            if f.path().extension().map_or(false, |x| x == "rs" || x == "py") && f.file_name() != "registre.rs" {
                textes += &std::fs::read_to_string(f.path()).unwrap_or_default();
            }
        }
    }
    let sans: Vec<&str> = catalogue().iter().map(|r| r.id).filter(|id| !textes.contains(&format!("\"{id}\""))).collect();
    // règles sans cas d'essai, dites ici une à une avec la raison ; la liste ne doit pas s'allonger en silence
    let admises = [
        "geo.displacement_invalide",   // demande un bloc dispinfo complet : à construire avec la phase 4
        "asset.materiau_introuvable",  // les cas d'essai n'ont pas de fichiers de jeu ; vu sur la map réelle (aucun)
        "asset.texture_introuvable",
        "asset.modele_introuvable",
        "asset.materiau_de_modele",
        "asset.modele_non_statique",
        "asset.skin",
        "asset.local",
        "phys.enfoui",                 // demande de vrais modèles : vu sur rp_megalopole (lampes dans un mur)
        "phys.imbrique",
        "phys.porte_bloquee",
        "phys.porte_genee",
        "phys.reseau_sans_terminus",
        "budget.depasse",
        "budget.lumieres_nommees",
        "joueur.sans_apparition",
    ];
    let oubliees: Vec<&&str> = sans.iter().filter(|id| !admises.contains(id)).collect();
    assert!(oubliees.is_empty(), "règles sans aucun test : {oubliees:?}");
    let en_trop: Vec<&&str> = admises.iter().filter(|id| !sans.contains(id)).collect();
    assert!(en_trop.is_empty(), "règles dites sans test mais qui en ont un : {en_trop:?}");
}

/// H5 : au-delà de 192 areaportals le jeu plante ; l'alerte part à 90 %.
#[test]
fn plus_de_192_areaportals() {
    let ligne = |valeur| budgets::Ligne { nom: "areaportals".into(), libelle: "areaportals".into(), valeur, exact: true, log: None, limite: None, source_limite: String::new() };
    let constats = |valeur| {
        let b = budgets::Budgets { lignes: vec![ligne(valeur)], ..Default::default() };
        let mut out = Vec::new();
        budgets::verifier(&b, &mut out);
        out.iter().map(|c| c.regle).collect::<Vec<_>>()
    };
    assert!(constats(134).is_empty());
    assert_eq!(constats(173), vec!["budget.serre"]);
    assert_eq!(constats(193), vec!["budget.depasse"]);
}

/// H8 : les épaves empilées du désert, relevées à la lecture du tour, sortent par la règle « flotte ».
#[test]
fn les_epaves_empilees_sortent() {
    let vmf = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Desktop/rp_megalopole/out/rp_megalopole_v1.vmf");
    if !vmf.is_file() {
        return;
    }
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(vmf.parent().unwrap().parent().unwrap().join("compile/gameinfo.txt")).unwrap();
    let scene = Scene::load(&vmf, &vfs, &SceneOptions::default()).unwrap();
    if scene.revision != "951d5486688f642f" {
        return;
    }
    let r = valider(&scene, &vfs, &Config::default(), &[Famille::Physique]);
    let epaves = r.constats.iter().filter(|c| (c.regle == "phys.flotte" || c.regle == "phys.decolle") && c.modele.contains("epave")).count();
    assert!(epaves >= 30, "{epaves} épaves signalées");
}
