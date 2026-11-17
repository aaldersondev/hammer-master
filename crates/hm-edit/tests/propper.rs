//! Propper++ : deux blocs empilés deviennent un modèle sans leurs faces enfouies, deux pièces de collision, une face
//! d'outil ignorée ; sommets ramenés au milieu du dessous ; triangles dans le sens direct vu de dehors ; UV de Hammer.
use glam::DVec3;
use hm_assets::Vfs;
use hm_doc::{Commande, Ref, Session};
use hm_edit::propper::{preparer, OptionsPropper};

fn options() -> OptionsPropper {
    OptionsPropper { modele: "hammer_master/essai".into(), surfaceprop: "concrete".into(), masse: 500.0, collision: true }
}

/// Les triangles d'un SMD : (matériau, trois sommets (position, normale, uv)).
fn triangles(smd: &str) -> Vec<(String, Vec<([f64; 3], [f64; 3], [f64; 2])>)> {
    let l: Vec<&str> = smd.lines().skip_while(|x| *x != "triangles").skip(1).take_while(|x| *x != "end").collect();
    l.chunks(4)
        .map(|c| {
            let s = c[1..]
                .iter()
                .map(|x| {
                    let v: Vec<f64> = x.split_whitespace().skip(1).map(|n| n.parse().unwrap()).collect();
                    ([v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7]])
                })
                .collect();
            (c[0].to_string(), s)
        })
        .collect()
}

#[test]
fn deux_blocs_empiles() {
    let mut s = Session::nouvelle();
    let bloc = |s: &mut Session, lo: [f64; 3], hi: [f64; 3], m: &str| s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: m.into(), classe: None }).unwrap().crees[0];
    let a = bloc(&mut s, [100.0, 200.0, 0.0], [228.0, 264.0, 32.0], "brick/brickwall001a");
    let b = bloc(&mut s, [100.0, 200.0, 32.0], [228.0, 264.0, 96.0], "concrete/concretefloor001a");
    let p = preparer(s.doc(), &[a, b], &Vfs::new(), &options()).unwrap();
    // 12 faces, dont le dessus de a et le dessous de b enfouis l'un dans l'autre
    assert_eq!((p.faces_enfouies, p.triangles, p.pieces), (2, 20, 2));
    assert_eq!(p.origine, DVec3::new(164.0, 232.0, 0.0), "milieu du dessous");
    let t = triangles(&p.smd);
    assert_eq!(t.len(), 20);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for (_, v) in &t {
        for (q, ..) in v {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        // sens direct vu de dehors : (b − a) × (c − a) va dans le sens de la normale
        let d = |i: usize, j: usize| DVec3::from(v[i].0) - DVec3::from(v[j].0);
        assert!(d(1, 0).cross(d(2, 0)).dot(DVec3::from(v[0].1)) > 0.0);
    }
    assert_eq!((lo, hi), ([-64.0, -32.0, 0.0], [64.0, 32.0, 96.0]));
    assert!(t.iter().any(|(m, _)| m == "brick_brickwall001a") && t.iter().any(|(m, _)| m == "concrete_concretefloor001a"));
    // UV de Hammer : face du dessus de b, axes du monde, échelle 0,25, texture de 512 (VFS vide) : 128 unités = 1
    let dessus: Vec<_> = t.iter().filter(|(_, v)| v.iter().all(|(q, ..)| q[2] == 96.0)).collect();
    assert_eq!(dessus.len(), 2);
    let us: Vec<f64> = dessus.iter().flat_map(|(_, v)| v.iter().map(|x| x.2[0])).collect();
    let ecart = us.iter().cloned().fold(f64::MIN, f64::max) - us.iter().cloned().fold(f64::MAX, f64::min);
    assert!((ecart - 1.0).abs() < 1e-4, "128 unités de large = une répétition : {ecart}");
    // collision : deux pièces fermées de 12 triangles, rétrécies, chacune d'une seule normale
    let ph = triangles(p.phys.as_ref().unwrap());
    assert_eq!(ph.len(), 24);
    let normales: std::collections::BTreeSet<String> = ph.iter().map(|(_, v)| format!("{:?}", v[0].1)).collect();
    assert_eq!(normales.len(), 2);
    assert!(ph.iter().all(|(_, v)| v.iter().all(|(q, ..)| q[2] > 0.0 && q[0] > -64.0)), "rétrécies dans la boîte");
    // QC et VMT
    for x in ["$modelname \"hammer_master/essai.mdl\"", "$origin 0 0 0 -90", "$cdmaterials \"models/hammer_master/essai/\"", "$concave", "$maxconvexpieces 10", "$staticprop"] {
        assert!(p.qc.contains(x), "{x} manque :\n{}", p.qc);
    }
    assert_eq!(p.vmts.len(), 2);
    assert!(p.vmts.iter().any(|(c, t)| c == "materials/models/hammer_master/essai/brick_brickwall001a.vmt" && t.contains("VertexLitGeneric") && t.contains("\"$basetexture\" \"brick/brickwall001a\"")));
}

/// Les solides d'une func_detail entrent dans le modèle ; un trigger et une entité ponctuelle sélectionnés avec n'y
/// entrent pas et ne sont pas dans ce que le prop remplace.
#[test]
fn entites_a_brosses() {
    let mut s = Session::nouvelle();
    let bloc = |s: &mut Session, lo: [f64; 3], hi: [f64; 3], m: &str| s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: m.into(), classe: None }).unwrap().crees[0];
    let monde = bloc(&mut s, [0.0; 3], [64.0, 64.0, 16.0], "brick/brickwall001a");
    let d = bloc(&mut s, [0.0, 0.0, 16.0], [32.0, 32.0, 48.0], "brick/brickwall001a");
    let detail = s.executer(Commande::LierAEntite { objets: vec![d], classe: "func_detail".into() }).unwrap().crees[0];
    let t = bloc(&mut s, [100.0, 0.0, 0.0], [164.0, 64.0, 64.0], "tools/toolstrigger");
    let trigger = s.executer(Commande::LierAEntite { objets: vec![t], classe: "trigger_multiple".into() }).unwrap().crees[0];
    let lumiere = s.executer(Commande::CreerEntite { classe: "light".into(), origine: [0.0, 0.0, 100.0], cles: vec![] }).unwrap().crees[0];
    let p = preparer(s.doc(), &[monde, detail, trigger, lumiere], &Vfs::new(), &options()).unwrap();
    assert_eq!(p.pieces, 2, "le monde et la func_detail");
    assert!(p.utilises.contains(&monde) && p.utilises.contains(&detail));
    assert!(!p.utilises.contains(&trigger) && !p.utilises.contains(&lumiere));
}

#[test]
fn faces_d_outil_et_refus() {
    let mut s = Session::nouvelle();
    let a = s.executer(Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: "tools/toolsnodraw".into(), classe: None }).unwrap().crees[0];
    // tout en nodraw : rien à dessiner
    assert!(preparer(s.doc(), &[a], &Vfs::new(), &options()).is_err());
    let Ref::Solide(id) = a else { panic!() };
    let face = s.doc().solide(id).unwrap().faces[0].id;
    s.executer(Commande::MateriauFaces { faces: vec![hm_doc::RefFace { solide: id, face }], materiau: "brick/brickwall001a".into() }).unwrap();
    let p = preparer(s.doc(), &[a], &Vfs::new(), &options()).unwrap();
    assert_eq!((p.triangles, p.pieces), (2, 1), "seule la face peinte se dessine, la brosse entière fait la collision");
    assert!(preparer(s.doc(), &[a], &Vfs::new(), &OptionsPropper { modele: "a b".into(), ..options() }).is_err());
    let p = preparer(s.doc(), &[a], &Vfs::new(), &OptionsPropper { collision: false, ..options() }).unwrap();
    assert!(p.phys.is_none() && !p.qc.contains("$collisionmodel"));
}

/// Avec le jeu installé : studiomdl compile le propper et la boîte du modèle est celle des brosses, ramenée à
/// l'origine (axes de la map gardés). Les fichiers d'essai sont retirés du jeu ensuite.
#[test]
fn compile_avec_studiomdl() {
    let (Some(gi), Some(_)) = (hm_assets::jeu::gameinfo_gmod(), hm_assets::jeu::garrysmod()) else { return };
    let jeu = gi.parent().unwrap().to_path_buf();
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(&gi).unwrap();
    let mut s = Session::nouvelle();
    let a = s.executer(Commande::CreerBloc { min: [100.0, 200.0, 0.0], max: [228.0, 264.0, 32.0], materiau: "brick/brickwall001a".into(), classe: None }).unwrap().crees[0];
    let b = s.executer(Commande::CreerBloc { min: [100.0, 200.0, 32.0], max: [132.0, 216.0, 96.0], materiau: "concrete/concretefloor001a".into(), classe: None }).unwrap().crees[0];
    let o = OptionsPropper { modele: "hammer_master_essai/propper_test".into(), ..options() };
    let p = preparer(s.doc(), &[a, b], &vfs, &o).unwrap();
    let src = std::env::temp_dir().join(format!("hm_propper_{}", std::process::id()));
    let qc = hm_edit::propper::ecrire(&p, &src, &jeu).unwrap();
    let r = hm_edit::propper::compiler(&qc, &jeu);
    let dossier = jeu.join("models/hammer_master_essai");
    let bornes = hm_edit::propper::bornes_mdl(&dossier.join("propper_test.mdl"));
    let phy = dossier.join("propper_test.phy").is_file();
    let _ = std::fs::remove_dir_all(&dossier);
    let _ = std::fs::remove_dir_all(jeu.join("materials/models/hammer_master_essai"));
    let _ = std::fs::remove_dir_all(&src);
    let journal = r.unwrap();
    println!("{}", journal.lines().filter(|l| l.contains("convex") || l.contains("WARNING")).collect::<Vec<_>>().join("\n"));
    assert!(journal.contains("2 convex sub-parts") || journal.contains("Model has 2 convex"), "deux pièces de collision :\n{journal}");
    assert!(phy, "le .phy (collision) est écrit");
    let (lo, hi) = bornes.expect("le .mdl est écrit");
    // brosses de x 100..228, y 200..264, z 0..96 ; origine (164, 232, 0)
    assert!(lo.iter().zip([-64.0, -32.0, 0.0]).all(|(a, b)| (a - b).abs() < 0.5) && hi.iter().zip([64.0, 32.0, 96.0]).all(|(a, b)| (a - b).abs() < 0.5), "boîte {lo:?} {hi:?}");
}
