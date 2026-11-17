//! Semer au pinceau : types lus au format de Hammer++, une entité par case libre sous le pinceau, posée sur la
//! surface ; repasser ne double rien ; retirer enlève ce type seulement.
use glam::DVec3;
use hm_doc::{Commande, Session};
use hm_edit::semis::{cases, lire_types, retirer, semer, Pinceau};

const EXEMPLE: &str = r#""Example: Jungle Foliage" //This is what shows up in Type
{
	base //The base values
	{
		"classname" "prop_static"
		grid "64 64" // The default grid size
	}
	10 //chance
	{
		"model" "models/props_island/island_plant01.mdl"
		"ignorenormals" "1"
	}
	30
	{
		"model" "models/props_island/island_plant02.mdl"
		"skin" "1"
	}
}
"Rochers"
{
	base { grid "128 96" }
	1 { "model" "models/props_wasteland/rockgranite02a.mdl" }
}
"#;

#[test]
fn types_de_hammerpp() {
    let t = lire_types(EXEMPLE);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].nom, "Example: Jungle Foliage");
    assert_eq!(t[0].grille, [64.0, 64.0]);
    assert_eq!(t[0].base, [("classname".to_string(), "prop_static".to_string())]);
    assert_eq!(t[0].cas.len(), 2);
    assert_eq!(t[1].grille, [128.0, 96.0]);
    // le tirage suit les poids : 10 contre 30
    let n = (0..1000).filter(|i| t[0].tirer(*i as f64 / 1000.0).iter().any(|(_, v)| v.ends_with("plant01.mdl"))).count();
    assert_eq!(n, 250);
    assert!(t[0].tirer(0.9).contains(&("skin".to_string(), "1".to_string())));
}

#[test]
fn semer_puis_retirer() {
    let t = &lire_types(EXEMPLE)[0];
    let mut s = Session::nouvelle();
    s.executer(Commande::CreerBloc { min: [0.0, 0.0, -16.0], max: [1024.0, 1024.0, 0.0], materiau: "dev/dev_measuregeneric01".into(), classe: None }).unwrap();
    let p = Pinceau { rayon: 130.0, lacet_hasard: true, ecart: false, aligner: false };
    let centre = DVec3::new(512.0, 512.0, 0.0);
    // centres à ±32 (4 cases, à 45) et ±96 / ±32 (8 cases, à 101) ; ±96 / ±96 (à 136) dehors
    let attendu = cases([512.0, 512.0], 130.0, [64.0, 64.0]).len();
    assert_eq!(attendu, 12);
    let c = semer(s.doc(), t, centre, p, 7);
    assert_eq!(c.len(), attendu, "une entité par case");
    s.executer(Commande::Lot { nom: "semer".into(), commandes: c }).unwrap();
    for e in s.doc().entites() {
        let o = e.origine().unwrap();
        assert!(o.z.abs() < 1e-6, "posée sur le sol : {o}");
        assert!(((o.x / 64.0).fract() - 0.5).abs() < 1e-9, "au milieu de sa case sans écart : {o}");
        assert_eq!(e.classe(), "prop_static");
    }
    // repasser au même endroit : rien de neuf ; un peu à côté : seulement les cases nouvelles
    assert!(semer(s.doc(), t, centre, p, 8).is_empty());
    let plus = semer(s.doc(), t, centre + DVec3::new(64.0, 0.0, 0.0), p, 9);
    assert!(!plus.is_empty() && plus.len() < attendu);
    // retirer : celles du type sous le pinceau, pas le reste
    s.executer(Commande::CreerEntite { classe: "info_target".into(), origine: [512.0, 512.0, 0.0], cles: vec![] }).unwrap();
    let r = retirer(s.doc(), t, centre + DVec3::new(32.0, 0.0, 0.0), 300.0).unwrap();
    s.executer(r).unwrap();
    assert_eq!(s.doc().entites().count(), 1, "seule l'info_target reste");
    // en coordonnées négatives aussi
    s.executer(Commande::CreerBloc { min: [-1024.0, -1024.0, -16.0], max: [0.0, 0.0, 0.0], materiau: "dev/dev_measuregeneric01".into(), classe: None }).unwrap();
    assert_eq!(semer(s.doc(), t, DVec3::new(-512.0, -512.0, 0.0), p, 3).len(), 12);
    // une case sans sol dessous : rien
    assert!(semer(s.doc(), t, DVec3::new(5000.0, 5000.0, 0.0), p, 1).is_empty());
}
