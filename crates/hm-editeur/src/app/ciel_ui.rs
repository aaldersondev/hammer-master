//! Le ciel dans la vue 3D (menu Affichage, « Ciel 2D et skybox 3D ») : les six faces de `skyname` derrière la map,
//! et le contenu de la boîte de `sky_camera` redessiné à son échelle, vus au travers des faces de ciel.
use glam::{DVec3, Vec3};
use hm_render::ciel::{CielRendu, SUFFIXES};
use hm_render::vivant::Bibliotheque;
use hm_render::Renderer;

use super::Editeur;

#[derive(Default)]
pub struct EtatCiel {
    /// Ciel montré (Alt+V) : les faces de ciel laissent voir le ciel 2D et la skybox 3D, comme en jeu. Coupé par
    /// défaut, comme dans Hammer++ : elles montrent alors leur texture d'outil, et le vide est noir.
    pub montrer: bool,
    /// (version du document, skybox 3D trouvée)
    skybox: Option<(u64, Option<(DVec3, f64, (DVec3, DVec3))>)>,
}

impl Editeur {
    /// La skybox 3D pour cette version du document (calculée une fois par version).
    pub(super) fn skybox_courante(&mut self) -> Option<(DVec3, f64, (DVec3, DVec3))> {
        let v = self.session.version();
        if self.ciel.skybox.as_ref().map_or(true, |c| c.0 != v) {
            self.ciel.skybox = Some((v, hm_doc::geo::boite_skybox(self.session.doc())));
        }
        self.ciel.skybox.as_ref().unwrap().1
    }
}

/// Ce que la vue 3D montre du ciel : les faces de `skyname` si elles se trouvent toutes, la skybox 3D si elle existe.
pub fn ciel_rendu(r: &Renderer, bib: &mut Bibliotheque, vfs: &hm_assets::Vfs, skyname: &str, skybox: Option<(DVec3, f64, (DVec3, DVec3))>) -> CielRendu {
    let mut faces = [0u32; 6];
    let mut demi = [false; 6];
    let mut toutes = !skyname.trim().is_empty();
    for (i, s) in SUFFIXES.iter().enumerate() {
        if !toutes {
            break;
        }
        let info = bib.materiau(r, vfs, &format!("skybox/{}{s}", skyname.trim()));
        toutes &= info.trouve;
        faces[i] = info.indice;
        // côtés de 512 × 256 : le haut de la face seulement
        demi[i] = i < 4 && info.taille.1 * 2 == info.taille.0;
    }
    CielRendu { faces: toutes.then_some(faces), demi, ciel3d: skybox.map(|(o, k, (lo, hi))| (o.as_vec3(), k as f32, (lo.as_vec3() - Vec3::ONE, hi.as_vec3() + Vec3::ONE))) }
}
