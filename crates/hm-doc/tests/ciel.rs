//! La boîte de la skybox 3D : fermée par les brosses de ciel autour de `sky_camera`, un bâtiment à l'intérieur ne la
//! raccourcit pas, la map principale (plus loin, avec son propre ciel) n'y entre pas.
use hm_doc::{Commande, Session};

fn bloc(s: &mut Session, lo: [f64; 3], hi: [f64; 3], m: &str) {
    s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: m.into(), classe: None }).unwrap();
}

/// Six murs de ciel autour de l'intérieur `lo`–`hi`.
fn coque(s: &mut Session, lo: [f64; 3], hi: [f64; 3]) {
    let c = "TOOLS/TOOLSSKYBOX";
    let e = 16.0;
    bloc(s, [lo[0] - e, lo[1] - e, lo[2] - e], [hi[0] + e, hi[1] + e, lo[2]], c);
    bloc(s, [lo[0] - e, lo[1] - e, hi[2]], [hi[0] + e, hi[1] + e, hi[2] + e], c);
    bloc(s, [lo[0] - e, lo[1], lo[2]], [lo[0], hi[1], hi[2]], c);
    bloc(s, [hi[0], lo[1], lo[2]], [hi[0] + e, hi[1], hi[2]], c);
    bloc(s, [lo[0] - e, lo[1] - e, lo[2]], [hi[0] + e, lo[1], hi[2]], c);
    bloc(s, [lo[0] - e, hi[1], lo[2]], [hi[0] + e, hi[1] + e, hi[2]], c);
}

#[test]
fn boite_de_la_skybox_3d() {
    let mut s = Session::nouvelle();
    // la map principale et son ciel
    coque(&mut s, [-1024.0, -1024.0, 0.0], [1024.0, 1024.0, 512.0]);
    bloc(&mut s, [-1024.0, -1024.0, 0.0], [1024.0, 1024.0, 16.0], "DEV/DEV_MEASUREGENERIC01");
    // la skybox, plus bas : un sol de terrain, un bâtiment juste à côté de la caméra
    coque(&mut s, [-256.0, -256.0, -2048.0], [256.0, 256.0, -1792.0]);
    bloc(&mut s, [-256.0, -256.0, -2048.0], [256.0, 256.0, -2032.0], "NATURE/BLENDGRASSGRAVEL001A");
    bloc(&mut s, [32.0, -16.0, -2032.0], [64.0, 16.0, -1900.0], "BRICK/BRICKWALL003A");
    assert!(hm_doc::geo::boite_skybox(s.doc()).is_none(), "sans sky_camera");
    s.executer(Commande::CreerEntite { classe: "sky_camera".into(), origine: [0.0, 0.0, -1950.0], cles: vec![("scale".into(), "16".into())] }).unwrap();
    let (o, k, (lo, hi)) = hm_doc::geo::boite_skybox(s.doc()).expect("la skybox");
    assert_eq!(o.to_array(), [0.0, 0.0, -1950.0]);
    assert_eq!(k, 16.0);
    // le bâtiment (à x = 32) ne ferme pas la boîte ; le sol de terrain non plus, la coque de ciel si
    assert_eq!(lo.to_array(), [-256.0, -256.0, -2048.0], "{lo:?}");
    assert_eq!(hi.to_array(), [256.0, 256.0, -1792.0], "{hi:?}");
    // échelle absente : 16
    let mut s2 = Session::nouvelle();
    coque(&mut s2, [-64.0, -64.0, -64.0], [64.0, 64.0, 64.0]);
    s2.executer(Commande::CreerEntite { classe: "sky_camera".into(), origine: [0.0, 0.0, 0.0], cles: vec![] }).unwrap();
    assert_eq!(hm_doc::geo::boite_skybox(s2.doc()).unwrap().1, 16.0);
}
