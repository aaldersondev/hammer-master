//! Banc des lecteurs de formats, sur les vrais fichiers de ce PC, à travers l'interface de `hm-assets`.
//! Il garde les versions fixées de `vformats`, `source-mdl` et `source-vpk` : une montée de version qui lit moins
//! bien fait échouer ce test. Chiffres de référence du 12/11/2026 dans `eval/resultats_2026-11-12.txt`.
use std::path::Path;
use std::time::Instant;

use hm_assets::{load_material, load_model, load_texture, Vfs};
use rayon::prelude::*;

const GMOD: &str = r"C:\Program Files (x86)\Steam\steamapps\common\GarrysMod";

fn banc(nom: &str, vfs: &Vfs, max_modeles_ko: usize) {
    let t = Instant::now();
    let vmt = vfs.list(".vmt");
    let ko_vmt: Vec<_> = vmt.par_iter().filter(|(n, _)| n.starts_with("materials/") && !load_material(vfs, n).found).map(|(n, _)| n.clone()).collect();
    let vtf = vfs.list(".vtf");
    let ko_vtf: Vec<_> = vtf.par_iter().filter(|(n, _)| n.starts_with("materials/") && load_texture(vfs, n, 64).is_none()).map(|(n, _)| n.clone()).collect();
    let mdl: Vec<_> = vfs.list(".mdl").into_iter().filter(|(n, _)| vfs.locate(&n.replace(".mdl", ".vvd")).is_some()).collect();
    let res: Vec<_> = mdl.par_iter().map(|(n, _)| load_model(vfs, n)).collect();
    let tris: usize = res.iter().flatten().map(|m| m.triangles).sum();
    let ko_mdl: Vec<_> = res.iter().filter_map(|r| r.as_ref().err().cloned()).collect();
    println!(
        "{nom} : {} VMT ({} illisibles), {} VTF ({} illisibles), {} modèles ({} illisibles), {tris} triangles, {:.1} s",
        vmt.len(), ko_vmt.len(), vtf.len(), ko_vtf.len(), mdl.len(), ko_mdl.len(), t.elapsed().as_secs_f32()
    );
    for k in ko_vmt.iter().chain(&ko_vtf).chain(&ko_mdl).take(8) {
        println!("    illisible : {k}");
    }
    assert!(ko_vmt.len() <= 2, "{nom} : matériaux illisibles");
    assert!(ko_vtf.is_empty(), "{nom} : textures illisibles");
    assert!(ko_mdl.len() <= max_modeles_ko, "{nom} : modèles illisibles");
}

#[test]
fn projet_rp_megalopole() {
    let content = format!("{}/Desktop/rp_megalopole/content", std::env::var("USERPROFILE").unwrap_or_default());
    if !Path::new(&content).is_dir() {
        println!("rp_megalopole absent : banc sauté");
        return;
    }
    let mut vfs = Vfs::new();
    vfs.add_dir(&content);
    banc("rp_megalopole", &vfs, 0);
}

#[test]
fn archives_de_garrysmod() {
    if !Path::new(GMOD).is_dir() {
        println!("Garry's Mod absent : banc sauté");
        return;
    }
    let mut vfs = Vfs::new();
    for a in ["garrysmod/garrysmod.vpk", "garrysmod/fallbacks.vpk", "sourceengine/hl2_textures.vpk", "sourceengine/hl2_misc.vpk", "sourceengine/content_hl2.vpk", "sourceengine/content_cstrike.vpk"] {
        vfs.add_vpk(Path::new(GMOD).join(a)).unwrap();
    }
    assert_eq!(vfs.mounts().len(), 6);
    banc("VPK de Garry's Mod", &vfs, 3);
}

#[test]
fn gameinfo_de_rp_megalopole() {
    let gi = format!("{}/Desktop/rp_megalopole/compile/gameinfo.txt", std::env::var("USERPROFILE").unwrap_or_default());
    if !Path::new(&gi).is_file() {
        return;
    }
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(&gi).unwrap();
    for m in vfs.mounts() {
        println!("  monté : {}", m.label);
    }
    // un modèle du projet, un de HL2, un de CS:S
    for m in ["models/megalopole/kit/clim.mdl", "models/props_c17/fence01a.mdl", "models/props/de_inferno/wall_lamp.mdl"] {
        let model = load_model(&vfs, m).unwrap();
        println!("  {m} : {} triangles, {} skin(s), matériau {:?}, depuis {}", model.triangles, model.skins.len(), model.skins[0][0].first(), vfs.mounts()[model.mount].label);
        let mat = load_material(&vfs, &model.skins[0][0].iter().find(|c| load_material(&vfs, c).found).cloned().unwrap_or_default());
        assert!(mat.found, "{m} : matériau introuvable");
        assert!(load_texture(&vfs, mat.base.as_deref().unwrap(), 1024).is_some());
    }
}
