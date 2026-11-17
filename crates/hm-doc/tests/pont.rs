//! Le pont fenêtre ↔ MCP : une « fenêtre » (une session servie par un fil) s'annonce ; un client la trouve par le
//! fichier de sa map, y exécute une commande (la version monte, le document relu la contient), et un client qui n'a
//! pas le jeton est raccroché sans rien obtenir.
use hm_doc::pont::{self, Demande, Reponse};
use hm_doc::{Commande, Session};

#[test]
fn fenetre_trouvee_commande_et_jeton() {
    let dir = std::env::temp_dir().join(format!("hm_pont_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // seul test de ce fichier : la variable ne touche aucun autre test
    std::env::set_var("LOCALAPPDATA", &dir);
    let carte = dir.join("salle.vmf");
    let mut s = Session::nouvelle();
    s.enregistrer(Some(&carte)).unwrap();
    let ecoute = pont::ecouter(|| {}).unwrap();
    std::thread::spawn(move || {
        for (d, rep) in ecoute.demandes.iter() {
            let mut r = Reponse::default();
            match d {
                Demande::Commande { commande } => match s.executer(commande) {
                    Ok(x) => r.crees = x.crees,
                    Err(e) => r.erreur = Some(e),
                },
                Demande::Document => r.vmf = Some(String::from_utf8_lossy(&s.doc().ecrire()).into_owned()),
                _ => {}
            }
            r.carte = s.chemin().map(|p| p.to_path_buf());
            r.version = s.version();
            rep.send(r).unwrap();
        }
    });
    std::fs::write(dir.join("autre.vmf"), "").unwrap();
    assert!(pont::fenetre_de(&dir.join("autre.vmf")).is_none(), "une autre map : pas de fenêtre");
    let (mut c, etat) = pont::fenetre_de(&carte).expect("fenêtre trouvée par sa map");
    let r = c.demander(Demande::Commande { commande: Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: None } }).unwrap();
    assert!(r.erreur.is_none(), "{:?}", r.erreur);
    assert_eq!(r.crees.len(), 1);
    assert!(r.version > etat.version);
    let vmf = c.demander(Demande::Document).unwrap().vmf.unwrap();
    let relu = Session::depuis_octets(vmf.as_bytes(), None).unwrap();
    assert_eq!(relu.doc().solides_monde().count(), 1, "le bloc est dans le document relu");
    // sans le bon jeton : raccroché, rien obtenu
    let annonce: pont::Annonce = serde_json::from_slice(&std::fs::read(pont::dossier().join(format!("{}.json", std::process::id()))).unwrap()).unwrap();
    let mut intrus = pont::Client::connecter(&pont::Annonce { jeton: "faux".into(), ..annonce }).unwrap();
    assert!(intrus.demander(Demande::Document).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
