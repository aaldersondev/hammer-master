//! La cubemap des reflets de l'éditeur : celle que Hammer++ apporte, six faces carrées décodées, pas noires ; celle
//! du jeu (`engine/defaultcubemap`) est noire dans Garry's Mod, et n'est donc pas prise.
use hm_assets::Vfs;

#[test]
fn cubemap_de_l_editeur() {
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(&gi).unwrap();
    let moyennes = |c: &hm_assets::texture::Cubemap| -> Vec<f32> {
        c.faces.iter().map(|f| f[0].chunks_exact(4).map(|p| p[0] as f32 + p[1] as f32 + p[2] as f32).sum::<f32>() / (c.taille * c.taille) as f32).collect()
    };
    let jeu = hm_assets::texture::load_cubemap(&vfs, "engine/defaultcubemap").expect("cubemap du jeu");
    println!("engine/defaultcubemap : {} px, {:?}", jeu.taille, moyennes(&jeu));
    assert!(moyennes(&jeu).iter().all(|m| *m < 1.0), "celle de Garry's Mod est noire");
    let Some(c) = hm_assets::texture::cubemap_editeur(&vfs) else {
        println!("Hammer++ absent : pas de reflet");
        return;
    };
    assert_eq!(c.faces.len(), 6);
    for f in &c.faces {
        assert_eq!(f[0].len() as u32, c.taille * c.taille * 4);
    }
    println!("cubemap de Hammer++ : {} px, {:?}", c.taille, moyennes(&c));
    assert!(moyennes(&c).iter().all(|m| *m > 30.0), "six faces éclairées");
}
