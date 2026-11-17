//! Un cas construit par règle : une petite map où le défaut existe et une où il n'existe pas.
//! Les règles qui demandent de vrais modèles (WC, traversée, réseaux) sont essayées sur rp_megalopole
//! (`tests/megalopole.rs`).
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use hm_assets::Vfs;
use hm_regles::{valider, Config, Famille, Niveau, Rapport};
use hm_scene::{Scene, SceneOptions};

static N: AtomicU32 = AtomicU32::new(100);
fn id() -> u32 {
    N.fetch_add(1, Ordering::Relaxed)
}

const MUR: &str = "DEV/DEV_MEASUREGENERIC01B";

/// Un solide droit, avec un matériau par défaut et un par face au besoin (ordre : dessus, dessous, -x, +x, +y, -y).
fn boite_mats(lo: [f64; 3], hi: [f64; 3], mats: [&str; 6], axes: Option<[&str; 2]>) -> String {
    let ([x0, y0, z0], [x1, y1, z1]) = (lo, hi);
    let faces = [
        ([x0, y1, z1], [x1, y1, z1], [x1, y0, z1], "[1 0 0 0] 0.25", "[0 -1 0 0] 0.25"),
        ([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], "[1 0 0 0] 0.25", "[0 -1 0 0] 0.25"),
        ([x0, y1, z1], [x0, y0, z1], [x0, y0, z0], "[0 1 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ([x1, y1, z0], [x1, y0, z0], [x1, y0, z1], "[0 1 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ([x1, y1, z1], [x0, y1, z1], [x0, y1, z0], "[1 0 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ([x1, y0, z0], [x0, y0, z0], [x0, y0, z1], "[1 0 0 0] 0.25", "[0 0 -1 0] 0.25"),
    ];
    let mut s = format!("\tsolid\n\t{{\n\t\t\"id\" \"{}\"\n", id());
    for (k, (a, b, c, u, v)) in faces.iter().enumerate() {
        let (u, v) = axes.map_or((*u, *v), |x| (x[0], x[1]));
        let p = |q: &[f64; 3]| format!("({} {} {})", q[0], q[1], q[2]);
        s += &format!("\t\tside\n\t\t{{\n\t\t\t\"id\" \"{}\"\n\t\t\t\"plane\" \"{} {} {}\"\n\t\t\t\"material\" \"{}\"\n\t\t\t\"uaxis\" \"{u}\"\n\t\t\t\"vaxis\" \"{v}\"\n\t\t\t\"lightmapscale\" \"16\"\n\t\t}}\n", id(), p(a), p(b), p(c), mats[k]);
    }
    s + "\t}\n"
}

fn boite(lo: [f64; 3], hi: [f64; 3]) -> String {
    boite_mats(lo, hi, [MUR; 6], None)
}

/// Une salle fermée de 512 × 512 × 256 (sol à 0), murs de 16.
fn salle() -> Vec<String> {
    let e = 16.0;
    vec![
        boite([-256.0, -256.0, -e], [256.0, 256.0, 0.0]),
        boite([-256.0, -256.0, 256.0], [256.0, 256.0, 256.0 + e]),
        boite([-256.0 - e, -256.0, 0.0], [-256.0, 256.0, 256.0]),
        boite([256.0, -256.0, 0.0], [256.0 + e, 256.0, 256.0]),
        boite([-256.0, -256.0 - e, 0.0], [256.0, -256.0, 256.0]),
        boite([-256.0, 256.0, 0.0], [256.0, 256.0 + e, 256.0]),
    ]
}

fn entite(classe: &str, cles: &str, solides: &[String]) -> String {
    format!("entity\n{{\n\t\"id\" \"{}\"\n\t\"classname\" \"{classe}\"\n{cles}{}}}\n", id(), solides.concat())
}

fn carte(monde: &[String], entites: &[String]) -> String {
    format!(
        "versioninfo\n{{\n\t\"mapversion\" \"1\"\n}}\nworld\n{{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n{}}}\n{}{}cameras\n{{\n\t\"activecamera\" \"-1\"\n}}\n",
        monde.concat(),
        entite("info_player_start", "\t\"origin\" \"0 0 8\"\n", &[]),
        entites.concat()
    )
}

fn rapport(texte: &str, familles: &[Famille]) -> Rapport {
    let d = std::env::temp_dir().join(format!("hm_regles_cas_{}_{}", std::process::id(), id()));
    std::fs::create_dir_all(&d).unwrap();
    let p: PathBuf = d.join("cas.vmf");
    std::fs::write(&p, texte).unwrap();
    let vfs = Vfs::new();
    let scene = Scene::load(&p, &vfs, &SceneOptions::default()).unwrap();
    let r = valider(&scene, &vfs, &Config::default(), familles);
    let _ = std::fs::remove_dir_all(&d);
    r
}

fn geo(texte: &str) -> Rapport {
    rapport(texte, &[Famille::Geometrie])
}

fn regles(r: &Rapport) -> Vec<&str> {
    let mut v: Vec<&str> = r.constats.iter().map(|c| c.regle).collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn une_salle_propre_ne_declenche_rien() {
    let r = rapport(&carte(&salle(), &[]), &[Famille::Geometrie, Famille::Physique]);
    assert!(r.constats.is_empty(), "{:?}", r.constats.iter().map(|c| &c.message).collect::<Vec<_>>());
}

#[test]
fn plan_degenere_et_solide_sans_volume() {
    let mut m = salle();
    // trois points alignés sur la première face
    let b = boite([0.0, 0.0, 0.0], [64.0, 64.0, 64.0]).replacen("(0 64 64) (64 64 64) (64 0 64)", "(0 0 64) (32 0 64) (64 0 64)", 1);
    m.push(b);
    let r = geo(&carte(&m, &[]));
    assert!(regles(&r).contains(&"geo.plan_degenere"), "{:?}", regles(&r));
    assert_eq!(r.de("geo.plan_degenere")[0].niveau, Niveau::Erreur);
    // un « solide » de trois faces seulement : ses plans ne ferment aucun volume
    let b = boite([0.0, 0.0, 0.0], [64.0, 64.0, 64.0]);
    let trois: Vec<&str> = b.split("\t\tside\n").collect();
    let ouvert = format!("{}\t\tside\n{}\t\tside\n{}\t\tside\n{}\t}}\n", trois[0], trois[1], trois[2], trois[3]);
    let mut m = salle();
    m.push(ouvert);
    let r = geo(&carte(&m, &[]));
    assert_eq!(r.de("geo.sans_volume").len(), 1, "{:?}", regles(&r));
}

#[test]
fn face_inutile() {
    let mut m = salle();
    // un septième plan, hors du solide : il ne garde aucun polygone
    let b = boite([0.0, 0.0, 0.0], [64.0, 64.0, 64.0]);
    let extra = format!("\t\tside\n\t\t{{\n\t\t\t\"id\" \"{}\"\n\t\t\t\"plane\" \"(0 64 200) (64 64 200) (64 0 200)\"\n\t\t\t\"material\" \"{MUR}\"\n\t\t\t\"uaxis\" \"[1 0 0 0] 0.25\"\n\t\t\t\"vaxis\" \"[0 -1 0 0] 0.25\"\n\t\t}}\n\t}}\n", id());
    m.push(b.strip_suffix("\t}\n").unwrap().to_string() + &extra);
    let r = geo(&carte(&m, &[]));
    assert_eq!(r.de("geo.face_inutile").len(), 1, "{:?}", regles(&r));
}

#[test]
fn micro_solide_hors_limites_hors_grille() {
    let mut m = salle();
    m.push(boite([0.0, 0.0, 0.0], [0.5, 0.5, 0.5]));
    m.push(boite([17000.0, 0.0, 0.0], [17064.0, 64.0, 64.0]));
    // entre ±16 376 et ±16 384 : vbsp ne la refuse pas, il plante (marge de portals.cpp)
    m.push(boite([16316.0, 0.0, 0.0], [16380.0, 64.0, 64.0]));
    m.push(boite([16300.0, 200.0, 0.0], [16376.0, 264.0, 64.0]));
    m.push(boite([100.0, 100.0, 0.0], [164.25, 164.0, 64.0]));
    let r = geo(&carte(&m, &[]));
    assert_eq!(r.de("geo.micro_solide").len(), 1);
    assert_eq!(r.de("geo.hors_limites").len(), 2, "{:?}", r.de("geo.hors_limites").iter().map(|c| &c.message).collect::<Vec<_>>());
    assert!(r.de("geo.hors_limites").iter().any(|c| c.message.contains("plante")));
    // le micro-solide (0,5) et la face à x = 164,25 sont hors de la grille de 1
    assert_eq!(r.de("geo.hors_grille").len(), 2);
    assert!(r.de("geo.hors_grille").iter().any(|c| c.message.contains("164.250")));
}

#[test]
fn chevauchement_de_solides_du_monde() {
    let mut m = salle();
    m.push(boite([0.0, 0.0, 0.0], [64.0, 64.0, 64.0]));
    m.push(boite([32.0, 32.0, 0.0], [96.0, 96.0, 64.0]));
    // deux solides qui se touchent seulement : pas de chevauchement
    m.push(boite([-128.0, 0.0, 0.0], [-64.0, 64.0, 64.0]));
    m.push(boite([-192.0, 0.0, 0.0], [-128.0, 64.0, 64.0]));
    let r = geo(&carte(&m, &[]));
    let c = r.de("geo.chevauchement");
    assert_eq!(c.len(), 1, "{:?}", c.iter().map(|c| &c.message).collect::<Vec<_>>());
    assert!(c[0].message.contains("32 × 32 × 64"), "{}", c[0].message);
    assert_eq!(c[0].mesure, 32.0 * 32.0 * 64.0);
}

#[test]
fn faces_superposees_et_scintillement() {
    // deux solides du monde côte à côte dans le même plan ne se recouvrent pas : rien
    let mut m = salle();
    m.push(boite([0.0, 0.0, 0.0], [64.0, 64.0, 64.0]));
    m.push(boite([64.0, 0.0, 0.0], [128.0, 64.0, 64.0]));
    assert!(geo(&carte(&m, &[])).constats.is_empty());
    // un func_brush à cheval sur un solide du monde, dessus et flancs dans les mêmes plans : scintillement
    let mut m = salle();
    m.push(boite([0.0, 0.0, 0.0], [64.0, 64.0, 64.0]));
    let e = entite("func_brush", "", &[boite_mats([32.0, 0.0, 32.0], [96.0, 64.0, 64.0], ["DEV/DEV_MEASUREWALL01A"; 6], None)]);
    let r = geo(&carte(&m, &[e]));
    // trois paires de faces dans le même plan : les dessus (32 × 64) et les deux flancs en y (32 × 32)
    let z = r.de("geo.zfighting");
    assert_eq!(z.len(), 3, "{:?}", r.constats.iter().map(|c| &c.message).collect::<Vec<_>>());
    let mut aires: Vec<f32> = z.iter().map(|c| c.mesure).collect();
    aires.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(aires, vec![1024.0, 1024.0, 2048.0]);
    // deux solides du monde qui se chevauchent, dessus au même niveau, matériaux différents : matériau indéterminé
    let mut m = salle();
    m.push(boite([0.0, 0.0, 0.0], [64.0, 64.0, 64.0]));
    m.push(boite_mats([32.0, 0.0, 0.0], [96.0, 64.0, 64.0], ["DEV/DEV_MEASUREWALL01A"; 6], None));
    let r = geo(&carte(&m, &[]));
    assert!(!r.de("geo.faces_superposees").is_empty());
    assert!(r.de("geo.zfighting").is_empty());
}

#[test]
fn texture_perpendiculaire() {
    let mut m = salle();
    // axes de texture tous deux horizontaux : perpendiculaires aux quatre faces verticales... non, parallèles au
    // sol : les faces du dessus et du dessous sont texturées, les quatre autres ont un axe selon leur normale ou pas
    m.push(boite_mats([0.0, 0.0, 0.0], [64.0, 64.0, 64.0], [MUR; 6], Some(["[1 0 0 0] 0.25", "[0 0 -1 0] 0.25"])));
    let r = geo(&carte(&m, &[]));
    // u × v = y : seules les faces de normale ±y gardent une texture ; les quatre autres sont perpendiculaires
    assert_eq!(r.de("geo.texture_perpendiculaire").len(), 4);
    assert_eq!(r.de("geo.texture_perpendiculaire")[0].niveau, Niveau::Erreur);
}

#[test]
fn areaportal_a_deux_solides() {
    let e = entite("func_areaportal", "", &[boite([0.0, 0.0, 0.0], [8.0, 64.0, 64.0]), boite([16.0, 0.0, 0.0], [24.0, 64.0, 64.0])]);
    let r = geo(&carte(&salle(), &[e]));
    assert_eq!(r.de("geo.areaportal_multiple").len(), 1);
}

#[test]
fn ce_qui_flotte_et_ce_qui_est_decolle() {
    let detail = |z0: f64| entite("func_detail", "", &[boite([0.0, 0.0, z0], [64.0, 64.0, z0 + 32.0])]);
    // posé au sol : rien
    assert!(rapport(&carte(&salle(), &[detail(0.0)]), &[Famille::Physique]).constats.is_empty());
    // à 1 du sol : dans le jeu admis (2)
    assert!(rapport(&carte(&salle(), &[detail(1.0)]), &[Famille::Physique]).constats.is_empty());
    // à 3 du sol : décollé
    let r = rapport(&carte(&salle(), &[detail(3.0)]), &[Famille::Physique]);
    assert_eq!(regles(&r), vec!["phys.decolle"]);
    // à 100 du sol, loin de tout : flotte
    let r = rapport(&carte(&salle(), &[detail(100.0)]), &[Famille::Physique]);
    assert_eq!(regles(&r), vec!["phys.flotte"]);
    assert!(r.constats[0].message.contains("100 de vide dessous"), "{}", r.constats[0].message);
    // deux blocs empilés en l'air : un seul constat, pour le groupe
    let deux = entite("func_detail", "", &[boite([0.0, 0.0, 100.0], [64.0, 64.0, 132.0]), boite([0.0, 0.0, 132.0], [32.0, 32.0, 150.0])]);
    let r = rapport(&carte(&salle(), &[deux]), &[Famille::Physique]);
    assert_eq!(r.constats.len(), 1);
    assert_eq!(r.constats[0].autres.len(), 1);
    // suspendu au plafond : rien
    let pendu = entite("func_detail", "", &[boite([0.0, 0.0, 224.0], [64.0, 64.0, 256.0])]);
    assert!(rapport(&carte(&salle(), &[pendu]), &[Famille::Physique]).constats.is_empty());
    // une porte en l'air (elle bouge) : rien
    let porte = entite("func_door", "", &[boite([0.0, 0.0, 100.0], [64.0, 8.0, 200.0])]);
    assert!(rapport(&carte(&salle(), &[porte]), &[Famille::Physique]).constats.is_empty());
}

#[test]
fn une_exception_du_projet_garde_le_constat_mais_le_marque() {
    let detail = entite("func_detail", "", &[boite([0.0, 0.0, 100.0], [64.0, 64.0, 132.0])]);
    let d = std::env::temp_dir().join(format!("hm_regles_exc_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("cas.vmf"), carte(&salle(), &[detail])).unwrap();
    std::fs::write(d.join("hammer-master.toml"), "[[exception]]\nregle = \"phys.flotte\"\nclasse = \"func_detail\"\nraison = \"plateforme en lévitation, voulue\"\n").unwrap();
    let (config, fichier) = Config::du_projet(&d.join("cas.vmf")).unwrap();
    assert!(fichier.is_some());
    let vfs = Vfs::new();
    let scene = Scene::load(d.join("cas.vmf"), &vfs, &SceneOptions::default()).unwrap();
    let r = valider(&scene, &vfs, &config, &[Famille::Physique]);
    assert_eq!(r.constats.len(), 1);
    assert_eq!(r.constats[0].excepte.as_deref(), Some("plateforme en lévitation, voulue"));
    assert_eq!((r.compte(Niveau::Avertissement), r.exceptes()), (0, 1));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn budgets_comptes_dans_le_vmf() {
    let e = [
        entite("func_detail", "", &[boite([0.0, 0.0, 0.0], [64.0, 64.0, 32.0])]),
        entite("func_brush", "", &[boite([100.0, 0.0, 0.0], [164.0, 64.0, 32.0])]),
        entite("func_areaportal", "", &[boite([-100.0, 0.0, 0.0], [-98.0, 64.0, 64.0])]),
        entite("light", "\t\"origin\" \"0 0 200\"\n\t\"targetname\" \"neon\"\n", &[]),
        entite("light_spot", "\t\"origin\" \"50 0 200\"\n", &[]),
        entite("env_cubemap", "\t\"origin\" \"0 0 64\"\n", &[]),
    ];
    let r = rapport(&carte(&salle(), &e), &[Famille::Budgets]);
    let b = r.budgets.unwrap();
    let v = |nom: &str| b.lignes.iter().find(|l| l.nom == nom).unwrap().valeur;
    assert_eq!(v("brushes"), 9); // 6 murs + détail + func_brush + areaportal
    assert_eq!(v("models"), 2); // le monde et le func_brush
    assert_eq!((v("areaportals"), v("cubemapsamples"), v("lumieres"), v("lumieres_nommees"), v("static prop count")), (1, 1, 2, 1, 0));
    // lumière : le sol de 512 × 512 au pas de 16 fait 33 × 33 luxels ; l'intérieur de la salle en compte bien plus
    assert!(b.lumiere.luxels > 6 * 33 * 17, "{}", b.lumiere.luxels);
    // mais pas les faces extérieures, tournées vers le vide, que vbsp retire : intérieur 2 × 33 × 33 + 4 × 33 × 17
    // = 4 422, plus le détail et le func_brush (10 408 quand elles comptaient)
    assert!(b.lumiere.luxels < 6000, "{}", b.lumiere.luxels);
}

#[test]
fn joueur_piece_inaccessible_et_piege() {
    // deux salles accolées sans porte : la seconde est inaccessible
    let mut m = salle();
    let (d, e) = (544.0, 16.0);
    m.extend([
        boite([-256.0 + d, -256.0, -e], [256.0 + d, 256.0, 0.0]),
        boite([-256.0 + d, -256.0, 256.0], [256.0 + d, 256.0, 256.0 + e]),
        boite([256.0 + d, -256.0, 0.0], [256.0 + d + e, 256.0, 256.0]),
        boite([-256.0 + d, -256.0 - e, 0.0], [256.0 + d, -256.0, 256.0]),
        boite([-256.0 + d, 256.0, 0.0], [256.0 + d, 256.0 + e, 256.0]),
        boite([256.0 + e, -256.0, 0.0], [-256.0 + d, 256.0, 256.0]),
    ]);
    let r = rapport(&carte(&m, &[]), &[Famille::Joueur]);
    let j = r.joueur.as_ref().unwrap();
    assert_eq!((j.apparitions, j.pieces, j.pieces_inaccessibles), (1, 2, 1), "{}", hm_regles::joueur::resume(j));
    assert_eq!(r.de("joueur.inaccessible").len(), 1);
    // une fosse de 128 de profondeur dans la première salle : on y tombe, on n'en sort pas
    let mut m = vec![
        boite([-256.0, -256.0, -160.0], [256.0, 256.0, -144.0]),
        boite([-256.0, -256.0, 256.0], [256.0, 256.0, 272.0]),
        boite([-272.0, -256.0, -144.0], [-256.0, 256.0, 256.0]),
        boite([256.0, -256.0, -144.0], [272.0, 256.0, 256.0]),
        boite([-256.0, -272.0, -144.0], [256.0, -256.0, 256.0]),
        boite([-256.0, 256.0, -144.0], [256.0, 272.0, 256.0]),
    ];
    // le plancher à 0 couvre tout sauf un carré de 128 dans un coin
    m.push(boite([-256.0, -256.0, -144.0], [128.0, 256.0, 0.0]));
    m.push(boite([128.0, -256.0, -144.0], [256.0, 128.0, 0.0]));
    let r = rapport(&carte(&m, &[]), &[Famille::Joueur]);
    assert_eq!(r.de("joueur.piege").len(), 1, "{}", hm_regles::joueur::resume(r.joueur.as_ref().unwrap()));
    // la même fosse, profonde de 48 seulement : on en ressort en sautant
    let mut m2 = m.clone();
    m2.push(boite([128.0, 128.0, -144.0], [256.0, 256.0, -48.0]));
    let r = rapport(&carte(&m2, &[]), &[Famille::Joueur]);
    assert!(r.de("joueur.piege").is_empty());
}

/// La question de Counter-Strike: Source : une archive rangée dans l'installation de Garry's Mod est chez tous les
/// joueurs ; la même archive dans le dossier d'un autre jeu ne l'est pas, et déclenche "asset.autre_jeu".
#[test]
fn provenance_par_montage() {
    use hm_regles::assets::{provenance, Provenance};
    let c = Config::default();
    let p = |l: &str| provenance(l, &c);
    assert_eq!(p(r"C:\Steam\steamapps\common\GarrysMod\sourceengine\content_cstrike_dir.vpk"), Provenance::Jeu);
    assert_eq!(p(r"C:\Steam\steamapps\common\GarrysMod\garrysmod\garrysmod_dir.vpk"), Provenance::Jeu);
    assert_eq!(p(r"C:\Steam\steamapps\common\Counter-Strike Source\cstrike\cstrike_pak_dir.vpk"), Provenance::AutreJeu);
    assert_eq!(p(r"C:\Steam\steamapps\common\GarrysMod\garrysmod\addons\mon_addon"), Provenance::Local);
    assert_eq!(p(r"C:\Steam\steamapps\common\GarrysMod\garrysmod"), Provenance::Local);
    assert_eq!(p(r"C:\Users\x\Desktop\ma_map\content"), Provenance::Projet);
    assert_eq!(p("ma_map.bsp (contenu embarqué)"), Provenance::Projet);
    let mut c2 = Config::default();
    c2.montages_locaux = vec!["*/ma_map/brouillon".into()];
    assert_eq!(provenance(r"C:\Users\x\Desktop\ma_map\brouillon", &c2), Provenance::Local);
}

/// Le modèle sémantique à côté du VMF : une pièce déclarée sans chemin sort par "sem.piece_inaccessible", une boîte
/// déclarée dans du plein par "sem.piece_sans_sol", un fichier faux par "sem.modele_invalide".
#[test]
fn pieces_declarees_par_le_modele() {
    // deux salles accolées sans porte ; le point d'apparition est dans la première
    let (d, e) = (544.0, 16.0);
    let mut m = salle();
    m.extend([
        boite([-256.0 + d, -256.0, -e], [256.0 + d, 256.0, 0.0]),
        boite([-256.0 + d, -256.0, 256.0], [256.0 + d, 256.0, 256.0 + e]),
        boite([256.0 + d, -256.0, 0.0], [256.0 + d + e, 256.0, 256.0]),
        boite([-256.0 + d, -256.0 - e, 0.0], [256.0 + d, -256.0, 256.0]),
        boite([-256.0 + d, 256.0, 0.0], [256.0 + d, 256.0 + e, 256.0]),
        boite([256.0 + e, -256.0, 0.0], [-256.0 + d, 256.0, 256.0]),
    ]);
    let dossier = std::env::temp_dir().join(format!("hm_regles_sem_{}", std::process::id()));
    std::fs::create_dir_all(&dossier).unwrap();
    let vmf = dossier.join("cas.vmf");
    std::fs::write(&vmf, carte(&m, &[])).unwrap();
    let piece = |id: &str, x0: f64, x1: f64, sol: f64| format!(r#"{{"id":"{id}","role":"bureau","emprise":[{x0},-256,{x1},256],"sol":{sol}}}"#);
    let ecrire = |pieces: &[String], liaisons: &str| std::fs::write(dossier.join("cas.sem.json"), format!(r#"{{"version":1,"carte":"cas","pieces":[{}],"liaisons":[{liaisons}]}}"#, pieces.join(","))).unwrap();
    let lancer = || {
        let vfs = Vfs::new();
        let scene = Scene::load(&vmf, &vfs, &SceneOptions::default()).unwrap();
        valider(&scene, &vfs, &Config::default(), &[Famille::Joueur])
    };
    ecrire(&[piece("a", -256.0, 256.0, 0.0), piece("b", 288.0, 800.0, 0.0), piece("dans_le_mur", -256.0, 256.0, 600.0)], "");
    let r = lancer();
    assert!(r.modele.as_deref().unwrap().starts_with("3 pièce(s)"));
    let inacc = r.de("sem.piece_inaccessible");
    assert_eq!(inacc.len(), 1, "{:?}", r.constats.iter().map(|c| &c.message).collect::<Vec<_>>());
    assert!(inacc[0].message.contains("« b »"));
    assert_eq!(r.de("sem.piece_sans_sol").len(), 1);
    // avec un modèle, les pièces devinées ne sont plus jugées : pas de doublon
    assert!(r.de("joueur.inaccessible").is_empty());
    // une liaison déclarée entre les deux salles : plus rien d'inaccessible
    ecrire(&[piece("a", -256.0, 256.0, 0.0), piece("b", 288.0, 800.0, 0.0)], r#"{"id":"porte_magique","genre":"teleporteur","de":[100,0,0],"vers":[544,0,0]}"#);
    assert!(lancer().de("sem.piece_inaccessible").is_empty());
    // un fichier qui se contredit : dit comme une erreur, et les règles tournent quand même sans modèle
    ecrire(&[piece("a", -256.0, 256.0, 0.0), piece("a", 288.0, 800.0, 0.0)], "");
    let r = lancer();
    assert_eq!(r.de("sem.modele_invalide").len(), 1);
    assert!(r.de("sem.modele_invalide")[0].message.contains("deux fois"));
    assert!(r.modele.is_none());
    let _ = std::fs::remove_dir_all(&dossier);
}

/// Écrit une map et son modèle sémantique dans un dossier à part, et lance les familles demandées.
fn avec_modele(vmf: &str, sem: &str, familles: &[Famille]) -> Rapport {
    let d = std::env::temp_dir().join(format!("hm_regles_lotb_{}_{}", std::process::id(), id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("cas.vmf"), vmf).unwrap();
    std::fs::write(d.join("cas.sem.json"), sem).unwrap();
    let vfs = Vfs::new();
    let scene = Scene::load(d.join("cas.vmf"), &vfs, &SceneOptions::default()).unwrap();
    let r = valider(&scene, &vfs, &Config::default(), familles);
    let _ = std::fs::remove_dir_all(&d);
    r
}

fn sem(pieces: &str, reste: &str) -> String {
    format!(r#"{{"version":1,"carte":"cas","pieces":[{pieces}]{reste}}}"#)
}

/// Règles de rôle : "sem.role_incomplet", "sem.role_trop_grand", "sem.piece_sous_meublee", "sem.role_inconnu".
#[test]
fn regles_de_role() {
    let nomme = |nom: &str, x: f64| entite("func_brush", &format!("\t\"targetname\" \"{nom}\"\n"), &[boite([x, 0.0, 0.0], [x + 32.0, 32.0, 40.0])]);
    let piece = |role: &str| sem(&format!(r#"{{"id":"p","role":"{role}","emprise":[-256,-256,256,256],"sol":0}}"#), "");
    // un accueil sans comptoir
    let r = avec_modele(&carte(&salle(), &[]), &piece("accueil"), &[Famille::Semantique]);
    assert_eq!(r.de("sem.role_incomplet").len(), 1);
    assert!(r.de("sem.role_incomplet")[0].message.contains("comptoir"));
    // le même, avec un bloc nommé « comptoir_1 » : complet
    let r = avec_modele(&carte(&salle(), &[nomme("comptoir_1", 0.0)]), &piece("accueil"), &[Famille::Semantique]);
    assert!(r.de("sem.role_incomplet").is_empty(), "{:?}", r.constats.iter().map(|c| &c.message).collect::<Vec<_>>());
    // des WC de 512 × 512 : trop grands pour le rôle (384 × 384 au plus), et il y manque tout
    let r = avec_modele(&carte(&salle(), &[]), &piece("wc"), &[Famille::Semantique]);
    assert_eq!(r.de("sem.role_trop_grand").len(), 1);
    assert!(r.de("sem.role_trop_grand")[0].message.contains("1.8 fois"));
    assert_eq!(r.de("sem.role_incomplet").len(), 1);
    // un bureau avec sa table et sa chaise, mais 2 objets pour 4 tranches de 256 × 256 : sous-meublé
    let r = avec_modele(&carte(&salle(), &[nomme("table_1", 0.0), nomme("chaise_1", 64.0)]), &piece("bureau"), &[Famille::Semantique]);
    assert!(r.de("sem.role_incomplet").is_empty());
    assert_eq!(r.de("sem.piece_sous_meublee").len(), 1);
    // une pièce « vide » : rien n'est exigé ; un rôle inconnu : dit une fois, en information
    assert!(avec_modele(&carte(&salle(), &[]), &piece("vide"), &[Famille::Semantique]).constats.is_empty());
    let r = avec_modele(&carte(&salle(), &[]), &piece("salle_des_machines"), &[Famille::Semantique]);
    assert_eq!(regles(&r), vec!["sem.role_inconnu"]);
    // sans modèle, aucune règle sémantique ne tourne
    assert!(rapport(&carte(&salle(), &[]), &[Famille::Semantique]).constats.is_empty());
}

/// "sem.baie_sans_porte" : un bureau dont le mur a une ouverture de 64 sans porte ; avec une porte, plus rien.
#[test]
fn baie_sans_porte() {
    let e = 16.0;
    // la salle, dont le mur est (x = 256) est coupé en deux pour laisser une baie de y = -32 à 32
    let mut m = vec![
        boite([-256.0, -256.0, -e], [600.0, 256.0, 0.0]),
        boite([-256.0, -256.0, 256.0], [600.0, 256.0, 256.0 + e]),
        boite([-256.0 - e, -256.0, 0.0], [-256.0, 256.0, 256.0]),
        boite([600.0, -256.0, 0.0], [600.0 + e, 256.0, 256.0]),
        boite([-256.0, -256.0 - e, 0.0], [600.0, -256.0, 256.0]),
        boite([-256.0, 256.0, 0.0], [600.0, 256.0 + e, 256.0]),
    ];
    m.push(boite([256.0, -256.0, 0.0], [256.0 + e, -32.0, 256.0]));
    m.push(boite([256.0, 32.0, 0.0], [256.0 + e, 256.0, 256.0]));
    let bureau = sem(r#"{"id":"b","role":"bureau","emprise":[-256,-256,256,256],"sol":0}"#, "");
    let r = avec_modele(&carte(&m, &[]), &bureau, &[Famille::Semantique]);
    let b = r.de("sem.baie_sans_porte");
    assert_eq!(b.len(), 1, "{:?}", r.constats.iter().map(|c| &c.message).collect::<Vec<_>>());
    assert!(b[0].message.contains("de 64 de large") || b[0].message.contains("de 56 de large") || b[0].message.contains("de 72 de large"), "{}", b[0].message);
    // une porte dans la baie
    let porte = entite("func_door", "", &[boite([260.0, -32.0, 0.0], [268.0, 32.0, 112.0])]);
    assert!(avec_modele(&carte(&m, &[porte]), &bureau, &[Famille::Semantique]).de("sem.baie_sans_porte").is_empty());
    // la même baie dans une cafétéria : ce rôle n'a pas à se fermer
    let cafet = sem(r#"{"id":"b","role":"vide","emprise":[-256,-256,256,256],"sol":0}"#, "");
    assert!(avec_modele(&carte(&m, &[]), &cafet, &[Famille::Semantique]).de("sem.baie_sans_porte").is_empty());
}

/// "sem.toit_sans_appui" : un toit sans appui déclaré, un appui qui ne monte pas jusqu'au toit, un appui juste.
#[test]
fn toit_sans_appui() {
    let dalle = entite("func_detail", "", &[boite([-64.0, -64.0, 160.0], [64.0, 64.0, 172.0])]);
    let poteau = entite("func_detail", "", &[boite([-8.0, -8.0, 0.0], [8.0, 8.0, 160.0])]);
    let toit = |appuis: &str| sem("", &format!(r#","toits":[{{"id":"auvent","boite":[-64,-64,160,64,64,172],"appuis":[{appuis}]}}]"#));
    let r = avec_modele(&carte(&salle(), &[dalle.clone(), poteau.clone()]), &toit(""), &[Famille::Semantique]);
    assert!(r.de("sem.toit_sans_appui")[0].message.contains("aucun appui déclaré"));
    // l'appui déclaré là où il y a le poteau : bon
    assert!(avec_modele(&carte(&salle(), &[dalle.clone(), poteau]), &toit("[0,0,0]"), &[Famille::Semantique]).de("sem.toit_sans_appui").is_empty());
    // le même appui déclaré, mais le poteau n'existe pas : le défaut de l'auvent du 10/11
    let r = avec_modele(&carte(&salle(), &[dalle]), &toit("[0,0,0]"), &[Famille::Semantique]);
    assert_eq!(r.de("sem.toit_sans_appui").len(), 1);
    assert!(r.de("sem.toit_sans_appui")[0].message.contains("ne monte pas"));
}

/// "phys.porte_a_faux" : un panneau de 8 d'épaisseur posé contre un pan de mur plus étroit que lui.
#[test]
fn panneau_qui_depasse() {
    let pan = |y0: f64, y1: f64| boite([0.0, y0, 0.0], [32.0, y1, 200.0]);
    let panneau = entite("func_detail", "", &[boite([-8.0, -128.0, 64.0], [0.0, 128.0, 160.0])]);
    // le pan de mur s'arrête au milieu du panneau : la moitié du panneau dépasse au bout du mur
    let mut m = salle();
    m.push(pan(-128.0, 0.0));
    let r = rapport(&carte(&m, &[panneau.clone()]), &[Famille::Physique]);
    assert_eq!(r.de("phys.porte_a_faux").len(), 1, "{:?}", r.constats.iter().map(|c| &c.message).collect::<Vec<_>>());
    // la mesure est prise sur six colonnes : 128 de dépassement, à une colonne près
    let depasse = r.de("phys.porte_a_faux")[0].mesure;
    assert!((85.0..=171.0).contains(&depasse), "{depasse}");
    // une enseigne dressée sur un muret bas, qui dépasse vers le haut sur toute sa longueur : voulu, rien
    let mut m = salle();
    m.push(boite([0.0, -128.0, 0.0], [32.0, 128.0, 100.0]));
    assert!(rapport(&carte(&m, &[panneau.clone()]), &[Famille::Physique]).de("phys.porte_a_faux").is_empty());
    // un pan de mur aussi large que le panneau : rien
    let mut m = salle();
    m.push(pan(-128.0, 128.0));
    assert!(rapport(&carte(&m, &[panneau]), &[Famille::Physique]).de("phys.porte_a_faux").is_empty());
}

/// "geo.nodraw_visible" : un bloc dont une face en nodraw regarde la salle.
#[test]
fn face_sans_texture_visible() {
    let nd = "TOOLS/TOOLSNODRAW";
    // faces : dessus, dessous, -x, +x, +y, -y ; la face +x en nodraw donne sur la salle
    let bloc = boite_mats([-256.0, -64.0, 0.0], [-128.0, 64.0, 128.0], [MUR, MUR, MUR, nd, MUR, MUR], None);
    let mut m = salle();
    m.push(bloc);
    let r = rapport(&carte(&m, &[]), &[Famille::Joueur]);
    assert_eq!(r.de("geo.nodraw_visible").len(), 1, "{:?}", r.constats.iter().map(|c| &c.message).collect::<Vec<_>>());
    // la face en nodraw est celle qui est contre le mur (-x) : personne ne la voit
    let bloc = boite_mats([-256.0, -64.0, 0.0], [-128.0, 64.0, 128.0], [MUR, MUR, nd, MUR, MUR, MUR], None);
    let mut m = salle();
    m.push(bloc);
    assert!(rapport(&carte(&m, &[]), &[Famille::Joueur]).de("geo.nodraw_visible").is_empty());
}

/// La table des rôles embarquée est celle du skill gmod-map-maker, mot pour mot.
#[test]
fn la_table_des_roles_est_celle_du_skill() {
    let skill = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".claude/skills/gmod-map-maker/data/roles.yaml");
    let Ok(texte) = std::fs::read_to_string(&skill) else {
        println!("skill absent : contrôle sauté");
        return;
    };
    assert_eq!(texte.replace("\r\n", "\n"), hm_regles::semantique::ROLES_YAML.replace("\r\n", "\n"), "regles/roles.yaml a dérivé de {}", skill.display());
    let t = hm_regles::semantique::roles();
    assert!(t["wc"].exige.len() == 3 && t["wc"].surface_max == Some(147456.0) && t["vide"].vide);
}

/// "sem.amenagement_brouillon" : une pièce meublée d'après un modèle que l'utilisateur n'a pas validé est refusée ;
/// une fois le modèle validé elle passe ; modifié ensuite, il est refusé de nouveau.
#[test]
fn amenagement_en_brouillon() {
    use hm_model::amenagement::{Amenagement, Validations};
    let d = std::env::temp_dir().join(format!("hm_regles_amen_{}", std::process::id()));
    std::fs::create_dir_all(d.join("amenagements")).unwrap();
    std::fs::write(d.join("cas.vmf"), carte(&salle(), &[])).unwrap();
    std::fs::write(d.join("cas.sem.json"), sem(r#"{"id":"p","role":"vide","emprise":[-256,-256,256,256],"sol":0,"amenagement":"salon_essai"}"#, "")).unwrap();
    let toml = "nom = \"salon_essai\"\nrole = \"vide\"\n[piece]\nlargeur = [200, 600]\nprofondeur = [200, 600]\n[[ensemble]]\nnom = \"table\"\nx = { centre = 0 }\ny = { centre = 0 }\n[[ensemble.pose]]\nmodele = \"models/x/table.mdl\"\n";
    let lancer = || {
        let vfs = Vfs::new();
        let scene = Scene::load(d.join("cas.vmf"), &vfs, &SceneOptions::default()).unwrap();
        valider(&scene, &vfs, &Config::default(), &[Famille::Semantique])
    };
    // le modèle n'existe pas
    assert!(lancer().de("sem.amenagement_brouillon")[0].message.contains("introuvable"));
    // il existe, en brouillon
    let fichier = d.join("amenagements/salon_essai.toml");
    std::fs::write(&fichier, toml).unwrap();
    let r = lancer();
    assert_eq!(r.de("sem.amenagement_brouillon").len(), 1);
    assert!(r.de("sem.amenagement_brouillon")[0].message.contains("en brouillon"));
    assert_eq!(r.de("sem.amenagement_brouillon")[0].niveau, Niveau::Erreur);
    // validé (ce que fait `hm amenagement-valider` dans un terminal)
    let mut v = Validations::charger(&d.join("amenagements"));
    v.valider(&Amenagement::lire(&fichier).unwrap(), "2026-11-13 22:00:00");
    v.enregistrer(&d.join("amenagements")).unwrap();
    assert!(lancer().de("sem.amenagement_brouillon").is_empty());
    // modifié après validation : de nouveau refusé
    std::fs::write(&fichier, toml.replace("x = { centre = 0 }", "x = { centre = 16 }")).unwrap();
    assert_eq!(lancer().de("sem.amenagement_brouillon").len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// Un comptoir fait de solides sans nom : le modèle sémantique le déclare, et la règle de rôle le compte.
#[test]
fn un_objet_declare_compte_pour_le_role() {
    let d = std::env::temp_dir().join(format!("hm_regles_objet_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("cas.vmf"), carte(&salle(), &[])).unwrap();
    let piece = r#"{"id":"p","role":"accueil","emprise":[-256,-256,256,256],"sol":0}"#;
    let lancer = |objets: &str| {
        std::fs::write(d.join("cas.sem.json"), format!(r#"{{"version":1,"carte":"cas","pieces":[{piece}]{objets}}}"#)).unwrap();
        let vfs = Vfs::new();
        let scene = Scene::load(d.join("cas.vmf"), &vfs, &SceneOptions::default()).unwrap();
        valider(&scene, &vfs, &Config::default(), &[Famille::Semantique])
    };
    // sans déclaration : l'accueil n'a pas de comptoir
    let r = lancer("");
    assert_eq!(r.de("sem.role_incomplet").len(), 1);
    assert!(r.de("sem.role_incomplet")[0].message.contains("comptoir"));
    // déclaré dans la pièce : la règle est satisfaite ; déclaré ailleurs : non
    assert!(lancer(r#","objets":[{"nom":"comptoir_plateau","pos":[0,0,52]}]"#).de("sem.role_incomplet").is_empty());
    assert_eq!(lancer(r#","objets":[{"nom":"comptoir_plateau","pos":[900,0,52]}]"#).de("sem.role_incomplet").len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}
