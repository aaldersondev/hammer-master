//! « Export to .DXF » de Hammer : les faces des solides (monde et entités, sauf ceux rangés cachés) en `3DFACE`
//! d'un DXF texte, un calque par matériau ; un polygone de plus de quatre sommets est découpé en éventail de triangles
//! (une `3DFACE` a trois ou quatre coins, le quatrième répète le troisième pour un triangle).
use std::fmt::Write;

use hm_doc::Document;

pub fn dxf(doc: &Document) -> String {
    let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
    for (_, solide) in doc.tous_les_solides().filter(|(_, x)| !x.cache) {
        for (f, poly) in solide.faces.iter().zip(solide.polygones()) {
            if poly.len() < 3 {
                continue;
            }
            let calque: String = f.materiau.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
            let coins: Vec<[glam::DVec3; 4]> = if poly.len() == 4 { vec![[poly[0], poly[1], poly[2], poly[3]]] } else { (1..poly.len() - 1).map(|i| [poly[0], poly[i], poly[i + 1], poly[i + 1]]).collect() };
            for c in coins {
                let _ = write!(s, "0\n3DFACE\n8\n{calque}\n");
                for (k, p) in c.iter().enumerate() {
                    let _ = write!(s, "1{k}\n{}\n2{k}\n{}\n3{k}\n{}\n", p.x, p.y, p.z);
                }
            }
        }
    }
    s + "0\nENDSEC\n0\nEOF\n"
}

#[cfg(test)]
mod tests {
    use hm_doc::{Commande, Session};

    #[test]
    fn un_bloc_six_faces() {
        let mut s = Session::nouvelle();
        s.executer(Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [64.0, 32.0, 16.0], materiau: "dev/dev_measuregeneric01".into(), classe: None }).unwrap();
        let t = super::dxf(s.doc());
        assert_eq!(t.matches("\n3DFACE\n").count(), 6, "six faces à quatre coins");
        assert!(t.starts_with("0\nSECTION\n2\nENTITIES\n") && t.ends_with("0\nENDSEC\n0\nEOF\n"));
        assert!(t.contains("\n8\ndev_dev_measuregeneric01\n"));
        assert!(t.contains("\n10\n64\n") || t.contains("\n11\n64\n") || t.contains("\n12\n64\n"), "les coins sont en unités de la map");
    }
}
