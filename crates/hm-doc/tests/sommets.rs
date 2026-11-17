//! Édition de sommets : formes connues obtenues en déplaçant sommets et arêtes, en coupant une face ; refus mesurés
//! (face non plane, faces coplanaires) ; annulation à l'octet.
use hm_doc::brosses::volume;
use hm_doc::sommets::OpSommet;
use hm_doc::{Commande, Ref, Session};

fn bloc(s: &mut Session) -> u64 {
    match s.executer(Commande::CreerBloc { min: [0.0; 3], max: [256.0, 256.0, 256.0], materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: None }).unwrap().crees[0] {
        Ref::Solide(i) => i,
        _ => unreachable!(),
    }
}

fn faire(s: &mut Session, c: Commande) {
    let avant = s.doc().ecrire();
    s.executer(c.clone()).unwrap_or_else(|e| panic!("{c:?} : {e}"));
    let apres = s.doc().ecrire();
    s.annuler().unwrap();
    assert!(s.doc().ecrire() == avant);
    s.retablir().unwrap();
    assert!(s.doc().ecrire() == apres);
    let relu = Session::depuis_octets(&apres, None).unwrap();
    assert!(relu.doc().ecrire() == apres);
}

fn dep(points: &[[f64; 3]], delta: [f64; 3]) -> OpSommet {
    OpSommet::Deplacer { points: points.to_vec(), delta }
}

#[test]
fn monter_le_dessus_et_pencher_une_arete() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s);
    // les quatre sommets du dessus montent de 64
    let dessus = [[0.0, 0.0, 256.0], [256.0, 0.0, 256.0], [256.0, 256.0, 256.0], [0.0, 256.0, 256.0]];
    faire(&mut s, Commande::ModifierSommets { solide: id, operations: vec![dep(&dessus, [0.0, 0.0, 64.0])] });
    let b = s.doc().solide(id).unwrap();
    assert!((volume(b) - 256.0 * 256.0 * 320.0).abs() < 1e-3);
    // l'arête du dessus côté y = 256 (désignée par son milieu) descend de 128 : le dessus penche
    faire(&mut s, Commande::ModifierSommets { solide: id, operations: vec![dep(&[[128.0, 256.0, 320.0]], [0.0, 0.0, -128.0])] });
    let b = s.doc().solide(id).unwrap();
    assert_eq!(b.faces.len(), 6);
    assert!((volume(b) - 256.0 * 256.0 * (320.0 + 192.0) / 2.0).abs() < 1e-3, "{}", volume(b));
    // les numéros des faces sont gardés
    assert!(b.faces.iter().all(|f| f.id >= 1 && f.id <= 6));
}

#[test]
fn un_coin_seul_est_refuse() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s);
    let avant = s.doc().ecrire();
    let e = s.executer(Commande::ModifierSommets { solide: id, operations: vec![dep(&[[256.0, 256.0, 256.0]], [0.0, 0.0, -64.0])] }).unwrap_err();
    assert!(e.contains("n'est plus plane"), "{e}");
    assert!(s.doc().ecrire() == avant);
    // une coupe sans pli : deux faces dans le même plan
    let e = s.executer(Commande::ModifierSommets { solide: id, operations: vec![OpSommet::CouperFace { a: [0.0, 0.0, 256.0], b: [256.0, 256.0, 256.0] }] }).unwrap_err();
    assert!(e.contains("même plan"), "{e}");
}

#[test]
fn toit_a_deux_pans() {
    // le dessus coupé entre les milieux de deux arêtes opposées, puis la ligne de coupe montée : un pignon
    let mut s = Session::nouvelle();
    let id = bloc(&mut s);
    let ops = vec![
        OpSommet::CouperFace { a: [128.0, 0.0, 256.0], b: [128.0, 256.0, 256.0] },
        dep(&[[128.0, 0.0, 256.0], [128.0, 256.0, 256.0]], [0.0, 0.0, 128.0]),
    ];
    faire(&mut s, Commande::ModifierSommets { solide: id, operations: ops });
    let b = s.doc().solide(id).unwrap();
    assert_eq!(b.faces.len(), 7, "deux pans au lieu du dessus");
    assert!((volume(b) - (256.0 * 256.0 * 256.0 + 256.0 * 128.0 / 2.0 * 256.0)).abs() < 1e-3, "{}", volume(b));
    assert_eq!(b.faces.iter().filter(|f| f.id > 6).count(), 1, "un seul numéro neuf : la face née de la coupe");
}

#[test]
fn souder_une_arete_sur_une_autre() {
    // l'arête du dessus côté y = 256 glissée sur celle côté y = 0 : les sommets se soudent, le bloc devient un coin
    let mut s = Session::nouvelle();
    let id = bloc(&mut s);
    faire(&mut s, Commande::ModifierSommets { solide: id, operations: vec![dep(&[[128.0, 256.0, 256.0]], [0.0, -256.0, 0.0])] });
    let b = s.doc().solide(id).unwrap();
    assert_eq!(b.faces.len(), 5);
    assert!((volume(b) - 256.0 * 256.0 * 256.0 / 2.0).abs() < 1e-3);
}

/// La mise à l'échelle des sommets de l'éditeur (Alt+E) : un déplacement par sommet vers c + (p − c)·k, en une
/// commande ; les faces restent planes à la fin.
#[test]
fn sommets_a_l_echelle() {
    let mut s = Session::nouvelle();
    let id = bloc(&mut s);
    let c = 128.0;
    let mut operations = Vec::new();
    for x in [0.0, 256.0] {
        for y in [0.0, 256.0] {
            for z in [0.0, 256.0] {
                let p = [x, y, z];
                operations.push(OpSommet::Deplacer { points: vec![p], delta: p.map(|v| (c + (v - c) * 1.5) - v) });
            }
        }
    }
    s.executer(Commande::ModifierSommets { solide: id, operations }).unwrap();
    let (lo, hi) = hm_doc::boite_de(s.doc(), &[Ref::Solide(id)]).unwrap();
    assert_eq!((lo.x, hi.x, lo.z, hi.z), (-64.0, 320.0, -64.0, 320.0));
    assert!((volume(s.doc().solide(id).unwrap()) - 384.0f64.powi(3)).abs() < 1.0);
}
