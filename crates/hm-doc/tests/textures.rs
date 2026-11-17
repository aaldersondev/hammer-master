//! Alignement des textures : chaque commande mesurée sur les coordonnées de texture des sommets des faces.
use glam::DVec3;
use hm_doc::{polygone, Alignement, Commande, Face, Justif, Ref, RefFace, ReglageFaces, Session};

fn bloc(s: &mut Session, lo: [f64; 3], hi: [f64; 3]) -> u64 {
    match s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: "BRICK/BRICKWALL003A".into(), classe: None }).unwrap().crees[0] {
        Ref::Solide(id) => id,
        _ => unreachable!(),
    }
}

fn face(s: &Session, id: u64, normale: DVec3) -> (RefFace, Face) {
    let f = s.doc().solide(id).unwrap().faces.iter().find(|f| f.normale().dot(normale) > 0.99).unwrap().clone();
    (RefFace { solide: id, face: f.id }, f)
}

fn uv(f: &Face, p: DVec3) -> (f64, f64) {
    (p.dot(f.u.dir) / f.u.echelle + f.u.decalage, p.dot(f.v.dir) / f.v.echelle + f.v.decalage)
}

#[test]
fn regler_et_tourner() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s, [0.0; 3], [256.0, 128.0, 64.0]);
    let (rf, _) = face(&s, id, DVec3::Z);
    s.executer(Commande::ReglerFaces { faces: vec![rf], reglage: ReglageFaces { echelle_u: Some(0.5), decalage_v: Some(32.0), echelle_lumiere: Some(8.0), ..Default::default() } }).unwrap();
    let (_, f) = face(&s, id, DVec3::Z);
    assert_eq!((f.u.echelle, f.v.decalage, f.echelle_lumiere), (0.5, 32.0, 8.0));
    // 90° : les axes restent dans le plan, orthogonaux, et tournés d'un quart
    let (u0, v0) = (f.u.dir, f.v.dir);
    s.executer(Commande::ReglerFaces { faces: vec![rf], reglage: ReglageFaces { rotation: Some(90.0), ..Default::default() } }).unwrap();
    let (_, f) = face(&s, id, DVec3::Z);
    assert!(f.u.dir.dot(u0).abs() < 1e-9 && f.u.dir.dot(v0).abs() > 0.999 && f.u.dir.dot(f.v.dir).abs() < 1e-9);
    assert_eq!(f.rotation, 90.0);
    // revenir à 0 rend les axes d'origine
    s.executer(Commande::ReglerFaces { faces: vec![rf], reglage: ReglageFaces { rotation: Some(0.0), ..Default::default() } }).unwrap();
    let (_, f) = face(&s, id, DVec3::Z);
    assert!((f.u.dir - u0).length() < 1e-9 && (f.v.dir - v0).length() < 1e-9);
}

#[test]
fn justifier_et_ajuster() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s, [16.0, 0.0, 0.0], [144.0, 64.0, 32.0]); // dessus : 128 × 64
    let (rf, _) = face(&s, id, DVec3::Z);
    let t = vec![("BRICK/BRICKWALL003A".to_string(), [512.0, 512.0])];
    s.executer(Commande::JustifierFaces { faces: vec![rf], justif: Justif::Ajuster, ensemble: false, tailles: t.clone() }).unwrap();
    let (_, f) = face(&s, id, DVec3::Z);
    let poly = polygone(s.doc(), rf).unwrap();
    let us: Vec<(f64, f64)> = poly.iter().map(|p| uv(&f, *p)).collect();
    let (umin, umax) = us.iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(x.0), a.1.max(x.0)));
    let (vmin, vmax) = us.iter().fold((f64::MAX, f64::MIN), |a, x| (a.0.min(x.1), a.1.max(x.1)));
    // une seule répétition, qui commence au bord
    assert!((umax - umin - 512.0).abs() < 1e-6 && (vmax - vmin - 512.0).abs() < 1e-6, "{umin} {umax} {vmin} {vmax}");
    assert!(umin.rem_euclid(512.0).min(512.0 - umin.rem_euclid(512.0)) < 1e-6);
    assert!(vmin.rem_euclid(512.0).min(512.0 - vmin.rem_euclid(512.0)) < 1e-6);
    // gauche : le bord gauche tombe sur un début de texture ; droite : le bord droit sur une fin
    s.executer(Commande::ReglerFaces { faces: vec![rf], reglage: ReglageFaces { echelle_u: Some(0.25), echelle_v: Some(0.25), decalage_u: Some(17.0), ..Default::default() } }).unwrap();
    for (j, borne) in [(Justif::Gauche, 0usize), (Justif::Droite, 1)] {
        s.executer(Commande::JustifierFaces { faces: vec![rf], justif: j, ensemble: false, tailles: t.clone() }).unwrap();
        let (_, f) = face(&s, id, DVec3::Z);
        let us: Vec<f64> = poly.iter().map(|p| uv(&f, *p).0).collect();
        let b = if borne == 0 { us.iter().cloned().fold(f64::MAX, f64::min) } else { us.iter().cloned().fold(f64::MIN, f64::max) };
        let r = b.rem_euclid(512.0);
        assert!(r.min(512.0 - r) < 1e-6, "{j:?} : bord à {b}");
    }
}

#[test]
fn aligner_monde_et_face() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let (rf, f0) = face(&s, id, DVec3::X);
    s.executer(Commande::ReglerFaces { faces: vec![rf], reglage: ReglageFaces { rotation: Some(30.0), ..Default::default() } }).unwrap();
    s.executer(Commande::AlignerFaces { faces: vec![rf], sur: Alignement::Monde }).unwrap();
    let (_, f) = face(&s, id, DVec3::X);
    assert!((f.u.dir - f0.u.dir).length() < 1e-9 && f.rotation == 0.0);
    // une face inclinée : alignée sur la face, ses axes sont dans son plan
    s.executer(Commande::Tourner { objets: vec![Ref::Solide(id)], centre: [32.0; 3], axe: [0.0, 1.0, 0.0], degres: 30.0, verrou_texture: false }).unwrap();
    let f = s.doc().solide(id).unwrap().faces.iter().find(|f| f.normale().z.abs() > 0.3 && f.normale().z.abs() < 0.9).unwrap().clone();
    let rf = RefFace { solide: id, face: f.id };
    s.executer(Commande::AlignerFaces { faces: vec![rf], sur: Alignement::Face }).unwrap();
    let g = s.doc().solide(id).unwrap().faces.iter().find(|x| x.id == f.id).unwrap().clone();
    assert!(g.u.dir.dot(g.normale()).abs() < 1e-9 && g.v.dir.dot(g.normale()).abs() < 1e-9);
}

#[test]
fn enrouler_sur_la_face_voisine() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s, [0.0; 3], [128.0, 128.0, 128.0]);
    let (dessus, _) = face(&s, id, DVec3::Z);
    let (cote, _) = face(&s, id, DVec3::X);
    s.executer(Commande::ReglerFaces { faces: vec![dessus], reglage: ReglageFaces { decalage_u: Some(40.0), decalage_v: Some(13.0), rotation: Some(15.0), ..Default::default() } }).unwrap();
    s.executer(Commande::EnroulerFaces { source: dessus, cibles: vec![cote] }).unwrap();
    let (_, fd) = face(&s, id, DVec3::Z);
    let (_, fc) = face(&s, id, DVec3::X);
    // l'arête commune (x = 128, z = 128) a les mêmes coordonnées de texture des deux côtés
    for y in [0.0, 37.0, 128.0] {
        let p = DVec3::new(128.0, y, 128.0);
        let (a, b) = (uv(&fd, p), uv(&fc, p));
        assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "{p} : {a:?} contre {b:?}");
    }
    assert_eq!(fc.materiau, fd.materiau);
}

#[test]
fn remplacer_un_materiau() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let b = bloc(&mut s, [128.0, 0.0, 0.0], [192.0, 64.0, 64.0]);
    let r = s.executer(Commande::RemplacerMateriau { ancien: "brick/brickwall003a".into(), nouveau: "CONCRETE/CONCRETEWALL001A".into(), objets: Some(vec![Ref::Solide(a)]) }).unwrap();
    assert_eq!(r.message, "6 face(s) remplacée(s)");
    assert!(s.doc().solide(a).unwrap().faces.iter().all(|f| f.materiau == "CONCRETE/CONCRETEWALL001A"));
    assert!(s.doc().solide(b).unwrap().faces.iter().all(|f| f.materiau == "BRICK/BRICKWALL003A"));
}

#[test]
fn glisser_un_champ_fait_une_seule_etape() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let (rf, _) = face(&s, id, DVec3::Z);
    let avant = s.doc().ecrire();
    let n = s.historique().faits().len();
    for i in 1..=10 {
        s.executer_continu(Commande::ReglerFaces { faces: vec![rf], reglage: ReglageFaces { decalage_u: Some(i as f64 * 4.0), ..Default::default() } }, "decalage_u").unwrap();
    }
    assert_eq!(s.historique().faits().len(), n + 1);
    assert_eq!(face(&s, id, DVec3::Z).1.u.decalage, 40.0);
    let apres = s.doc().ecrire();
    s.annuler();
    assert!(s.doc().ecrire() == avant, "une seule annulation revient avant le glisser");
    s.retablir();
    assert!(s.doc().ecrire() == apres);
    // une autre clé (ou une autre commande entre deux) ouvre une nouvelle étape
    s.executer_continu(Commande::ReglerFaces { faces: vec![rf], reglage: ReglageFaces { decalage_v: Some(8.0), ..Default::default() } }, "decalage_v").unwrap();
    assert_eq!(s.historique().faits().len(), n + 2);
}
