//! Fondre une func_instance dans la map (« Collapse » de Hammer, comme vbsp) : contenu posé selon l'origine et les
//! angles, noms corrigés selon fixup_style (les noms en @ gardés), sorties suivies, paramètres $ remplacés ; une
//! seule étape d'annulation.
use glam::DVec3;
use hm_doc::io::Sortie;
use hm_doc::{boite_de, Commande, Ref, Session};

/// Le fichier d'instance : un bloc 100 × 20 × 10 à l'origine, une porte, un bouton qui l'ouvre, un relais « @fin »
/// dont le paramètre vient de `$vitesse`.
fn fichier_instance() -> String {
    let mut s = Session::nouvelle();
    s.executer(Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [100.0, 20.0, 10.0], materiau: "dev/dev_measuregeneric01".into(), classe: None }).unwrap();
    let mut ids = Vec::new();
    for (nom, x) in [("porte", 0.0), ("bouton", 32.0), ("@fin", 64.0)] {
        let r = s.executer(Commande::CreerEntite { classe: "logic_relay".into(), origine: [x, 0.0, 16.0], cles: vec![("targetname".into(), nom.into()), ("vitesse".into(), "$vitesse".into())] }).unwrap();
        ids.push(r.crees[0]);
    }
    let Ref::Entite(bouton) = ids[1] else { panic!() };
    let vers = |cible: &str| Sortie { sortie: "OnTrigger".into(), cible: cible.into(), entree: "Trigger".into(), parametre: String::new(), delai: 0.0, fois: -1 };
    s.executer(Commande::Sorties { entite: bouton, sorties: vec![vers("porte"), vers("@fin")] }).unwrap();
    String::from_utf8(s.doc().ecrire()).unwrap()
}

#[test]
fn fondre_une_instance() {
    let vmf = fichier_instance();
    let mut s = Session::nouvelle();
    let cles = vec![("targetname".to_string(), "aile".to_string()), ("file".to_string(), "instances/x.vmf".to_string()), ("angles".to_string(), "0 90 0".to_string()), ("replace01".to_string(), "$vitesse 250".to_string())];
    let Ref::Entite(inst) = s.executer(Commande::CreerEntite { classe: "func_instance".into(), origine: [1000.0, 0.0, 0.0], cles }).unwrap().crees[0] else { panic!() };
    let avant = s.doc().ecrire();
    let r = s.executer(Commande::FondreInstance { entite: inst, vmf: vmf.clone() }).unwrap();
    assert!(s.doc().entite(inst).is_none(), "l'instance est retirée");
    // le bloc tourné de 90° autour de Z puis posé en (1000, 0, 0) : x de 980 à 1000, y de 0 à 100
    let solide = r.crees.iter().copied().find(|c| matches!(c, Ref::Solide(_))).unwrap();
    let (lo, hi) = boite_de(s.doc(), &[solide]).unwrap();
    assert!((lo - DVec3::new(980.0, 0.0, 0.0)).length() < 1e-6 && (hi - DVec3::new(1000.0, 100.0, 10.0)).length() < 1e-6, "{lo} {hi}");
    let mut noms = s.doc().noms();
    noms.sort();
    assert_eq!(noms, ["@fin", "aile-bouton", "aile-porte"]);
    let bouton = s.doc().entite(s.doc().entites_nommees("aile-bouton")[0]).unwrap();
    let cibles: Vec<String> = bouton.sorties_lues().into_iter().map(|x| x.cible).collect();
    assert_eq!(cibles, ["aile-porte", "@fin"]);
    assert_eq!(bouton.get("vitesse"), Some("250"));
    let porte = s.doc().entite(s.doc().entites_nommees("aile-porte")[0]).unwrap();
    assert!((porte.origine().unwrap() - DVec3::new(1000.0, 0.0, 16.0)).length() < 1e-6);
    s.annuler().unwrap();
    assert!(s.doc().ecrire() == avant, "une seule étape");
    // suffixe, puis sans correction
    for (style, attendu) in [("1", ["@fin", "bouton-aile", "porte-aile"]), ("2", ["@fin", "bouton", "porte"])] {
        s.executer(Commande::Cles { entite: inst, cles: vec![("fixup_style".into(), Some(style.into()))] }).unwrap();
        s.executer(Commande::FondreInstance { entite: inst, vmf: vmf.clone() }).unwrap();
        let mut noms = s.doc().noms();
        noms.sort();
        assert_eq!(noms, attendu);
        s.annuler().unwrap();
    }
    let Ref::Entite(autre) = s.executer(Commande::CreerEntite { classe: "info_target".into(), origine: [0.0; 3], cles: vec![] }).unwrap().crees[0] else { panic!() };
    assert!(s.executer(Commande::FondreInstance { entite: autre, vmf }).is_err());
}
