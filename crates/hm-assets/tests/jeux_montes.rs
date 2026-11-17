//! Les jeux que Garry's Mod monte (`cfg/mountdepots.txt`) : son contenu livré d'abord, puis l'installation du jeu
//! si elle existe ; rien pour un jeu désactivé ; puis, sur ce PC, du contenu de HL2 qui n'est pas dans le gameinfo
//! devient lisible.
use std::path::PathBuf;

#[test]
fn installation_repli_et_desactive() {
    let d = std::env::temp_dir().join(format!("hm_montes_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (gmod, communs) = (d.join("common/GarrysMod"), d.join("common"));
    std::fs::create_dir_all(gmod.join("garrysmod/cfg")).unwrap();
    std::fs::create_dir_all(gmod.join("sourceengine")).unwrap();
    std::fs::create_dir_all(communs.join("Half-Life 2/hl2")).unwrap();
    std::fs::create_dir_all(communs.join("Half-Life 2/ep2")).unwrap();
    for f in ["Half-Life 2/hl2/hl2_misc_dir.vpk", "Half-Life 2/hl2/hl2_misc_000.vpk", "Half-Life 2/ep2/ep2_pak_dir.vpk"] {
        std::fs::write(communs.join(f), b"").unwrap();
    }
    std::fs::write(gmod.join("sourceengine/content_cstrike_dir.vpk"), b"").unwrap();
    std::fs::write(gmod.join("sourceengine/content_hl2_dir.vpk"), b"").unwrap();
    std::fs::write(gmod.join("sourceengine/content_tf_dir.vpk"), b"").unwrap();
    std::fs::write(gmod.join("garrysmod/cfg/mountdepots.txt"), "\"gamedepotsystem\"\n{\n\t\"hl2\"\t\t\"1\"\n\t\"cstrike\"\t\t\"1\"\n\t\"tf\"\t\t\"0\"\n}\n").unwrap();
    let l = hm_assets::jeu::jeux_montes(&gmod, &[communs.clone()]);
    let attendu: Vec<PathBuf> = vec![
        gmod.join("sourceengine/content_hl2_dir.vpk"),
        communs.join("Half-Life 2/hl2/hl2_misc_dir.vpk"),
        communs.join("Half-Life 2/hl2"),
        communs.join("Half-Life 2/ep2/ep2_pak_dir.vpk"),
        communs.join("Half-Life 2/ep2"),
        gmod.join("sourceengine/content_cstrike_dir.vpk"),
    ];
    assert_eq!(l, attendu);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn contenu_de_hl2_lisible_sur_ce_pc() {
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(gi).unwrap();
    // absents du gameinfo de GMod, présents dans HL2 (ou son repli) : le pin et la roche de grotte de la map de test
    for f in ["models/props_foliage/tree_pine04.mdl", "materials/nature/rockwall_cave02a.vmt"] {
        assert!(vfs.locate(f).is_some(), "{f} : introuvable");
    }
}
