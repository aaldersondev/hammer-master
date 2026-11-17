//! Règles sémantiques : elles jugent la map sur ce que son modèle (`<carte>.sem.json`) dit qu'elle est.
//!
//! Sans modèle, aucune de ces règles ne tourne : une boîte ne peut pas être « des WC incomplets » si rien ne dit
//! que ce sont des WC.
use std::collections::{BTreeMap, HashMap};

use glam::Vec3;
use hm_model::{Modele, Piece};
use hm_scene::Scene;
use serde::Deserialize;

use crate::monde::{Genre, Monde};
use crate::{Config, Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle { id: "sem.role_incomplet", niveau: Niveau::Erreur, titre: "Rôle incomplet", explication: "Une pièce ne contient pas ce que son rôle exige : des WC sans cuvette, sans lavabo ou sans rien qui ferme ; un bureau sans table ni siège.", source: "regles/roles.yaml, copie de data/roles.yaml du skill gmod-map-maker (ROLE_INCOMPLET) ; origine : les 13 salles de WC du 11/11/2026" },
    Regle { id: "sem.role_trop_grand", niveau: Niveau::Avertissement, titre: "Pièce trop grande pour son rôle", explication: "La surface de la pièce dépasse ce que son rôle admet : quatre toilettes dans 630 × 890 ne font pas des WC, quoi qu'on y ajoute.", source: "regles/roles.yaml, surface_max (ROLE_TROP_GRAND du skill)" },
    Regle { id: "sem.piece_sous_meublee", niveau: Niveau::Avertissement, titre: "Pièce sous-meublée", explication: "La pièce a moins d'objets que son rôle n'en demande par tranche de 256 × 256 de sol.", source: "regles/roles.yaml, objets_min (PIECE_SOUS_MEUBLEE du skill)" },
    Regle { id: "sem.baie_sans_porte", niveau: Niveau::Avertissement, titre: "Baie sans porte", explication: "Une pièce dont le rôle demande qu'on puisse la fermer (bureau, WC, salle de bain, chambre, réunion, cellule, réserve, vestiaire) a une ouverture de la largeur d'une porte sans porte.", source: "contrôle propre à l'outil ; signalement L01 (« il faut créer des portes de bureaux ») du 11/11/2026" },
    Regle { id: "sem.degagement", niveau: Niveau::Avertissement, titre: "Équipements trop proches", explication: "Deux équipements sanitaires d'une même pièce sont à moins de 12 unités l'un de l'autre : on ne peut pas se tenir devant l'un sans toucher l'autre.", source: "choix de l'outil, pas une norme ; signalement L09 (« toilette et baignoire trop proches ») du 11/11/2026" },
    Regle { id: "sem.toit_sans_appui", niveau: Niveau::Erreur, titre: "Toit sans appui", explication: "Un toit, une dalle ou un auvent du modèle ne déclare aucun appui, ou un appui déclaré ne monte pas jusqu'à lui.", source: "modèle sémantique (toits) ; défaut de référence : l'auvent des stations du 10/11/2026" },
    Regle { id: "sem.voie_sans_extremite", niveau: Niveau::Avertissement, titre: "Bout de voie non déclaré", explication: "Une voie s'arrête à un endroit que le modèle ne déclare pas comme extrémité, ou une extrémité déclarée « terminus » n'a pas de terminus posé.", source: "modèle sémantique (voies) ; défaut de référence : le viaduc du M1 du 10/11/2026" },
    Regle { id: "sem.amenagement_brouillon", niveau: Niveau::Erreur, titre: "Aménagement non validé", explication: "Une pièce est meublée d'après un modèle d'aménagement qui est encore en brouillon, qui a été modifié depuis sa validation, ou qui n'existe pas. Seul l'utilisateur valide un modèle (`hm amenagement-valider`, dans un terminal).", source: "modèles d'aménagement du projet (dossier amenagements/, validations.json)" },
    Regle { id: "sem.role_inconnu", niveau: Niveau::Information, titre: "Rôle sans exigence connue", explication: "Le rôle donné à des pièces n'est pas décrit dans regles/roles.yaml : rien n'y est exigé, rien n'y est vérifié.", source: "regles/roles.yaml" },
];

/// Ce qu'une pièce doit contenir pour être ce qu'elle dit être.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// Laissée vide exprès : rien n'est exigé.
    #[serde(default)]
    pub vide: bool,
    /// Groupes de mots : pour chaque groupe, un objet de la pièce doit contenir un des mots.
    #[serde(default)]
    pub exige: Vec<Vec<String>>,
    /// Surface d'air intérieur au-delà de laquelle le rôle n'est plus crédible, en unités carrées.
    #[serde(default)]
    pub surface_max: Option<f32>,
    /// Nombre minimal d'objets par tranche de 256 × 256 de sol.
    #[serde(default)]
    pub objets_min: Option<u32>,
}

#[derive(Deserialize)]
struct Fichier {
    roles: BTreeMap<String, Role>,
}

/// Le texte de `regles/roles.yaml`, embarqué à la compilation.
pub const ROLES_YAML: &str = include_str!("../../../regles/roles.yaml");

/// Les rôles connus. La table est celle du skill gmod-map-maker ; un test vérifie que la copie n'a pas dérivé.
pub fn roles() -> BTreeMap<String, Role> {
    serde_yaml::from_str::<Fichier>(ROLES_YAML).expect("regles/roles.yaml lisible").roles
}

/// Un objet posé dans une pièce : un modèle (ou un membre d'un modèle fusionné) ou une entité nommée.
#[derive(Clone, Debug)]
pub struct Objet {
    /// Chemin du modèle, ou nom de l'entité.
    pub nom: String,
    pub pos: Vec3,
    /// Rang de l'objet de la scène qui le porte.
    pub objet: u32,
}

/// Tous les objets de la map que les règles de rôle regardent : chaque modèle posé (les modèles fusionnés sont
/// dépliés en leurs membres), et chaque entité à solides nommée.
pub fn objets(scene: &Scene, modele: &Modele) -> Vec<Objet> {
    let assemblages: HashMap<&str, &hm_model::Assemblage> = modele.assemblages.iter().map(|a| (a.modele.as_str(), a)).collect();
    let mut out = Vec::new();
    for p in &scene.props {
        let o = &scene.objects[p.object as usize];
        match assemblages.get(o.model.as_str()) {
            Some(a) => {
                for m in &a.membres {
                    out.push(Objet { nom: m.modele.to_ascii_lowercase(), pos: p.transform.transform_point3(Vec3::from(m.pos)), objet: p.object });
                }
            }
            None => out.push(Objet { nom: o.model.clone(), pos: p.transform.w_axis.truncate(), objet: p.object }),
        }
        if !o.name.is_empty() {
            out.push(Objet { nom: o.name.to_ascii_lowercase(), pos: (o.min + o.max) * 0.5, objet: p.object });
        }
    }
    // ce que le constructeur de la map a déclaré : des solides sans nom dans le VMF, dont lui sait ce qu'ils sont
    for o in &modele.objets {
        out.push(Objet { nom: o.nom.to_ascii_lowercase(), pos: Vec3::from(o.pos), objet: 0 });
    }
    for b in &scene.brushes {
        if b.object != 0 && !b.name.is_empty() {
            let o = &scene.objects[b.object as usize];
            out.push(Objet { nom: b.name.to_ascii_lowercase(), pos: (o.min + o.max) * 0.5, objet: b.object });
        }
    }
    out
}

/// Les objets d'une pièce : ceux dont le point de pose est dans son emprise (à 8 près, une porte est dans le mur),
/// entre son sol et son plafond.
pub fn objets_de<'a>(piece: &Piece, tous: &'a [Objet]) -> Vec<&'a Objet> {
    let haut = piece.sol + piece.hauteur.unwrap_or(160.0);
    tous.iter().filter(|o| o.pos.x >= piece.emprise[0] - 8.0 && o.pos.x <= piece.emprise[2] + 8.0 && o.pos.y >= piece.emprise[1] - 8.0 && o.pos.y <= piece.emprise[3] + 8.0 && o.pos.z >= piece.sol - 16.0 && o.pos.z <= haut).collect()
}

fn boite_de(piece: &Piece) -> (Vec3, Vec3) {
    (Vec3::new(piece.emprise[0], piece.emprise[1], piece.sol), Vec3::new(piece.emprise[2], piece.emprise[3], piece.sol + piece.hauteur.unwrap_or(128.0)))
}

fn nommer(piece: &Piece) -> String {
    format!("pièce « {} » ({}, {:.0} × {:.0})", piece.id, piece.role, piece.largeur(), piece.profondeur())
}

pub fn verifier(scene: &Scene, monde: &Monde, modele: &Modele, config: &Config, out: &mut Vec<Constat>) {
    let table = roles();
    let tous = objets(scene, modele);
    let mut inconnus: BTreeMap<&str, Vec<&Piece>> = BTreeMap::new();
    for piece in &modele.pieces {
        let Some(role) = table.get(&piece.role) else {
            inconnus.entry(piece.role.as_str()).or_default().push(piece);
            continue;
        };
        if role.vide || piece.vide_expres {
            continue;
        }
        let (lo, hi) = boite_de(piece);
        let dedans = objets_de(piece, &tous);
        // les noms de fichier seuls : « models/props/cs_office/… » ne doit pas faire croire à un bureau
        // … augmentés de ce que le projet dit d'eux (`vocabulaire` : un modèle « poste_cellule » est un lit et des WC)
        let etendus: Vec<String> = dedans
            .iter()
            .map(|o| {
                let n = o.nom.rsplit('/').next().unwrap_or(&o.nom);
                let plus: Vec<&str> = config.vocabulaire.iter().filter(|(cle, _)| n.contains(cle.as_str())).flat_map(|(_, mots)| mots.iter().map(|m| m.as_str())).collect();
                if plus.is_empty() {
                    n.to_string()
                } else {
                    format!("{n} {}", plus.join(" "))
                }
            })
            .collect();
        let noms: Vec<&str> = etendus.iter().map(|s| s.as_str()).collect();
        let manque: Vec<String> = role.exige.iter().filter(|groupe| !noms.iter().any(|n| groupe.iter().any(|mot| n.contains(mot.as_str())))).map(|g| g.join(" ou ")).collect();
        if !manque.is_empty() {
            out.push(
                Constat::lieu("sem.role_incomplet", lo, hi, format!("{} : il manque {} ; elle contient {} objet(s){}", nommer(piece), manque.iter().map(|m| format!("[{m}]")).collect::<Vec<_>>().join(" et "), dedans.len(), resume_objets(&noms)))
                    .mesure(manque.len() as f32),
            );
        }
        if let Some(max) = role.surface_max {
            if piece.surface() > max {
                out.push(Constat::lieu("sem.role_trop_grand", lo, hi, format!("{} : {:.0} unités carrées, pour {:.0} admises par ce rôle ({:.1} fois trop)", nommer(piece), piece.surface(), max, piece.surface() / max)).mesure(piece.surface() / max));
            }
        }
        if let Some(min) = role.objets_min {
            let tranches = (piece.surface() / 65536.0).ceil().max(1.0) as u32;
            if (dedans.len() as u32) < min * tranches {
                out.push(Constat::lieu("sem.piece_sous_meublee", lo, hi, format!("{} : {} objet(s), pour {} attendus ({} par tranche de 256 × 256, {} tranche(s))", nommer(piece), dedans.len(), min * tranches, min, tranches)).mesure((min * tranches - dedans.len() as u32) as f32));
            }
        }
    }
    for (role, pieces) in inconnus {
        let lo = pieces.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(boite_de(p).0));
        let hi = pieces.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(boite_de(p).1));
        let mut c = Constat::lieu("sem.role_inconnu", lo, hi, format!("rôle « {role} » : {} pièce(s), aucune exigence connue (la première : {})", pieces.len(), pieces[0].id)).mesure(pieces.len() as f32);
        // pas de lieu précis à montrer : la boîte englobe toutes les pièces du rôle
        c.min = c.max;
        out.push(c);
    }
    amenagements(scene, modele, out);
    baies(scene, monde, modele, out);
    degagements(scene, modele, &tous, out);
    toits(monde, modele, out);
    voies(scene, monde, modele, config, &tous, out);
}

/// Rôles d'une pièce qui doit pouvoir se fermer.
const A_FERMER: &[&str] = &["bureau", "wc", "salle_de_bain", "chambre", "reunion", "cellule", "stockage", "vestiaire"];

fn est_porte(classe: &str) -> bool {
    classe.starts_with("prop_door") || classe == "func_door" || classe == "func_door_rotating" || classe == "func_movelinear"
}

/// Le tour de chaque pièce à fermer, à 40 du sol : mur, porte ou vide, tous les 8.
fn baies(scene: &Scene, monde: &Monde, modele: &Modele, out: &mut Vec<Constat>) {
    for piece in modele.pieces.iter().filter(|p| A_FERMER.contains(&p.role.as_str())) {
        let e = piece.emprise;
        let z = piece.sol + 40.0;
        // quatre côtés : (origine, direction le long du côté, normale sortante, longueur)
        let cotes = [
            (Vec3::new(e[0], e[1], z), Vec3::X, -Vec3::Y, e[2] - e[0]),
            (Vec3::new(e[0], e[3], z), Vec3::X, Vec3::Y, e[2] - e[0]),
            (Vec3::new(e[0], e[1], z), Vec3::Y, -Vec3::X, e[3] - e[1]),
            (Vec3::new(e[2], e[1], z), Vec3::Y, Vec3::X, e[3] - e[1]),
        ];
        for (o, le_long, dehors, long) in cotes {
            let n = (long / 8.0).floor() as usize;
            // 0 mur, 1 vide, 2 porte
            let etats: Vec<u8> = (1..n)
                .map(|i| {
                    let dedans = o + le_long * (i as f32 * 8.0) - dehors * 6.0;
                    match monde.rayon(dedans, dedans + dehors * 34.0, |c, _| c.visible && c.genre != Genre::Terrain && (c.genre != Genre::Prop || !c.approche || c.max.z - c.min.z >= 64.0)) {
                        None => 1,
                        Some((_, k)) => {
                            if est_porte(&scene.objects[monde.corps[k as usize].objet as usize].class) {
                                2
                            } else {
                                0
                            }
                        }
                    }
                })
                .collect();
            let mut i = 0;
            while i < etats.len() {
                if etats[i] == 0 {
                    i += 1;
                    continue;
                }
                let debut = i;
                while i < etats.len() && etats[i] != 0 {
                    i += 1;
                }
                let largeur = (i - debut) as f32 * 8.0;
                let a_porte = etats[debut..i].contains(&2);
                if a_porte || !(40.0..=136.0).contains(&largeur) {
                    continue; // une porte est là, ou ce n'est pas une baie de porte (fente, ou cloison ouverte)
                }
                let centre = o + le_long * ((debut + i) as f32 * 4.0 + 8.0);
                // une porte ouverte ou posée en retrait compte aussi : toute porte à moins de 56 du milieu de la baie
                let proche = monde.corps_pres(centre - Vec3::splat(56.0), centre + Vec3::splat(56.0)).iter().any(|&k| est_porte(&scene.objects[monde.corps[k as usize].objet as usize].class));
                if !proche {
                    out.push(
                        Constat::lieu("sem.baie_sans_porte", centre - Vec3::new(64.0, 64.0, 40.0), centre + Vec3::new(64.0, 64.0, 88.0), format!("{} : une ouverture de {largeur:.0} de large, sans porte, vers ({:.0} {:.0} {:.0})", nommer(piece), centre.x, centre.y, piece.sol))
                            .mesure(largeur),
                    );
                }
            }
        }
    }
}

const SANITAIRES: &[&str] = &["toilet", "cuvette", "lavabo", "sink", "vasque", "baignoire", "bath", "douche", "shower", "urinoir", "urinal"];

/// Deux équipements sanitaires d'une même pièce à moins de 12 l'un de l'autre. Seuls les modèles posés un à un sont
/// jugés : un membre d'un modèle fusionné n'a pas de boîte connue.
fn degagements(scene: &Scene, modele: &Modele, tous: &[Objet], out: &mut Vec<Constat>) {
    for piece in modele.pieces.iter().filter(|p| p.role == "salle_de_bain" || p.role == "wc") {
        let equip: Vec<&Objet> = objets_de(piece, tous)
            .into_iter()
            .filter(|o| {
                let n = o.nom.rsplit('/').next().unwrap_or(&o.nom);
                SANITAIRES.iter().any(|m| n.contains(m)) && scene.objects[o.objet as usize].model == o.nom
            })
            .collect();
        for (i, a) in equip.iter().enumerate() {
            for b in &equip[i + 1..] {
                let (oa, ob) = (&scene.objects[a.objet as usize], &scene.objects[b.objet as usize]);
                // même genre d'équipement côte à côte (deux lavabos, une rangée d'urinoirs) : voulu
                if oa.model == ob.model {
                    continue;
                }
                let ecart = (0..2).map(|k| (oa.min[k].max(ob.min[k]) - oa.max[k].min(ob.max[k])).max(0.0)).fold(0.0f32, f32::max);
                if ecart < 12.0 {
                    let (lo, hi) = (oa.min.min(ob.min), oa.max.max(ob.max));
                    let court = |m: &str| m.rsplit('/').next().unwrap_or(m).to_string();
                    out.push(Constat::objet(scene, "sem.degagement", a.objet, format!("{} : {} et {} sont à {ecart:.0} l'un de l'autre", nommer(piece), court(&oa.model), court(&ob.model))).boite(lo, hi).mesure(12.0 - ecart).autres(vec![b.objet]));
                }
            }
        }
    }
}

/// Chaque toit déclare ses appuis, et chaque appui monte en plein jusqu'au toit.
fn toits(monde: &Monde, modele: &Modele, out: &mut Vec<Constat>) {
    for t in &modele.toits {
        let (lo, hi) = (Vec3::new(t.boite[0], t.boite[1], t.boite[2]), Vec3::new(t.boite[3], t.boite[4], t.boite[5]));
        if t.appuis.is_empty() {
            out.push(Constat::lieu("sem.toit_sans_appui", lo, hi, format!("toit « {} » : aucun appui déclaré", t.id)));
            continue;
        }
        for a in &t.appuis {
            let pied = Vec3::from(*a);
            // du pied au dessous du toit, tous les 16 : du plein partout
            let n = (((lo.z - pied.z) / 16.0).floor() as i32).max(1);
            let trous = (0..n).filter(|i| monde.dans(pied + Vec3::Z * (4.0 + *i as f32 * 16.0), |c, _| c.visible).is_none()).count();
            if lo.z <= pied.z || trous > 0 {
                out.push(Constat::lieu("sem.toit_sans_appui", lo.min(pied), hi, format!("toit « {} » : l'appui déclaré en ({:.0} {:.0} {:.0}) ne monte pas en plein jusqu'au toit ({} hauteur(s) sur {} dans le vide)", t.id, pied.x, pied.y, pied.z, trous, n)).mesure(trous as f32));
            }
        }
    }
}

/// Chaque bout de voie trouvé dans la map est une extrémité déclarée, et chaque terminus déclaré est posé.
fn voies(scene: &Scene, monde: &Monde, modele: &Modele, config: &Config, tous: &[Objet], out: &mut Vec<Constat>) {
    if modele.voies.is_empty() {
        return;
    }
    let declarees: Vec<(&str, &hm_model::Extremite)> = modele.voies.iter().flat_map(|v| v.extremites.iter().map(move |e| (v.id.as_str(), e))).collect();
    for (b, objet, _) in crate::reseau::bouts_libres(scene, monde, config) {
        if !declarees.iter().any(|(_, e)| Vec3::from(e.en).distance(b.pos) <= 320.0) {
            out.push(Constat::objet(scene, "sem.voie_sans_extremite", objet, format!("la voie s'arrête à {} ({:.0} {:.0} {:.0}) : aucune extrémité déclarée à cet endroit", b.nom, b.pos.x, b.pos.y, b.pos.z)).boite(b.pos - Vec3::splat(160.0), b.pos + Vec3::splat(160.0)));
        }
    }
    for (voie, e) in declarees {
        if e.genre != "terminus" {
            continue;
        }
        let p = Vec3::from(e.en);
        // le terminus peut être un modèle posé seul ou un membre d'un modèle fusionné
        let pose = tous.iter().any(|o| o.pos.distance(p) <= 320.0 && crate::physique::contient(&config.terminus, &o.nom));
        if !pose {
            out.push(Constat::lieu("sem.voie_sans_extremite", p - Vec3::splat(160.0), p + Vec3::splat(160.0), format!("voie « {voie} » : terminus déclaré en ({:.0} {:.0} {:.0}), mais aucun modèle de terminus n'y est posé", p.x, p.y, p.z)));
        }
    }
}

fn resume_objets(noms: &[&str]) -> String {
    if noms.is_empty() {
        return String::new();
    }
    let mut compte: BTreeMap<&str, usize> = BTreeMap::new();
    for n in noms {
        *compte.entry(n.split(' ').next().unwrap_or(n).trim_end_matches(".mdl")).or_default() += 1;
    }
    let mut v: Vec<(&str, usize)> = compte.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    format!(" : {}{}", v.iter().take(6).map(|(n, k)| if *k > 1 { format!("{k} {n}") } else { n.to_string() }).collect::<Vec<_>>().join(", "), if v.len() > 6 { ", …" } else { "" })
}

/// Le dossier des modèles d'aménagement d'un projet : `amenagements/` à côté de `hammer-master.toml`, sinon à côté
/// du VMF.
pub fn dossier_amenagements(vmf: &std::path::Path) -> std::path::PathBuf {
    let base = Config::trouver(vmf).and_then(|p| p.parent().map(|d| d.to_path_buf())).or_else(|| vmf.parent().map(|d| d.to_path_buf())).unwrap_or_default();
    base.join("amenagements")
}

/// Une pièce meublée d'après un modèle : ce modèle existe, il est validé, et il est fait pour ce rôle.
fn amenagements(scene: &Scene, modele: &Modele, out: &mut Vec<Constat>) {
    use hm_model::amenagement::{trouver, Statut};
    let dossier = dossier_amenagements(std::path::Path::new(&scene.path));
    for piece in modele.pieces.iter().filter(|p| !p.amenagement.is_empty()) {
        let (lo, hi) = boite_de(piece);
        let quoi = match trouver(&dossier, &piece.amenagement) {
            Err(e) => Some(format!("le modèle d'aménagement « {} » est introuvable ou illisible ({e:#})", piece.amenagement)),
            Ok((_, Statut::Brouillon)) => Some(format!("le modèle d'aménagement « {} » est en brouillon : il n'a pas été validé, ou il a changé depuis", piece.amenagement)),
            Ok((a, Statut::Valide)) if a.role != piece.role => Some(format!("le modèle d'aménagement « {} » est fait pour le rôle « {} »", piece.amenagement, a.role)),
            Ok(_) => None,
        };
        if let Some(q) = quoi {
            out.push(Constat::lieu("sem.amenagement_brouillon", lo, hi, format!("{} : {q}", nommer(piece))));
        }
    }
}
