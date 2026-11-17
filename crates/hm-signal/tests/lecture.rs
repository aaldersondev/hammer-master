//! Lecture des deux formats de signalement, sans perte.
use std::path::Path;

use hm_signal::{jugement_revision, Etats, Signalement};

fn fixture(nom: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(nom)).unwrap()
}

/// Les 18 signalements faits en jeu les 11 et 12/11/2026 sur rp_megalopole (ancien format).
#[test]
fn les_18_signalements_de_megalopole_sont_lus_sans_perte() {
    let texte = fixture("megalopole_defauts_1112.txt");
    let lignes: Vec<&str> = texte.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lignes.len(), 18);
    for (i, ligne) in lignes.iter().enumerate() {
        let s = Signalement::de_json(ligne, Path::new("absent")).unwrap();
        let v: serde_json::Value = serde_json::from_str(ligne).unwrap();
        assert_eq!(s.n as usize, i + 1);
        assert_eq!(s.id, format!("L{:02}", i + 1));
        assert_eq!(s.format, 1);
        // chaque champ du fichier se retrouve tel quel
        assert_eq!(s.texte, v["texte"].as_str().unwrap());
        assert_eq!(s.quand, v["quand"].as_str().unwrap());
        assert_eq!(s.carte, "rp_megalopole_v1");
        assert_eq!(s.texture, v["texture"].as_str().unwrap());
        assert_eq!(s.monde, v["monde"].as_bool().unwrap());
        for (lu, cle) in [(s.pos, "pos"), (s.oeil, "oeil"), (s.vise, "vise"), (s.normale, "normale")] {
            for k in 0..3 {
                assert_eq!(lu[k] as f64, v[cle][k].as_f64().unwrap() as f32 as f64, "{cle} du n° {}", i + 1);
            }
        }
        assert_eq!(s.angles[0] as f64, v["angles"][0].as_f64().unwrap() as f32 as f64);
        assert_eq!(s.angles[1] as f64, v["angles"][1].as_f64().unwrap() as f32 as f64);
        assert_eq!(s.angles[2], 0.0, "l'ancien format n'a pas de roulis");
        assert_eq!(s.objet.is_some(), v.get("objet").is_some());
        if let Some(o) = &s.objet {
            assert_eq!(o.classe, v["objet"]["classe"].as_str().unwrap());
            assert_eq!(o.modele, v["objet"]["modele"].as_str().unwrap_or(""));
        }
        // ce que l'ancien format ne disait pas reste vide, sans valeur inventée
        assert!(s.vue.is_none() && s.revision.is_empty() && s.bsp.is_none() && s.joueur.is_empty());
        assert!(s.capture.is_none(), "dossier absent : pas de capture");
        assert!(jugement_revision(&s, "951d5486688f642f").starts_with("révision inconnue"));
    }
}

/// L'échantillon produit par l'addon lui-même, tourné hors du jeu (tests/addon_hors_jeu.py).
#[test]
fn le_format_2_de_l_addon_est_lu() {
    let texte = fixture("echantillon_format2.txt");
    let s: Vec<Signalement> = texte.lines().map(|l| Signalement::de_json(l, Path::new("absent")).unwrap()).collect();
    assert_eq!(s.len(), 2);
    let a = &s[0];
    assert_eq!((a.id.as_str(), a.format, a.n), ("001", 2, 1));
    assert_eq!(a.texte, "la chaise traverse le mur");
    assert_eq!(a.revision, "951d5486688f642f");
    let vue = a.vue.as_ref().unwrap();
    assert_eq!((vue.fov, vue.fov_reglage, vue.ecran), (Some(106.26), Some(90.0), [1920, 1080]));
    assert_eq!(a.objet.as_ref().unwrap().modele, "models/props_c17/chair02a.mdl");
    assert_eq!(a.joueur.get("noclip"), Some(&false));
    assert_eq!(a.joueur.len(), 5);
    assert!(a.bsp.as_ref().unwrap().taille > 0);
    assert!(jugement_revision(a, "951d5486688f642f").starts_with("fait sur la révision ouverte"));
    assert!(jugement_revision(a, "0000000000000000").starts_with("ATTENTION"));
    assert_eq!(s[1].texte, "texture étirée ici");
    let (_, _, _, suppose) = a.vue_a_rendre(90.0);
    assert!(!suppose);
}

/// Les deux lignes écrites par le vrai Garry's Mod à l'essai du 13/11/2026 (rp_megalopole estampillée, 1920 × 1080,
/// `fov_desired 100`) : la première à pied, la seconde en noclip.
#[test]
fn les_lignes_du_vrai_jeu_sont_lues() {
    let texte = fixture("essai_en_jeu_1311.txt");
    let s: Vec<Signalement> = texte.lines().map(|l| Signalement::de_json(l, Path::new("absent")).unwrap()).collect();
    assert_eq!(s.len(), 2);
    for (i, a) in s.iter().enumerate() {
        assert_eq!((a.id.clone(), a.format, a.n as usize), (format!("{:03}", i + 1), 2, i + 1));
        assert_eq!((a.texte.as_str(), a.carte.as_str(), a.revision.as_str()), ("essai phase 2", "rp_megalopole_v1", "951d5486688f642f"));
        assert!(a.monde && a.objet.is_none() && a.texture == "megalopole/kj_gravier");
        assert_eq!(a.bsp.as_ref().unwrap().taille, 970_075_956);
        assert_eq!(a.joueur.len(), 5);
        assert!(jugement_revision(a, "951d5486688f642f").starts_with("fait sur la révision ouverte"));
        let vue = a.vue.as_ref().unwrap();
        assert_eq!((vue.fov, vue.fov_reglage, vue.ecran), (Some(115.63), Some(100.0), [1920, 1080]));
        // l'œil est 64 au-dessus des pieds, et la vue dessinée part bien de l'œil
        assert_eq!(a.oeil[2] - a.pos[2], 64.0);
        assert_eq!(vue.origine, a.oeil);
        // Le champ horizontal donné par le jeu est le champ réglé (base 4/3) porté au format de l'écran : c'est
        // l'hypothèse faite pour les signalements de l'ancien format, ici vérifiée sur le vrai jeu.
        let (_, _, fov_y, suppose) = a.vue_a_rendre(90.0);
        assert!(!suppose);
        assert!((fov_y - hm_render::fov_y_from_game(100.0)).abs() < 0.02, "{fov_y}");
    }
    assert_eq!((s[0].joueur.get("noclip"), s[0].joueur.get("au_sol")), (Some(&false), Some(&true)));
    assert_eq!((s[1].joueur.get("noclip"), s[1].joueur.get("au_sol")), (Some(&true), Some(&false)));
}

#[test]
fn une_ligne_incomplete_est_refusee_avec_le_champ_en_cause() {
    let e = Signalement::de_json(r#"{"n":1,"texte":"x","pos":[0,0,0],"angles":[0,0],"vise":[0,0,0]}"#, Path::new(".")).unwrap_err();
    assert!(e.to_string().contains("oeil"), "{e}");
}

#[test]
fn les_etats_se_gardent_pres_du_vmf() {
    let d = std::env::temp_dir().join(format!("hm_signal_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let texte = fixture("megalopole_defauts_1112.txt");
    let s = Signalement::de_json(texte.lines().next().unwrap(), Path::new("absent")).unwrap();
    let mut e = Etats::charger(&d);
    assert_eq!(e.de(&s).etat, "ouvert");
    assert!(e.fixer(&s, "fini", "", "").is_err());
    e.fixer(&s, "corrige", "portes posées le 12/11", "2026-11-13 12:00:00").unwrap();
    e.enregistrer(&d).unwrap();
    let relu = Etats::charger(&d);
    assert_eq!(relu.de(&s).etat, "corrige");
    assert_eq!(relu.de(&s).note, "portes posées le 12/11");
    let _ = std::fs::remove_dir_all(&d);
}
