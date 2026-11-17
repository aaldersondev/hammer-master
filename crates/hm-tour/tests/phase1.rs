//! Tests de la phase 1 : une map d'essai fabriquée ici (deux pièces, une porte), puis les critères mesurés sur
//! rp_megalopole quand le projet est présent sur ce PC.
use std::path::{Path, PathBuf};
use std::time::Instant;

use glam::Vec3;
use hm_assets::Vfs;
use hm_espace::Field;
use hm_render::{Camera, Gpu, Lighting, Renderer, View};
use hm_scene::{Scene, SceneOptions};

/// Un solide en boîte, avec les trois points de chaque plan en sens horaire vus de l'extérieur.
fn boite(id: &mut u32, a: [i32; 3], b: [i32; 3], mat: &str) -> String {
    let (x0, y0, z0, x1, y1, z1) = (a[0], a[1], a[2], b[0], b[1], b[2]);
    let plans = [
        format!("({x0} {y1} {z1}) ({x1} {y1} {z1}) ({x1} {y0} {z1})"),
        format!("({x0} {y0} {z0}) ({x1} {y0} {z0}) ({x1} {y1} {z0})"),
        format!("({x0} {y1} {z1}) ({x0} {y0} {z1}) ({x0} {y0} {z0})"),
        format!("({x1} {y1} {z0}) ({x1} {y0} {z0}) ({x1} {y0} {z1})"),
        format!("({x1} {y1} {z1}) ({x0} {y1} {z1}) ({x0} {y1} {z0})"),
        format!("({x1} {y0} {z0}) ({x0} {y0} {z0}) ({x0} {y0} {z1})"),
    ];
    *id += 1;
    let mut s = format!("\tsolid\n\t{{\n\t\t\"id\" \"{id}\"\n");
    for p in plans {
        *id += 1;
        s += &format!("\t\tside\n\t\t{{\n\t\t\t\"id\" \"{id}\"\n\t\t\t\"plane\" \"{p}\"\n\t\t\t\"material\" \"{mat}\"\n\t\t\t\"uaxis\" \"[1 0 0 0] 0.25\"\n\t\t\t\"vaxis\" \"[0 -1 0 0] 0.25\"\n\t\t\t\"lightmapscale\" \"16\"\n\t\t}}\n");
    }
    s + "\t}\n"
}

/// Deux pièces de 256 × 256 × 128 côte à côte, séparées par un mur percé d'une porte de 64 ; une table dans la
/// seconde. Tout est fermé : sol, plafond, quatre murs.
fn carte_deux_pieces() -> PathBuf {
    let mut id = 1;
    let m = "dev/dev_measurewall01a";
    let mut w = String::from("world\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n");
    w += &boite(&mut id, [-16, -16, -16], [544, 272, 0], m); // sol
    w += &boite(&mut id, [-16, -16, 128], [544, 272, 144], m); // plafond
    w += &boite(&mut id, [-16, -16, 0], [0, 272, 128], m); // mur ouest
    w += &boite(&mut id, [528, -16, 0], [544, 272, 128], m); // mur est
    w += &boite(&mut id, [0, -16, 0], [528, 0, 128], m); // mur sud
    w += &boite(&mut id, [0, 256, 0], [528, 272, 128], m); // mur nord
    w += &boite(&mut id, [256, 0, 0], [272, 96, 128], m); // cloison, avant la porte
    w += &boite(&mut id, [256, 160, 0], [272, 256, 128], m); // cloison, après la porte
    w += &boite(&mut id, [256, 96, 112], [272, 160, 128], m); // linteau
    w += &boite(&mut id, [380, 100, 0], [440, 160, 40], m); // une « table » en brosse
    w += "}\n";
    let p = std::env::temp_dir().join(format!("hm_deux_pieces_{}.vmf", std::process::id()));
    std::fs::write(&p, w).unwrap();
    p
}

#[test]
fn deux_pieces_et_une_porte() {
    let p = carte_deux_pieces();
    let scene = Scene::load(&p, &Vfs::new(), &SceneOptions::default()).unwrap();
    assert_eq!(scene.report.solids, 10);
    assert_eq!(scene.report.bad_brushes, 0);
    let field = Field::build(&scene);
    for r in &field.regions {
        println!("  région {} : {} cellules, ({:.0} {:.0} {:.0}) à ({:.0} {:.0} {:.0}), couvert {:.2}, bords sur le vide {:.2}, petite {}, pièce {}", r.id, r.cells, r.min.x, r.min.y, r.min.z, r.max.x, r.max.y, r.max.z, r.interior, r.drop_edges, r.small, r.is_room());
    }
    let pieces: Vec<_> = field.regions.iter().filter(|r| r.is_room()).collect();
    assert_eq!(pieces.len(), 2, "deux pièces attendues : {}", field.summary());
    for r in &pieces {
        // le dessus de la table (40) fait partie de la seconde pièce : le sol moyen monte un peu
        assert!(r.floor >= 0.0 && r.floor < 8.0 && (r.ceil - 128.0).abs() < 1.0);
        assert!(r.max.x - r.min.x > 200.0 && r.max.y - r.min.y > 200.0);
    }
    // le rayon voit la cloison, puis passe par la porte
    let (d, _) = scene.rayon(Vec3::new(100.0, 40.0, 64.0), Vec3::X).unwrap();
    assert!((d - 156.0).abs() < 0.5, "cloison à 156, trouvé {d}");
    let (d, _) = scene.rayon(Vec3::new(100.0, 128.0, 64.0), Vec3::X).unwrap();
    assert!((d - 428.0).abs() < 0.5, "mur est à 428 par la porte, trouvé {d}");
    // le tour donne au moins une vue par pièce
    let vues = hm_tour::vues_pieces(&field, &scene);
    assert!(vues.len() >= 2);
    assert!(vues.iter().all(|v| field.libre(v.camera.eye)), "une caméra du tour est dans un mur");
    // rendu : l'image n'est pas vide et l'identifiant sous le centre est un solide de la map
    let Ok(gpu) = Gpu::new() else {
        println!("pas d'adaptateur graphique : rendu non testé");
        return;
    };
    let renderer = Renderer::new(gpu);
    let gs = renderer.upload(&scene);
    let frame = renderer.render(&gs, &View { camera: vues[0].camera, width: 320, height: 180, show_tools: false, clip: None, light: Lighting::neutral(&scene) });
    let id = frame.id_at(160, 90);
    assert!(id != 0 && (id as usize) < scene.objects.len(), "le centre de la vue ne montre aucun objet");
    assert!(frame.ids.iter().filter(|&&i| i != 0).count() > 320 * 180 * 9 / 10, "une pièce fermée ne doit pas montrer le vide");
    // une vue rendue deux fois a la même empreinte
    let mut ident = std::collections::BTreeMap::new();
    let e1 = hm_tour::suivi::empreinte_camera(&renderer, &gs, &scene, vues[0].camera, &mut ident);
    let e2 = hm_tour::suivi::empreinte_camera(&renderer, &gs, &scene, vues[0].camera, &mut ident);
    assert_eq!(e1, e2);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn criteres_sur_rp_megalopole() {
    let vmf = format!("{}/Desktop/rp_megalopole/out/rp_megalopole_v1.vmf", std::env::var("USERPROFILE").unwrap_or_default());
    if !Path::new(&vmf).is_file() {
        println!("rp_megalopole absent : critères non mesurés");
        return;
    }
    let Ok(gpu) = Gpu::new() else {
        println!("pas d'adaptateur graphique");
        return;
    };
    let t0 = Instant::now();
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(format!("{}/Desktop/rp_megalopole/compile/gameinfo.txt", std::env::var("USERPROFILE").unwrap())).unwrap();
    let scene = Scene::load(&vmf, &vfs, &SceneOptions::default()).unwrap();
    let renderer = Renderer::new(gpu);
    let gs = renderer.upload(&scene);
    let ouverture = t0.elapsed().as_secs_f32();
    assert!(scene.report.missing_models.is_empty(), "modèles illisibles : {:?}", scene.report.missing_models);
    assert!(scene.report.missing_materials.is_empty(), "matériaux introuvables : {:?}", scene.report.missing_materials);
    assert!(scene.props.len() > 15000 && scene.textures.len() > 500);
    let camera = Camera::from_angles(Vec3::new(-10783.8, -807.8, 64.0), -9.6, 3.2, hm_render::fov_y_from_game(90.0));
    let view = View { camera, width: 1280, height: 720, show_tools: false, clip: None, light: Lighting::neutral(&scene) };
    let _ = renderer.render(&gs, &view); // première image : mise en route des pipelines
    let t1 = Instant::now();
    let frame = renderer.render(&gs, &view);
    let vue = t1.elapsed().as_secs_f32();
    let objets: std::collections::HashSet<u32> = frame.ids.iter().copied().filter(|&i| i != 0).collect();
    println!("rp_megalopole : ouverture {ouverture:.2} s, vue {:.0} ms, {} objets visibles, {}", vue * 1e3, objets.len(), renderer.gpu.info);
    assert!(vue < 3.0, "critère de la phase 1 : une vue en moins de 3 s (mesuré {vue:.2} s)");
    assert!(ouverture < 30.0);
    assert!(objets.len() > 40);
}
