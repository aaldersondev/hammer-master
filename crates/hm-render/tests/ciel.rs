//! Le ciel 2D d'un vrai ciel du jeu : la caméra au centre regarde chacun des 12 bords du cube ; de part et d'autre
//! du bord, l'image doit se raccorder (écart au bord comparable à l'écart entre deux colonnes voisines à l'intérieur
//! d'une face). Contre-essai : deux faces permutées, le raccord casse et la mesure le voit.
use glam::Vec3;
use hm_render::ciel::{CielRendu, SUFFIXES};
use hm_render::vivant::{Bibliotheque, Cibles, LotMonde, OptionsVue, WVertex};
use hm_render::{Camera, Gpu, Lighting, Renderer, View};

const T: u32 = 256;

fn rendre(r: &Renderer, bib: &mut Bibliotheque, ciel: CielRendu, dir: Vec3) -> Vec<[f32; 3]> {
    let monde = bib.monde(r, &[], &[], &[]);
    let px = pixels(r, bib, &monde, &ciel, Camera::look_at(Vec3::ZERO, dir, 60.0));
    px.chunks_exact(4).map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]).collect()
}

/// L'image RGBA de la vue 3D avec son ciel (fond noir sans ciel 2D), outils affichés.
fn pixels(r: &Renderer, bib: &Bibliotheque, monde: &hm_render::vivant::MondeGpu, ciel: &CielRendu, camera: Camera) -> Vec<u8> {
    let d = &r.gpu.device;
    let cibles = Cibles::nouvelles(d, T, T);
    let constantes = [r.constantes_vue(), r.constantes_vue()];
    let light = Lighting { sun_dir: Vec3::Z, sun_color: Vec3::ONE, ambient: Vec3::ONE, headlight: 0.0, sky: [0.0; 3] };
    let view = View { camera, width: T, height: T, show_tools: true, clip: None, light };
    let (gb, gbg) = r.constantes_vue();
    let mut enc = d.create_command_encoder(&Default::default());
    r.dessiner_ciel(bib, monde, &view, &cibles, ciel, &constantes, false, &mut enc);
    r.dessiner_vivant(bib, monde, &view, &cibles, &gb, &gbg, OptionsVue { eclaire: false, ciel: true, rendu: 0, ..Default::default() }, &mut enc);
    let buf = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (T * T * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(cibles.couleur.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(T * 4), rows_per_image: Some(T) } }, wgpu::Extent3d { width: T, height: T, depth_or_array_layers: 1 });
    r.gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buf.slice(..).get_mapped_range().unwrap().to_vec()
}

/// Des plaques verticales face à −x, à `x` de `o`, de (y0, −25) à (y1, 25), dessinées des deux côtés.
fn plaques(o: Vec3, x: f32, plaques: &[(u32, bool, f32, f32)]) -> (Vec<WVertex>, Vec<LotMonde>) {
    let (mut sommets, mut lots) = (Vec::new(), Vec::new());
    for &(mat, outil, y0, y1) in plaques {
        let b = sommets.len() as u32;
        for (y, z) in [(y0, -25.0), (y1, -25.0), (y1, 25.0), (y0, 25.0)] {
            sommets.push(WVertex { pos: (o + Vec3::new(x, y, z)).to_array(), normal: [-1.0, 0.0, 0.0], uv: [0.0, 0.0], object: 1, alpha: 0.0 });
        }
        lots.push(LotMonde { materiau: mat, outil, ombre: false, indices: vec![b, b + 1, b + 2, b, b + 2, b + 3, b, b + 2, b + 1, b, b + 3, b + 2] });
    }
    (sommets, lots)
}

/// Le pixel RGB sous un point du monde.
fn sous(px: &[u8], camera: &Camera, p: Vec3) -> [u8; 3] {
    let (x, y) = camera.project(p, T, T).unwrap();
    let k = ((y as u32).min(T - 1) * T + (x as u32).min(T - 1)) as usize * 4;
    [px[k], px[k + 1], px[k + 2]]
}

/// (écart au bord, écart de référence dans les faces) ; `vertical` : le bord est une colonne (sinon une ligne).
fn raccord(img: &[[f32; 3]], vertical: bool) -> (f32, f32) {
    let px = |a: u32, b: u32| if vertical { img[(b * T + a) as usize] } else { img[(a * T + b) as usize] };
    let ecart = |p: [f32; 3], q: [f32; 3]| (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs();
    let c = T / 2;
    let (mut bord, mut refe, mut n) = (0.0, 0.0, 0.0);
    for k in T / 5..T * 4 / 5 {
        bord += ecart(px(c - 2, k), px(c + 1, k));
        refe += (ecart(px(c - 22, k), px(c - 19, k)) + ecart(px(c + 19, k), px(c + 22, k))) / 2.0;
        n += 1.0;
    }
    (bord / n, refe / n)
}

#[test]
fn ciel_2d_raccorde() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let mut rapport = String::new();
    let mut mauvais = Vec::new();
    // un ciel de HL2 à côtés de demi-hauteur (et un bas de 4 × 4 : ses bords ne sont pas mesurés), un ciel complet
    for nom in ["sky_day01_01", "sky_wasteland02", "sky_c17_05"] {
        let (mut faces, mut demi) = ([0u32; 6], [false; 6]);
        for (i, s) in SUFFIXES.iter().enumerate() {
            let info = bib.materiau(&r, &vfs, &format!("skybox/{nom}{s}"));
            assert!(info.trouve, "skybox/{nom}{s} introuvable");
            faces[i] = info.indice;
            demi[i] = info.taille.1 * 2 == info.taille.0;
        }
        let complet = !demi.iter().any(|d| *d);
        // les 12 bords : 4 autour de l'horizon (bord vertical à l'image), 4 en haut, 4 en bas (bord horizontal)
        let mut bords: Vec<(Vec3, bool, String)> = Vec::new();
        for (x, y) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            bords.push((Vec3::new(x, y, 0.0), true, format!("horizon ({x}, {y})")));
        }
        for z in if complet { vec![1.0, -1.0] } else { vec![1.0] } {
            for d in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y] {
                bords.push((d + Vec3::Z * z, false, format!("{} ({:?})", if z > 0.0 { "haut" } else { "bas" }, d.to_array())));
            }
        }
        rapport += &format!("{nom}{}
", if complet { "" } else { " (côtés de demi-hauteur)" });
        for (d, vertical, b_nom) in &bords {
            let img = rendre(&r, &mut bib, CielRendu { faces: Some(faces), demi, ciel3d: None }, d.normalize());
            let (b, refe) = raccord(&img, *vertical);
            rapport += &format!("  {b_nom:24} bord {b:6.2}  intérieur {refe:6.2}
");
            if b > 2.0 * refe + 6.0 {
                mauvais.push(format!("{nom} {b_nom}"));
            }
        }
        // contre-essai : bk (+Y) et lf (−X) échangées, le bord entre +X et +Y casse
        let mut faux = faces;
        faux.swap(1, 2);
        let img = rendre(&r, &mut bib, CielRendu { faces: Some(faux), demi, ciel3d: None }, Vec3::new(1.0, 1.0, 0.0).normalize());
        let (b, refe) = raccord(&img, true);
        rapport += &format!("  contre-essai (bk et lf échangées) : bord {b:.2}, intérieur {refe:.2}
");
        // exigé sur le ciel de jour, contrasté (un ciel de nuit sombre casse moins fort)
        if nom == "sky_day01_01" {
            assert!(b > 2.0 * refe + 6.0, "{nom} : la mesure ne voit pas deux faces échangées (bord {b}, intérieur {refe})
{rapport}");
        }
    }
    println!("{rapport}");
    assert!(mauvais.is_empty(), "bords qui ne se raccordent pas : {mauvais:?}
{rapport}");
}

/// La skybox 3D comme en jeu : une face nodraw de son contenu n'y est pas dessinée, même quand l'éditeur montre les
/// outils avec leur vraie texture (sinon elle sort en grande bande jaune, × l'échelle, dans le ciel). Contre-essai :
/// une face blanche à côté, elle, y est.
#[test]
fn skybox_3d_sans_nodraw() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    bib.outils_reels = true;
    let nodraw = bib.materiau(&r, &vfs, "tools/toolsnodraw");
    assert!(nodraw.trouve);
    let blanc = bib.couleur(&r, [255, 255, 255, 255]);
    // deux plaques dans la boîte de la sky_camera : nodraw à gauche (+y), blanche à droite
    let o = Vec3::new(0.0, 0.0, 4000.0);
    let (sommets, lots) = plaques(o, 100.0, &[(nodraw.indice, true, 5.0, 30.0), (blanc, false, -30.0, -5.0)]);
    let monde = bib.monde(&r, &sommets, &lots, &[]);
    let camera = Camera::look_at(Vec3::ZERO, Vec3::X, 60.0);
    let ciel = CielRendu { faces: None, demi: [false; 6], ciel3d: Some((o, 16.0, (o - Vec3::splat(256.0), o + Vec3::splat(256.0)))) };
    let px = pixels(&r, &bib, &monde, &ciel, camera);
    // le centre de chaque plaque, ramené autour de la caméra : (p − o) × 16
    let (n, w) = (sous(&px, &camera, Vec3::new(1600.0, 17.5 * 16.0, 0.0)), sous(&px, &camera, Vec3::new(1600.0, -17.5 * 16.0, 0.0)));
    assert!(w.iter().all(|c| *c > 200), "la plaque blanche de la skybox 3D devrait se voir : {w:?}");
    assert!(n.iter().all(|c| *c < 20), "la face nodraw de la skybox 3D ne devrait pas se voir (fond noir) : {n:?}");
}

/// Une face de ciel de la map cache ce qui est derrière, comme en jeu : on y voit le ciel, pas la salle de la skybox 3D
/// posée au-dessus de la map. Contre-essai : la même plaque blanche sans face de ciel devant se voit.
#[test]
fn face_de_ciel_cache_ce_qui_est_derriere() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    bib.outils_reels = true;
    let ciel_mat = bib.materiau(&r, &vfs, "tools/toolsskybox");
    assert!(ciel_mat.trouve);
    let blanc = bib.couleur(&r, [255, 255, 255, 255]);
    let camera = Camera::look_at(Vec3::ZERO, Vec3::X, 60.0);
    let sans_ciel = CielRendu { faces: None, demi: [false; 6], ciel3d: None };
    // la plaque blanche loin derrière (x = 400) ; la face de ciel devant (x = 100) ne couvre que la gauche (+y)
    let blanche = plaques(Vec3::ZERO, 400.0, &[(blanc, false, -100.0, 100.0)]);
    let devant = plaques(Vec3::ZERO, 100.0, &[(ciel_mat.indice, true, 0.0, 100.0)]);
    let n = blanche.0.len() as u32;
    let sommets: Vec<WVertex> = blanche.0.iter().chain(&devant.0).copied().collect();
    let mut lots = blanche.1;
    lots.extend(devant.1.into_iter().map(|mut l| {
        l.indices.iter_mut().for_each(|i| *i += n);
        l
    }));
    let monde = bib.monde(&r, &sommets, &lots, &[]);
    let px = pixels(&r, &bib, &monde, &sans_ciel, camera);
    let (cache, libre) = (sous(&px, &camera, Vec3::new(400.0, 50.0, 0.0)), sous(&px, &camera, Vec3::new(400.0, -50.0, 0.0)));
    assert!(libre.iter().all(|c| *c > 200), "la plaque blanche sans face de ciel devant devrait se voir : {libre:?}");
    assert!(cache.iter().all(|c| *c < 20), "derrière la face de ciel, on devrait voir le ciel (fond noir) : {cache:?}");
}

/// Exploration : quels ciels du jeu ont un bas texturé (pour vérifier l'orientation de dn).
#[test]
#[ignore]
fn ciels_a_bas_texture() {
    let Ok(gpu) = Gpu::new() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(hm_assets::jeu::gameinfo_gmod().unwrap()).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let mut noms: Vec<String> = vfs.list_dans("materials/skybox", ".vmt").into_iter().filter_map(|(p, _)| p.strip_suffix("dn.vmt").map(|x| x.rsplit('/').next().unwrap().to_string())).collect();
    noms.sort();
    noms.dedup();
    for nom in noms {
        let mut faces = [0u32; 6];
        let mut ok = true;
        for (i, s) in SUFFIXES.iter().enumerate() {
            let info = bib.materiau(&r, &vfs, &format!("skybox/{nom}{s}"));
            ok &= info.trouve;
            faces[i] = info.indice;
        }
        if !ok {
            continue;
        }
        let img = rendre(&r, &mut bib, CielRendu { faces: Some(faces), demi: [false; 6], ciel3d: None }, Vec3::new(0.001, 0.0, -1.0).normalize());
        let m: [f32; 3] = img.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]).map(|x| x / img.len() as f32);
        let v: f32 = img.iter().map(|p| (0..3).map(|k| (p[k] - m[k]).powi(2)).sum::<f32>()).sum::<f32>() / img.len() as f32;
        println!("{nom:28} variance du bas {v:8.1}");
    }
}
