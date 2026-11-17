//! Entrées et sorties : lecture des deux séparateurs, écriture dans celui de l'entité ou de la map, texte d'origine
//! gardé pour une sortie inchangée, entrées retrouvées depuis les sorties des autres, refus sans trace.
use hm_doc::io::{lire, Sortie, ESC};
use hm_doc::{Commande, Ref, Session};

fn s(sortie: &str, cible: &str, entree: &str, parametre: &str, delai: f64, fois: i32) -> Sortie {
    Sortie { sortie: sortie.into(), cible: cible.into(), entree: entree.into(), parametre: parametre.into(), delai, fois }
}

#[test]
fn lire_les_deux_separateurs() {
    assert_eq!(lire("OnPressed", "train_01,SetSpeed,50,0,-1"), s("OnPressed", "train_01", "SetSpeed", "50", 0.0, -1));
    assert_eq!(lire("OnEqualTo", "indlight\u{1b}Color\u{1b}0 255 0\u{1b}0\u{1b}-1"), s("OnEqualTo", "indlight", "Color", "0 255 0", 0.0, -1));
    assert_eq!(lire("OnTrigger", "bob,ToggleSprite,,3.14,1"), s("OnTrigger", "bob", "ToggleSprite", "", 3.14, 1));
}

fn entite(sess: &mut Session, classe: &str, nom: &str) -> u64 {
    match sess.executer(Commande::CreerEntite { classe: classe.into(), origine: [0.0; 3], cles: vec![("targetname".into(), nom.into())] }).unwrap().crees[0] {
        Ref::Entite(i) => i,
        _ => unreachable!(),
    }
}

#[test]
fn ecrire_garder_retrouver() {
    let mut sess = Session::nouvelle();
    let bouton = entite(&mut sess, "func_button", "bouton");
    let lampe = entite(&mut sess, "light", "lampe");
    // map neuve : ESC (comme Hammer++ et les maps livrées avec Garry's Mod)
    let liste = vec![s("OnPressed", "lampe", "Toggle", "", 0.0, -1), s("OnPressed", "lampe", "SetPattern", "az", 1.5, 1)];
    let avant = sess.doc().ecrire();
    sess.executer(Commande::Sorties { entite: bouton, sorties: liste.clone() }).unwrap();
    let t = String::from_utf8_lossy(&sess.doc().ecrire()).into_owned();
    assert!(t.contains("\"OnPressed\" \"lampe\u{1b}Toggle\u{1b}\u{1b}0\u{1b}-1\""), "{t}");
    assert_eq!(sess.doc().entite(bouton).unwrap().sorties_lues(), liste);
    // retrouvées comme entrées de la lampe
    let e = sess.doc().entrees_vers(lampe);
    assert_eq!(e.len(), 2);
    assert!(e.iter().all(|(src, _)| *src == bouton));
    sess.annuler();
    assert!(sess.doc().ecrire() == avant);
    sess.retablir();
    // relu, puis réécrit à l'identique
    let relu = Session::depuis_octets(&sess.doc().ecrire(), None).unwrap();
    assert!(relu.doc().ecrire() == sess.doc().ecrire());
    assert_eq!(sess.doc().noms(), vec!["bouton".to_string(), "lampe".to_string()]);
    // un paramètre refusé ne laisse rien
    let apres = sess.doc().ecrire();
    assert!(sess.executer(Commande::Sorties { entite: bouton, sorties: vec![s("OnPressed", "lampe", "Toggle", "a\"b", 0.0, -1)] }).is_err());
    assert!(sess.executer(Commande::Sorties { entite: bouton, sorties: vec![s("OnPressed", "lampe", "Toggle", "", -1.0, -1)] }).is_err());
    assert!(sess.executer(Commande::Sorties { entite: bouton, sorties: vec![s("OnPressed", "lampe", "Toggle", "", 0.0, 0)] }).is_err());
    assert!(sess.doc().ecrire() == apres);
}

#[test]
fn une_map_a_virgules_garde_ses_virgules() {
    let p = std::path::Path::new("C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/garrysmod/gamemodes/terrortown/mapexamples/ttt_traps.vmf");
    let Ok(mut sess) = Session::ouvrir(p) else { return };
    let (id, sorties) = sess.doc().entites().find(|e| e.sorties.as_ref().map_or(false, |v| v.len() >= 2)).map(|e| (e.id, e.sorties_lues())).unwrap();
    let original = sess.doc().ecrire();
    // réécrire les mêmes sorties : rien ne change, à l'octet
    sess.executer(Commande::Sorties { entite: id, sorties: sorties.clone() }).unwrap();
    assert!(sess.doc().ecrire() == original);
    // en ajouter une : elle s'écrit avec des virgules, les autres gardent leur texte
    let mut plus = sorties.clone();
    plus.push(s("OnPressed", "porte", "Open", "", 2.0, -1));
    sess.executer(Commande::Sorties { entite: id, sorties: plus }).unwrap();
    let e = sess.doc().entite(id).unwrap();
    let derniere = e.sorties.as_ref().unwrap().last().unwrap();
    assert_eq!(derniere.1, "porte,Open,,2,-1");
    assert!(!derniere.1.contains(ESC));
}
