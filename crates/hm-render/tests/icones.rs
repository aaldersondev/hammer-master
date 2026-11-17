//! Les icônes des entités (`iconsprite`) sont des panneaux du monde, tournés vers la caméra par le shader (sommets
//! marqués -2, coin dans la normale) : vus de face comme de biais ou d'en haut, et cachés par un mur devant, comme
//! dans Hammer (comparé le 15/11 sur rp_rockford, où nos icônes peintes par-dessus l'image traversaient les vitres).
use glam::Vec3;
use hm_render::vivant::{Bibliotheque, Cibles, LotMonde, OptionsVue, WVertex};
use hm_render::{Camera, Gpu, Lighting, Renderer, View};

const T: u32 = 64;

/// Le pixel du centre de l'image (sRGB), caméra en `oeil` tournée vers l'origine, avec l'icône à l'origine et,
/// s'il est donné, un mur rouge de 64 × 64 centré en `mur` face à la caméra.
fn centre(r: &Renderer, bib: &mut Bibliotheque, icone: u32, marque: f32, oeil: Vec3, mur: Option<Vec3>) -> [u8; 4] {
    let d = &r.gpu.device;
    let mut sommets: Vec<WVertex> = [(-8.0, -8.0, [0.0, 1.0]), (-8.0, 8.0, [0.0, 0.0]), (8.0, 8.0, [1.0, 0.0]), (8.0, -8.0, [1.0, 1.0])]
        .iter()
        .map(|&(x, y, uv)| WVertex { pos: [0.0; 3], normal: [x, y, 0.0], uv, object: 1, alpha: marque })
        .collect();
    let deux_sens = |b: u32| vec![b, b + 1, b + 2, b, b + 2, b + 3, b, b + 2, b + 1, b, b + 3, b + 2];
    let mut lots = vec![LotMonde { materiau: icone, outil: false, ombre: false, indices: deux_sens(0) }];
    if let Some(c) = mur {
        // un carré perpendiculaire au regard
        let f = (-oeil).normalize();
        let droite = f.cross(Vec3::Z).normalize_or(Vec3::X);
        let haut = droite.cross(f);
        for (a, b) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)] {
            sommets.push(WVertex { pos: (c + droite * 32.0 * a + haut * 32.0 * b).to_array(), normal: (-f).to_array(), uv: [0.0, 0.0], object: 2, alpha: 0.0 });
        }
        lots.push(LotMonde { materiau: bib.couleur(r, [255, 0, 0, 255]), outil: false, ombre: false, indices: deux_sens(4) });
    }
    let monde = bib.monde(r, &sommets, &lots, &[]);
    let cibles = Cibles::nouvelles(d, T, T);
    let (gb, gbg) = r.constantes_vue();
    let dir = (-oeil).normalize();
    let camera = Camera::from_angles(oeil, (-dir.z).asin().to_degrees(), dir.y.atan2(dir.x).to_degrees(), 30.0);
    let light = Lighting { sun_dir: Vec3::Z, sun_color: Vec3::ZERO, ambient: Vec3::ONE, headlight: 0.0, sky: [0.0; 3] };
    let view = View { camera, width: T, height: T, show_tools: false, clip: None, light };
    let mut enc = d.create_command_encoder(&Default::default());
    r.dessiner_vivant(bib, &monde, &view, &cibles, &gb, &gbg, OptionsVue { rendu: 1, ..Default::default() }, &mut enc);
    r.gpu.queue.submit([enc.finish()]);
    r.couleur_sous(&cibles, T / 2, T / 2)
}

#[test]
fn icone_face_a_la_camera_et_cachee_par_un_mur() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let icone = bib.materiau(&r, &vfs, "editor/light");
    assert!(icone.trouve, "editor/light est dans les fichiers du jeu");
    let clair = |c: [u8; 4]| c[0] as u32 + c[1] as u32 + c[2] as u32 > 300;
    for oeil in [Vec3::new(100.0, 0.0, 0.0), Vec3::new(0.0, -70.0, 70.0), Vec3::new(-60.0, 60.0, 30.0), Vec3::new(0.5, 0.0, 120.0)] {
        let vu = centre(&r, &mut bib, icone.indice, -2.0, oeil, None);
        println!("depuis {oeil:?} : {vu:?}");
        assert!(clair(vu), "l'ampoule se voit depuis {oeil:?} : {vu:?}");
        // un mur à mi-chemin la cache
        let cache = centre(&r, &mut bib, icone.indice, -2.0, oeil, Some(oeil * 0.5));
        assert!(cache[0] > 200 && cache[1] < 40, "le mur rouge cache l'icône depuis {oeil:?} : {cache:?}");
    }
    // contre-essai : sans la marque, les quatre sommets restent au même point (rien à voir)
    let rien = centre(&r, &mut bib, icone.indice, 0.0, Vec3::new(100.0, 0.0, 0.0), None);
    assert!(!clair(rien), "sans la marque, pas de panneau : {rien:?}");
}
