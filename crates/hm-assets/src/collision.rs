//! Collision d'un modèle : les convexes de son fichier `.phy`, en plans.
use vformats::Limits;

use crate::vfs::Vfs;

/// Un convexe en repère du modèle : ses sommets et ses plans (`n · x <= d` à l'intérieur).
#[derive(Clone, Debug)]
pub struct Hull {
    pub points: Vec<[f32; 3]>,
    pub planes: Vec<[f32; 4]>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Convexes de collision d'un modèle (`models/x.mdl`). `None` : pas de fichier `.phy`, le modèle ne bloque rien.
pub fn load_collision(vfs: &Vfs, model: &str) -> Option<Vec<Hull>> {
    let base = crate::model::normalize_model(model);
    let base = base.strip_suffix(".mdl")?;
    let found = vfs.read(&format!("{base}.phy"))?;
    let phy = vformats::phy::parse_lossy(&found.bytes, &Limits::default()).ok()?;
    let mut hulls = Vec::new();
    for ledge in phy.ledges() {
        let pts = &ledge.vertices;
        if pts.len() < 4 {
            continue;
        }
        let n = pts.len() as f32;
        let c = pts.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n, a[2] + p[2] / n]);
        let mut planes: Vec<[f32; 4]> = Vec::new();
        for t in &ledge.triangles {
            let (Some(&a), Some(&b), Some(&cc)) = (pts.get(t[0]), pts.get(t[1]), pts.get(t[2])) else { continue };
            let mut nrm = cross(sub(b, a), sub(cc, a));
            let len = dot(nrm, nrm).sqrt();
            if len < 1e-6 {
                continue;
            }
            nrm = [nrm[0] / len, nrm[1] / len, nrm[2] / len];
            let mut d = dot(nrm, a);
            if dot(nrm, c) > d {
                nrm = [-nrm[0], -nrm[1], -nrm[2]];
                d = -d;
            }
            if !planes.iter().any(|p| (p[3] - d).abs() < 0.05 && dot([p[0], p[1], p[2]], nrm) > 0.9995) {
                planes.push([nrm[0], nrm[1], nrm[2], d]);
            }
        }
        if planes.len() >= 4 {
            hulls.push(Hull { points: pts.clone(), planes });
        }
    }
    Some(hulls)
}
