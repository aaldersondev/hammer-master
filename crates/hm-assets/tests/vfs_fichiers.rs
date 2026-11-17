//! Un dossier monté réduit à quelques fichiers (le projecteur de Hammer++) : ceux-là se lisent et se listent, pas
//! les autres fichiers du même dossier ; un nom absent du disque est ignoré.
use hm_assets::Vfs;

#[test]
fn seulement_les_fichiers_nommes() {
    let d = std::env::temp_dir().join(format!("hm_vfs_fichiers_{}", std::process::id()));
    std::fs::create_dir_all(d.join("models/editor")).unwrap();
    std::fs::create_dir_all(d.join("materials/autre")).unwrap();
    std::fs::write(d.join("models/editor/spot_color.mdl"), b"mdl").unwrap();
    std::fs::write(d.join("materials/autre/cache.vmt"), b"vmt").unwrap();
    let mut vfs = Vfs::new();
    vfs.add_fichiers(&d, &[r"Models\Editor\spot_color.mdl", "models/editor/absent.vvd"]);
    assert_eq!(vfs.read("models/editor/SPOT_COLOR.mdl").map(|f| f.bytes), Some(b"mdl".to_vec()));
    assert!(vfs.read("materials/autre/cache.vmt").is_none(), "le reste du dossier n'est pas monté");
    assert!(vfs.locate("models/editor/absent.vvd").is_none());
    assert_eq!(vfs.list(".mdl").len(), 1);
    assert!(vfs.list_dans("materials", ".vmt").is_empty());
    let _ = std::fs::remove_dir_all(&d);
}
