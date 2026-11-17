//! Ce qu'il faut embarquer dans le `.bsp` : les fichiers que la map emploie (matériaux des faces, des overlays et
//! décals, du ciel ; modèles, leurs fichiers voisins et leurs matériaux ; textures nommées dans ces .vmt) et que les
//! joueurs n'ont pas, c'est-à-dire qui ne viennent pas du jeu de base (provenance de `hm-regles`).
use std::collections::BTreeSet;

use hm_assets::Vfs;
use hm_doc::Document;
use hm_regles::assets::{provenance, Provenance};

/// Les chemins dans le jeu (`materials/…`, `models/…`) à embarquer, avec leur contenu.
pub fn fichiers(doc: &Document, vfs: &Vfs, config: &hm_regles::Config) -> Vec<(String, Vec<u8>)> {
    let montages: Vec<Provenance> = vfs.mounts().iter().map(|m| provenance(&m.label, config)).collect();
    // le fichier existe et ne vient pas du jeu de base
    let a_prendre = |rel: &str| vfs.read(rel).filter(|f| montages.get(f.mount).map_or(false, |p| *p != Provenance::Jeu));
    let mut materiaux: BTreeSet<String> = BTreeSet::new();
    let mut modeles: BTreeSet<String> = BTreeSet::new();
    for (_, s) in doc.tous_les_solides() {
        for f in &s.faces {
            materiaux.insert(f.materiau.to_ascii_lowercase());
        }
    }
    for e in doc.entites().chain(std::iter::once(doc.monde())) {
        for k in ["material", "texture"] {
            if let Some(m) = e.get(k).filter(|m| !m.trim().is_empty()) {
                materiaux.insert(m.to_ascii_lowercase());
            }
        }
        if let Some(m) = e.get("model").filter(|m| m.to_ascii_lowercase().ends_with(".mdl")) {
            modeles.insert(m.to_ascii_lowercase().replace('\\', "/"));
        }
    }
    if let Some(ciel) = doc.monde().get("skyname").filter(|c| !c.trim().is_empty()) {
        for s in ["rt", "bk", "lf", "ft", "up", "dn"] {
            materiaux.insert(format!("skybox/{}{s}", ciel.trim().to_ascii_lowercase()));
        }
    }
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut pris: BTreeSet<String> = BTreeSet::new();
    let mut prendre = |rel: String, out: &mut Vec<(String, Vec<u8>)>| -> bool {
        if pris.contains(&rel) {
            return true;
        }
        match a_prendre(&rel) {
            Some(f) => {
                pris.insert(rel.clone());
                out.push((rel, f.bytes));
                true
            }
            None => false,
        }
    };
    for m in &modeles {
        if !prendre(m.clone(), &mut out) {
            continue;
        }
        let base = m.trim_end_matches(".mdl");
        for ext in ["vvd", "phy", "ani", "vtx", "dx90.vtx", "dx80.vtx", "sw.vtx"] {
            prendre(format!("{base}.{ext}"), &mut out);
        }
        // ses matériaux : le premier chemin qui existe, pour chaque maillage de chaque skin
        if let Ok(md) = hm_assets::load_model(vfs, m) {
            for skin in &md.skins {
                for essais in skin {
                    if let Some(c) = essais.iter().find(|c| vfs.read(&format!("materials/{c}.vmt")).is_some()) {
                        materiaux.insert(c.to_ascii_lowercase());
                    }
                }
            }
        }
    }
    for m in &materiaux {
        let vmt = format!("materials/{}.vmt", m.replace('\\', "/"));
        if !prendre(vmt.clone(), &mut out) {
            continue;
        }
        // les textures que nomme le .vmt ($basetexture, $bumpmap, $envmapmask, $detail…), et un .vmt inclus
        let texte = String::from_utf8_lossy(&out.iter().find(|(r, _)| *r == vmt).unwrap().1).into_owned();
        for mot in texte.split(|c: char| c.is_whitespace() || c == '"' || c == '{' || c == '}') {
            let mut t = mot.trim().to_ascii_lowercase().replace('\\', "/");
            // `\\` échappé dans le .vmt : un seul séparateur dans le chemin du BSP
            while t.contains("//") {
                t = t.replace("//", "/");
            }
            if t.is_empty() || t.starts_with('$') || t.starts_with('%') {
                continue;
            }
            if t.ends_with(".vmt") {
                prendre(t, &mut out);
            } else {
                let t = t.trim_end_matches(".vtf");
                prendre(format!("materials/{t}.vtf"), &mut out);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hm_doc::{Commande, Session};

    #[test]
    fn seul_ce_que_les_joueurs_n_ont_pas() {
        // un dossier local avec un matériau et ses deux textures, monté devant le jeu
        let d = std::env::temp_dir().join(format!("hm_embarque_{}", std::process::id()));
        std::fs::create_dir_all(d.join("materials/hm_local")).unwrap();
        std::fs::write(d.join("materials/hm_local/brique.vmt"), "\"LightmappedGeneric\"\n{\n\t\"$basetexture\" \"hm_local/brique\"\n\t\"$bumpmap\" \"hm_local\\\\brique_normal\"\n\t\"$surfaceprop\" \"brick\"\n}\n").unwrap();
        std::fs::write(d.join("materials/hm_local/brique.vtf"), b"vtf1").unwrap();
        std::fs::write(d.join("materials/hm_local/brique_normal.vtf"), b"vtf2").unwrap();
        let mut vfs = Vfs::new();
        vfs.add_dir(&d);
        if let Some(gi) = hm_assets::jeu::gameinfo_gmod() {
            vfs.add_gameinfo(gi).unwrap();
        }
        let mut s = Session::nouvelle();
        s.executer(Commande::CreerBloc { min: [0.0; 3], max: [64.0; 3], materiau: "HM_LOCAL/BRIQUE".into(), classe: None }).unwrap();
        s.executer(Commande::CreerBloc { min: [100.0, 0.0, 0.0], max: [164.0, 64.0, 64.0], materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: None }).unwrap();
        let f = fichiers(s.doc(), &vfs, &hm_regles::Config::default());
        let noms: Vec<&str> = f.iter().map(|(r, _)| r.as_str()).collect();
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(noms, ["materials/hm_local/brique.vmt", "materials/hm_local/brique.vtf", "materials/hm_local/brique_normal.vtf"], "le matériau du jeu (dev) et le ciel du jeu restent dehors");
        assert_eq!(f[1].1, b"vtf1");
    }
}
