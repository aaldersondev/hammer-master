//! Constructeur : ce qu'une spec donne, vérifié sur la géométrie écrite. L'égalité avec le générateur Python du
//! skill, octet pour octet, est vérifiée à part par `tests/construire_differentiel.py` (il faut Python et le jeu).
use std::path::PathBuf;

use hm_model::construire::{construire, Construction, InfoModele, Options};
use hm_vmf::{Node, Vmf};

fn batir(spec: &str) -> Construction {
    let spec: serde_yaml::Value = serde_yaml::from_str(spec).unwrap();
    // tout modèle existe, repose sur son origine et passe en prop_static
    let modele = |_: &str| Some(InfoModele { bas: 0.0, statique: Some(true) });
    construire(&spec, &Options { phase: None, base: PathBuf::from("."), modules_skill: None, modele: &modele })
}

const ENTETE: &str = "carte: essai\nciel: sky_day01_09\nmurs: 16\ndalles: 16\nmateriaux: {sol: a/sol, mur: a/mur, plafond: a/plafond, detail: a/detail}\n";
const APPARITION: &str = "entites:\n  - {classe: info_player_start, zone: a, position: [64, 64]}\n";

type Boite = ([f64; 3], [f64; 3]);

/// La boîte d'un solide : les bornes des points de ses plans.
fn boite(s: &Node) -> Boite {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for side in s.children_named("side") {
        for p in hm_vmf::parse_plane(side.get("plane").unwrap()).unwrap() {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
    }
    (lo, hi)
}

fn volume(b: &Boite) -> f64 {
    (0..3).map(|a| b.1[a] - b.0[a]).product()
}

fn commun(a: &Boite, b: &Boite) -> f64 {
    (0..3).map(|k| (a.1[k].min(b.1[k]) - a.0[k].max(b.0[k])).max(0.0)).product()
}

fn monde(c: &Construction) -> (Vmf, Vec<Boite>) {
    let v = Vmf::parse(c.vmf.clone().into_bytes()).unwrap();
    let b = v.world().unwrap().solids().map(boite).collect();
    (v, b)
}

fn sans_chevauchement(b: &[Boite]) {
    for (i, x) in b.iter().enumerate() {
        for y in &b[i + 1..] {
            assert_eq!(commun(x, y), 0.0, "deux solides du monde se chevauchent : {x:?} et {y:?}");
        }
    }
}

#[test]
fn une_piece_seule_est_fermee_par_six_solides() {
    let c = batir(&format!("{ENTETE}zones:\n  - {{nom: a, type: piece, origine: [0, 0, 0], taille: [256, 256, 128], lumiere: aucune}}\n{APPARITION}"));
    assert!(c.erreurs.is_empty(), "{:?}", c.erreurs);
    let (v, b) = monde(&c);
    assert_eq!(b.len(), 6, "un sol, un plafond, quatre murs");
    sans_chevauchement(&b);
    // l'enveloppe : 16 d'épaisseur autour de l'air, rien dans l'air
    let air: Boite = ([0.0; 3], [256.0, 256.0, 128.0]);
    let enveloppe = 288.0 * 288.0 * 160.0 - volume(&air);
    assert_eq!(b.iter().map(volume).sum::<f64>(), enveloppe);
    assert!(b.iter().all(|x| commun(x, &air) == 0.0));
    // chaque solide montre une seule face à la pièce, les cinq autres sont en nodraw
    for s in v.world().unwrap().solids() {
        let vues: Vec<&str> = s.children_named("side").map(|f| f.get("material").unwrap()).filter(|m| *m != "tools/toolsnodraw").collect();
        assert_eq!(vues.len(), 1, "{vues:?}");
        assert!(["a/sol", "a/mur", "a/plafond"].contains(&vues[0]));
    }
    // le sol passe sous les murs : il fait toute l'emprise de l'enveloppe
    assert!(b.iter().any(|x| x.0 == [-16.0, -16.0, -16.0] && x.1 == [272.0, 272.0, 0.0]));
    // une cubemap et un point d'apparition, pas de lumière
    assert_eq!(c.entites, 2);
    // le modèle sémantique : une pièce, sans rôle déclaré elle est dite vide
    assert_eq!(c.sem.pieces.len(), 1);
    assert_eq!((c.sem.pieces[0].id.as_str(), c.sem.pieces[0].role.as_str(), c.sem.pieces[0].hauteur), ("a", "vide", Some(128.0)));
    c.sem.controler().unwrap();
}

#[test]
fn deux_pieces_partagent_leur_mur_et_l_ouverture_le_traverse() {
    let spec = format!(
        "{ENTETE}zones:\n  - {{nom: a, type: piece, role: bureau, origine: [0, 0, 0], taille: [256, 256, 128], lumiere: aucune}}\n  - {{nom: b, type: couloir, contre: {{zone: a, cote: est, aligne: debut}}, taille: [128, 256, 128], lumiere: aucune}}\nouvertures:\n  - {{type: porte, nom: porte_a, entre: [a, b], decalage: 96, entite: aucune, cadre: false}}\n{APPARITION}"
    );
    let c = batir(&spec);
    assert!(c.erreurs.is_empty(), "{:?}", c.erreurs);
    let (_, b) = monde(&c);
    sans_chevauchement(&b);
    // b est de l'autre côté d'un mur de 16 ; la baie de 64 × 128 le traverse à 96 du bord sud
    let (air_a, air_b): (Boite, Boite) = (([0.0; 3], [256.0, 256.0, 128.0]), ([272.0, 0.0, 0.0], [400.0, 256.0, 128.0]));
    let baie: Boite = ([256.0, 96.0, 0.0], [272.0, 160.0, 128.0]);
    for vide in [&air_a, &air_b, &baie] {
        assert!(b.iter().all(|x| commun(x, vide) == 0.0), "un solide occupe un volume d'air");
    }
    // tout le reste de l'enveloppe commune est plein : un seul mur entre les deux pièces, pas deux
    let enveloppe = (416.0 + 16.0) * 288.0 * 160.0 - volume(&air_a) - volume(&air_b) - volume(&baie);
    assert_eq!(b.iter().map(volume).sum::<f64>(), enveloppe);
    // le plan garde l'ouverture, le modèle sémantique les deux pièces et leurs rôles
    assert_eq!(c.plan["ouvertures"][0]["nom"], "porte_a");
    let roles: Vec<(&str, &str)> = c.sem.pieces.iter().map(|p| (p.id.as_str(), p.role.as_str())).collect();
    assert_eq!(roles, vec![("a", "bureau"), ("b", "circulation")]);
}

#[test]
fn un_escalier_arrive_sur_sa_dalle_ou_le_dit() {
    let base = format!("{ENTETE}zones:\n  - {{nom: a, type: piece, origine: [0, 0, 0], taille: [512, 640, 384], lumiere: aucune}}\n");
    let escalier = "  - {type: escalier, zone: a, depart: [400, 80], direction: nord, largeur: 112, hauteur: 192, marche: 8, giron: 16, style: limon, garde_corps: [ouest]}\n";
    let avec = batir(&format!("{base}elements:\n  - {{type: dalle, nom: mezzanine, zone: a, rect: [0, 448, 512, 192], z: 192, garde_corps: [sud], lumiere: aucune}}\n{escalier}{APPARITION}"));
    assert!(avec.erreurs.is_empty() && avec.avertissements.is_empty(), "{:?} {:?}", avec.erreurs, avec.avertissements);
    let v = Vmf::parse(avec.vmf.clone().into_bytes()).unwrap();
    let details: Vec<usize> = v.entities().filter(|e| e.classname() == "func_detail").map(|e| e.solids().count()).collect();
    // la dalle ; 23 marches (192 / 8 - 1, la dernière est la dalle) ; le garde-corps rampant ; celui de la dalle
    assert_eq!(details, vec![1, 23, 1, 1]);
    // le garde-corps de la dalle s'arrête là où l'escalier arrive (x = 400), il ne lui barre pas le passage
    let garde = boite(v.entities().filter(|e| e.classname() == "func_detail").nth(3).unwrap().solids().next().unwrap());
    assert_eq!((garde.0[0], garde.1[0], garde.1[2] - garde.0[2]), (0.0, 400.0, 48.0));
    // la dernière marche est à une marche sous la dalle, et s'arrête à son bord
    let marches: Vec<Boite> = v.entities().filter(|e| e.classname() == "func_detail").nth(1).unwrap().solids().map(boite).collect();
    let haut = marches.iter().map(|m| m.1[2]).fold(0.0, f64::max);
    let fond = marches.iter().map(|m| m.1[1]).fold(0.0, f64::max);
    assert_eq!((haut, fond), (192.0 - 8.0, 448.0));
    // sans la dalle : la map se construit, mais l'escalier qui ne mène nulle part est signalé
    let sans = batir(&format!("{base}elements:\n{escalier}{APPARITION}"));
    assert!(sans.erreurs.is_empty());
    assert_eq!(sans.avertissements.len(), 1);
    assert!(sans.avertissements[0].contains("n'arrive sur aucune dalle à z=192"), "{}", sans.avertissements[0]);
}

#[test]
fn une_spec_fausse_n_ecrit_rien_et_dit_pourquoi() {
    let c = batir(&format!("{ENTETE}zones:\n  - {{nom: a, type: piece, origine: [0, 0, 0], taille: [256, 256, 128]}}\n  - {{nom: b, type: piece, origine: [128, 0, 0], taille: [256, 256, 96]}}\n{APPARITION}"));
    assert!(c.vmf.is_empty(), "rien à écrire");
    assert!(c.erreurs.iter().any(|e| e == "zones a et b se chevauchent"), "{:?}", c.erreurs);
    assert!(c.erreurs.iter().any(|e| e.starts_with("zone b : plafond à 96 (< 128)")), "{:?}", c.erreurs);
    // une règle d'échelle devient un avertissement quand l'élément dit que c'est voulu
    let c = batir(&format!("{ENTETE}zones:\n  - {{nom: a, type: piece, origine: [0, 0, 0], taille: [256, 256, 96], exception: \"cave\", lumiere: aucune}}\n{APPARITION}"));
    assert!(c.erreurs.is_empty() && !c.vmf.is_empty(), "{:?}", c.erreurs);
    assert_eq!(c.avertissements, vec!["zone a : plafond à 96 (< 128) (exception : cave)".to_string()]);
    // un ciel inconnu arrête tout avant de creuser, comme une zone mal placée
    let c = batir("carte: essai\nciel: ciel_inconnu\nmateriaux: {sol: a/sol}\nzones:\n  - {nom: a, type: piece, origine: [0, 0, 0], taille: [256, 256, 128], lumiere: aucune}\n");
    assert_eq!(c.erreurs.len(), 1, "{:?}", c.erreurs);
    assert!(c.erreurs[0].contains("ciel 'ciel_inconnu' absent"));
    // pas de point d'apparition, un rôle de matériau introuvable : toutes les fautes sont dites d'un coup
    let c = batir("carte: essai\nciel: sky_day01_09\nmateriaux: {sol: a/sol}\nzones:\n  - {nom: a, type: piece, origine: [0, 0, 0], taille: [256, 256, 128], lumiere: aucune}\n");
    for attendu in ["aucun matériau pour le rôle 'mur' (zone a)", "aucun matériau pour le rôle 'plafond' (zone a)", "aucun info_player_start"] {
        assert!(c.erreurs.iter().any(|e| e.contains(attendu)), "« {attendu} » manque dans {:?}", c.erreurs);
    }
    assert!(c.vmf.is_empty());
}

#[test]
fn un_modele_absent_ou_refuse_par_vbsp_est_une_erreur() {
    let spec: serde_yaml::Value = serde_yaml::from_str(&format!(
        "{ENTETE}zones:\n  - {{nom: a, type: piece, origine: [0, 0, 0], taille: [256, 256, 128], lumiere: aucune}}\nentites:\n  - {{classe: info_player_start, zone: a, position: [64, 64]}}\n  - {{classe: prop_static, zone: a, position: [128, 128], modele: models/x/bidon.mdl}}\n  - {{classe: prop_dynamic, zone: a, position: [160, 128], modele: models/x/bidon.mdl}}\n  - {{classe: prop_static, zone: a, position: [192, 128], modele: models/x/absent.mdl}}\n  - {{classe: prop_static, zone: a, position: [128, 192], modele: models/x/etagere.mdl}}\n"
    ))
    .unwrap();
    // le bidon est fait pour une entité dynamique ; l'étagère a sa boîte 12 sous son origine
    let modele = |m: &str| match m {
        "models/x/bidon.mdl" => Some(InfoModele { bas: 0.0, statique: Some(false) }),
        "models/x/etagere.mdl" => Some(InfoModele { bas: -12.0, statique: Some(true) }),
        _ => None,
    };
    let c = construire(&spec, &Options { phase: None, base: PathBuf::from("."), modules_skill: None, modele: &modele });
    assert_eq!(c.erreurs.len(), 2, "{:?}", c.erreurs);
    assert!(c.erreurs[0].contains("vbsp refuse models/x/bidon.mdl en prop_static"), "{}", c.erreurs[0]);
    assert!(c.erreurs[1].contains("modèle models/x/absent.mdl absent"), "{}", c.erreurs[1]);
    // sans les deux fautifs : l'étagère est relevée de 12 pour que son bas touche le sol
    let texte = serde_yaml::to_string(&spec).unwrap().replace("classe: prop_static\n  zone: a\n  position:\n  - 128\n  - 128", "classe: prop_dynamic\n  zone: a\n  position:\n  - 128\n  - 128").replace("models/x/absent.mdl", "models/x/etagere.mdl");
    let c = construire(&serde_yaml::from_str(&texte).unwrap(), &Options { phase: None, base: PathBuf::from("."), modules_skill: None, modele: &modele });
    assert!(c.erreurs.is_empty(), "{:?}", c.erreurs);
    assert!(c.vmf.contains("\"origin\" \"128 192 12\""), "l'étagère n'est pas posée au sol");
}

/// Les tables du skill que le constructeur embarque n'ont pas dérivé de celles du skill installé.
#[test]
fn les_tables_du_skill_sont_a_jour() {
    let skill = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".claude/skills/gmod-map-maker/data");
    let ici = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../regles");
    for nom in ["whitelist.yaml", "ciels_disponibles.tsv"] {
        let Ok(du_skill) = std::fs::read_to_string(skill.join(nom)) else {
            eprintln!("skill gmod-map-maker absent : {nom} non comparé");
            continue;
        };
        let copie = std::fs::read_to_string(ici.join(nom)).unwrap();
        assert_eq!(copie.replace("\r\n", "\n"), du_skill.replace("\r\n", "\n"), "regles/{nom} a dérivé de {}", skill.join(nom).display());
    }
}

// ---------------------------------------------------------------------------------------------- lot F : modules posés

const MODULE: &str = "carte: quai\nmodule: true\nphase: texture\nciel: sky_day01_09\nmateriaux: {detail: a/beton}\n";

/// Les solides de toutes les entités à solides (les blocs d'un module sont des func_detail), avec leurs faces.
fn solides_detail(c: &Construction) -> Vec<(Boite, Vec<(String, String, String)>)> {
    let v = Vmf::parse(c.vmf.clone().into_bytes()).unwrap();
    let mut out = Vec::new();
    for e in v.entities() {
        for s in e.solids() {
            let faces = s.children_named("side").map(|f| (f.get("material").unwrap().to_string(), f.get("uaxis").unwrap().to_string(), f.get("plane").unwrap().to_string())).collect();
            out.push((boite(s), faces));
        }
    }
    out
}

/// La normale d'une face d'après les trois points de son plan (sens de Hammer : vers l'extérieur du solide).
fn normale(plan: &str) -> [f64; 3] {
    let p = hm_vmf::parse_plane(plan).unwrap();
    let (a, b) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
    let n = [b[1] * a[2] - b[2] * a[1], b[2] * a[0] - b[0] * a[2], b[0] * a[1] - b[1] * a[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    [n[0] / l, n[1] / l, n[2] / l]
}

#[test]
fn un_module_pose_et_tourne_met_ses_blocs_au_bon_endroit() {
    // repère : origine (1000, 2000, 96), quart de tour. Un point (x, y) du module va en (1000 - y, 2000 + x).
    let c = batir(&format!(
        "{MODULE}repere: {{origine: [1000, 2000, 96], angle: 90}}\nelements:\n  - {{type: bloc, nom: socle, rect: [8, 16, 32, 40], z: 0, hauteur: 8, materiaux: {{dessus: a/beton, cotes: a/beton, dessous: a/beton, nord: a/affiche}}}}\n"
    ));
    assert!(c.erreurs.is_empty(), "{:?}", c.erreurs);
    let s = solides_detail(&c);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].0, ([944.0, 2008.0, 96.0], [984.0, 2040.0, 104.0]));
    // le côté « nord » du module (ses y croissants) regarde l'ouest du monde une fois tourné
    let affiche: Vec<_> = s[0].1.iter().filter(|f| f.0.eq_ignore_ascii_case("a/affiche")).collect();
    assert_eq!(affiche.len(), 1, "une seule face porte l'affiche");
    let n = normale(&affiche[0].2);
    assert!((n[0] + 1.0).abs() < 1e-6 && n[1].abs() < 1e-6, "l'affiche regarde {n:?}, pas l'ouest");
    // sans repère, le même bloc reste où la spec le dit
    let c = batir(&format!("{MODULE}elements:\n  - {{type: bloc, nom: socle, rect: [8, 16, 32, 40], z: 0, hauteur: 8}}\n"));
    assert_eq!(solides_detail(&c)[0].0, ([8.0, 16.0, 0.0], [40.0, 56.0, 8.0]));
}

#[test]
fn seulement_ne_garde_un_element_que_pour_sa_variante() {
    let spec = |variante: &str| {
        format!("{MODULE}{variante}elements:\n  - {{type: bloc, nom: commun, rect: [0, 0, 64, 64], z: 0, hauteur: 8}}\n  - {{type: bloc, nom: propre, seulement: [gare, docks], rect: [128, 0, 64, 64], z: 0, hauteur: 8}}\n")
    };
    assert_eq!(solides_detail(&batir(&spec("variante: gare\n"))).len(), 2);
    assert_eq!(solides_detail(&batir(&spec("variante: gosses\n"))).len(), 1);
    assert_eq!(solides_detail(&batir(&spec(""))).len(), 1, "sans variante, un élément réservé n'est pas construit");
}

#[test]
fn la_pose_par_face_ajuste_un_panneau_comme_le_generateur_de_megalopole() {
    // le plan de ligne de la station de la gare : 168 × 42, une image de 1024 × 256, lisible des deux côtés.
    // Valeurs relevées dans la map écrite par gen/city.py avant le lot F.
    let d = std::env::temp_dir().join(format!("hm_model_pose_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("table.json"), r#"{"a/plan": {"w": 1024, "h": 256}, "a/metal": {"w": 512, "h": 512, "tile": [128, 128], "lm": 32}}"#).unwrap();
    let spec: serde_yaml::Value = serde_yaml::from_str(&format!(
        "{MODULE}grille_detail: 0.5\ntextures: {{pose: face, table: table.json}}\nrepere: {{origine: [1700, 2560, 809], angle: 0}}\nelements:\n  - {{type: bloc, nom: plan, rect: [16, 297, 168, 2], z: 119, hauteur: 42, materiaux: {{dessus: a/metal, cotes: a/metal, dessous: a/metal, nord: a/plan, sud: a/plan}}, ajuste: [nord, sud]}}\n"
    ))
    .unwrap();
    let modele = |_: &str| Some(InfoModele { bas: 0.0, statique: Some(true) });
    let c = construire(&spec, &Options { phase: None, base: d.clone(), modules_skill: None, modele: &modele });
    assert!(c.erreurs.is_empty(), "{:?}", c.erreurs);
    let s = solides_detail(&c);
    assert_eq!(s[0].0, ([1716.0, 2857.0, 928.0], [1884.0, 2859.0, 970.0]));
    let mut axes: Vec<&str> = s[0].1.iter().filter(|f| f.0.eq_ignore_ascii_case("a/plan")).map(|f| f.1.as_str()).collect();
    axes.sort();
    assert_eq!(axes, ["[-1 0 0 219.4286] 0.164062", "[1 0 0 804.5714] 0.164062"]);
    // une face non ajustée garde la taille de sa tuile : 128 pour 512 points, soit 0,25
    assert!(s[0].1.iter().filter(|f| f.0.eq_ignore_ascii_case("a/metal")).all(|f| f.1.ends_with("] 0.25")), "{:?}", s[0].1);
    let _ = std::fs::remove_dir_all(&d);
    // une table absente est une erreur, pas une pose par défaut
    let c = construire(&spec, &Options { phase: None, base: PathBuf::from("."), modules_skill: None, modele: &modele });
    assert!(c.erreurs.iter().any(|e| e.contains("table") && e.contains("introuvable")), "{:?}", c.erreurs);
}

#[test]
fn les_toits_declares_vont_au_modele_semantique_dans_le_repere_du_monde() {
    let c = batir(&format!(
        "{MODULE}variante: station_nord\nrepere: {{origine: [1000, 2000, 96], angle: 90}}\nelements:\n  - {{type: bloc, nom: quai, rect: [0, 0, 64, 64], z: -8, hauteur: 8}}\ntoits:\n  - nom: auvent\n    boite: [-10, 0, 100, 10, 50, 120]\n    appuis: [[0, 40, 0]]\n"
    ));
    assert!(c.erreurs.is_empty(), "{:?}", c.erreurs);
    assert_eq!(c.sem.toits.len(), 1);
    let t = &c.sem.toits[0];
    assert_eq!(t.id, "station_nord.auvent");
    assert_eq!(t.boite, [950.0, 1990.0, 196.0, 1000.0, 2010.0, 216.0]);
    assert_eq!(t.appuis, vec![[960.0, 2000.0, 96.0]]);
    // et le bloc nommé est un objet déclaré
    assert!(c.sem.objets.iter().any(|o| o.nom == "quai"));
}

#[test]
fn une_entite_sans_orientation_ne_porte_pas_la_cle_angles() {
    let c = batir(&format!(
        "{MODULE}entites:\n  - {{classe: light, position: [0, 0, 64], angle: aucun, cles: {{_light: \"255 255 255 200\"}}}}\n  - {{classe: info_target, position: [32, 0, 64], angle: 90}}\n"
    ));
    assert!(c.erreurs.is_empty(), "{:?}", c.erreurs);
    let v = Vmf::parse(c.vmf.clone().into_bytes()).unwrap();
    let lumiere = v.entities().find(|e| e.classname() == "light").unwrap();
    assert!(lumiere.get("angles").is_none(), "la lumière porte angles = {:?}", lumiere.get("angles"));
    let cible = v.entities().find(|e| e.classname() == "info_target").unwrap();
    assert_eq!(cible.get("angles"), Some("0 90 0"));
}
