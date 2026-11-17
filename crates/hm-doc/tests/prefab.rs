//! Préfabriqués : enregistrés ramenés à l'origine (milieu du dessous), insérés au point voulu, jeton `&i` numéroté
//! à chaque insertion (noms et sorties), annulation à l'octet.
use glam::DVec3;
use hm_doc::io::Sortie;
use hm_doc::{boite_de, presse, Commande, Ref, Session};

fn entite(r: &Ref) -> u64 {
    match r {
        Ref::Entite(i) => *i,
        _ => panic!("{r:?}"),
    }
}

#[test]
fn enregistrer_puis_inserer_deux_fois() {
    // la source : une porte nommée porte_&i et un déclencheur qui l'ouvre, loin de l'origine
    let mut a = Session::nouvelle();
    let porte = a.executer(Commande::CreerBloc { min: [1000.0, 1000.0, 64.0], max: [1064.0, 1008.0, 192.0], materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: Some("func_door".into()) }).unwrap().crees[0];
    a.executer(Commande::Cles { entite: entite(&porte), cles: vec![("targetname".into(), Some("porte_&i".into()))] }).unwrap();
    let declic = a.executer(Commande::CreerBloc { min: [1000.0, 960.0, 64.0], max: [1064.0, 990.0, 128.0], materiau: "TOOLS/TOOLSTRIGGER".into(), classe: Some("trigger_multiple".into()) }).unwrap().crees[0];
    a.executer(Commande::Sorties { entite: entite(&declic), sorties: vec![Sortie { sortie: "OnStartTouch".into(), cible: "porte_&i".into(), entree: "Open".into(), parametre: String::new(), delai: 0.0, fois: -1 }] }).unwrap();
    let vmf = presse::prefabriquer(a.doc(), &[porte, declic]).unwrap();
    // ramené : le milieu du dessous à l'origine
    let p = Session::depuis_octets(vmf.as_bytes(), None).unwrap();
    let tous: Vec<Ref> = p.doc().entites().map(|e| Ref::Entite(e.id)).collect();
    let (lo, hi) = boite_de(p.doc(), &tous).unwrap();
    assert!(((lo + hi) / 2.0).truncate().length() < 1e-6 && lo.z.abs() < 1e-6, "{lo:?} {hi:?}");

    // inséré deux fois dans une autre map
    let mut b = Session::nouvelle();
    let avant = b.doc().ecrire();
    b.executer(Commande::InsererPrefab { vmf: vmf.clone(), origine: [256.0, 0.0, 0.0] }).unwrap();
    b.annuler();
    assert!(b.doc().ecrire() == avant);
    b.retablir();
    b.executer(Commande::InsererPrefab { vmf, origine: [512.0, 0.0, 32.0] }).unwrap();
    let noms = b.doc().noms();
    assert_eq!(noms, vec!["porte_1".to_string(), "porte_2".to_string()]);
    let cibles: Vec<String> = b.doc().entites().flat_map(|e| e.sorties_lues()).map(|s| s.cible).collect();
    assert_eq!(cibles.len(), 2);
    assert!(cibles.contains(&"porte_1".to_string()) && cibles.contains(&"porte_2".to_string()));
    // la seconde porte : posée en (512, 0, 32), au-dessus de son origine comme dans la source
    let p2 = b.doc().entites().find(|e| e.nom() == "porte_2").unwrap().id;
    let (lo, hi) = boite_de(b.doc(), &[Ref::Entite(p2)]).unwrap();
    assert!((((lo + hi) / 2.0).x - 512.0).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!((lo.z - (32.0 + 0.0)).abs() < 1e-6 || lo.z > 32.0);
    let _ = DVec3::ZERO;
}
