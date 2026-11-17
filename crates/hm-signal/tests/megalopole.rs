//! Vue comparée sur la vraie map : pour dix des 18 signalements des 11 et 12/11/2026, l'objet que l'outil trouve
//! sous le centre de la vue est celui que la phrase décrit. Contrôlé à la main une fois sur les images
//! (`demo/phase2/planche_signalements_*.jpg`), le 13/11/2026, puis figé ici pour la révision 951d5486688f642f.
use std::path::Path;

use hm_assets::Vfs;
use hm_render::planche::Police;
use hm_render::{Gpu, Renderer};
use hm_scene::{Scene, SceneOptions};
use hm_signal::{comparer, Signalement};

const REVISION: &str = "951d5486688f642f";

/// (signalement, ce que la phrase désigne, classe attendue, morceau du modèle ou du matériau attendu)
const ATTENDUS: [(&str, &str, &str, &str); 10] = [
    ("L01", "il faut créer des portes de bureaux", "prop_door_rotating", "kit/porte_b"),
    ("L03", "H7 à retravaillé", "prop_static", "kit/h7_"),
    ("L04", "terrain de basket à refaire", "func_detail", "megalopole/court"),
    ("L05", "remplacer ce batiment par une grosse discothèque", "worldspawn", "megalopole/club_facade"),
    ("L06", "panneau qui dépasse", "func_detail", "megalopole/sign_cybercafe"),
    ("L07", "vitre devant l'ascenseur", "prop_dynamic", "kit/asc_vantail_verre"),
    ("L09", "toilette et baignoire trop proche", "prop_static", "kit/baignoire"),
    ("L10", "bug d'ombres au niveau des HLM", "worldspawn", "megalopole/fac_corpo_b"),
    ("L15", "il faudrait faire un vrai bar avec des bouteilles", "prop_static", "megalopole/fusion/"),
    ("L18", "trou aux cadres des portes de bureaux", "prop_door_rotating", "kit/porte_b"),
];

#[test]
fn l_objet_vise_est_celui_que_la_phrase_decrit() {
    let profil = std::env::var("USERPROFILE").unwrap_or_default();
    let vmf = format!("{profil}/Desktop/rp_megalopole/out/rp_megalopole_v1.vmf");
    if !Path::new(&vmf).is_file() {
        println!("rp_megalopole absent : contrôle sauté");
        return;
    }
    let Ok(gpu) = Gpu::new() else {
        println!("pas d'adaptateur graphique");
        return;
    };
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(format!("{profil}/Desktop/rp_megalopole/compile/gameinfo.txt")).unwrap();
    let scene = Scene::load(&vmf, &vfs, &SceneOptions::default()).unwrap();
    if scene.revision != REVISION {
        println!("la map a changé (révision {}) : les objets figés valaient pour {REVISION}, contrôle sauté", scene.revision);
        return;
    }
    let renderer = Renderer::new(gpu);
    let gs = renderer.upload(&scene);
    let police = Police::systeme();
    let texte = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/megalopole_defauts_1112.txt")).unwrap();
    let liste: Vec<Signalement> = texte.lines().map(|l| Signalement::de_json(l, Path::new("absent")).unwrap()).collect();
    for (id, phrase, classe, morceau) in ATTENDUS {
        let s = liste.iter().find(|s| s.id == id).unwrap();
        assert!(s.texte.starts_with(phrase), "{id} : « {} »", s.texte);
        // champ de vision du jeu au moment des signalements : fov_desired 100 (cfg/config.cfg)
        let c = comparer(&renderer, &gs, &scene, s, &police, &[], 100.0);
        let o = &scene.objects[c.objet as usize];
        assert_eq!(o.class, classe, "{id} « {phrase} » : trouvé {} {} {}", o.class, o.model, o.material);
        assert!(o.model.contains(morceau) || o.material.contains(morceau), "{id} « {phrase} » : trouvé {} {} {}", o.class, o.model, o.material);
        assert_eq!((c.image.width, c.image.height), (1924, 570)); // deux vues de 960 × 540 et le bandeau de 30
    }

    // La pièce où se trouve ce qu'on vise. Un point visé est sur une surface : il ne doit ni tomber à l'étage du
    // dessous, ni dans le désert sous la ville (les deux erreurs vues le 13/11 avant correction).
    let champ = hm_espace::Field::build(&scene);
    let lieu = |id: &str| {
        let s = liste.iter().find(|s| s.id == id).unwrap();
        (champ.decrire_lieu(glam::Vec3::from(s.pos) + glam::Vec3::Z * 8.0), champ.decrire_lieu_vise(s.vise.into(), s.oeil.into()))
    };
    // L07, « vitre devant l'ascenseur » : la gaine de l'ascenseur (80 × 96, haute de plus de 800)
    let (_, vise) = lieu("L07");
    assert!(vise.starts_with("pièce n°") && vise.contains("(80 × 96"), "{vise}");
    // L09, salle de bain : le joueur et la baignoire visée sont dans la même petite pièce
    let (joueur, vise) = lieu("L09");
    assert!(joueur.starts_with("pièce n°") && joueur == vise, "{joueur} / {vise}");
    // L18, porte de bureau à l'étage 6400 : même étage, pas le désert
    let (joueur, vise) = lieu("L18");
    assert!(joueur.contains("sol à 6400") && vise.contains("sol à 6400"), "{joueur} / {vise}");
    // L05, vu de la rue : un espace ouvert, pas une pièce
    assert!(lieu("L05").0.starts_with("espace ouvert"));

    // Les deux signalements du vrai jeu (essai du 13/11/2026, place du spawn, vers le jardin en contrebas) : le rayon
    // de l'outil touche le même gravier, à la même distance que la trace du jeu, à 2 unités près.
    let texte = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/essai_en_jeu_1311.txt")).unwrap();
    for ligne in texte.lines() {
        let s = Signalement::de_json(ligne, Path::new("absent")).unwrap();
        let c = comparer(&renderer, &gs, &scene, &s, &police, &[], 90.0);
        let o = &scene.objects[c.objet as usize];
        assert_eq!((o.class.as_str(), o.material.as_str()), ("worldspawn", "megalopole/kj_gravier"), "{}", s.id);
        assert!(!c.fov_suppose);
        let d = c.distance_outil.unwrap();
        assert!((d - c.distance_jeu).abs() < 2.0, "{} : {d} dans l'outil, {} en jeu", s.id, c.distance_jeu);
        // la place est en haut, le jardin visé 256 plus bas : le sol dit est celui de l'endroit, pas une moyenne
        let joueur = champ.decrire_lieu(glam::Vec3::from(s.pos).with_z(16.0));
        let vise = champ.decrire_lieu_vise(s.vise.into(), s.oeil.into());
        assert!(joueur.starts_with("espace ouvert") && joueur.contains("sol à 8 "), "{joueur}");
        assert!(vise.starts_with("espace ouvert") && vise.contains("sol à -248 "), "{vise}");
    }
}
