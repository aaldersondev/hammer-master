//! Opérations sur les brosses : découpe, creusement, fusion, primitives, évidement de tout solide. Chaque résultat
//! est mesuré (nombre de solides, volumes, validité) ; chaque commande s'annule et se rétablit à l'octet.
use glam::DVec3;
use hm_doc::brosses::{volume, Arche};
use hm_doc::{Commande, Forme, Garder, Ref, Session};

fn faire(s: &mut Session, c: Commande) -> hm_doc::Resultat {
    let avant = s.doc().ecrire();
    let r = s.executer(c.clone()).unwrap_or_else(|e| panic!("{c:?} : {e}"));
    let apres = s.doc().ecrire();
    assert_ne!(avant, apres, "{c:?} n'a rien changé");
    s.annuler().unwrap();
    assert!(s.doc().ecrire() == avant, "annuler {c:?} ne rend pas le fichier d'avant");
    s.retablir().unwrap();
    assert!(s.doc().ecrire() == apres, "rétablir {c:?} ne rend pas le fichier d'après");
    let relu = Session::depuis_octets(&apres, None).unwrap();
    assert!(relu.doc().ecrire() == apres, "{c:?} : le fichier écrit ne se relit pas à l'identique");
    r
}

fn bloc(s: &mut Session, lo: [f64; 3], hi: [f64; 3]) -> Ref {
    s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: None }).unwrap().crees[0]
}

/// Les solides du monde : (nombre, volume total), tous valides.
fn bilan(s: &Session) -> (usize, f64) {
    let v: Vec<f64> = s.doc().solides_monde().map(|x| {
        assert!(x.valide(), "solide {} invalide", x.id);
        volume(x)
    }).collect();
    (v.len(), v.iter().sum())
}

fn proche(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3 * b.abs().max(1.0)
}

#[test]
fn decouper_droit_et_en_biais() {
    let mut s = Session::nouvelle();
    let b = bloc(&mut s, [0.0; 3], [256.0, 128.0, 64.0]);
    // plan x = 64, normale +X (points en sens horaire vu de +X)
    let plan = [[64.0, 128.0, 0.0], [64.0, 0.0, 0.0], [64.0, 0.0, 64.0]];
    let r = faire(&mut s, Commande::Decouper { objets: vec![b], plan, garder: Garder::LesDeux, materiau: "TOOLS/TOOLSNODRAW".into() });
    assert_eq!(r.crees.len(), 2);
    let (n, v) = bilan(&s);
    assert_eq!(n, 2);
    assert!(proche(v, 256.0 * 128.0 * 64.0));
    let vols: Vec<f64> = s.doc().solides_monde().map(volume).collect();
    assert!(proche(vols[0], 64.0 * 128.0 * 64.0) && proche(vols[1], 192.0 * 128.0 * 64.0), "{vols:?}");
    // la face de coupe a le matériau demandé, les autres gardent le leur
    assert_eq!(s.doc().solides_monde().flat_map(|x| x.faces.iter()).filter(|f| f.materiau == "TOOLS/TOOLSNODRAW").count(), 2);
    s.annuler();
    // ne garder que devant (x > 64)
    faire(&mut s, Commande::Decouper { objets: vec![b], plan, garder: Garder::Devant, materiau: "x".into() });
    let (n, v) = bilan(&s);
    assert!(n == 1 && proche(v, 192.0 * 128.0 * 64.0));
    let (lo, _) = s.doc().solides_monde().next().unwrap().boite().unwrap();
    assert!((lo.x - 64.0).abs() < 1e-6);
    s.annuler();
    // en biais, sur la diagonale de dessus : deux prismes de moitié
    let diag = [[0.0, 0.0, 0.0], [256.0, 128.0, 0.0], [256.0, 128.0, 64.0]];
    faire(&mut s, Commande::Decouper { objets: vec![b], plan: diag, garder: Garder::LesDeux, materiau: "x".into() });
    let vols: Vec<f64> = s.doc().solides_monde().map(volume).collect();
    assert!(vols.len() == 2 && vols.iter().all(|v| proche(*v, 256.0 * 128.0 * 64.0 / 2.0)), "{vols:?}");
    assert!(s.doc().solides_monde().all(|x| x.faces.len() == 5));
    // un plan qui ne coupe rien : refusé sans trace
    let avant = s.doc().ecrire();
    assert!(s.executer(Commande::Decouper { objets: vec![b], plan: [[1000.0, 1.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 0.0, 1.0]], garder: Garder::LesDeux, materiau: "x".into() }).is_err());
    assert!(s.doc().ecrire() == avant);
}

#[test]
fn creuser_un_coin() {
    let mut s = Session::nouvelle();
    bloc(&mut s, [0.0; 3], [256.0, 256.0, 64.0]);
    let outil = bloc(&mut s, [192.0, 192.0, -16.0], [320.0, 320.0, 128.0]);
    faire(&mut s, Commande::Creuser { outils: vec![outil], cibles: None });
    // l'outil reste ; le mur perd le coin de 64 × 64 × 64
    let (n, v) = bilan(&s);
    let v_outil = 128.0 * 128.0 * 144.0;
    assert!(n >= 3, "{n} solides");
    assert!(proche(v - v_outil, 256.0 * 256.0 * 64.0 - 64.0 * 64.0 * 64.0), "{}", v - v_outil);
}

#[test]
fn fusionner() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0; 3], [128.0, 128.0, 64.0]);
    let b = bloc(&mut s, [128.0, 0.0, 0.0], [256.0, 128.0, 64.0]);
    let r = faire(&mut s, Commande::Fusionner { objets: vec![a, b] });
    assert!(r.message.is_empty(), "{}", r.message);
    let (n, v) = bilan(&s);
    assert!(n == 1 && proche(v, 256.0 * 128.0 * 64.0));
    assert_eq!(s.doc().solides_monde().next().unwrap().faces.len(), 6);
    // deux blocs écartés : l'enveloppe ajoute du volume, et le dit
    let c = bloc(&mut s, [0.0, 512.0, 0.0], [64.0, 576.0, 64.0]);
    let d = bloc(&mut s, [256.0, 512.0, 128.0], [320.0, 576.0, 192.0]);
    let r = faire(&mut s, Commande::Fusionner { objets: vec![c, d] });
    assert!(r.message.contains("ajoute"), "{}", r.message);
    assert!(s.executer(Commande::Fusionner { objets: vec![a] }).is_err());
}

#[test]
fn primitives() {
    let mut s = Session::nouvelle();
    let m = "DEV/DEV_MEASUREGENERIC01".to_string();
    let boite = |x: f64| ([x, 0.0, 0.0], [x + 256.0, 256.0, 128.0]);
    let (lo, hi) = boite(0.0);
    let r = faire(&mut s, Commande::Primitive { forme: Forme::Cylindre { cotes: 8 }, min: lo, max: hi, materiau: m.clone(), classe: None });
    let id = match r.crees[0] {
        Ref::Solide(i) => i,
        _ => unreachable!(),
    };
    let c = s.doc().solide(id).unwrap();
    assert_eq!(c.faces.len(), 10);
    // octogone inscrit dans un cercle de rayon 128 : aire 2·√2·r²
    assert!(proche(volume(c), 2.0 * 2f64.sqrt() * 128.0 * 128.0 * 128.0), "{}", volume(c));
    let (lo, hi) = boite(512.0);
    let r = faire(&mut s, Commande::Primitive { forme: Forme::Pointe { cotes: 4 }, min: lo, max: hi, materiau: m.clone(), classe: None });
    let p = s.doc().solide(match r.crees[0] { Ref::Solide(i) => i, _ => unreachable!() }).unwrap();
    assert_eq!(p.faces.len(), 5);
    // pyramide à base carrée inscrite (losange de diagonales 256) : base × h / 3
    assert!(proche(volume(p), 256.0 * 256.0 / 2.0 * 128.0 / 3.0), "{}", volume(p));
    let (lo, hi) = boite(1024.0);
    let r = faire(&mut s, Commande::Primitive { forme: Forme::Coin, min: lo, max: hi, materiau: m.clone(), classe: None });
    let w = s.doc().solide(match r.crees[0] { Ref::Solide(i) => i, _ => unreachable!() }).unwrap();
    assert!(w.faces.len() == 5 && proche(volume(w), 256.0 * 256.0 * 128.0 / 2.0));
    let (lo, hi) = boite(1536.0);
    let r = faire(&mut s, Commande::Primitive { forme: Forme::Sphere { cotes: 8 }, min: lo, max: hi, materiau: m.clone(), classe: None });
    let sp = s.doc().solide(match r.crees[0] { Ref::Solide(i) => i, _ => unreachable!() }).unwrap();
    assert!(sp.valide() && sp.faces.len() >= 16, "{} faces", sp.faces.len());
    let vb = 4.0 / 3.0 * std::f64::consts::PI * 128.0 * 128.0 * 64.0;
    assert!(volume(sp) < vb && volume(sp) > vb * 0.5, "{} pour une boule de {vb}", volume(sp));
    // arche : 8 pièces groupées, un demi-anneau
    let (lo, hi) = boite(2048.0);
    let arche = Arche { largeur_mur: 32.0, cotes: 8, arc: 180.0, depart: 0.0, hauteur_ajoutee: 0.0 };
    let r = faire(&mut s, Commande::Primitive { forme: Forme::Arche(arche), min: lo, max: hi, materiau: m.clone(), classe: None });
    assert!(matches!(r.crees[0], Ref::Groupe(_)));
    let g = match r.crees[0] { Ref::Groupe(g) => g, _ => unreachable!() };
    let membres: Vec<_> = s.doc().solides_monde().filter(|x| x.groupe() == g).collect();
    assert_eq!(membres.len(), 8);
    assert!(membres.iter().all(|x| x.valide() && x.faces.len() == 6));
    // escalier de 4 marches : volume 256 × 256 × 128 × (1+2+3+4)/16
    let (lo, hi) = boite(2560.0);
    faire(&mut s, Commande::Primitive { forme: Forme::Escalier { marches: 4, sens: 0 }, min: lo, max: hi, materiau: m.clone(), classe: None });
    let marches: Vec<f64> = s.doc().solides_monde().filter(|x| x.boite().unwrap().0.x >= 2560.0 - 1e-6).map(volume).collect();
    assert_eq!(marches.len(), 4);
    assert!(proche(marches.iter().sum(), 256.0 * 256.0 * 128.0 * 10.0 / 16.0));
    // en entité : un func_detail
    let (lo, hi) = boite(3072.0);
    let r = faire(&mut s, Commande::Primitive { forme: Forme::Cylindre { cotes: 12 }, min: lo, max: hi, materiau: m.clone(), classe: Some("func_detail".into()) });
    assert!(matches!(r.crees[0], Ref::Entite(_)));
    // trop de côtés pour la boîte, ou boîte plate : refusé
    assert!(s.executer(Commande::Primitive { forme: Forme::Coin, min: [0.0; 3], max: [64.0, 64.0, 0.0], materiau: m, classe: None }).is_err());
}

#[test]
fn evider_un_cylindre() {
    let mut s = Session::nouvelle();
    let r = s.executer(Commande::Primitive { forme: Forme::Cylindre { cotes: 8 }, min: [0.0; 3], max: [256.0, 256.0, 128.0], materiau: "BRICK/BRICKWALL003A".into(), classe: None }).unwrap();
    let v0 = bilan(&s).1;
    let r = faire(&mut s, Commande::Evider { objets: r.crees, epaisseur: 16.0 });
    assert_eq!(r.crees.len(), 10, "un mur par face");
    let (n, v) = bilan(&s);
    assert_eq!(n, 10);
    // l'intérieur : octogone d'apothème rentré de 16, hauteur 96
    let apo = 128.0 * (std::f64::consts::PI / 8.0).cos() - 16.0;
    let aire = 8.0 * apo * apo * (std::f64::consts::PI / 8.0).tan();
    assert!(proche(v, v0 - aire * 96.0), "{v} pour {}", v0 - aire * 96.0);
    // les murs gardent le matériau du solide
    assert!(s.doc().solides_monde().all(|x| x.faces.iter().all(|f| f.materiau == "BRICK/BRICKWALL003A")));
    let _ = DVec3::ZERO;
}

/// Un solide convexe d'après des points (outil polygone) : un prisme triangulaire extrudé, son volume ; des points
/// dans un même plan sont refusés.
#[test]
fn solide_d_apres_des_points() {
    let mut s = hm_doc::Session::nouvelle();
    let base = [[0.0, 0.0], [256.0, 0.0], [0.0, 128.0]];
    let points: Vec<[f64; 3]> = [0.0, 64.0].iter().flat_map(|z| base.iter().map(move |p| [p[0], p[1], *z])).collect();
    let r = s.executer(hm_doc::Commande::CreerEnveloppe { points, materiau: "dev/dev_measuregeneric01".into(), classe: None }).unwrap();
    let hm_doc::Ref::Solide(id) = r.crees[0] else { panic!() };
    let sol = s.doc().solide(id).unwrap();
    assert_eq!(sol.faces.len(), 5);
    assert!((hm_doc::brosses::volume(sol) - 256.0 * 128.0 / 2.0 * 64.0).abs() < 1.0);
    let plat = vec![[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [0.0, 64.0, 0.0], [64.0, 64.0, 0.0]];
    assert!(s.executer(hm_doc::Commande::CreerEnveloppe { points: plat, materiau: "dev/dev_measuregeneric01".into(), classe: None }).is_err());
}
