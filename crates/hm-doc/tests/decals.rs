//! Décals et overlays : clés écrites, coins dans le monde, base qui suit un déplacement et une rotation, surface la
//! plus proche d'un décal.
use glam::DVec3;
use hm_doc::{Commande, Ref, RefFace, Session};

fn proche(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-6
}

#[test]
fn overlay_sur_un_mur() {
    let mut s = Session::nouvelle();
    let b = s.executer(Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [16.0, 256.0, 128.0], materiau: "BRICK/BRICKWALL003A".into(), classe: None }).unwrap().crees[0];
    let sid = match b {
        Ref::Solide(i) => i,
        _ => unreachable!(),
    };
    // la face x = 16 (normale +X)
    let f = s.doc().solide(sid).unwrap().faces.iter().find(|f| f.normale().x > 0.99).unwrap().clone();
    let r = s.executer(Commande::PoserOverlay { face: RefFace { solide: sid, face: f.id }, point: [16.0, 128.0, 64.0], materiau: "decals/decalgraffiti001a".into(), taille: [64.0, 32.0] }).unwrap();
    let e = match r.crees[0] {
        Ref::Entite(i) => i,
        _ => unreachable!(),
    };
    let ent = s.doc().entite(e).unwrap().clone();
    assert_eq!(ent.classe(), "info_overlay");
    assert_eq!(ent.get("sides"), Some(f.id.to_string().as_str()));
    assert_eq!(ent.get("BasisNormal"), Some("1 0 0"));
    let coins = hm_doc::decals::coins_overlay(&ent, 0.0).unwrap();
    // le quadrilatère est dans le plan du mur, 64 × 32 autour du point
    assert!(coins.iter().all(|(p, _)| (p.x - 16.0).abs() < 1e-6));
    let (lo, hi) = coins.iter().fold((DVec3::splat(1e9), DVec3::splat(-1e9)), |(a, b), (p, _)| (a.min(*p), b.max(*p)));
    assert!(proche(hi - lo, DVec3::new(0.0, 64.0, 32.0)), "{:?}", hi - lo);
    // déplacé : sa base suit ; tourné de 90° autour de Z : sa normale aussi
    s.executer(Commande::Deplacer { objets: vec![Ref::Entite(e)], delta: [0.0, 32.0, 0.0], verrou_texture: true }).unwrap();
    assert_eq!(s.doc().entite(e).unwrap().get("BasisOrigin"), Some("16 160 64"));
    s.executer(Commande::Tourner { objets: vec![Ref::Entite(e)], centre: [16.0, 160.0, 64.0], axe: [0.0, 0.0, 1.0], degres: 90.0, verrou_texture: true }).unwrap();
    let n = hm_doc::modele::lire_vec3(s.doc().entite(e).unwrap().get("BasisNormal").unwrap()).unwrap();
    assert!(proche(n, DVec3::Y), "{n:?}");
    // refus sans trace
    let avant = s.doc().ecrire();
    assert!(s.executer(Commande::PoserOverlay { face: RefFace { solide: sid, face: 999_999 }, point: [0.0; 3], materiau: "x".into(), taille: [8.0, 8.0] }).is_err());
    assert!(s.executer(Commande::PoserDecal { point: [0.0; 3], materiau: "".into() }).is_err());
    assert!(s.doc().ecrire() == avant);
}

#[test]
fn decal_et_surface_proche() {
    let mut s = Session::nouvelle();
    s.executer(Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [16.0, 256.0, 128.0], materiau: "BRICK/BRICKWALL003A".into(), classe: None }).unwrap();
    s.executer(Commande::PoserDecal { point: [18.0, 100.0, 64.0], materiau: "decals/decalsigndanger001e".into() }).unwrap();
    let e = s.doc().entites().find(|e| e.classe() == "infodecal").unwrap();
    assert_eq!(e.get("texture"), Some("decals/decalsigndanger001e"));
    let (p, n) = hm_doc::decals::surface_proche(s.doc(), e.origine().unwrap(), 16.0).unwrap();
    assert!(proche(p, DVec3::new(16.0, 100.0, 64.0)) && proche(n, DVec3::X), "{p:?} {n:?}");
    assert!(hm_doc::decals::surface_proche(s.doc(), DVec3::new(200.0, 100.0, 64.0), 16.0).is_none());
}
