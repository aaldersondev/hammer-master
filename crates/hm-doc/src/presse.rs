//! Presse-papiers : les objets copiés sont un petit VMF. On peut donc les coller dans une autre map, ou les
//! relire dans un éditeur de texte.
use std::collections::BTreeSet;

use crate::commandes::{resoudre, Ref, Res};
use crate::kv::Format;
use crate::modele::{Document, Proprio};

/// Le morceau de VMF qui contient ces objets (avec leurs groupes), numéros d'origine gardés.
pub fn copier(doc: &Document, objets: &[Ref]) -> Res<String> {
    let (solides, entites, groupes) = resoudre(doc, objets)?;
    if solides.is_empty() && entites.is_empty() {
        return Err("rien à copier".into());
    }
    let mut frag = Document::nouveau();
    frag.format = Format { crlf: doc.format.crlf, latin1: false };
    let groupes: BTreeSet<u64> = groupes.into_iter().collect();
    for g in doc.groupes.values().filter(|g| groupes.contains(&g.id)) {
        frag.groupes.insert(g.id, g.clone());
    }
    for id in &solides {
        let s = doc.solide(*id).unwrap();
        // un solide pris seul dans une entité est copié comme solide du monde
        let mut s = s.clone();
        if let Some(g) = s.editeur.get("groupid").and_then(|v| v.trim().parse::<u64>().ok()) {
            if !groupes.contains(&g) {
                s.editeur.retirer("groupid");
            }
        }
        frag.proprio.insert(s.id, Proprio::Monde);
        frag.solides.insert(s.id, s);
    }
    for id in &entites {
        let e = doc.entites[id].clone();
        for s in &e.solides {
            frag.proprio.insert(s.id, Proprio::Entite(e.id));
        }
        frag.entites.insert(e.id, e);
    }
    Ok(String::from_utf8_lossy(&frag.ecrire()).into_owned())
}

/// Un préfabriqué : les objets copiés, ramenés pour que le milieu de leur dessous soit à l'origine (wiki Valve,
/// « Prefab » : l'origine du préfabriqué est le point d'insertion ; Hammer garde les coordonnées et conseille de
/// centrer à la main, on le fait ici).
pub fn prefabriquer(doc: &Document, objets: &[Ref]) -> Res<String> {
    let texte = copier(doc, objets)?;
    let (mut frag, _) = Document::lire(texte.as_bytes()).map_err(|e| e.0)?;
    let tous: Vec<Ref> = frag.solides.keys().map(|i| Ref::Solide(*i)).chain(frag.entites.keys().map(|i| Ref::Entite(*i))).collect();
    let (lo, hi) = crate::commandes::boite_de(&frag, &tous).ok_or("rien à enregistrer")?;
    let f = crate::geo::Affine::translation(-glam::DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z));
    for s in frag.solides.values_mut() {
        *s = crate::geo::transformer_solide(s, &f, true);
    }
    for e in frag.entites.values_mut() {
        *e = crate::geo::transformer_entite(e, &f, true);
    }
    Ok(String::from_utf8_lossy(&frag.ecrire()).into_owned())
}
