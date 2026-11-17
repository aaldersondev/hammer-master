//! Un aménagement posé sur une boîte : une pose à 10 du mur de gauche et 20 du fond (vus depuis la porte), tournée
//! de 90 ; selon le mur de la porte, elle tombe à la bonne place du monde et au bon lacet ; une boîte trop étroite
//! est refusée ; la commande est annulable d'un coup.
use hm_doc::{Commande, Session};
use hm_model::amenagement::Amenagement;

fn modele() -> Amenagement {
    let d = std::env::temp_dir().join(format!("hm_amenagement_boite_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("essai.toml");
    std::fs::write(&f, "nom = \"essai\"\nrole = \"essai\"\n[piece]\nlargeur = [100, 400]\nprofondeur = [100, 400]\nhauteur_min = 64\n[[ensemble]]\nnom = \"meuble\"\nx = { gauche = 10 }\ny = { fond = 20 }\nlacet = 90\n[[ensemble.pose]]\nmodele = \"models/props_c17/oildrum001.mdl\"\n").unwrap();
    let a = Amenagement::lire(&f).unwrap();
    let _ = std::fs::remove_dir_all(&d);
    a
}

#[test]
fn pose_selon_le_mur_de_la_porte() {
    let a = modele();
    let (lo, hi) = ([0.0, 0.0, 0.0], [200.0, 300.0, 128.0]);
    for (porte, ou, lacet) in [("y-", [10.0, 280.0], 90.0), ("y+", [190.0, 20.0], 270.0), ("x-", [180.0, 290.0], 0.0)] {
        let mut s = Session::nouvelle();
        let c = hm_edit::commande_amenagement_boite(&a, lo, hi, porte, s.doc()).unwrap();
        s.executer(c).unwrap();
        let e = s.doc().entites().find(|e| e.classe() == "prop_static").expect("la pose");
        let o = e.origine().unwrap();
        assert!((o.x - ou[0]).abs() < 0.01 && (o.y - ou[1]).abs() < 0.01 && o.z.abs() < 0.01, "porte {porte} : {o:?} au lieu de {ou:?}");
        assert!((e.angles().y - lacet).abs() < 0.01, "porte {porte} : lacet {}", e.angles().y);
        assert_eq!(e.get("model"), Some("models/props_c17/oildrum001.mdl"));
        // une seule étape d'historique
        s.annuler().unwrap();
        assert_eq!(s.doc().entites().count(), 0);
    }
    // trop étroite pour le modèle (largeur 100 au moins)
    let s = Session::nouvelle();
    let r = hm_edit::commande_amenagement_boite(&a, [0.0, 0.0, 0.0], [50.0, 300.0, 128.0], "y-", s.doc());
    assert!(r.is_err(), "une pièce de 50 de large aurait dû être refusée");
}
