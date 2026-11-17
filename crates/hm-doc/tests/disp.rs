//! Displacements : créer, relire, sculpter (relever, lisser), peindre l'alpha, coudre ; positions mesurées, accord
//! avec le calcul de `hm-geom` (celui du rendu), annulation à l'octet.
use glam::DVec3;
use hm_doc::disp::{Disp, Pinceau};
use hm_doc::{polygone, Commande, Ref, RefFace, Session};

fn faire(s: &mut Session, c: Commande) -> hm_doc::Resultat {
    let avant = s.doc().ecrire();
    let r = s.executer(c.clone()).unwrap_or_else(|e| panic!("{c:?} : {e}"));
    let apres = s.doc().ecrire();
    assert_ne!(avant, apres);
    s.annuler().unwrap();
    assert!(s.doc().ecrire() == avant);
    s.retablir().unwrap();
    assert!(s.doc().ecrire() == apres);
    let relu = Session::depuis_octets(&apres, None).unwrap();
    assert!(relu.doc().ecrire() == apres, "{c:?} : relu différent");
    r
}

/// Un sol de 512 × 512 (dessus en z = 0) et la référence de sa face du dessus.
fn sol(s: &mut Session, x0: f64) -> RefFace {
    let b = s.executer(Commande::CreerBloc { min: [x0, 0.0, -16.0], max: [x0 + 512.0, 512.0, 0.0], materiau: "NATURE/BLENDGRASSGRAVEL001A".into(), classe: None }).unwrap().crees[0];
    let id = match b {
        Ref::Solide(i) => i,
        _ => unreachable!(),
    };
    let f = s.doc().solide(id).unwrap().faces.iter().find(|f| f.normale().z > 0.99).unwrap().id;
    RefFace { solide: id, face: f }
}

fn points(s: &Session, rf: RefFace) -> Vec<DVec3> {
    let f = s.doc().solide(rf.solide).unwrap().faces.iter().find(|x| x.id == rf.face).unwrap();
    let d = Disp::lire(f.disp.as_ref().unwrap()).unwrap();
    d.points(&polygone(s.doc(), rf).unwrap(), f.normale()).unwrap().1
}

#[test]
fn creer_relire_et_accord_avec_le_rendu() {
    let mut s = Session::nouvelle();
    let rf = sol(&mut s, 0.0);
    faire(&mut s, Commande::CreerDisp { faces: vec![rf], puissance: 3 });
    let p = points(&s, rf);
    assert_eq!(p.len(), 81);
    assert!(p.iter().all(|q| q.z.abs() < 1e-9));
    // relever puis relire : le bloc écrit se relit à l'identique
    faire(&mut s, Commande::PeindreDisp { faces: None, centre: [256.0, 256.0, 0.0], rayon: 200.0, pinceau: Pinceau::Relever { hauteur: 64.0 } });
    let f = s.doc().solide(rf.solide).unwrap().faces.iter().find(|x| x.id == rf.face).unwrap().clone();
    let d = Disp::lire(f.disp.as_ref().unwrap()).unwrap();
    assert_eq!(Disp::lire(&d.ecrire()).unwrap(), d);
    // le calcul du rendu (hm-geom, depuis le texte du bloc) donne les mêmes points
    let mut w = hm_doc::kv::Ecrivain::new(hm_doc::kv::Format { crlf: false, latin1: false });
    w.bloc(f.disp.as_ref().unwrap(), 0);
    let vmf = hm_vmf::Vmf::parse(w.out).unwrap();
    let poly = polygone(s.doc(), rf).unwrap();
    let g = hm_geom::build_displacement(vmf.root.children.first().unwrap(), &poly, f.normale()).unwrap();
    let nous = d.points(&poly, f.normale()).unwrap().1;
    assert!(g.points.iter().zip(&nous).all(|(a, b)| (*a - *b).length() < 1e-3));
}

#[test]
fn relever_lisser_alpha() {
    let mut s = Session::nouvelle();
    let rf = sol(&mut s, 0.0);
    s.executer(Commande::CreerDisp { faces: vec![rf], puissance: 3 }).unwrap();
    let r = faire(&mut s, Commande::PeindreDisp { faces: Some(vec![rf]), centre: [256.0, 256.0, 0.0], rayon: 128.0, pinceau: Pinceau::Relever { hauteur: 64.0 } });
    assert_eq!(r.message, "1 displacement(s) touché(s)");
    let p = points(&s, rf);
    // le centre (point 40 d'une grille de 9) monte de 64 ; les points à 128 et plus ne bougent pas
    let centre = p.iter().min_by(|a, b| (a.truncate() - glam::DVec2::splat(256.0)).length().total_cmp(&(b.truncate() - glam::DVec2::splat(256.0)).length())).unwrap();
    assert!((centre.z - 64.0).abs() < 1e-3, "{centre:?}");
    assert!(p.iter().filter(|q| (q.truncate() - glam::DVec2::splat(256.0)).length() >= 128.0).all(|q| q.z.abs() < 1e-6));
    // lisser fait redescendre le sommet
    faire(&mut s, Commande::PeindreDisp { faces: None, centre: [256.0, 256.0, 64.0], rayon: 200.0, pinceau: Pinceau::Lisser { force: 1.0 } });
    let p2 = points(&s, rf);
    let c2 = p2.iter().map(|q| q.z).fold(f64::MIN, f64::max);
    assert!(c2 < 64.0 - 1.0 && c2 > 0.0, "{c2}");
    // alpha au centre
    faire(&mut s, Commande::PeindreDisp { faces: None, centre: [256.0, 256.0, 30.0], rayon: 100.0, pinceau: Pinceau::Alpha { valeur: 255.0, force: 1.0 } });
    let f = s.doc().solide(rf.solide).unwrap().faces.iter().find(|x| x.id == rf.face).unwrap();
    let d = Disp::lire(f.disp.as_ref().unwrap()).unwrap();
    assert!(d.alphas.iter().cloned().fold(0.0, f64::max) > 200.0 && d.alphas.iter().any(|a| *a == 0.0));
    // une bosse raide (pente 2) : ses triangles sont étiquetés trop raides (0), le reste plat (9)
    s.executer(Commande::PeindreDisp { faces: None, centre: [64.0, 64.0, 0.0], rayon: 100.0, pinceau: Pinceau::Relever { hauteur: 300.0 } }).unwrap();
    let f = s.doc().solide(rf.solide).unwrap().faces.iter().find(|x| x.id == rf.face).unwrap();
    let d = Disp::lire(f.disp.as_ref().unwrap()).unwrap();
    assert!(d.etiquettes.iter().any(|t| *t == 0) && d.etiquettes.iter().any(|t| *t == 9), "{:?}", d.etiquettes);
    // un coup hors de portée ne fait pas d'étape
    let n = s.historique().faits().len();
    s.executer(Commande::PeindreDisp { faces: None, centre: [5000.0, 0.0, 0.0], rayon: 64.0, pinceau: Pinceau::Relever { hauteur: 8.0 } }).unwrap();
    assert_eq!(s.historique().faits().len(), n);
}

#[test]
fn coudre_deux_voisins() {
    let mut s = Session::nouvelle();
    let a = sol(&mut s, 0.0);
    let b = sol(&mut s, 512.0);
    s.executer(Commande::CreerDisp { faces: vec![a, b], puissance: 2 }).unwrap();
    // seul A est relevé près du bord commun (x = 512) : les bords ne se touchent plus
    s.executer(Commande::PeindreDisp { faces: Some(vec![a]), centre: [512.0, 256.0, 0.0], rayon: 200.0, pinceau: Pinceau::Relever { hauteur: 40.0 } }).unwrap();
    let bord = |p: &[DVec3]| -> Vec<DVec3> { p.iter().filter(|q| (q.x - 512.0).abs() < 1e-6).copied().collect() };
    assert!(bord(&points(&s, a)).iter().any(|q| q.z > 1.0));
    assert!(bord(&points(&s, b)).iter().all(|q| q.z.abs() < 1e-6));
    let r = faire(&mut s, Commande::CoudreDisp { faces: vec![a, b] });
    assert_eq!(r.message, "5 point(s) cousu(s)");
    let (ba, bb) = (bord(&points(&s, a)), bord(&points(&s, b)));
    for p in &ba {
        assert!(bb.iter().any(|q| (*q - *p).length() < 1e-3), "{p:?} sans pendant");
    }
    assert!(ba.iter().any(|q| q.z > 1.0), "la couture prend la moyenne, pas le plus bas");
}

#[test]
fn refus() {
    let mut s = Session::nouvelle();
    let rf = sol(&mut s, 0.0);
    let avant = s.doc().ecrire();
    assert!(s.executer(Commande::CreerDisp { faces: vec![rf], puissance: 5 }).is_err());
    assert!(s.executer(Commande::DetruireDisp { faces: vec![rf] }).is_err());
    // une face à trois côtés (coin)
    let c = s.executer(Commande::Primitive { forme: hm_doc::Forme::Coin, min: [1000.0, 0.0, 0.0], max: [1256.0, 256.0, 128.0], materiau: "x".into(), classe: None }).unwrap().crees[0];
    let id = match c {
        Ref::Solide(i) => i,
        _ => unreachable!(),
    };
    let tri = s.doc().solide(id).unwrap().faces.iter().find(|f| polygone(s.doc(), RefFace { solide: id, face: f.id }).unwrap().len() == 3).unwrap().id;
    let avant2 = s.doc().ecrire();
    let e = s.executer(Commande::CreerDisp { faces: vec![RefFace { solide: id, face: tri }], puissance: 2 }).unwrap_err();
    assert!(e.contains("quatre côtés"), "{e}");
    assert!(s.doc().ecrire() == avant2);
    let _ = avant;
}
