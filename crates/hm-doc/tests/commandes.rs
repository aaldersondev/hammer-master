//! Chaque commande : son effet, puis annuler (le fichier redevient celui d'avant, à l'octet) et rétablir (celui
//! d'après, à l'octet). Une commande refusée ne laisse aucune trace.
use glam::DVec3;
use hm_doc::{boite_de, Commande, Ref, RefFace, Session};

fn fixture(nom: &str) -> Session {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(nom);
    Session::ouvrir(&p).unwrap()
}

/// Exécute, vérifie annuler / rétablir à l'octet, et rend le résultat.
fn faire(s: &mut Session, c: Commande) -> hm_doc::Resultat {
    let avant = s.doc().ecrire();
    let n = s.historique().faits().len();
    let r = s.executer(c.clone()).unwrap_or_else(|e| panic!("{c:?} : {e}"));
    let apres = s.doc().ecrire();
    assert_ne!(avant, apres, "{c:?} n'a rien changé");
    assert_eq!(s.historique().faits().len(), n + 1);
    s.annuler().unwrap();
    assert!(s.doc().ecrire() == avant, "annuler {c:?} ne rend pas le fichier d'avant");
    s.retablir().unwrap();
    assert!(s.doc().ecrire() == apres, "rétablir {c:?} ne rend pas le fichier d'après");
    // le fichier écrit se relit et se réécrit à l'identique
    let relu = Session::depuis_octets(&apres, None).unwrap();
    assert!(relu.doc().ecrire() == apres, "{c:?} : le fichier écrit ne se relit pas à l'identique");
    r
}

fn boite(s: &Session, r: Ref) -> (DVec3, DVec3) {
    boite_de(s.doc(), &[r]).unwrap()
}

fn proche(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-6
}

#[test]
fn creer_un_bloc_puis_le_transformer() {
    let mut s = Session::nouvelle();
    let r = faire(&mut s, Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [256.0, 128.0, 64.0], materiau: "dev/dev_measuregeneric01".into(), classe: None });
    let b = r.crees[0];
    assert_eq!(s.selection, vec![b]);
    assert!(proche(boite(&s, b).1, DVec3::new(256.0, 128.0, 64.0)));

    faire(&mut s, Commande::Deplacer { objets: vec![b], delta: [16.0, -32.0, 8.0], verrou_texture: true });
    assert!(proche(boite(&s, b).0, DVec3::new(16.0, -32.0, 8.0)));

    let c = (boite(&s, b).0 + boite(&s, b).1) / 2.0;
    faire(&mut s, Commande::Tourner { objets: vec![b], centre: c.to_array(), axe: [0.0, 0.0, 1.0], degres: 90.0, verrou_texture: true });
    let (lo, hi) = boite(&s, b);
    assert!(((hi - lo) - DVec3::new(128.0, 256.0, 64.0)).length() < 1e-6, "{:?}", hi - lo);

    let (lo, hi) = boite(&s, b);
    faire(&mut s, Commande::Redimensionner { objets: vec![b], de: [lo.to_array(), hi.to_array()], vers: [lo.to_array(), (lo + DVec3::new(64.0, 64.0, 32.0)).to_array()], verrou_texture: true });
    assert!(proche(boite(&s, b).1 - boite(&s, b).0, DVec3::new(64.0, 64.0, 32.0)));

    faire(&mut s, Commande::Miroir { objets: vec![b], centre: [0.0, 0.0, 0.0], axe: 0 });
    assert!(boite(&s, b).1.x <= 1e-6);
    assert!(s.doc().solide(match b {
        Ref::Solide(i) => i,
        _ => unreachable!(),
    })
    .unwrap()
    .valide());
}

#[test]
fn refus_sans_trace() {
    let mut s = Session::nouvelle();
    let b = s.executer(Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: "x".into(), classe: None }).unwrap().crees[0];
    let avant = s.doc().ecrire();
    let n = s.historique().faits().len();
    assert!(s.executer(Commande::Redimensionner { objets: vec![b], de: [[0.0; 3], [64.0; 3]], vers: [[0.0; 3], [64.0, 64.0, 0.0]], verrou_texture: true }).is_err());
    assert!(s.executer(Commande::CreerBloc { min: [0.0; 3], max: [64.0, 64.0, 0.0], materiau: "x".into(), classe: None }).is_err());
    assert!(s.executer(Commande::Deplacer { objets: vec![Ref::Solide(999_999)], delta: [1.0, 0.0, 0.0], verrou_texture: true }).is_err());
    assert!(s.executer(Commande::Cles { entite: 1, cles: vec![("message".into(), Some("a\"b".into()))] }).is_err());
    assert!(s.doc().ecrire() == avant);
    assert_eq!(s.historique().faits().len(), n);
}

#[test]
fn entites_et_cles() {
    let mut s = fixture("adminroom.vmf");
    let r = faire(&mut s, Commande::CreerEntite { classe: "prop_static".into(), origine: [32.0, -16.0, 0.0], cles: vec![("model".into(), "models/props_c17/oildrum001.mdl".into())] });
    let Ref::Entite(e) = r.crees[0] else { panic!() };
    assert_eq!(s.doc().entite(e).unwrap().get("model"), Some("models/props_c17/oildrum001.mdl"));
    faire(&mut s, Commande::Cles { entite: e, cles: vec![("skin".into(), Some("2".into())), ("model".into(), None)] });
    let ent = s.doc().entite(e).unwrap();
    assert_eq!((ent.get("skin"), ent.get("model")), (Some("2"), None));
    // le monde se désigne par son numéro
    let monde = s.doc().monde().id;
    faire(&mut s, Commande::Cles { entite: monde, cles: vec![("skyname".into(), Some("sky_day02_01".into()))] });
    assert_eq!(s.doc().monde().get("skyname"), Some("sky_day02_01"));
    // une entité tournée de 90° autour de Z voit son lacet tourner de 90°
    faire(&mut s, Commande::Tourner { objets: vec![Ref::Entite(e)], centre: [32.0, -16.0, 0.0], axe: [0.0, 0.0, 1.0], degres: 90.0, verrou_texture: true });
    assert_eq!(s.doc().entite(e).unwrap().get("angles"), Some("0 90 0"));
}

#[test]
fn modifier_une_map_de_hammer_plus_plus() {
    let mut s = fixture("adminroom.vmf");
    let (_, solide) = s.doc().tous_les_solides().next().map(|(p, s)| (p, s.id)).unwrap();
    let face = s.doc().solide(solide).unwrap().faces[0].id;
    faire(&mut s, Commande::MateriauFaces { faces: vec![RefFace { solide, face }], materiau: "BRICK/BRICKWALL003A".into() });
    assert_eq!(s.doc().solide(solide).unwrap().faces[0].materiau, "BRICK/BRICKWALL003A");
    faire(&mut s, Commande::Deplacer { objets: vec![Ref::Solide(solide)], delta: [0.0, 0.0, 64.0], verrou_texture: true });
    faire(&mut s, Commande::MateriauObjets { objets: vec![Ref::Solide(solide)], materiau: "TOOLS/TOOLSNODRAW".into() });
    assert!(s.doc().solide(solide).unwrap().faces.iter().all(|f| f.materiau == "TOOLS/TOOLSNODRAW"));
    faire(&mut s, Commande::Supprimer { objets: vec![Ref::Solide(solide)] });
    assert!(s.doc().solide(solide).is_none());
}

#[test]
fn dupliquer_copier_coller() {
    let mut s = Session::nouvelle();
    let a = s.executer(Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: "x".into(), classe: None }).unwrap().crees[0];
    let e = s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [32.0, 32.0, 65.0], cles: vec![] }).unwrap().crees[0];
    let r = faire(&mut s, Commande::Dupliquer { objets: vec![a, e], delta: [128.0, 0.0, 0.0] });
    assert_eq!(r.crees.len(), 2);
    assert_eq!(s.selection, r.crees);
    assert_eq!(s.doc().nombre_objets(), (2, 2));
    let copie = r.crees.iter().find(|r| matches!(r, Ref::Solide(_))).copied().unwrap();
    assert!(proche(boite(&s, copie).0, DVec3::new(128.0, 0.0, 0.0)));
    assert_ne!(copie, a);
    // numéros de faces neufs
    let (Ref::Solide(ia), Ref::Solide(ic)) = (a, copie) else { panic!() };
    let fa: Vec<u64> = s.doc().solide(ia).unwrap().faces.iter().map(|f| f.id).collect();
    assert!(s.doc().solide(ic).unwrap().faces.iter().all(|f| !fa.contains(&f.id)));
    // presse-papiers
    s.selection = vec![a];
    let texte = s.copier().unwrap();
    let r = faire(&mut s, Commande::Coller { vmf: texte, decalage: [0.0, 256.0, 0.0] });
    assert!(proche(boite(&s, r.crees[0]).0, DVec3::new(0.0, 256.0, 0.0)));
}

#[test]
fn groupes_et_entites_a_brosses() {
    let mut s = Session::nouvelle();
    let a = s.executer(Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: "x".into(), classe: None }).unwrap().crees[0];
    let b = s.executer(Commande::CreerBloc { min: [128.0, 0.0, 0.0], max: [192.0, 64.0, 64.0], materiau: "x".into(), classe: None }).unwrap().crees[0];
    let r = faire(&mut s, Commande::Grouper { objets: vec![a, b] });
    let Ref::Groupe(g) = r.crees[0] else { panic!() };
    let (solides, _, _) = hm_doc::resoudre(s.doc(), &[Ref::Groupe(g)]).unwrap();
    assert_eq!(solides.len(), 2);
    // déplacer le groupe déplace ses membres
    faire(&mut s, Commande::Deplacer { objets: vec![Ref::Groupe(g)], delta: [0.0, 0.0, 32.0], verrou_texture: true });
    assert!(proche(boite(&s, a).0, DVec3::new(0.0, 0.0, 32.0)));
    faire(&mut s, Commande::Degrouper { groupes: vec![g] });
    assert!(s.doc().groupe(g).is_none());
    assert_eq!(s.doc().solide(match a { Ref::Solide(i) => i, _ => 0 }).unwrap().groupe(), 0);
    // lier à une entité puis rendre au monde
    let r = faire(&mut s, Commande::LierAEntite { objets: vec![a, b], classe: "func_detail".into() });
    let Ref::Entite(e) = r.crees[0] else { panic!() };
    assert_eq!(s.doc().entite(e).unwrap().solides.len(), 2);
    assert_eq!(s.doc().solides_monde().count(), 0);
    faire(&mut s, Commande::RemettreAuMonde { entites: vec![e] });
    assert_eq!(s.doc().solides_monde().count(), 2);
    assert!(s.doc().entite(e).is_none());
    // supprimer le dernier solide d'une entité à brosses supprime l'entité
    let r = s.executer(Commande::LierAEntite { objets: vec![a], classe: "func_detail".into() }).unwrap();
    let Ref::Entite(e) = r.crees[0] else { panic!() };
    faire(&mut s, Commande::Supprimer { objets: vec![a] });
    assert!(s.doc().entite(e).is_none());
}

#[test]
fn lot_une_seule_etape() {
    let mut s = Session::nouvelle();
    let r = faire(
        &mut s,
        Commande::Lot {
            nom: "pièce".into(),
            commandes: vec![
                Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [512.0, 512.0, 16.0], materiau: "x".into(), classe: None },
                Commande::CreerBloc { min: [0.0, 0.0, 128.0], max: [512.0, 512.0, 144.0], materiau: "x".into(), classe: None },
            ],
        },
    );
    assert_eq!(r.crees.len(), 2);
    assert_eq!(s.historique().faits().len(), 1);
    assert_eq!(s.historique().faits()[0].nom, "pièce");
}

#[test]
fn longue_suite_puis_tout_annuler() {
    let mut s = fixture("dispatch.vmf");
    let depart = s.doc().ecrire();
    let mut etats = vec![depart.clone()];
    let solides: Vec<u64> = s.doc().tous_les_solides().map(|(_, s)| s.id).take(5).collect();
    let mut cmds = vec![Commande::CreerBloc { min: [0.0; 3], max: [32.0; 3], materiau: "x".into(), classe: Some("func_detail".into()) }];
    for (i, id) in solides.iter().enumerate() {
        cmds.push(Commande::Deplacer { objets: vec![Ref::Solide(*id)], delta: [i as f64 * 8.0, 0.0, 16.0], verrou_texture: i % 2 == 0 });
        cmds.push(Commande::Tourner { objets: vec![Ref::Solide(*id)], centre: [0.0; 3], axe: [0.0, 0.0, 1.0], degres: 45.0, verrou_texture: true });
    }
    cmds.push(Commande::Supprimer { objets: solides.iter().take(2).map(|i| Ref::Solide(*i)).collect() });
    for c in cmds {
        s.executer(c).unwrap();
        etats.push(s.doc().ecrire());
    }
    // l'historique se parcourt dans les deux sens et retombe sur chaque état
    for i in (0..etats.len()).rev() {
        s.aller_a(i);
        assert!(s.doc().ecrire() == etats[i], "état {i}");
    }
    s.aller_a(etats.len() - 1);
    assert!(s.doc().ecrire() == *etats.last().unwrap());
    // une nouvelle commande après des annulations efface ce qui pouvait être rétabli
    s.aller_a(3);
    s.executer(Commande::CreerBloc { min: [0.0; 3], max: [8.0; 3], materiau: "x".into(), classe: None }).unwrap();
    assert!(s.retablir().is_none());
}

#[test]
fn commandes_en_json() {
    let c = Commande::Deplacer { objets: vec![Ref::Solide(12), Ref::Entite(3)], delta: [1.0, 2.0, 3.0], verrou_texture: true };
    let j = serde_json::to_string(&c).unwrap();
    assert_eq!(j, r#"{"commande":"deplacer","objets":[{"type":"solide","id":12},{"type":"entite","id":3}],"delta":[1.0,2.0,3.0],"verrou_texture":true}"#);
    let relu: Commande = serde_json::from_str(r#"{"commande":"deplacer","objets":[{"type":"solide","id":12}],"delta":[1,2,3]}"#).unwrap();
    assert!(matches!(relu, Commande::Deplacer { verrou_texture: true, .. }));
}

/// 200 commandes tirées au hasard sur une vraie map, puis l'historique parcouru en arrière et en avant : chaque
/// état retrouvé à l'octet.
#[test]
fn au_hasard_sur_une_vraie_map() {
    for carte in ["adminroom.vmf", "dispatch.vmf", "carfall.vmf"] {
        let mut s = fixture(carte);
        let mut graine: u64 = 0x2545_F491_4F6C_DD1D;
        let mut tirer = |n: usize| {
            graine ^= graine << 13;
            graine ^= graine >> 7;
            graine ^= graine << 17;
            (graine % n.max(1) as u64) as usize
        };
        let mut etats = vec![s.doc().ecrire()];
        let mut refusees = 0;
        for _ in 0..200 {
            let solides: Vec<u64> = s.doc().tous_les_solides().map(|(_, s)| s.id).collect();
            let entites: Vec<u64> = s.doc().entites().map(|e| e.id).collect();
            let groupes: Vec<u64> = s.doc().groupes().map(|g| g.id).collect();
            let mut objets = Vec::new();
            for _ in 0..1 + tirer(3) {
                match tirer(3) {
                    0 if !solides.is_empty() => objets.push(Ref::Solide(solides[tirer(solides.len())])),
                    1 if !entites.is_empty() => objets.push(Ref::Entite(entites[tirer(entites.len())])),
                    _ if !groupes.is_empty() => objets.push(Ref::Groupe(groupes[tirer(groupes.len())])),
                    _ => {}
                }
            }
            objets.sort();
            objets.dedup();
            let v = |t: &mut dyn FnMut(usize) -> usize| [t(9) as f64 * 16.0 - 64.0, t(9) as f64 * 16.0 - 64.0, t(5) as f64 * 8.0];
            let c = match tirer(11) {
                0 => Commande::CreerBloc { min: [0.0; 3], max: [16.0 + tirer(4) as f64 * 16.0; 3], materiau: "x".into(), classe: if tirer(2) == 0 { Some("func_detail".into()) } else { None } },
                1 => Commande::Deplacer { objets: objets.clone(), delta: v(&mut tirer), verrou_texture: tirer(2) == 0 },
                2 => Commande::Tourner { objets: objets.clone(), centre: v(&mut tirer), axe: [0.0, 0.0, 1.0], degres: [15.0, 45.0, 90.0, -30.0][tirer(4)], verrou_texture: true },
                3 => Commande::Supprimer { objets: objets.clone() },
                4 => Commande::Dupliquer { objets: objets.clone(), delta: v(&mut tirer) },
                5 => Commande::Grouper { objets: objets.clone() },
                6 if !groupes.is_empty() => Commande::Degrouper { groupes: vec![groupes[tirer(groupes.len())]] },
                7 => Commande::LierAEntite { objets: objets.clone(), classe: "func_brush".into() },
                8 if !entites.is_empty() => Commande::Cles { entite: entites[tirer(entites.len())], cles: vec![("hm_essai".into(), Some(tirer(100).to_string()))] },
                9 => Commande::MateriauObjets { objets: objets.clone(), materiau: "DEV/DEV_MEASUREWALL01A".into() },
                _ => Commande::Miroir { objets: objets.clone(), centre: v(&mut tirer), axe: tirer(3) },
            };
            match s.executer(c) {
                Ok(_) => {
                    let o = s.doc().ecrire();
                    if o != *etats.last().unwrap() {
                        etats.push(o);
                    }
                }
                Err(_) => refusees += 1,
            }
            assert_eq!(s.historique().faits().len() + 1, etats.len());
        }
        for i in (0..etats.len()).rev() {
            s.aller_a(i);
            assert!(s.doc().ecrire() == etats[i], "{carte} : état {i}");
        }
        for i in 0..etats.len() {
            s.aller_a(i);
            assert!(s.doc().ecrire() == etats[i], "{carte} : état {i} au retour");
        }
        // l'état final se relit à l'identique
        let fin = etats.last().unwrap();
        assert!(Session::depuis_octets(fin, None).unwrap().doc().ecrire() == *fin);
        eprintln!("{carte} : {} commandes faites, {refusees} refusées", etats.len() - 1);
    }
}

#[test]
fn evider_un_bloc() {
    let mut s = Session::nouvelle();
    let b = s.executer(Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [512.0, 256.0, 128.0], materiau: "BRICK/BRICKWALL003A".into(), classe: None }).unwrap().crees[0];
    let r = faire(&mut s, Commande::Evider { objets: vec![b], epaisseur: 16.0 });
    assert_eq!(r.crees.len(), 6);
    assert_eq!(s.doc().solides_monde().count(), 6);
    // les murs remplissent exactement la coquille : volume total = boîte − intérieur
    let vol: f64 = s.doc().solides_monde().map(|x| {
        let (lo, hi) = x.boite().unwrap();
        let d = hi - lo;
        d.x * d.y * d.z
    }).sum();
    assert!((vol - (512.0 * 256.0 * 128.0 - 480.0 * 224.0 * 96.0)).abs() < 1e-6);
    assert!(s.doc().solides_monde().all(|x| x.valide()));
    // trop mince : refusé
    let c = s.executer(Commande::CreerBloc { min: [0.0; 3], max: [16.0, 64.0, 64.0], materiau: "x".into(), classe: None }).unwrap().crees[0];
    assert!(s.executer(Commande::Evider { objets: vec![c], epaisseur: 8.0 }).is_err());
}

#[test]
fn couleur_d_editeur() {
    let mut s = Session::nouvelle();
    let a = faire(&mut s, Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: "dev/dev_measuregeneric01".into(), classe: None }).crees[0];
    let b = faire(&mut s, Commande::CreerBloc { min: [100.0, 0.0, 0.0], max: [164.0, 64.0, 64.0], materiau: "dev/dev_measuregeneric01".into(), classe: None }).crees[0];
    let g = faire(&mut s, Commande::Grouper { objets: vec![a, b] }).crees[0];
    // un groupe : ses deux solides prennent la couleur
    faire(&mut s, Commande::CouleurEditeur { objets: vec![g], couleur: [255, 32, 7] });
    for r in [a, b] {
        let Ref::Solide(id) = r else { panic!() };
        assert_eq!(s.doc().solide(id).unwrap().couleur(), [255, 32, 7]);
    }
}
