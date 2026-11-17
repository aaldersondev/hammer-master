//! Règles sur les fichiers du jeu : introuvables, venus d'un montage que les joueurs n'ont pas, refusés.
use std::collections::BTreeMap;

use glam::Vec3;
use hm_assets::Vfs;
use hm_scene::{ObjKind, Scene};

use crate::{motif, Config, Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle { id: "asset.materiau_introuvable", niveau: Niveau::Erreur, titre: "Matériau introuvable", explication: "Le fichier .vmt d'un matériau posé sur un solide n'existe dans aucun montage : damier rose et noir en jeu.", source: "lecture des montages du gameinfo.txt du projet" },
    Regle { id: "asset.texture_introuvable", niveau: Niveau::Avertissement, titre: "Texture introuvable", explication: "Un matériau existe mais sa texture de base (.vtf) manque.", source: "lecture des montages" },
    Regle { id: "asset.modele_introuvable", niveau: Niveau::Erreur, titre: "Modèle introuvable ou illisible", explication: "Le .mdl d'une entité manque ou ne se lit pas : « ERROR » rouge en jeu.", source: "lecture des montages" },
    Regle { id: "asset.materiau_de_modele", niveau: Niveau::Avertissement, titre: "Matériau de modèle introuvable", explication: "Un modèle demande un matériau qu'aucun de ses dossiers de recherche ne contient.", source: "table des matériaux du .mdl" },
    Regle { id: "asset.modele_non_statique", niveau: Niveau::Erreur, titre: "Modèle refusé en prop_static", explication: "Un prop_static utilise un modèle compilé sans $staticprop, ou dont les clés portent un bloc prop_data sans allowstatic (un modèle d'entité dynamique) : dans les deux cas vbsp le retire de la map.", source: "drapeau STATIC_PROP et clés du .mdl ; IsStaticProp dans utils/vbsp/staticprop.cpp du SDK 2013" },
    Regle { id: "asset.skin", niveau: Niveau::Avertissement, titre: "Skin hors limites", explication: "La clé skin dépasse le nombre de familles de skins du modèle.", source: "table des skins du .mdl" },
    Regle { id: "asset.autre_jeu", niveau: Niveau::Avertissement, titre: "Fichier d'un autre jeu", explication: "Le fichier ne vient que du contenu d'un autre jeu (Counter-Strike: Source…) : les joueurs qui ne l'ont pas monté voient un damier ou « ERROR ». À embarquer dans la map, ou à remplacer.", source: "provenance par montage" },
    Regle { id: "asset.local", niveau: Niveau::Avertissement, titre: "Fichier présent seulement sur ce PC", explication: "Le fichier vient d'un dossier libre du jeu ou d'un addon local, qui n'est ni dans le jeu de base ni embarqué dans la map.", source: "provenance par montage" },
];

/// D'où vient un fichier, d'après le montage qui l'a fourni.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum Provenance {
    /// Contenu du projet, embarqué dans la map à la compilation.
    Projet,
    /// Garry's Mod et Half-Life 2, que tout joueur a.
    Jeu,
    /// Contenu d'un autre jeu monté.
    AutreJeu,
    /// Dossier libre du jeu ou addon local.
    Local,
}

pub fn provenance(label: &str, config: &Config) -> Provenance {
    let l = label.replace('\\', "/").to_ascii_lowercase();
    if config.montages_locaux.iter().any(|m| motif(m, &l)) {
        return Provenance::Local;
    }
    if config.montages_embarques.iter().any(|m| motif(m, &l)) || l.contains("contenu embarqué") {
        return Provenance::Projet;
    }
    let fichier = l.rsplit('/').next().unwrap_or("");
    if fichier.ends_with(".vpk") {
        // Les archives rangées dans l'installation de Garry's Mod sont chez tous les joueurs. Depuis la mise à jour
        // de juillet 2025, cela comprend `content_cstrike` et `content_hl2` : l'essentiel de Counter-Strike: Source
        // et des épisodes de Half-Life 2, sans leurs maps, leurs voix ni leur musique
        // (gmod.facepunch.com/news/july-2025-update, lu le 13/11/2026). Une archive d'un autre dossier de jeu ne
        // l'est pas.
        return if l.contains("/garrysmod/") { Provenance::Jeu } else { Provenance::AutreJeu };
    }
    if l.contains("/garrysmod/addons/") || l.ends_with("/garrysmod/garrysmod") || l.ends_with("/garrysmod/sourceengine") || l.ends_with("/garrysmod/platform") || l.contains("/garrysmod/garrysmod/download") {
        return Provenance::Local;
    }
    Provenance::Projet
}

pub fn verifier(scene: &Scene, vfs: &Vfs, config: &Config, out: &mut Vec<Constat>) {
    let montages: Vec<(String, Provenance)> = vfs.mounts().iter().map(|m| (m.label.clone(), provenance(&m.label, config))).collect();
    let de = |mount: usize| montages.get(mount).cloned().unwrap_or((String::new(), Provenance::Projet));
    // ---- où chaque matériau de solide est employé : premier objet, nombre de solides
    let mut emploi: BTreeMap<&str, (u32, usize)> = BTreeMap::new();
    for b in &scene.brushes {
        if b.object == 0 {
            continue;
        }
        let mut vus: Vec<&str> = Vec::new();
        for f in &b.brush.faces {
            if !vus.contains(&f.material.as_str()) {
                vus.push(&f.material);
                let e = emploi.entry(&f.material).or_insert((b.object, 0));
                e.1 += 1;
            }
        }
    }
    let ou = |nom: &str| emploi.get(nom).copied();
    for m in &scene.materials {
        if m.tool.is_some() || m.name.is_empty() {
            continue;
        }
        let Some((objet, n)) = ou(&m.name) else { continue }; // matériaux de modèles : vus plus bas
        if !m.found {
            let mut c = Constat::objet(scene, "asset.materiau_introuvable", objet, format!("matériau « {} » introuvable, posé sur {n} solide(s)", m.name)).mesure(n as f32);
            // le matériau manquant lui-même (pas celui qui domine sur l'objet) : la correction le remplace
            c.materiau = m.name.clone();
            out.push(c);
            continue;
        }
        if let Some(mount) = m.mount {
            let (label, p) = de(mount);
            let regle = match p {
                Provenance::AutreJeu => Some("asset.autre_jeu"),
                Provenance::Local => Some("asset.local"),
                _ => None,
            };
            if let Some(regle) = regle {
                out.push(Constat::objet(scene, regle, objet, format!("matériau « {} », posé sur {n} solide(s), ne vient que de {label}", m.name)).mesure(n as f32));
            }
        }
    }
    for t in &scene.report.missing_textures {
        let qui = scene.materials.iter().find(|m| m.base.as_deref() == Some(t.as_str()));
        let objet = qui.and_then(|m| ou(&m.name)).map_or(0, |x| x.0);
        let msg = format!("texture « {t} » introuvable{}", qui.map_or(String::new(), |m| format!(" (matériau « {} »)", m.name)));
        out.push(if objet != 0 { Constat::objet(scene, "asset.texture_introuvable", objet, msg) } else { Constat::lieu("asset.texture_introuvable", Vec3::ZERO, Vec3::ZERO, msg) });
    }
    // ---- modèles : par modèle, ses exemplaires
    let mut exemplaires: Vec<Vec<usize>> = vec![Vec::new(); scene.models.len()];
    for (k, p) in scene.props.iter().enumerate() {
        exemplaires[p.model as usize].push(k);
    }
    for (mi, sm) in scene.models.iter().enumerate() {
        let ex = &exemplaires[mi];
        let Some(&premier) = ex.first() else { continue };
        let objet = scene.props[premier].object;
        let Some(m) = &sm.model else {
            out.push(Constat::objet(scene, "asset.modele_introuvable", objet, format!("modèle « {} » introuvable ou illisible ({}), posé {} fois", sm.name, sm.error.as_deref().unwrap_or("?"), ex.len())).mesure(ex.len() as f32));
            continue;
        };
        let (label, p) = de(m.mount);
        let regle = match p {
            Provenance::AutreJeu => Some("asset.autre_jeu"),
            Provenance::Local => Some("asset.local"),
            _ => None,
        };
        if let Some(regle) = regle {
            out.push(Constat::objet(scene, regle, objet, format!("modèle « {} », posé {} fois, ne vient que de {label}", sm.name, ex.len())).mesure(ex.len() as f32));
        }
        // prop_static sans $staticprop
        let statiques: Vec<usize> = ex.iter().copied().filter(|&k| scene.objects[scene.props[k].object as usize].class == "prop_static").collect();
        if (!m.static_prop || m.prop_data) && !statiques.is_empty() {
            let pourquoi = if !m.static_prop { "compilé sans $staticprop" } else { "fait pour une entité dynamique (ses clés portent un bloc prop_data sans allowstatic)" };
            out.push(Constat::objet(scene, "asset.modele_non_statique", scene.props[statiques[0]].object, format!("modèle « {} » {pourquoi}, posé en prop_static {} fois", sm.name, statiques.len())).mesure(statiques.len() as f32));
        }
        // skins
        let n_skins = m.skins.len().max(1) as u32;
        let hors: Vec<usize> = ex.iter().copied().filter(|&k| scene.props[k].skin >= n_skins).collect();
        if let Some(&k) = hors.first() {
            out.push(Constat::objet(scene, "asset.skin", scene.props[k].object, format!("modèle « {} » : skin {} demandé, {} skin(s) existent ({} exemplaire(s))", sm.name, scene.props[k].skin, n_skins, hors.len())).mesure(hors.len() as f32));
        }
        // matériaux du modèle
        let mut manquants: Vec<&str> = Vec::new();
        for skin in &sm.materials {
            for &mat in skin {
                let mm = &scene.materials[mat as usize];
                if !mm.found && mm.tool.is_none() && !manquants.contains(&mm.name.as_str()) {
                    manquants.push(&mm.name);
                }
            }
        }
        if !manquants.is_empty() {
            out.push(Constat::objet(scene, "asset.materiau_de_modele", objet, format!("modèle « {} » ({} fois) : matériau(x) introuvable(s) : {}", sm.name, ex.len(), manquants.join(", "))).mesure(ex.len() as f32));
        }
    }
    // modèles d'entités sans objet (aucun exemplaire avec origine) : rien à dire
    let _ = ObjKind::Prop;
}

impl Provenance {
    pub fn nom(self) -> &'static str {
        match self {
            Provenance::Projet => "projet (embarqué dans la map)",
            Provenance::Jeu => "jeu (chez tous les joueurs)",
            Provenance::AutreJeu => "autre jeu (à monter par chaque joueur)",
            Provenance::Local => "local (ce PC seulement)",
        }
    }
}

/// Tableau de provenance : (montage, provenance, matériaux, modèles distincts, poses de ces modèles).
pub fn provenances(scene: &Scene, vfs: &Vfs, config: &Config) -> Vec<(String, Provenance, usize, usize, usize)> {
    let mut poses = vec![0usize; scene.models.len()];
    for p in &scene.props {
        poses[p.model as usize] += 1;
    }
    let mut t: Vec<(String, Provenance, usize, usize, usize)> = vfs.mounts().iter().map(|m| (m.label.clone(), provenance(&m.label, config), 0, 0, 0)).collect();
    for m in &scene.materials {
        if let (true, Some(k)) = (m.found, m.mount) {
            if let Some(x) = t.get_mut(k) {
                x.2 += 1;
            }
        }
    }
    for (k, sm) in scene.models.iter().enumerate() {
        if let Some(m) = &sm.model {
            if let Some(x) = t.get_mut(m.mount) {
                x.3 += 1;
                x.4 += poses[k];
            }
        }
    }
    t.retain(|x| x.2 + x.3 > 0);
    t
}
