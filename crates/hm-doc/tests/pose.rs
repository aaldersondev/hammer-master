//! Rayon sur les brosses et pose au sol : point et normale mesurés sur des formes connues ; la commande s'annule à
//! l'octet.
use glam::DVec3;
use hm_doc::{geo, Commande, Ref, Session};

fn bloc(s: &mut Session, lo: [f64; 3], hi: [f64; 3], mat: &str, classe: Option<&str>) -> Ref {
    s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: mat.into(), classe: classe.map(String::from) }).unwrap().crees[0]
}

fn proche(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-6
}

#[test]
fn rayon_sur_un_sol_et_ce_qu_il_traverse() {
    let mut s = Session::nouvelle();
    bloc(&mut s, [0.0, 0.0, -16.0], [512.0, 512.0, 0.0], "DEV/DEV_MEASUREGENERIC01", None);
    // dessus : un déclencheur, une brosse de ciel, puis une marche
    bloc(&mut s, [0.0, 0.0, 50.0], [512.0, 512.0, 60.0], "TOOLS/TOOLSTRIGGER", Some("trigger_multiple"));
    bloc(&mut s, [0.0, 0.0, 70.0], [512.0, 512.0, 80.0], "TOOLS/TOOLSSKYBOX", None);
    let marche = bloc(&mut s, [256.0, 0.0, 0.0], [512.0, 512.0, 32.0], "DEV/DEV_MEASUREGENERIC01", None);
    let doc = s.doc();
    let i = geo::rayon(doc, DVec3::new(100.0, 100.0, 200.0), -DVec3::Z, &|_| false).unwrap();
    assert!((i.t - 200.0).abs() < 1e-9 && proche(i.point, DVec3::new(100.0, 100.0, 0.0)) && proche(i.normale, DVec3::Z), "{i:?}");
    let i = geo::rayon(doc, DVec3::new(300.0, 100.0, 200.0), -DVec3::Z, &|_| false).unwrap();
    assert!(proche(i.point, DVec3::new(300.0, 100.0, 32.0)));
    // la marche ignorée : le sol dessous
    let id = match marche {
        Ref::Solide(i) => i,
        _ => unreachable!(),
    };
    let i = geo::rayon(doc, DVec3::new(300.0, 100.0, 200.0), -DVec3::Z, &|x| x == id).unwrap();
    assert!(proche(i.point, DVec3::new(300.0, 100.0, 0.0)));
    // parti de l'intérieur de la marche : elle n'est pas touchée
    let i = geo::rayon(doc, DVec3::new(300.0, 100.0, 16.0), -DVec3::Z, &|_| false).unwrap();
    assert!(proche(i.point, DVec3::new(300.0, 100.0, 0.0)));
    // de côté, sur la face de la marche
    let i = geo::rayon(doc, DVec3::new(0.0, 100.0, 10.0), DVec3::X, &|_| false).unwrap();
    assert!(proche(i.point, DVec3::new(256.0, 100.0, 10.0)) && proche(i.normale, -DVec3::X), "{i:?}");
    // vers le haut ou à côté : rien
    assert!(geo::rayon(doc, DVec3::new(100.0, 100.0, 200.0), DVec3::Z, &|_| false).is_none());
    assert!(geo::rayon(doc, DVec3::new(-100.0, 100.0, 200.0), -DVec3::Z, &|_| false).is_none());
}

#[test]
fn rampe_et_alignement() {
    let mut s = Session::nouvelle();
    let r = bloc(&mut s, [-256.0, -256.0, -16.0], [256.0, 256.0, 0.0], "DEV/DEV_MEASUREGENERIC01", None);
    // une rampe à 30° autour de X
    s.executer(Commande::Tourner { objets: vec![r], centre: [0.0; 3], axe: [1.0, 0.0, 0.0], degres: 30.0, verrou_texture: false }).unwrap();
    let i = geo::rayon(s.doc(), DVec3::new(0.0, 0.0, 300.0), -DVec3::Z, &|_| false).unwrap();
    let attendue = DVec3::new(0.0, -(30f64).to_radians().sin(), (30f64).to_radians().cos());
    assert!((i.normale - attendue).length() < 1e-6, "{:?}", i.normale);
    // posé aligné : son axe haut est la normale, le lacet gardé autour d'elle
    let (o, a) = geo::pose_sur(i.point, i.normale, 10.0, true, DVec3::new(0.0, 45.0, 0.0));
    assert!(proche(o, i.point + i.normale * 10.0));
    let m = hm_geom::angles_to_mat(a.x, a.y, a.z);
    assert!((m * DVec3::Z - i.normale).length() < 1e-4, "haut {:?} pour {a:?}", m * DVec3::Z);
    // posé droit : origine au-dessus du point, angles inchangés
    let (o, a) = geo::pose_sur(i.point, i.normale, 10.0, false, DVec3::new(0.0, 45.0, 0.0));
    assert!(proche(o, i.point + DVec3::Z * 10.0) && proche(a, DVec3::new(0.0, 45.0, 0.0)));
}

#[test]
fn commande_poser_au_sol() {
    let mut s = Session::nouvelle();
    bloc(&mut s, [-512.0, -512.0, -16.0], [512.0, 512.0, 0.0], "DEV/DEV_MEASUREGENERIC01", None);
    bloc(&mut s, [0.0, 0.0, 0.0], [128.0, 128.0, 48.0], "DEV/DEV_MEASUREGENERIC01", None);
    let cles = vec![("model".to_string(), "models/props_c17/oildrum001.mdl".to_string()), ("angles".to_string(), "0 90 0".to_string())];
    let a = s.executer(Commande::CreerEntite { classe: "prop_static".into(), origine: [-200.0, 0.0, 300.0], cles: cles.clone() }).unwrap().crees[0];
    let b = s.executer(Commande::CreerEntite { classe: "prop_static".into(), origine: [64.0, 64.0, 300.0], cles }).unwrap().crees[0];
    let vide = s.executer(Commande::CreerEntite { classe: "info_target".into(), origine: [5000.0, 0.0, 300.0], cles: vec![] }).unwrap().crees[0];
    let (ia, ib, iv) = match (a, b, vide) {
        (Ref::Entite(x), Ref::Entite(y), Ref::Entite(z)) => (x, y, z),
        _ => unreachable!(),
    };
    let avant = s.doc().ecrire();
    let r = s.executer(Commande::PoserAuSol { entites: vec![ia, ib, iv], bas: vec![2.0, 2.0, 0.0], aligner: false, plafond: false }).unwrap();
    assert_eq!(r.message, "2 posée(s) sur 3");
    assert!(proche(s.doc().entite(ia).unwrap().origine().unwrap(), DVec3::new(-200.0, 0.0, 2.0)));
    assert!(proche(s.doc().entite(ib).unwrap().origine().unwrap(), DVec3::new(64.0, 64.0, 50.0)));
    assert!(proche(s.doc().entite(iv).unwrap().origine().unwrap(), DVec3::new(5000.0, 0.0, 300.0)));
    assert_eq!(s.doc().entite(ia).unwrap().get("angles"), Some("0 90 0"));
    // déjà posé : rien ne bouge (le rayon part d'un peu au-dessus de son bas)
    let (apres, n) = (s.doc().ecrire(), s.historique().faits().len());
    s.executer(Commande::PoserAuSol { entites: vec![ib], bas: vec![2.0], aligner: false, plafond: false }).unwrap();
    assert!(s.doc().ecrire() == apres, "reposer une entité posée ne change rien");
    assert_eq!(s.historique().faits().len(), n, "et ne fait pas d'étape");
    s.annuler();
    assert!(s.doc().ecrire() == avant);
    s.retablir();
    assert!(s.doc().ecrire() == apres);
    // aligné sur une surface plate : droit, lacet gardé
    s.executer(Commande::PoserAuSol { entites: vec![ia], bas: vec![2.0], aligner: true, plafond: false }).unwrap();
    assert_eq!(s.doc().entite(ia).unwrap().get("angles"), Some("0 90 0"));
    assert!(s.executer(Commande::PoserAuSol { entites: vec![ia], bas: vec![], aligner: false, plafond: false }).is_err());
}

#[test]
fn commande_poser_au_plafond() {
    let mut s = Session::nouvelle();
    bloc(&mut s, [-512.0, -512.0, 256.0], [512.0, 512.0, 272.0], "DEV/DEV_MEASUREGENERIC01", None);
    let a = s.executer(Commande::CreerEntite { classe: "info_target".into(), origine: [0.0, 0.0, 100.0], cles: vec![] }).unwrap().crees[0];
    let Ref::Entite(ia) = a else { unreachable!() };
    // le haut de l'entité 8 au-dessus de son origine : elle monte jusqu'à toucher le plafond (z = 256)
    s.executer(Commande::PoserAuSol { entites: vec![ia], bas: vec![8.0], aligner: false, plafond: true }).unwrap();
    assert!(proche(s.doc().entite(ia).unwrap().origine().unwrap(), DVec3::new(0.0, 0.0, 248.0)));
    assert!(s.historique().faits().last().unwrap().nom.contains("plafond"));
}
