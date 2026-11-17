//! Le mélange de deux textures (WorldVertexTransition) sur la carte graphique : une plaque vue de dessus, avec un
//! vrai matériau de mélange du jeu, rendue avec l'alpha des sommets à 0 (première texture, la roche) puis à 1
//! (seconde texture, l'herbe) : l'image change nettement. Contre-essai : sans seconde texture, l'alpha ne change rien.
use glam::Vec3;
use hm_render::vivant::{Bibliotheque, Cibles, LotMonde, OptionsVue, WVertex};
use hm_render::{Camera, Gpu, Lighting, Renderer, View};

const T: u32 = 64;

/// Couleur moyenne (RGB) de l'image d'une plaque de 256 × 256 de ce matériau, sommets à `alpha`.
fn moyenne(r: &Renderer, bib: &mut Bibliotheque, materiau: u32, alpha: f32) -> [f32; 3] {
    rendre(r, bib, materiau, alpha, 0, Vec3::ONE)
}

/// Pareil, dans le mode de rendu `rendu`, sous une lumière ambiante `ambiante` (sans soleil).
fn rendre(r: &Renderer, bib: &mut Bibliotheque, materiau: u32, alpha: f32, rendu: u32, ambiante: Vec3) -> [f32; 3] {
    rendre_avec(r, bib, materiau, alpha, OptionsVue { rendu, ..Default::default() }, ambiante, None).0
}

/// Pareil avec des options de vue et une caméra en perspective donnée ; rend aussi la luminosité la plus faible.
fn rendre_avec(r: &Renderer, bib: &mut Bibliotheque, materiau: u32, alpha: f32, opts: OptionsVue, ambiante: Vec3, cam: Option<Camera>) -> ([f32; 3], f32) {
    let d = &r.gpu.device;
    let sommets: Vec<WVertex> = [(-128.0, -128.0), (-128.0, 128.0), (128.0, 128.0), (128.0, -128.0)]
        .iter()
        .map(|&(x, y)| WVertex { pos: [x, y, 0.0], normal: [0.0, 0.0, 1.0], uv: [(x + 128.0) / 256.0, (y + 128.0) / 256.0], object: 1, alpha })
        .collect();
    let monde = bib.monde(r, &sommets, &[LotMonde { materiau, outil: false, ombre: false, indices: vec![0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] }], &[]);
    let cibles = Cibles::nouvelles(d, T, T);
    let (gb, gbg) = r.constantes_vue();
    let camera = cam.unwrap_or_else(|| Camera::top_down(Vec3::ZERO, 100.0, 1000.0, 2000.0));
    let light = Lighting { sun_dir: Vec3::Z, sun_color: Vec3::ZERO, ambient: ambiante, headlight: 0.0, sky: [0.0; 3] };
    let view = View { camera, width: T, height: T, show_tools: false, clip: None, light };
    let mut enc = d.create_command_encoder(&Default::default());
    r.dessiner_vivant(bib, &monde, &view, &cibles, &gb, &gbg, opts, &mut enc);
    let buf = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (T * T * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(cibles.couleur.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(T * 4), rows_per_image: Some(T) } }, wgpu::Extent3d { width: T, height: T, depth_or_array_layers: 1 });
    r.gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    d.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let px = buf.slice(..).get_mapped_range().unwrap().to_vec();
    let n = (T * T) as f32;
    let mut m = [0.0f32; 3];
    let mut plus_sombre = f32::MAX;
    for c in px.chunks_exact(4) {
        for k in 0..3 {
            m[k] += c[k] as f32 / n;
        }
        plus_sombre = plus_sombre.min(c[0] as f32 + c[1] as f32 + c[2] as f32);
    }
    (m, plus_sombre)
}

fn ecart(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).abs()).sum()
}

#[test]
fn melange_selon_l_alpha_des_sommets() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let melange = bib.materiau(&r, &vfs, "nature/blendrockgrass004a");
    assert!(melange.trouve);
    let (roche, herbe) = (moyenne(&r, &mut bib, melange.indice, 0.0), moyenne(&r, &mut bib, melange.indice, 1.0));
    let simple = bib.materiau(&r, &vfs, "dev/dev_measuregeneric01");
    let (s0, s1) = (moyenne(&r, &mut bib, simple.indice, 0.0), moyenne(&r, &mut bib, simple.indice, 1.0));
    println!("mélange : alpha 0 {roche:?}, alpha 1 {herbe:?} ; simple : {s0:?}, {s1:?}");
    assert!(ecart(roche, herbe) > 20.0, "l'alpha des sommets doit faire passer d'une texture à l'autre : {roche:?} → {herbe:?}");
    assert!(ecart(s0, s1) < 1.0, "sans seconde texture, l'alpha ne change rien : {s0:?} → {s1:?}");
}

/// Les modes de la vue 3D : « textures » ignore la lumière (plus clair qu'« ombrées » sous une lumière faible),
/// « polygones pleins » remplace la texture par une couleur unie propre à l'objet.
#[test]
fn modes_de_rendu() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let m = bib.materiau(&r, &vfs, "dev/dev_measuregeneric01");
    let faible = Vec3::splat(0.25);
    let (ombre, texture, plein) = (rendre(&r, &mut bib, m.indice, 0.0, 0, faible), rendre(&r, &mut bib, m.indice, 0.0, 1, faible), rendre(&r, &mut bib, m.indice, 0.0, 2, Vec3::ONE));
    let pleine_lumiere = rendre(&r, &mut bib, m.indice, 0.0, 0, Vec3::ONE);
    println!("ombrées {ombre:?}, textures {texture:?}, pleine lumière {pleine_lumiere:?}, polygones pleins {plein:?}");
    assert!(texture.iter().sum::<f32>() > ombre.iter().sum::<f32>() * 1.5, "textures : sans la lumière faible (écart réduit par le gamma)");
    assert!(ecart(texture, pleine_lumiere) < 3.0, "textures : comme en pleine lumière");
    assert!(ecart(plein, pleine_lumiere) > 20.0, "polygones pleins : plus la texture");
}

/// La grille 3D trace des traits sombres sur une surface ; le brouillard d'une vue en perspective tire la couleur
/// vers la sienne au-delà de son début.
#[test]
fn grille_3d_et_brouillard() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let m = bib.materiau(&r, &vfs, "dev/dev_measuregeneric01");
    let (sans, sombre_sans) = rendre_avec(&r, &mut bib, m.indice, 0.0, OptionsVue { rendu: 1, ..Default::default() }, Vec3::ONE, None);
    let (avec, sombre_avec) = rendre_avec(&r, &mut bib, m.indice, 0.0, OptionsVue { rendu: 1, grille: 16.0, ..Default::default() }, Vec3::ONE, None);
    println!("grille : moyenne {sans:?} → {avec:?}, plus sombre {sombre_sans} → {sombre_avec}");
    assert!(sombre_avec < sombre_sans * 0.85, "des traits sombres (moitié de la lumière, en sRGB)");
    assert!(ecart(sans, avec) > 3.0);
    // vue en perspective à 300 au-dessus de la plaque : brouillard rouge dès 10 unités, plein à 100
    let cam = Camera::from_angles(Vec3::new(0.0, 0.0, 300.0), 90.0, 0.0, 60.0);
    let (net, _) = rendre_avec(&r, &mut bib, m.indice, 0.0, OptionsVue { rendu: 1, ..Default::default() }, Vec3::ONE, Some(cam));
    let (rouge, _) = rendre_avec(&r, &mut bib, m.indice, 0.0, OptionsVue { rendu: 1, brouillard: Some(([1.0, 0.0, 0.0], 10.0, 100.0, 1.0)), ..Default::default() }, Vec3::ONE, Some(cam));
    println!("brouillard : {net:?} → {rouge:?}");
    // la plaque ne remplit pas l'image (fond noir) : plus de vert ni de bleu nulle part, et plus de rouge qu'avant
    assert!(rouge[0] > net[0] && rouge[1] < 1.0 && rouge[2] < 1.0, "tout est dans le brouillard rouge : {rouge:?}");
}

/// Ctrl+Maj+F2 : une face d'outil dessinée avec sa vraie texture (l'éditeur) se cache avec les textures d'outils.
/// Contre-essai : la même face, pas marquée outil, reste.
#[test]
fn textures_d_outils_masquees() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    bib.outils_reels = true;
    let nodraw = bib.materiau(&r, &vfs, "tools/toolsnodraw");
    let d = &r.gpu.device;
    let sommets: Vec<WVertex> = [(-128.0, -128.0), (-128.0, 128.0), (128.0, 128.0), (128.0, -128.0)]
        .iter()
        .map(|&(x, y)| WVertex { pos: [x, y, 0.0], normal: [0.0, 0.0, 1.0], uv: [0.0, 0.0], object: 1, alpha: 0.0 })
        .collect();
    let rendu = |bib: &mut Bibliotheque, outil: bool, montrer: bool| -> f32 {
        let monde = bib.monde(&r, &sommets, &[LotMonde { materiau: nodraw.indice, outil, ombre: false, indices: vec![0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] }], &[]);
        let cibles = Cibles::nouvelles(d, T, T);
        let (gb, gbg) = r.constantes_vue();
        let light = Lighting { sun_dir: Vec3::Z, sun_color: Vec3::ZERO, ambient: Vec3::ONE, headlight: 0.0, sky: [0.0; 3] };
        let view = View { camera: Camera::top_down(Vec3::ZERO, 100.0, 1000.0, 2000.0), width: T, height: T, show_tools: montrer, clip: None, light };
        let mut enc = d.create_command_encoder(&Default::default());
        r.dessiner_vivant(bib, &monde, &view, &cibles, &gb, &gbg, OptionsVue { rendu: 1, ..Default::default() }, &mut enc);
        r.gpu.queue.submit([enc.finish()]);
        let c = r.couleur_sous(&cibles, T / 2, T / 2);
        c[0] as f32 + c[1] as f32 + c[2] as f32
    };
    let (vue, masquee, pas_outil) = (rendu(&mut bib, true, true), rendu(&mut bib, true, false), rendu(&mut bib, false, false));
    println!("nodraw : montrée {vue}, masquée {masquee}, pas marquée outil {pas_outil}");
    assert!(vue > 200.0, "la face nodraw se voit avec les textures d'outils");
    assert!(masquee < 5.0, "masquée avec les textures d'outils (fond noir)");
    assert!(pas_outil > 200.0, "contre-essai : sans la marque outil, elle reste");
}

/// Reflet de la cubemap de l'éditeur (celle de Hammer++) sur l'eau (`nature/water_hrp1` de rp_rockford, sombre et opaque) : de
/// face il reste faible (Fresnel), en vue rasante il éclaircit nettement l'eau, comme dans Hammer++.
#[test]
fn reflet_de_l_eau() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let eau = bib.materiau(&r, &vfs, "nature/water_hrp1");
    if !eau.trouve {
        return; // l'addon de rp_rockford n'est pas là
    }
    assert!(hm_assets::texture::load_cubemap(&vfs, "engine/defaultcubemap").is_some(), "la cubemap par défaut du jeu");
    let opts = OptionsVue { rendu: 1, ..Default::default() };
    let (de_face, _) = rendre_avec(&r, &mut bib, eau.indice, 0.0, opts, Vec3::ONE, Some(Camera::from_angles(Vec3::new(0.0, 0.0, 200.0), 90.0, 0.0, 30.0)));
    let (rasant, _) = rendre_avec(&r, &mut bib, eau.indice, 0.0, opts, Vec3::ONE, Some(Camera::from_angles(Vec3::new(-127.0, 0.0, 6.0), 8.0, 0.0, 5.0)));
    println!("eau : de face {de_face:?}, rasante {rasant:?}");
    assert!(de_face.iter().sum::<f32>() < 60.0, "de face, l'eau reste sombre : {de_face:?}");
    // sans ombrage (rendu 1), sans reflet les deux vues auraient la couleur du brouillard de l'eau
    assert!(rasant.iter().sum::<f32>() > de_face.iter().sum::<f32>() * 1.5, "en vue rasante, le reflet l'éclaircit : {rasant:?}");
}

/// Le voile de reflet des vitrages (vues de contrôle) et son absence dans l'éditeur, qui les montre comme Hammer :
/// la vitre de rp_rockford (`ocrp/urban/window3`, translucide, `$envmaptint [.5 .5 .75]`) sur fond noir.
#[test]
fn voile_des_vitrages() {
    let Ok(gpu) = Gpu::new() else { return };
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let r = Renderer::new(gpu);
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    let mut bib = Bibliotheque::nouvelle(&r);
    let m = bib.materiau(&r, &vfs, "ocrp/urban/window3");
    if !m.trouve {
        return; // l'addon de rp_rockford n'est pas là
    }
    let (voile, _) = rendre_avec(&r, &mut bib, m.indice, 0.0, OptionsVue { rendu: 1, ..Default::default() }, Vec3::ONE, None);
    let (nu, _) = rendre_avec(&r, &mut bib, m.indice, 0.0, OptionsVue { rendu: 1, sans_voile: true, ..Default::default() }, Vec3::ONE, None);
    println!("vitre : avec voile {voile:?}, sans {nu:?}");
    assert!(voile.iter().sum::<f32>() > nu.iter().sum::<f32>() + 60.0, "le voile éclaircit nettement la vitre");
}
