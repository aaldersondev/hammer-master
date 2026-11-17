//! Les displacements d'un terrain sont cousus bord à bord : si la grille est reconstruite dans le bon sens, les
//! sommets des bords de deux displacements voisins coïncident. Un sens inversé (lignes et colonnes échangées, comme
//! dans l'ancien aperçu Blender, qui rendait un terrain 25 unités trop haut) casse ces coutures.
use std::collections::HashMap;
use std::path::Path;

#[test]
fn coutures_des_displacements_de_rp_megalopole() {
    let vmf = format!("{}/Desktop/rp_megalopole/out/rp_megalopole_v1.vmf", std::env::var("USERPROFILE").unwrap_or_default());
    if !Path::new(&vmf).is_file() {
        println!("rp_megalopole absent : test sauté");
        return;
    }
    let v = hm_vmf::Vmf::open(&vmf).unwrap();
    let mut bords: Vec<(usize, [i64; 3])> = Vec::new();
    let mut n = 0;
    for s in v.world().unwrap().solids() {
        for f in hm_geom::build_brush(s).faces {
            if let Some(d) = f.disp {
                let k = d.k();
                for i in 0..k {
                    for j in 0..k {
                        if i == 0 || j == 0 || i == k - 1 || j == k - 1 {
                            let p = d.points[i * k + j];
                            bords.push((n, [(p.x * 2.0).round() as i64, (p.y * 2.0).round() as i64, (p.z * 2.0).round() as i64]));
                        }
                    }
                }
                n += 1;
            }
        }
    }
    assert!(n > 100, "{n} displacements trouvés");
    // un sommet de bord est « cousu » si un autre displacement a un sommet au même endroit (à 0,5 près)
    let mut par_point: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (d, p) in &bords {
        par_point.entry(*p).or_default().push(*d);
    }
    let cousus = bords.iter().filter(|(d, p)| par_point[p].iter().any(|o| o != d)).count();
    // les sommets de même position en plan mais de hauteur différente : la signature d'une grille mal orientée
    let mut par_xy: HashMap<[i64; 2], Vec<(usize, i64)>> = HashMap::new();
    for (d, p) in &bords {
        par_xy.entry([p[0], p[1]]).or_default().push((*d, p[2]));
    }
    let decales = bords.iter().filter(|(d, p)| par_xy[&[p[0], p[1]]].iter().any(|(o, z)| o != d && *z != p[2])).count();
    println!("{n} displacements, {} sommets de bord, {cousus} cousus à un voisin, {decales} décalés en hauteur par rapport à un voisin", bords.len());
    assert!(cousus * 2 > bords.len(), "moins de la moitié des bords sont cousus : grille mal orientée ?");
    assert!(decales * 50 < bords.len(), "{decales} sommets de bord ne sont pas à la hauteur de leur voisin");
}
