//! L'aperçu de l'éclairage sur la carte graphique : un sol vu de dessus, une plaque au-dessus, une lumière (puis un
//! soleil et un ciel). Les pixels rendus sont comparés à la formule de `hm_doc::lumiere` (vérifiée contre les
//! lightmaps de vrad dans `hm-compile`) ; l'ombre de la plaque doit tomber là où la géométrie l'envoie.
use glam::Vec3;
use hm_render::eclairage::{Eclairage, LumiereRendu};
use hm_render::vivant::{Bibliotheque, Cibles, LotMonde, WVertex};
use hm_render::{Camera, Gpu, Lighting, Renderer, View};

const T: u32 = 256;

/// Un quadrilatère horizontal à la hauteur `z`, des deux côtés.
fn quad(sommets: &mut Vec<WVertex>, idx: &mut Vec<u32>, lo: [f32; 2], hi: [f32; 2], z: f32) {
    let b = sommets.len() as u32;
    for (x, y) in [(lo[0], lo[1]), (lo[0], hi[1]), (hi[0], hi[1]), (hi[0], lo[1])] {
        sommets.push(WVertex { pos: [x, y, z], normal: [0.0, 0.0, 1.0], uv: [0.0, 0.0], object: 1, alpha: 0.0 });
    }
    idx.extend([b, b + 1, b + 2, b, b + 2, b + 3, b, b + 2, b + 1, b, b + 3, b + 2]);
}

/// L'éclairement linéaire (sRGB décodé) des pixels sous des points du sol.
fn rendre(r: &Renderer, bib: &mut Bibliotheque, e: &Eclairage, points: &[[f32; 2]]) -> Vec<f32> {
    let d = &r.gpu.device;
    let blanc = bib.couleur(r, [255, 255, 255, 255]);
    let (mut sommets, mut idx) = (Vec::new(), Vec::new());
    quad(&mut sommets, &mut idx, [-512.0, -512.0], [512.0, 512.0], 0.0);
    // la plaque : en dessous de la lumière, décalée vers +x
    quad(&mut sommets, &mut idx, [150.0, -50.0], [250.0, 50.0], 100.0);
    let monde = bib.monde(r, &sommets, &[LotMonde { materiau: blanc, outil: false, ombre: true, indices: idx }], &[]);
    let cibles = Cibles::nouvelles(d, T, T);
    let (gb, gbg) = r.constantes_vue();
    let camera = Camera::top_down(Vec3::ZERO, 512.0, 1000.0, 2000.0);
    let light = Lighting { sun_dir: Vec3::Z, sun_color: Vec3::ONE, ambient: Vec3::ZERO, headlight: 0.0, sky: [0.0; 3] };
    let view = View { camera, width: T, height: T, show_tools: false, clip: None, light };
    let mut enc = d.create_command_encoder(&Default::default());
    r.preparer_eclairage(bib, &monde, 1, e, &mut enc);
    r.dessiner_vivant(bib, &monde, &view, &cibles, &gb, &gbg, hm_render::vivant::OptionsVue { eclaire: true, ciel: false, rendu: 0, ..Default::default() }, &mut enc);
    let buf = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (T * T * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(cibles.couleur.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(T * 4), rows_per_image: Some(T) } }, wgpu::Extent3d { width: T, height: T, depth_or_array_layers: 1 });
    r.gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let px = buf.slice(..).get_mapped_range().unwrap().to_vec();
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    points
        .iter()
        .map(|p| {
            let (x, y) = camera.project(Vec3::new(p[0], p[1], 0.0), T, T).unwrap();
            let k = ((y as u32).min(T - 1) * T + (x as u32).min(T - 1)) as usize * 4;
            lin(px[k])
        })
        .collect()
}

#[test]
fn lumiere_et_ombre_sur_la_carte_graphique() {
    let Ok(gpu) = Gpu::new() else { return };
    let r = Renderer::new(gpu);
    let mut bib = Bibliotheque::nouvelle(&r);
    let bornes = (Vec3::new(-512.0, -512.0, 0.0), Vec3::new(512.0, 512.0, 100.0));

    // une lumière quadratique à 200 au-dessus du milieu : « 255 255 255 200 », ramenée à 100 unités
    let i = 200.0f32 / 255.0 * 10000.0;
    let l = LumiereRendu { position: Vec3::new(0.0, 0.0, 200.0), couleur: Vec3::splat(i), attenuation: [0.0, 0.0, 1.0], fondu: None, cone: None };
    let attendu = |p: [f32; 2]| {
        let v = l.position - Vec3::new(p[0], p[1], 0.0);
        i * (v.z / v.length()) / v.length_squared()
    };
    let e = Eclairage { lumieres: vec![l], soleil: None, ciel: Vec3::ZERO, ajout: Vec3::ZERO, ombres: true, bornes };
    // éclairés : à gauche, en haut, en bas ; dans l'ombre de la plaque : vers +x (elle tombe sur [300, 500])
    let points = [[-300.0, 0.0], [0.0, 300.0], [-100.0, -250.0], [400.0, 0.0], [350.0, 40.0]];
    let lu = rendre(&r, &mut bib, &e, &points);
    eprintln!("{} ; rendu {lu:?} ; attendu {:?}", r.gpu.info, points.iter().map(|p| attendu(*p)).collect::<Vec<_>>());
    for (k, p) in points.iter().enumerate().take(3) {
        let a = attendu(*p);
        assert!((lu[k] - a).abs() < 0.03 * a + 0.004, "point {p:?} : rendu {} pour {a} attendu", lu[k]);
    }
    for k in 3..5 {
        assert!(lu[k] < 0.01, "point {:?} : {} (devrait être dans l'ombre de la plaque, sinon {})", points[k], lu[k], attendu(points[k]));
    }
    // sans ombres : le même point est éclairé
    let e2 = Eclairage { ombres: false, ..e.clone() };
    let lu2 = rendre(&r, &mut bib, &e2, &points[3..4]);
    assert!((lu2[0] - attendu(points[3])).abs() < 0.03 * attendu(points[3]) + 0.004, "sans ombre : {} pour {}", lu2[0], attendu(points[3]));

    // un soleil à 45° venant de -x, et le ciel : l'ombre de la plaque tombe sur [250, 350] en x
    let soleil = Vec3::new(-1.0, 0.0, 1.0).normalize();
    let e3 = Eclairage { lumieres: vec![], soleil: Some((soleil, Vec3::splat(0.5))), ciel: Vec3::splat(0.2), ajout: Vec3::ZERO, ombres: true, bornes };
    let lu3 = rendre(&r, &mut bib, &e3, &[[-300.0, 0.0], [300.0, 0.0]]);
    let plein = 0.5 * soleil.z + 0.2;
    eprintln!("soleil : rendu {lu3:?} ; plein {plein}");
    assert!((lu3[0] - plein).abs() < 0.03 * plein + 0.004, "au soleil : {} pour {plein}", lu3[0]);
    assert!(lu3[1] < 0.2 + 0.01 && lu3[1] > 0.2 * 0.6, "à l'ombre du soleil, sous le ciel : {} (ciel seul 0.2)", lu3[1]);
}
