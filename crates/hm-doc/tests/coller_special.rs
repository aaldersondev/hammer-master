//! Coller spécial (« Paste Special » de Hammer, wiki Valve) : copies décalées et tournées de façon cumulée autour du
//! centre de l'original, groupées au besoin, noms uniques ou préfixés avec les sorties entre copies suivies ; une
//! seule étape d'annulation.
use glam::DVec3;
use hm_doc::io::Sortie;
use hm_doc::{boite_de, Commande, Ref, Session};

fn special(vmf: &str, copies: u32, decalage: [f64; 3], rotation: [f64; 3], grouper: bool, prefixe: &str, noms_uniques: bool) -> Commande {
    Commande::CollerSpecial { vmf: vmf.into(), copies, decalage, rotation, grouper, prefixe: prefixe.into(), noms_uniques }
}

#[test]
fn copies_decalees_et_tournees() {
    let mut s = Session::nouvelle();
    let b = s.executer(Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [200.0, 40.0, 16.0], materiau: "dev/dev_measuregeneric01".into(), classe: None }).unwrap().crees[0];
    let vmf = hm_doc::presse::copier(s.doc(), &[b]).unwrap();
    let avant = s.doc().ecrire();
    let n = s.historique().faits().len();
    // un escalier : 3 marches, chacune 16 plus haut et 40 plus loin
    let r = s.executer(special(&vmf, 3, [0.0, 40.0, 16.0], [0.0; 3], false, "", false)).unwrap();
    assert_eq!(r.crees.len(), 3);
    assert_eq!(s.historique().faits().len(), n + 1, "une seule étape");
    for (k, c) in r.crees.iter().enumerate() {
        let (lo, _) = boite_de(s.doc(), &[*c]).unwrap();
        let k = (k + 1) as f64;
        assert!((lo - DVec3::new(0.0, 40.0 * k, 16.0 * k)).length() < 1e-6, "copie {k} : {lo}");
    }
    s.annuler().unwrap();
    assert!(s.doc().ecrire() == avant);
    // tournées : 90° autour de Z à chaque copie, autour du centre (100, 20) : 200 × 40 devient 40 × 200, puis revient
    let r = s.executer(special(&vmf, 2, [0.0; 3], [0.0, 0.0, 90.0], true, "", false)).unwrap();
    assert_eq!(r.crees.len(), 1, "groupées : un seul groupe sélectionné");
    assert!(matches!(r.crees[0], Ref::Groupe(_)));
    let membres: Vec<_> = s.doc().tous_les_solides().map(|(_, x)| x.id).filter(|id| Ref::Solide(*id) != b).collect();
    let tailles: Vec<DVec3> = membres.iter().map(|id| {
        let (lo, hi) = boite_de(s.doc(), &[Ref::Solide(*id)]).unwrap();
        hi - lo
    }).collect();
    assert!(tailles.iter().any(|t| (*t - DVec3::new(40.0, 200.0, 16.0)).length() < 1e-6), "{tailles:?}");
    assert!(tailles.iter().any(|t| (*t - DVec3::new(200.0, 40.0, 16.0)).length() < 1e-6), "{tailles:?}");
    assert!(s.executer(special(&vmf, 0, [0.0; 3], [0.0; 3], false, "", false)).is_err());
}

#[test]
fn noms_uniques_et_prefixe_avec_les_sorties() {
    let mut s = Session::nouvelle();
    let bouton = s.executer(Commande::CreerEntite { classe: "logic_relay".into(), origine: [0.0, 0.0, 0.0], cles: vec![("targetname".into(), "bouton".into())] }).unwrap().crees[0];
    let porte = s.executer(Commande::CreerEntite { classe: "logic_relay".into(), origine: [64.0, 0.0, 0.0], cles: vec![("targetname".into(), "porte".into())] }).unwrap().crees[0];
    let Ref::Entite(ib) = bouton else { panic!() };
    let sortie = Sortie { sortie: "OnTrigger".into(), cible: "porte".into(), entree: "Trigger".into(), parametre: String::new(), delai: 0.0, fois: -1 };
    s.executer(Commande::Sorties { entite: ib, sorties: vec![sortie] }).unwrap();
    let vmf = hm_doc::presse::copier(s.doc(), &[bouton, porte]).unwrap();
    s.executer(special(&vmf, 2, [128.0, 0.0, 0.0], [0.0; 3], false, "", true)).unwrap();
    let mut noms = s.doc().noms();
    noms.sort();
    assert_eq!(noms, ["bouton", "bouton_1", "bouton_2", "porte", "porte_1", "porte_2"]);
    // chaque bouton copié vise la porte de sa copie
    for (b, p) in [("bouton_1", "porte_1"), ("bouton_2", "porte_2")] {
        let e = s.doc().entite(s.doc().entites_nommees(b)[0]).unwrap();
        assert_eq!(e.sorties_lues()[0].cible, p);
    }
    s.executer(special(&vmf, 1, [0.0, 128.0, 0.0], [0.0; 3], false, "aile_", false)).unwrap();
    let e = s.doc().entite(s.doc().entites_nommees("aile_bouton")[0]).unwrap();
    assert_eq!(e.sorties_lues()[0].cible, "aile_porte");
}
