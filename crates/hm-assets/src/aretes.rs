//! Les arêtes vives d'un modèle, pour le dessiner en fil de fer dans les vues 2D sans tout son maillage : une arête
//! est gardée si elle borde une seule face, ou si les deux faces qu'elle sépare font un angle de plus de `seuil`
//! degrés. Les sommets d'un modèle sont dédoublés par normale et par coordonnée de texture : on les soude par
//! position (au 1/64 d'unité).
use std::collections::HashMap;

use crate::model::{ModelMesh, ModelVertex};

/// `sommets` et `maillages` d'un modèle lu (`Model::vertices`, `Model::meshes`).
pub fn aretes_vives(sommets: &[ModelVertex], maillages: &[ModelMesh], seuil: f32) -> Vec<[[f32; 3]; 2]> {
    let cle = |p: [f32; 3]| [(p[0] * 64.0).round() as i64, (p[1] * 64.0).round() as i64, (p[2] * 64.0).round() as i64];
    let mut faces_de: HashMap<([i64; 3], [i64; 3]), (usize, [f32; 3], [[f32; 3]; 2])> = HashMap::new();
    let mut vives = Vec::new();
    let cos_seuil = seuil.to_radians().cos();
    for mesh in maillages {
        for t in mesh.indices.chunks_exact(3) {
            let p: Vec<[f32; 3]> = t.iter().map(|i| sommets[*i as usize].pos).collect();
            let n = normale(p[0], p[1], p[2]);
            for k in 0..3 {
                let (a, b) = (p[k], p[(k + 1) % 3]);
                let (ka, kb) = (cle(a), cle(b));
                if ka == kb {
                    continue;
                }
                let e = if ka < kb { (ka, kb) } else { (kb, ka) };
                match faces_de.get_mut(&e) {
                    Some(x) => {
                        x.0 += 1;
                        // deux faces : vive si elles se plient assez
                        if x.0 == 2 && x.1[0] * n[0] + x.1[1] * n[1] + x.1[2] * n[2] < cos_seuil {
                            vives.push(x.2);
                        }
                    }
                    None => {
                        faces_de.insert(e, (1, n, [a, b]));
                    }
                }
            }
        }
    }
    // les bords (une seule face)
    vives.extend(faces_de.values().filter(|x| x.0 == 1).map(|x| x.2));
    vives
}

fn normale(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 {
        [n[0] / l, n[1] / l, n[2] / l]
    } else {
        [0.0; 3]
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{ModelMesh, ModelVertex};

    /// Un cube de côté 2, sommets dédoublés par face (comme dans un .mdl) : ses 12 arêtes, aucune diagonale.
    #[test]
    fn cube() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]), ([-1., 0., 0.], [0., 0., 1.], [0., 1., 0.]), ([0., 1., 0.], [0., 0., 1.], [1., 0., 0.]), ([0., -1., 0.], [1., 0., 0.], [0., 0., 1.]), ([0., 0., 1.], [1., 0., 0.], [0., 1., 0.]), ([0., 0., -1.], [0., 1., 0.], [1., 0., 0.])];
        for (n, u, v) in faces {
            let b = vertices.len() as u32;
            for (a, c) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let pos = [n[0] + u[0] * a + v[0] * c, n[1] + u[1] * a + v[1] * c, n[2] + u[2] * a + v[2] * c];
                vertices.push(ModelVertex { pos, normal: n, uv: [0.0, 0.0] });
            }
            indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let a = super::aretes_vives(&vertices, &[ModelMesh { indices }], 20.0);
        assert_eq!(a.len(), 12, "{a:?}");
    }
}
