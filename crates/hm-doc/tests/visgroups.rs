//! Visgroups (créer, ajouter, retirer, cocher, renommer, supprimer) et cordon : effet, annulation à l'octet,
//! relecture du fichier écrit.
use hm_doc::{Commande, Ref, Session};

/// Exécute, vérifie annuler / rétablir à l'octet et la relecture, et rend le résultat.
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

fn id(r: Ref) -> u64 {
    match r {
        Ref::Solide(i) | Ref::Entite(i) | Ref::Groupe(i) => i,
    }
}

fn texte(s: &Session) -> String {
    String::from_utf8_lossy(&s.doc().ecrire()).into_owned()
}

#[test]
fn creer_cocher_decocher() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let b = bloc(&mut s, [128.0, 0.0, 0.0], [192.0, 64.0, 64.0]);
    let e = s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [32.0, 32.0, 65.0], cles: vec![] }).unwrap().crees[0];

    faire(&mut s, Commande::CreerVisgroup { nom: "Murs".into(), objets: vec![a, e], cache: false });
    let vg = s.doc().visgroups();
    assert_eq!(vg.len(), 1);
    assert_eq!((vg[0].id, vg[0].nom.as_str(), vg[0].parent), (1, "Murs", 0));
    assert_eq!(hm_doc::visgroups::de(&s.doc().solide(id(a)).unwrap().editeur), vec![1]);
    assert!(hm_doc::visgroups::de(&s.doc().solide(id(b)).unwrap().editeur).is_empty());

    // décoché : ses objets passent dans des blocs `hidden`, que vbsp ignore
    faire(&mut s, Commande::MontrerVisgroup { visgroup: 1, visible: false });
    assert!(s.doc().solide(id(a)).unwrap().cache);
    assert!(s.doc().entite(id(e)).unwrap().cache);
    assert!(!s.doc().solide(id(b)).unwrap().cache);
    let t = texte(&s);
    assert_eq!(t.matches("hidden\r\n").count() + t.matches("hidden\n").count(), 2, "{t}");
    // relu : toujours cachés, toujours dans le visgroup
    let relu = Session::depuis_octets(&s.doc().ecrire(), None).unwrap();
    assert!(relu.doc().solide(id(a)).unwrap().cache);
    assert!(relu.doc().entite(id(e)).unwrap().cache);
    assert_eq!(relu.doc().visgroups(), s.doc().visgroups());

    // un objet retiré d'un visgroup décoché redevient visible
    faire(&mut s, Commande::RetirerDuVisgroup { visgroup: 1, objets: vec![e] });
    assert!(!s.doc().entite(id(e)).unwrap().cache);
    assert!(s.doc().solide(id(a)).unwrap().cache);
    // ajouté à un visgroup décoché, il reste visible jusqu'au prochain décochage ; on le recoche
    faire(&mut s, Commande::AjouterAuVisgroup { visgroup: 1, objets: vec![b] });
    faire(&mut s, Commande::MontrerVisgroup { visgroup: 1, visible: true });
    assert!(!s.doc().solide(id(a)).unwrap().cache && !s.doc().solide(id(b)).unwrap().cache);
    assert!(!texte(&s).contains("hidden"));

    faire(&mut s, Commande::RenommerVisgroup { visgroup: 1, nom: "Murs du hall".into() });
    assert_eq!(s.doc().visgroups()[0].nom, "Murs du hall");

    // supprimé : les objets restent, visibles, sans appartenance
    faire(&mut s, Commande::MontrerVisgroup { visgroup: 1, visible: false });
    faire(&mut s, Commande::SupprimerVisgroup { visgroup: 1 });
    assert!(s.doc().visgroups().is_empty());
    assert!(!s.doc().solide(id(a)).unwrap().cache);
    assert!(hm_doc::visgroups::de(&s.doc().solide(id(a)).unwrap().editeur).is_empty());
    assert!(!texte(&s).contains("hidden"));

    // refus sans trace
    let avant = s.doc().ecrire();
    assert!(s.executer(Commande::MontrerVisgroup { visgroup: 42, visible: false }).is_err());
    assert!(s.executer(Commande::AjouterAuVisgroup { visgroup: 42, objets: vec![a] }).is_err());
    assert!(s.executer(Commande::SupprimerVisgroup { visgroup: 42 }).is_err());
    assert!(s.doc().ecrire() == avant);
}

#[test]
fn creer_un_visgroup_deja_decoche() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let b = bloc(&mut s, [128.0, 0.0, 0.0], [192.0, 64.0, 64.0]);
    faire(&mut s, Commande::CreerVisgroup { nom: "2 objets".into(), objets: vec![a, b], cache: true });
    assert!(s.doc().solide(id(a)).unwrap().cache && s.doc().solide(id(b)).unwrap().cache);
    let m = s.doc().membres_visgroups();
    assert_eq!(m[&1], vec![(a, true), (b, true)]);
}

#[test]
fn groupe_dans_un_visgroup() {
    let mut s = Session::nouvelle();
    let a = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let b = bloc(&mut s, [128.0, 0.0, 0.0], [192.0, 64.0, 64.0]);
    let g = s.executer(Commande::Grouper { objets: vec![a, b] }).unwrap().crees[0];
    faire(&mut s, Commande::CreerVisgroup { nom: "Groupe".into(), objets: vec![g], cache: false });
    faire(&mut s, Commande::MontrerVisgroup { visgroup: 1, visible: false });
    assert!(s.doc().solide(id(a)).unwrap().cache && s.doc().solide(id(b)).unwrap().cache);
}

#[test]
fn monter_et_descendre_un_visgroup() {
    // A (enfants A1, A2) et B : on ne bouge que parmi ceux de même parent
    let src = "versioninfo\n{\n}\nvisgroups\n{\n\tvisgroup\n\t{\n\t\t\"name\" \"A\"\n\t\t\"visgroupid\" \"1\"\n\t\t\"color\" \"255 0 0\"\n\t\tvisgroup\n\t\t{\n\t\t\t\"name\" \"A1\"\n\t\t\t\"visgroupid\" \"2\"\n\t\t\t\"color\" \"0 255 0\"\n\t\t}\n\t\tvisgroup\n\t\t{\n\t\t\t\"name\" \"A2\"\n\t\t\t\"visgroupid\" \"3\"\n\t\t\t\"color\" \"0 0 255\"\n\t\t}\n\t}\n\tvisgroup\n\t{\n\t\t\"name\" \"B\"\n\t\t\"visgroupid\" \"4\"\n\t\t\"color\" \"9 9 9\"\n\t}\n}\nworld\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n}\n";
    let mut s = Session::depuis_octets(src.as_bytes(), None).unwrap();
    let noms = |s: &Session| s.doc().visgroups().iter().map(|v| v.nom.clone()).collect::<Vec<_>>().join(" ");
    assert_eq!(noms(&s), "A A1 A2 B");
    faire(&mut s, Commande::DeplacerVisgroup { visgroup: 3, monter: true });
    assert_eq!(noms(&s), "A A2 A1 B");
    faire(&mut s, Commande::DeplacerVisgroup { visgroup: 1, monter: false });
    assert_eq!(noms(&s), "B A A2 A1");
    assert_eq!(s.doc().visgroups().iter().find(|v| v.nom == "A1").unwrap().parent, 1, "les enfants suivent leur parent");
    assert!(s.executer(Commande::DeplacerVisgroup { visgroup: 4, monter: true }).is_err(), "déjà le premier");
    assert!(s.executer(Commande::DeplacerVisgroup { visgroup: 2, monter: false }).is_err(), "déjà le dernier de ses frères");
    assert!(s.executer(Commande::DeplacerVisgroup { visgroup: 99, monter: true }).is_err());
}

#[test]
fn map_sans_bloc_visgroups_ni_cordons() {
    // un fichier minimal (comme ceux de BSPSource) : relu à l'octet, puis les commandes trouvent où écrire
    let src = b"versioninfo\n{\n\t\"editorversion\" \"400\"\n}\nworld\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n}\n";
    let mut s = Session::depuis_octets(src, None).unwrap();
    assert!(s.doc().ecrire() == src.to_vec());
    let a = bloc(&mut s, [0.0; 3], [64.0; 3]);
    faire(&mut s, Commande::CreerVisgroup { nom: "Nouveau".into(), objets: vec![a], cache: false });
    faire(&mut s, Commande::Cordon { min: [-128.0; 3], max: [128.0; 3], actif: true });
    let t = texte(&s);
    assert!(t.contains("visgroups") && t.contains("cordons"), "{t}");
}

#[test]
fn cordon_ne_compile_que_ce_qui_touche_la_boite() {
    let mut s = Session::nouvelle();
    let dedans = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let a_cheval = bloc(&mut s, [100.0, 0.0, 0.0], [300.0, 64.0, 64.0]);
    let dehors = bloc(&mut s, [1000.0, 0.0, 0.0], [1064.0, 64.0, 64.0]);
    let e_dedans = s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [32.0, 32.0, 65.0], cles: vec![] }).unwrap().crees[0];
    let e_dehors = s.executer(Commande::CreerEntite { classe: "light".into(), origine: [1032.0, 32.0, 100.0], cles: vec![] }).unwrap().crees[0];

    // sans cordon actif : le fichier tel quel
    assert!(s.doc().pour_compilation("tools/toolsskybox") == s.doc().ecrire());
    assert!(s.executer(Commande::Cordon { min: [0.0; 3], max: [256.0, 256.0, 0.0], actif: true }).is_err(), "boîte plate");
    faire(&mut s, Commande::Cordon { min: [-128.0, -128.0, -128.0], max: [256.0, 256.0, 256.0], actif: true });
    let c = s.doc().cordons();
    assert!(c.actif && c.liste.len() == 1 && c.liste[0].actif);
    assert_eq!(c.liste[0].boites, vec![[[-128.0; 3], [256.0; 3]]]);

    let compile = Session::depuis_octets(&s.doc().pour_compilation("tools/toolsskybox"), None).unwrap();
    let d = compile.doc();
    assert!(d.solide(id(dedans)).is_some() && d.solide(id(a_cheval)).is_some());
    assert!(d.solide(id(dehors)).is_none());
    assert!(d.entite(id(e_dedans)).is_some() && d.entite(id(e_dehors)).is_none());
    // six murs de la texture de cordon autour de la boîte
    let murs: Vec<_> = d.tous_les_solides().filter(|(_, s)| s.faces.iter().all(|f| f.materiau == "tools/toolsskybox")).collect();
    assert_eq!(murs.len(), 6);
    let (lo, hi) = murs.iter().fold((glam::DVec3::splat(f64::MAX), glam::DVec3::splat(f64::MIN)), |(lo, hi), (_, s)| {
        let (a, b) = s.boite().unwrap();
        (lo.min(a), hi.max(b))
    });
    assert_eq!((lo.to_array(), hi.to_array()), ([-144.0; 3], [272.0; 3]));
    assert!(murs.iter().all(|(_, s)| s.valide()));
    // le document lui-même n'a pas changé
    assert!(s.doc().solide(id(dehors)).is_some());

    // désactivé : tout est compilé
    faire(&mut s, Commande::Cordon { min: [-128.0; 3], max: [256.0; 3], actif: false });
    assert!(s.doc().pour_compilation("tools/toolsskybox") == s.doc().ecrire());
}

#[test]
fn plusieurs_cordons() {
    // deux salles éloignées et un bloc entre elles : chaque cordon coché garde ce qui touche une de ses boîtes
    let mut s = Session::nouvelle();
    let ouest = bloc(&mut s, [0.0; 3], [64.0; 3]);
    let est = bloc(&mut s, [2000.0, 0.0, 0.0], [2064.0, 64.0, 64.0]);
    let nord = bloc(&mut s, [0.0, 2000.0, 0.0], [64.0, 2064.0, 64.0]);
    let milieu = bloc(&mut s, [1000.0, 0.0, 0.0], [1064.0, 64.0, 64.0]);
    let boite = |lo: f64, y: f64| [[lo - 64.0, y - 64.0, -64.0], [lo + 128.0, y + 128.0, 128.0]];
    let cordon = |nom: &str, actif: bool, boites: Vec<[[f64; 3]; 2]>| hm_doc::Cordon { nom: nom.into(), actif, boites };
    // un cordon de deux boîtes (ouest, est) et un cordon décoché (nord)
    let liste = vec![cordon("salles", true, vec![boite(0.0, 0.0), boite(2000.0, 0.0)]), cordon("nord", false, vec![boite(0.0, 2000.0)])];
    faire(&mut s, Commande::Cordons { actif: true, liste: liste.clone() });
    assert_eq!(s.doc().cordons(), hm_doc::Cordons { actif: true, liste: liste.clone() }, "relu tel quel");
    let garde = |s: &Session| {
        let d = Session::depuis_octets(&s.doc().pour_compilation("tools/toolsskybox"), None).unwrap();
        let murs = d.doc().tous_les_solides().filter(|(_, x)| x.faces.iter().all(|f| f.materiau == "tools/toolsskybox")).count();
        ([ouest, est, nord, milieu].map(|r| d.doc().solide(id(r)).is_some()), murs)
    };
    assert_eq!(garde(&s), ([true, true, false, false], 12), "six murs par boîte");
    // cocher le nord
    let mut l2 = liste.clone();
    l2[1].actif = true;
    faire(&mut s, Commande::Cordons { actif: true, liste: l2 });
    assert_eq!(garde(&s), ([true, true, true, false], 18));
    // état du cordon coupé : tout est compilé
    faire(&mut s, Commande::Cordons { actif: false, liste: liste.clone() });
    assert!(s.doc().pour_compilation("tools/toolsskybox") == s.doc().ecrire());
    // une boîte plate est refusée, avec le nom de son cordon
    let e = s.executer(Commande::Cordons { actif: true, liste: vec![cordon("plat", true, vec![[[0.0; 3], [64.0, 64.0, 0.0]]])] }).unwrap_err();
    assert!(e.contains("plat"), "{e}");
    // `cordon` ne touche que la première boîte du premier cordon
    faire(&mut s, Commande::Cordon { min: [-32.0; 3], max: [96.0; 3], actif: true });
    let c = s.doc().cordons();
    assert_eq!((c.liste.len(), c.liste[0].boites[0], c.liste[0].boites[1]), (2, [[-32.0; 3], [96.0; 3]], boite(2000.0, 0.0)));
    // tout supprimer : la liste vide est relue vide
    faire(&mut s, Commande::Cordons { actif: false, liste: vec![] });
    assert!(s.doc().cordons().liste.is_empty());
}

#[test]
fn cordon_du_hammer_d_origine() {
    // le bloc `cordon` des maps du Hammer d'origine (une boîte) se lit, et reste d'accord avec la liste écrite
    let src = b"versioninfo\n{\n}\nworld\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n}\ncordon\n{\n\t\"mins\" \"(-1024 -1024 -1024)\"\n\t\"maxs\" \"(1024 1024 1024)\"\n\t\"active\" \"1\"\n}\n";
    let mut s = Session::depuis_octets(src, None).unwrap();
    assert!(s.doc().ecrire() == src.to_vec());
    let c = s.doc().cordons();
    assert!(c.actif);
    assert_eq!(c.liste[0].boites, vec![[[-1024.0; 3], [1024.0; 3]]]);
    faire(&mut s, Commande::Cordon { min: [0.0; 3], max: [512.0; 3], actif: false });
    let t = texte(&s);
    assert!(t.contains("\"mins\" \"(0 0 0)\"") && t.contains("\"active\" \"0\""), "{t}");
    assert!(!s.doc().cordons().actif);
}

#[test]
fn cocher_un_visgroup_d_une_vraie_map() {
    // une map de Hammer++ du PC, avec un visgroup « Propper » ; absente ailleurs : test sauté
    let p = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Desktop/map_mainlife_dark/map/gm.vmf");
    let Ok(mut s) = Session::ouvrir(&p) else { return };
    let vg = s.doc().visgroups();
    let Some(v) = vg.first().cloned() else { return };
    let membres: Vec<u64> = s.doc().entites().filter(|e| e.editeur.as_ref().map_or(false, |ed| hm_doc::visgroups::de(ed).contains(&v.id))).map(|e| e.id).collect();
    assert!(!membres.is_empty());
    faire(&mut s, Commande::MontrerVisgroup { visgroup: v.id, visible: false });
    assert!(membres.iter().all(|i| s.doc().entite(*i).unwrap().cache));
    let t = texte(&s);
    assert!(t.matches("hidden").count() >= membres.len());
    faire(&mut s, Commande::MontrerVisgroup { visgroup: v.id, visible: true });
    // recoché : le fichier redevient celui de Hammer++, à l'octet
    assert!(s.doc().ecrire() == std::fs::read(p).unwrap());
}

#[test]
fn camera_active_du_bloc_cameras() {
    // le bloc `cameras` de Hammer : la caméra n° activecamera (rp_rockford l'a en 0) ; -1 ou absente : aucune
    let vmf = "world\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n}\ncameras\n{\n\t\"activecamera\" \"1\"\n\tcamera\n\t{\n\t\t\"position\" \"[0 0 0]\"\n\t\t\"look\" \"[1 0 0]\"\n\t}\n\tcamera\n\t{\n\t\t\"position\" \"[-4948.12 -2661.49 791.474]\"\n\t\t\"look\" \"[-12865.8 -10746.5 -7430.22]\"\n\t}\n}\n";
    let (d, _) = hm_doc::Document::lire(vmf.as_bytes()).unwrap();
    assert_eq!(d.camera_active(), Some(([-4948.12, -2661.49, 791.474], [-12865.8, -10746.5, -7430.22])));
    let (d, _) = hm_doc::Document::lire(vmf.replace("\"activecamera\" \"1\"", "\"activecamera\" \"-1\"").as_bytes()).unwrap();
    assert_eq!(d.camera_active(), None);
    let (d, _) = hm_doc::Document::lire(b"world\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n}\n").unwrap();
    assert_eq!(d.camera_active(), None);
}
