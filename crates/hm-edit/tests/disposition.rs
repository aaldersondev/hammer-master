//! Aligner, caler chaque objet, transformer, infos de la map : mesuré sur les boîtes des objets après la commande, et
//! une seule étape d'annulation qui rend le fichier d'avant.
use glam::DVec3;
use hm_doc::{boite_de, Commande, Ref, Session};
use hm_edit::disposition::{aligner, caler_chacun, infos, transformer, Transformation};

fn bloc(s: &mut Session, lo: [f64; 3], hi: [f64; 3]) -> Ref {
    let r = s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: "dev/dev_measuregeneric01".into(), classe: None }).unwrap();
    r.crees[0]
}

fn boite(s: &Session, r: Ref) -> (DVec3, DVec3) {
    boite_de(s.doc(), &[r]).unwrap()
}

/// Exécute en une étape, vérifie qu'annuler rend le fichier d'avant, puis rétablit.
fn faire(s: &mut Session, f: impl FnOnce(&hm_doc::Document) -> Commande) {
    let c = f(s.doc());
    let avant = s.doc().ecrire();
    let n = s.historique().faits().len();
    s.executer(c).unwrap();
    assert_eq!(s.historique().faits().len(), n + 1, "une seule étape");
    s.annuler().unwrap();
    assert!(s.doc().ecrire() == avant);
    s.retablir().unwrap();
}

#[test]
fn aligner_sur_le_plus_loin() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0, 0.0, 0.0], [64.0, 64.0, 64.0]);
    let b = bloc(&mut s, [-200.0, 100.0, 0.0], [-100.0, 164.0, 32.0]);
    let c = bloc(&mut s, [300.0, 300.0, 16.0], [364.0, 400.0, 80.0]);
    // à gauche (x le plus petit) : b ne bouge pas, a et c viennent à x = -200
    faire(&mut s, |d| aligner(d, &[a, b, c], 0, false, true).unwrap());
    for r in [a, b, c] {
        assert!((boite(&s, r).0.x + 200.0).abs() < 1e-9);
    }
    assert_eq!(boite(&s, c).0.y, 300.0, "seul l'axe choisi bouge");
    // en haut (z le plus grand) : c (80) ne bouge pas
    faire(&mut s, |d| aligner(d, &[a, b, c], 2, true, true).unwrap());
    for r in [a, b, c] {
        assert!((boite(&s, r).1.z - 80.0).abs() < 1e-9);
    }
    // déjà alignés : rien à faire
    assert!(aligner(s.doc(), &[a, b, c], 2, true, true).is_none());
}

#[test]
fn caler_chaque_objet_sur_la_grille() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [3.0, 5.0, 1.0], [67.0, 69.0, 65.0]);
    let b = bloc(&mut s, [130.0, -7.0, 0.0], [190.0, 50.0, 30.0]);
    faire(&mut s, |d| caler_chacun(d, &[a, b], 16.0, true).unwrap());
    assert_eq!(boite(&s, a).0, DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(boite(&s, b).0, DVec3::new(128.0, -0.0, 0.0));
    assert_eq!(boite(&s, b).1 - boite(&s, b).0, DVec3::new(60.0, 57.0, 30.0), "la taille ne change pas");
    assert!(caler_chacun(s.doc(), &[a, b], 16.0, true).is_none());
}

#[test]
fn transformer_par_valeurs() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0, 0.0, 0.0], [200.0, 100.0, 50.0]);
    // 90° autour de Z, sens direct, autour du centre (100, 50, 25) : la boîte devient 100 × 200
    faire(&mut s, |d| transformer(d, &[a], Transformation::Tourner([0.0, 0.0, 90.0]), true).unwrap().unwrap());
    let (lo, hi) = boite(&s, a);
    assert!((lo - DVec3::new(50.0, -50.0, 0.0)).length() < 1e-6 && (hi - DVec3::new(150.0, 150.0, 50.0)).length() < 1e-6, "{lo} {hi}");
    // deux axes à la fois : une seule étape
    faire(&mut s, |d| transformer(d, &[a], Transformation::Tourner([90.0, 0.0, 90.0]), true).unwrap().unwrap());
    // échelle autour du centre
    let (lo, hi) = boite(&s, a);
    let c = (lo + hi) * 0.5;
    faire(&mut s, |d| transformer(d, &[a], Transformation::Echelle([2.0, 1.0, 0.5]), true).unwrap().unwrap());
    let (lo2, hi2) = boite(&s, a);
    assert!(((lo2 + hi2) * 0.5 - c).length() < 1e-6);
    assert!(((hi2 - lo2) - (hi - lo) * DVec3::new(2.0, 1.0, 0.5)).length() < 1e-6);
    assert!(transformer(s.doc(), &[a], Transformation::Echelle([1.0, 0.0, 1.0]), true).is_err());
    faire(&mut s, |d| transformer(d, &[a], Transformation::Deplacer([16.0, 0.0, -8.0]), true).unwrap().unwrap());
    assert!((boite(&s, a).0 - lo2 - DVec3::new(16.0, 0.0, -8.0)).length() < 1e-6);
    assert!(transformer(s.doc(), &[a], Transformation::Tourner([0.0; 3]), true).unwrap().is_none());
}

#[test]
fn tourner_dans_le_sens_direct() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0, 0.0, 0.0], [200.0, 100.0, 50.0]);
    let b = bloc(&mut s, [300.0, 45.0, 0.0], [310.0, 55.0, 50.0]);
    // centre de la sélection (155, 50) ; b est à +150 en x : +90° autour de Z (vu d'en haut, à gauche) le met à +150 en y
    faire(&mut s, |d| transformer(d, &[a, b], Transformation::Tourner([0.0, 0.0, 90.0]), true).unwrap().unwrap());
    let (lo, hi) = boite(&s, b);
    assert!(((lo + hi) * 0.5 - DVec3::new(155.0, 200.0, 25.0)).length() < 1e-6, "{lo} {hi}");
}

#[test]
fn infos_de_la_map() {
    let mut s = Session::nouvelle();
    bloc(&mut s, [0.0, 0.0, 0.0], [64.0, 64.0, 64.0]);
    let b = bloc(&mut s, [100.0, 0.0, 0.0], [164.0, 64.0, 64.0]);
    s.executer(Commande::LierAEntite { objets: vec![b], classe: "func_detail".into() }).unwrap();
    s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [0.0, 0.0, 100.0], cles: vec![] }).unwrap();
    let i = infos(s.doc());
    assert_eq!((i.solides, i.faces, i.displacements, i.entites_ponctuelles, i.entites_a_brosses), (2, 12, 0, 1, 1));
    assert_eq!(i.materiaux, vec![("dev/dev_measuregeneric01".to_string(), 12)]);
}
