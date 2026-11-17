//! Les commandes : toute modification du document en est une. Ce sont des données (sérialisables en JSON) : la
//! fenêtre les fabrique à partir de la souris et du clavier, le serveur MCP les reçoit de Claude, les tests les
//! rejouent.
use std::collections::BTreeSet;

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::geo::{self, Affine};
use crate::kv::{Bloc, Element};
use crate::modele::{Face, texte_vec3, Document, Entite, Groupe, Proprio, Solide};
use crate::textures::{self, Alignement, Justif, ReglageFaces};
use crate::tx::Tx;

/// Un objet que l'on désigne.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Ref {
    Solide(u64),
    Entite(u64),
    Groupe(u64),
}

/// Une face d'un solide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RefFace {
    pub solide: u64,
    pub face: u64,
}

fn vrai() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "commande", rename_all = "snake_case")]
pub enum Commande {
    /// Un bloc aligné sur les axes. `classe` : le lier tout de suite à une nouvelle entité (`func_detail`…).
    CreerBloc {
        min: [f64; 3],
        max: [f64; 3],
        materiau: String,
        #[serde(default)]
        classe: Option<String>,
    },
    /// Une entité ponctuelle.
    CreerEntite {
        classe: String,
        origine: [f64; 3],
        #[serde(default)]
        cles: Vec<(String, String)>,
    },
    Supprimer { objets: Vec<Ref> },
    Deplacer {
        objets: Vec<Ref>,
        delta: [f64; 3],
        #[serde(default = "vrai")]
        verrou_texture: bool,
    },
    /// Rotation en degrés autour d'un axe passant par `centre`.
    Tourner {
        objets: Vec<Ref>,
        centre: [f64; 3],
        axe: [f64; 3],
        degres: f64,
        #[serde(default = "vrai")]
        verrou_texture: bool,
    },
    /// Envoie la boîte englobante `de` sur la boîte `vers`.
    Redimensionner {
        objets: Vec<Ref>,
        de: [[f64; 3]; 2],
        vers: [[f64; 3]; 2],
        #[serde(default = "vrai")]
        verrou_texture: bool,
    },
    /// Miroir par rapport au plan d'axe `axe` (0 = X, 1 = Y, 2 = Z) passant par `centre`.
    Miroir {
        objets: Vec<Ref>,
        centre: [f64; 3],
        axe: usize,
    },
    /// Copie les objets, décalés de `delta` ; les copies sont rendues comme nouveaux objets.
    Dupliquer { objets: Vec<Ref>, delta: [f64; 3] },
    /// Colle un morceau de VMF (solides du monde, entités, groupes), décalé de `decalage`.
    Coller { vmf: String, decalage: [f64; 3] },
    /// « Paste Special » de Hammer (wiki Valve) : `copies` copies du morceau de VMF, la k-ième décalée de k fois
    /// `decalage` et tournée de k fois `rotation` (degrés autour de X, puis Y, puis Z, sens direct) autour du centre de
    /// l'original. `grouper` réunit les copies en un groupe ; les entités nommées reçoivent `prefixe` devant leur nom
    /// et, avec `noms_uniques`, un numéro qui n'existe pas encore dans la map ; les sorties entre elles (et
    /// `parentname`, `target`) suivent.
    CollerSpecial {
        vmf: String,
        copies: u32,
        decalage: [f64; 3],
        #[serde(default)]
        rotation: [f64; 3],
        #[serde(default)]
        grouper: bool,
        #[serde(default)]
        prefixe: String,
        #[serde(default)]
        noms_uniques: bool,
    },
    /// Matériau d'une liste de faces.
    MateriauFaces { faces: Vec<RefFace>, materiau: String },
    /// Matériau de toutes les faces des objets.
    MateriauObjets { objets: Vec<Ref>, materiau: String },
    /// Clés d'une entité (ou du monde, par son numéro) : `None` retire la clé.
    Cles { entite: u64, cles: Vec<(String, Option<String>)> },
    Grouper { objets: Vec<Ref> },
    Degrouper { groupes: Vec<u64> },
    /// Fait des solides une entité à brosses (Ctrl+T).
    LierAEntite { objets: Vec<Ref>, classe: String },
    /// Rend au monde les solides d'entités à brosses, qui disparaissent (Ctrl+Shift+W).
    RemettreAuMonde { entites: Vec<u64> },
    /// Échelle, décalage, rotation, échelle de lumière, lissage, axes de faces (champs absents : inchangés).
    ReglerFaces { faces: Vec<RefFace>, reglage: ReglageFaces },
    /// Justifie la texture (gauche, droite, haut, bas, centre, ajuster). `ensemble` : les faces sont traitées comme
    /// une seule. `tailles` : taille en texels de la texture de chaque matériau concerné.
    JustifierFaces {
        faces: Vec<RefFace>,
        justif: Justif,
        #[serde(default)]
        ensemble: bool,
        tailles: Vec<(String, [f64; 2])>,
    },
    /// Axes de texture alignés sur le monde ou sur la face.
    AlignerFaces { faces: Vec<RefFace>, sur: Alignement },
    /// Enroule la texture de `source` (matériau et alignement) sur chacune des `cibles`, continue au pli.
    EnroulerFaces { source: RefFace, cibles: Vec<RefFace> },
    /// Remplace un matériau par un autre, dans les objets donnés ou dans toute la map.
    RemplacerMateriau {
        ancien: String,
        nouveau: String,
        #[serde(default)]
        objets: Option<Vec<Ref>>,
    },
    /// Découpe des solides par un plan (trois points ; normale (p0 − p1) × (p2 − p1), « devant » est de son côté) et
    /// garde ce qui est derrière, devant, ou les deux (outil de découpe, Maj+X). La face de coupe prend `materiau`.
    Decouper {
        objets: Vec<Ref>,
        plan: [[f64; 3]; 3],
        garder: Garder,
        materiau: String,
    },
    /// Creuse les solides que touchent les `outils` (Ctrl+Maj+C) : chacun est remplacé par les morceaux qui restent
    /// hors des outils ; `cibles` limite les solides creusés (sinon tous ceux qui touchent).
    Creuser {
        outils: Vec<Ref>,
        #[serde(default)]
        cibles: Option<Vec<Ref>>,
    },
    /// Fond des solides en un seul, leur enveloppe convexe (Ctrl+Maj+M de Hammer++). Ils doivent avoir le même
    /// propriétaire (le monde, ou la même entité).
    Fusionner { objets: Vec<Ref> },
    /// Crée une primitive dans une boîte : cylindre, pointe, sphère, coin, arche, escalier.
    Primitive {
        forme: Forme,
        min: [f64; 3],
        max: [f64; 3],
        materiau: String,
        #[serde(default)]
        classe: Option<String>,
    },
    /// Insère un préfabriqué (un VMF) avec son origine en `origine` ; le jeton `&i` de ses noms devient un numéro
    /// qu'aucune insertion précédente n'a pris (wiki Valve, « Prefab »).
    InsererPrefab { vmf: String, origine: [f64; 3] },
    /// Fait des faces (à quatre côtés) des displacements plats de cette puissance (2 à 4).
    CreerDisp { faces: Vec<RefFace>, puissance: u32 },
    /// Rend des faces displacement à leur état de face plane.
    DetruireDisp { faces: Vec<RefFace> },
    /// Un coup de pinceau sur les displacements (ceux donnés, sinon tous ceux qu'il touche) : relever, abaisser,
    /// lisser, peindre l'alpha.
    PeindreDisp {
        #[serde(default)]
        faces: Option<Vec<RefFace>>,
        centre: [f64; 3],
        rayon: f64,
        pinceau: crate::disp::Pinceau,
    },
    /// Coud les bords communs de displacements voisins : les points d'un bord partagé prennent leur position moyenne.
    CoudreDisp { faces: Vec<RefFace> },
    /// Pose un décal (`infodecal`) en ce point : le jeu le colle sur la surface la plus proche (Maj+D).
    PoserDecal { point: [f64; 3], materiau: String },
    /// Pose un overlay (`info_overlay`) sur une face, centré en `point`, de `taille` unités (Maj+O).
    PoserOverlay { face: RefFace, point: [f64; 3], materiau: String, taille: [f64; 2] },
    /// Remplace les sorties d'une entité (onglet Sorties). Une sortie inchangée garde son texte d'origine ; une
    /// nouvelle s'écrit avec le séparateur de l'entité, sinon celui de la map (virgule ou ESC).
    Sorties { entite: u64, sorties: Vec<crate::io::Sortie> },
    /// Édite les sommets d'un solide (Maj+V) : déplacer des sommets ou des arêtes, couper une face (Ctrl+F). Refusé si
    /// une face n'est plus plane, si le solide n'est plus convexe, ou si deux faces restent dans le même plan.
    ModifierSommets { solide: u64, operations: Vec<crate::sommets::OpSommet> },
    /// Pose des entités sur la première surface sous elles (rayon vers le bas depuis leur origine) : `bas[i]` est la
    /// distance de l'origine au bas de l'entité (−min z des bornes de son modèle, que l'appelant lit) ; `aligner` :
    /// l'axe haut de l'entité suit la normale de la surface (lacet gardé). Une entité sans rien dessous ne bouge pas.
    /// `plafond` : vers le haut (« Move to Ceiling » de Hammer++), `bas[i]` est alors la distance de l'origine au haut
    /// de l'entité (max z).
    PoserAuSol {
        entites: Vec<u64>,
        bas: Vec<f64>,
        #[serde(default)]
        aligner: bool,
        #[serde(default)]
        plafond: bool,
    },
    /// Nouveau visgroup contenant ces objets ; `cache` : créé décoché (le « Hide selected objects » de Hammer).
    CreerVisgroup {
        nom: String,
        objets: Vec<Ref>,
        #[serde(default)]
        cache: bool,
    },
    AjouterAuVisgroup { visgroup: u64, objets: Vec<Ref> },
    RetirerDuVisgroup { visgroup: u64, objets: Vec<Ref> },
    /// Coche ou décoche un visgroup : ses objets sont montrés, ou rangés dans des blocs `hidden` (non compilés).
    MontrerVisgroup { visgroup: u64, visible: bool },
    RenommerVisgroup { visgroup: u64, nom: String },
    /// Monte (`monter`) ou descend un visgroup d'une place parmi ceux de même parent (les flèches du panneau).
    DeplacerVisgroup {
        visgroup: u64,
        #[serde(default)]
        monter: bool,
    },
    /// Supprime le visgroup (ses objets restent, visibles).
    SupprimerVisgroup { visgroup: u64 },
    /// La première boîte du premier cordon (créé s'il n'y en a pas, et coché), et l'état du cordon.
    Cordon { min: [f64; 3], max: [f64; 3], actif: bool },
    /// Remplace les cordons (Hammer++ en garde une liste) : `actif`, l'état du cordon ; seul ce qui touche une boîte
    /// d'un cordon coché est alors compilé.
    Cordons { actif: bool, liste: Vec<crate::visgroups::Cordon> },
    /// Évide des blocs alignés sur les axes : chacun devient six murs d'épaisseur `epaisseur`, à l'intérieur de sa
    /// boîte (une pièce fermée d'un seul geste). Chaque mur garde le matériau de la face qu'il remplace.
    Evider { objets: Vec<Ref>, epaisseur: f64 },
    /// Fond une `func_instance` dans la map (« Collapse » de Hammer, comme vbsp, wiki Valve « func_instance ») : le
    /// contenu de son fichier (`vmf`, que l'appelant lit) posé selon son origine et ses angles, les noms corrigés selon
    /// `fixup_style` (0 : `<instance>-<nom>`, 1 : `<nom>-<instance>`, 2 : inchangés ; un nom qui commence par @ est
    /// gardé) avec les sorties qui les visent, les paramètres `replaceNN` (« $variable valeur ») remplacés dans les
    /// valeurs ; l'instance est retirée.
    FondreInstance { entite: u64, vmf: String },
    /// Couleur d'éditeur (bloc `editor`, clé `color`) des objets : leurs solides et leurs entités, groupes dépliés
    /// (« Change Color of Objects » de Hammer++). Elle colore les solides dans les vues 2D.
    CouleurEditeur { objets: Vec<Ref>, couleur: [u8; 3] },
    /// Un solide convexe : l'enveloppe des `points` (au moins quatre, pas tous dans un plan), comme l'outil polygone
    /// de Hammer++ (un contour tracé dans une vue 2D, extrudé) ; `classe` : lié à cette entité à brosses.
    CreerEnveloppe {
        points: Vec<[f64; 3]>,
        materiau: String,
        #[serde(default)]
        classe: Option<String>,
    },
    /// Plusieurs commandes en une seule étape d'annulation.
    Lot { nom: String, commandes: Vec<Commande> },
}

/// Ce qu'une commande a produit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Resultat {
    /// Objets créés (à sélectionner après une création, une copie, un collage).
    pub crees: Vec<Ref>,
    pub message: String,
}

pub type Res<T> = Result<T, String>;

impl Commande {
    /// Nom lisible dans l'historique.
    pub fn nom(&self) -> String {
        let n = |v: &Vec<Ref>| if v.len() == 1 { "1 objet".to_string() } else { format!("{} objets", v.len()) };
        match self {
            Commande::CreerBloc { classe: Some(c), .. } => format!("créer un bloc ({c})"),
            Commande::CreerBloc { .. } => "créer un bloc".into(),
            Commande::CreerEntite { classe, .. } => format!("créer {classe}"),
            Commande::Supprimer { objets } => format!("supprimer {}", n(objets)),
            Commande::Deplacer { objets, delta, .. } => format!("déplacer {} de ({} {} {})", n(objets), crate::kv::nombre(delta[0]), crate::kv::nombre(delta[1]), crate::kv::nombre(delta[2])),
            Commande::Tourner { objets, degres, .. } => format!("tourner {} de {}°", n(objets), crate::kv::nombre(*degres)),
            Commande::Redimensionner { objets, .. } => format!("redimensionner {}", n(objets)),
            Commande::Miroir { objets, axe, .. } => format!("miroir de {} selon {}", n(objets), ["X", "Y", "Z"].get(*axe).unwrap_or(&"?")),
            Commande::Dupliquer { objets, .. } => format!("dupliquer {}", n(objets)),
            Commande::Coller { .. } => "coller".into(),
            Commande::CollerSpecial { copies, .. } => format!("coller spécial ({copies} copie(s))"),
            Commande::MateriauFaces { faces, materiau } => format!("{materiau} sur {} face(s)", faces.len()),
            Commande::MateriauObjets { objets, materiau } => format!("{materiau} sur {}", n(objets)),
            Commande::Cles { cles, .. } => format!("propriétés : {}", cles.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")),
            Commande::Grouper { objets } => format!("grouper {}", n(objets)),
            Commande::Degrouper { groupes } => format!("dégrouper {} groupe(s)", groupes.len()),
            Commande::LierAEntite { classe, .. } => format!("lier à {classe}"),
            Commande::RemettreAuMonde { entites } => format!("remettre au monde {} entité(s)", entites.len()),
            Commande::ReglerFaces { faces, .. } => format!("régler {} face(s)", faces.len()),
            Commande::JustifierFaces { faces, justif, .. } => format!("justifier {} face(s) ({justif:?})", faces.len()).to_lowercase(),
            Commande::AlignerFaces { faces, sur } => format!("aligner {} face(s) sur {}", faces.len(), if *sur == Alignement::Monde { "le monde" } else { "la face" }),
            Commande::EnroulerFaces { cibles, .. } => format!("enrouler la texture sur {} face(s)", cibles.len()),
            Commande::RemplacerMateriau { ancien, nouveau, .. } => format!("remplacer {ancien} par {nouveau}"),
            Commande::PoserAuSol { entites, plafond, .. } => format!("poser {} ({})", if *plafond { "au plafond" } else { "au sol" }, entites.len()),
            Commande::Decouper { objets, .. } => format!("découper {}", n(objets)),
            Commande::ModifierSommets { .. } => "modifier des sommets".into(),
            Commande::Sorties { sorties, .. } => format!("sorties ({})", sorties.len()),
            Commande::PoserDecal { materiau, .. } => format!("décal {materiau}"),
            Commande::InsererPrefab { .. } => "insérer un préfabriqué".into(),
            Commande::CreerDisp { faces, puissance } => format!("displacement de puissance {puissance} ({} face(s))", faces.len()),
            Commande::DetruireDisp { faces } => format!("détruire {} displacement(s)", faces.len()),
            Commande::PeindreDisp { pinceau, .. } => match pinceau {
                crate::disp::Pinceau::Relever { hauteur } if *hauteur < 0.0 => "abaisser un displacement".into(),
                crate::disp::Pinceau::Relever { .. } => "relever un displacement".into(),
                crate::disp::Pinceau::Lisser { .. } => "lisser un displacement".into(),
                crate::disp::Pinceau::Alpha { .. } => "peindre l'alpha".into(),
            },
            Commande::CoudreDisp { faces } => format!("coudre {} displacement(s)", faces.len()),
            Commande::PoserOverlay { materiau, .. } => format!("overlay {materiau}"),
            Commande::Creuser { outils, .. } => format!("creuser avec {}", n(outils)),
            Commande::Fusionner { objets } => format!("fusionner {}", n(objets)),
            Commande::Primitive { forme, .. } => format!("créer {}", forme.nom()),
            Commande::CreerVisgroup { nom, objets, .. } => format!("visgroup « {nom} » ({})", n(objets)),
            Commande::AjouterAuVisgroup { objets, .. } => format!("ajouter {} au visgroup", n(objets)),
            Commande::RetirerDuVisgroup { objets, .. } => format!("retirer {} du visgroup", n(objets)),
            Commande::MontrerVisgroup { visible, .. } => if *visible { "montrer un visgroup".into() } else { "cacher un visgroup".into() },
            Commande::RenommerVisgroup { nom, .. } => format!("renommer un visgroup en « {nom} »"),
            Commande::DeplacerVisgroup { monter, .. } => if *monter { "monter un visgroup".into() } else { "descendre un visgroup".into() },
            Commande::SupprimerVisgroup { .. } => "supprimer un visgroup".into(),
            Commande::Cordon { actif, .. } => if *actif { "cordon actif".into() } else { "cordon inactif".into() },
            Commande::Cordons { actif, liste } => format!("cordons ({}{})", liste.len(), if *actif { ", actifs" } else { "" }),
            Commande::Evider { objets, epaisseur } => format!("évider {} (murs de {})", n(objets), crate::kv::nombre(*epaisseur)),
            Commande::FondreInstance { .. } => "fondre une instance".into(),
            Commande::CouleurEditeur { objets, .. } => format!("couleur de {}", n(objets)),
            Commande::CreerEnveloppe { points, .. } => format!("créer un solide de {} points", points.len()),
            Commande::Lot { nom, .. } => nom.clone(),
        }
    }

    pub(crate) fn executer(&self, tx: &mut Tx) -> Res<Resultat> {
        match self {
            Commande::CreerBloc { min, max, materiau, classe } => {
                let (lo, hi) = (DVec3::from(*min).min(DVec3::from(*max)), DVec3::from(*min).max(DVec3::from(*max)));
                if (hi - lo).min_element() < 1e-6 {
                    return Err("un bloc doit avoir une épaisseur dans les trois directions".into());
                }
                let id = tx.nouvel_id();
                let faces = [tx.nouvelle_face(), tx.nouvelle_face(), tx.nouvelle_face(), tx.nouvelle_face(), tx.nouvelle_face(), tx.nouvelle_face()];
                let s = geo::boite(id, faces, lo, hi, materiau);
                match classe {
                    None => {
                        tx.ajouter_solide(Proprio::Monde, s);
                        Ok(Resultat { crees: vec![Ref::Solide(id)], message: String::new() })
                    }
                    Some(c) => {
                        let eid = tx.nouvel_id();
                        let mut e = Entite::nouvelle(eid, c);
                        e.editeur = Some(geo::editeur_neuf(eid));
                        e.solides.push(s);
                        tx.ajouter_entite(e);
                        Ok(Resultat { crees: vec![Ref::Entite(eid)], message: String::new() })
                    }
                }
            }
            Commande::CreerEntite { classe, origine, cles } => {
                if classe.trim().is_empty() {
                    return Err("une entité a besoin d'une classe".into());
                }
                let id = tx.nouvel_id();
                let mut e = Entite::nouvelle(id, classe.trim());
                for (k, v) in cles {
                    if !k.eq_ignore_ascii_case("classname") && !k.eq_ignore_ascii_case("origin") && !k.eq_ignore_ascii_case("id") {
                        e.set(k, v);
                    }
                }
                e.set("origin", &texte_vec3(DVec3::from(*origine)));
                e.editeur = Some(geo::editeur_neuf(id));
                tx.ajouter_entite(e);
                Ok(Resultat { crees: vec![Ref::Entite(id)], message: String::new() })
            }
            Commande::Supprimer { objets } => {
                let (solides, entites, groupes) = resoudre(tx.doc(), objets)?;
                for e in &entites {
                    tx.retirer_entite(*e);
                }
                let mut touchees = BTreeSet::new();
                for s in &solides {
                    if let Some(Proprio::Entite(e)) = tx.doc().proprio(*s) {
                        touchees.insert(e);
                    }
                    tx.retirer_solide(*s);
                }
                for g in &groupes {
                    tx.retirer_groupe(*g);
                }
                // une entité à brosses vidée de tous ses solides n'a plus de sens
                for e in touchees {
                    if tx.doc().entites.get(&e).map_or(false, |x| x.solides.is_empty()) {
                        tx.retirer_entite(e);
                    }
                }
                Ok(Resultat { crees: vec![], message: format!("{} solide(s), {} entité(s) supprimés", solides.len(), entites.len()) })
            }
            Commande::Deplacer { objets, delta, verrou_texture } => transformer(tx, objets, &Affine::translation(DVec3::from(*delta)), *verrou_texture),
            Commande::Tourner { objets, centre, axe, degres, verrou_texture } => transformer(tx, objets, &Affine::rotation(DVec3::from(*centre), DVec3::from(*axe), *degres), *verrou_texture),
            Commande::Redimensionner { objets, de, vers, verrou_texture } => {
                let de = (DVec3::from(de[0]), DVec3::from(de[1]));
                let vers = (DVec3::from(vers[0]), DVec3::from(vers[1]));
                if (vers.1 - vers.0).abs().min_element() < 1e-6 {
                    return Err("la nouvelle boîte est plate".into());
                }
                transformer(tx, objets, &Affine::boite(de, vers), *verrou_texture)
            }
            Commande::Miroir { objets, centre, axe } => {
                if *axe > 2 {
                    return Err("axe de miroir : 0, 1 ou 2".into());
                }
                transformer(tx, objets, &Affine::miroir(DVec3::from(*centre), *axe), true)
            }
            Commande::Dupliquer { objets, delta } => {
                let frag = crate::presse::copier(tx.doc(), objets)?;
                coller(tx, &frag, DVec3::from(*delta))
            }
            Commande::Coller { vmf, decalage } => coller(tx, vmf, DVec3::from(*decalage)),
            Commande::CollerSpecial { vmf, copies, decalage, rotation, grouper, prefixe, noms_uniques } => coller_special(tx, vmf, *copies, DVec3::from(*decalage), DVec3::from(*rotation), *grouper, prefixe, *noms_uniques),
            Commande::MateriauFaces { faces, materiau } => {
                for rf in faces {
                    let s = tx.solide_mut(rf.solide).ok_or_else(|| format!("solide {} introuvable", rf.solide))?;
                    let f = s.faces.iter_mut().find(|f| f.id == rf.face).ok_or_else(|| format!("face {} introuvable sur le solide {}", rf.face, rf.solide))?;
                    f.materiau = materiau.clone();
                }
                Ok(Resultat::default())
            }
            Commande::MateriauObjets { objets, materiau } => {
                let (solides, _, _) = resoudre(tx.doc(), objets)?;
                let mut tous: Vec<u64> = solides;
                for r in objets {
                    if let Ref::Entite(e) = r {
                        tous.extend(tx.doc().entites.get(e).map(|e| e.solides.iter().map(|s| s.id).collect::<Vec<_>>()).unwrap_or_default());
                    }
                }
                for id in tous {
                    if let Some(s) = tx.solide_mut(id) {
                        for f in &mut s.faces {
                            f.materiau = materiau.clone();
                        }
                    }
                }
                Ok(Resultat::default())
            }
            Commande::Cles { entite, cles } => {
                let monde = *entite == tx.doc().monde.id;
                let e = if monde { Some(tx.monde_mut()) } else { tx.entite_mut(*entite) };
                let e = e.ok_or_else(|| format!("entité {entite} introuvable"))?;
                for (k, v) in cles {
                    if k.eq_ignore_ascii_case("id") {
                        return Err("le numéro d'un objet ne se change pas".into());
                    }
                    match v {
                        Some(v) => {
                            if v.contains('"') {
                                return Err(format!("{k} : un guillemet ne peut pas entrer dans un VMF"));
                            }
                            e.set(k, v)
                        }
                        None => e.retirer(k),
                    }
                }
                Ok(Resultat::default())
            }
            Commande::Grouper { objets } => {
                if objets.is_empty() {
                    return Err("rien à grouper".into());
                }
                let id = tx.nouvel_id();
                let mut g = Groupe { id, editeur: geo::editeur_neuf(id), gabarit: Vec::new() };
                g.editeur.elements.retain(|e| !matches!(e, Element::Paire(k, _) if k == "visgroupautoshown"));
                tx.ajouter_groupe(g);
                for r in objets {
                    let ed = editeur_de(tx, *r).ok_or_else(|| format!("{r:?} introuvable"))?;
                    ed.set("groupid", &id.to_string());
                }
                Ok(Resultat { crees: vec![Ref::Groupe(id)], message: String::new() })
            }
            Commande::Degrouper { groupes } => {
                for g in groupes {
                    let parent = tx.doc().groupes.get(g).ok_or_else(|| format!("groupe {g} introuvable"))?.editeur.get("groupid").map(String::from);
                    for r in membres_directs(tx.doc(), *g) {
                        let ed = editeur_de(tx, r).unwrap();
                        match &parent {
                            Some(p) => ed.set("groupid", p),
                            None => ed.retirer("groupid"),
                        }
                    }
                    tx.retirer_groupe(*g);
                }
                Ok(Resultat::default())
            }
            Commande::LierAEntite { objets, classe } => {
                let (solides, _, _) = resoudre(tx.doc(), objets)?;
                if solides.is_empty() {
                    return Err("aucun solide à lier".into());
                }
                let eid = tx.nouvel_id();
                let mut e = Entite::nouvelle(eid, classe);
                e.editeur = Some(geo::editeur_neuf(eid));
                for s in &solides {
                    if let Some(s) = tx.retirer_solide(*s) {
                        e.solides.push(s);
                    }
                }
                tx.ajouter_entite(e);
                Ok(Resultat { crees: vec![Ref::Entite(eid)], message: String::new() })
            }
            Commande::RemettreAuMonde { entites } => {
                let mut crees = Vec::new();
                for id in entites {
                    let e = tx.retirer_entite(*id).ok_or_else(|| format!("entité {id} introuvable"))?;
                    if e.solides.is_empty() {
                        return Err(format!("{} n'a pas de solides", e.classe()));
                    }
                    for s in e.solides {
                        crees.push(Ref::Solide(s.id));
                        tx.ajouter_solide(Proprio::Monde, s);
                    }
                }
                Ok(Resultat { crees, message: String::new() })
            }
            Commande::ReglerFaces { faces, reglage } => {
                for rf in faces {
                    let f = face_mut(tx, *rf)?;
                    textures::appliquer_reglage(f, reglage);
                }
                Ok(Resultat::default())
            }
            Commande::JustifierFaces { faces, justif, ensemble, tailles } => {
                // les points de chaque face, et ceux de toutes les faces si elles sont traitées comme une seule
                let mut points: Vec<(RefFace, Vec<DVec3>)> = Vec::new();
                for rf in faces {
                    points.push((*rf, polygone(tx.doc(), *rf)?));
                }
                let tous: Vec<DVec3> = points.iter().flat_map(|(_, p)| p.iter().copied()).collect();
                for (rf, poly) in &points {
                    let f = face_mut(tx, *rf)?;
                    let t = tailles.iter().find(|(m, _)| m.eq_ignore_ascii_case(&f.materiau)).map(|(_, t)| *t).or_else(|| tailles.first().map(|(_, t)| *t)).unwrap_or([512.0, 512.0]);
                    let pts = if *ensemble { &tous } else { poly };
                    textures::justifier(f, (DVec3::ZERO, DVec3::ZERO, pts), t, *justif);
                }
                Ok(Resultat::default())
            }
            Commande::AlignerFaces { faces, sur } => {
                for rf in faces {
                    textures::aligner(face_mut(tx, *rf)?, *sur);
                }
                Ok(Resultat::default())
            }
            Commande::EnroulerFaces { source, cibles } => {
                let src = tx.doc().solide(source.solide).and_then(|s| s.faces.iter().find(|f| f.id == source.face)).cloned().ok_or("face source introuvable")?;
                let psrc = polygone(tx.doc(), *source)?;
                for rf in cibles {
                    if rf == source {
                        continue;
                    }
                    let pc = polygone(tx.doc(), *rf)?;
                    let pivot = textures::pivot_commun(&psrc, &pc);
                    textures::enrouler(&src, face_mut(tx, *rf)?, pivot);
                }
                Ok(Resultat::default())
            }
            Commande::RemplacerMateriau { ancien, nouveau, objets } => {
                let ids: Vec<u64> = match objets {
                    Some(o) => {
                        let (mut s, e, _) = resoudre(tx.doc(), o)?;
                        for id in e {
                            s.extend(tx.doc().entites[&id].solides.iter().map(|x| x.id));
                        }
                        s
                    }
                    None => tx.doc().tous_les_solides().map(|(_, s)| s.id).collect(),
                };
                let mut n = 0;
                for id in ids {
                    let touche = tx.doc().solide(id).map_or(false, |s| s.faces.iter().any(|f| f.materiau.eq_ignore_ascii_case(ancien)));
                    if touche {
                        for f in &mut tx.solide_mut(id).unwrap().faces {
                            if f.materiau.eq_ignore_ascii_case(ancien) {
                                f.materiau = nouveau.clone();
                                n += 1;
                            }
                        }
                    }
                }
                Ok(Resultat { crees: vec![], message: format!("{n} face(s) remplacée(s)") })
            }
            Commande::InsererPrefab { vmf, origine } => {
                let texte = if vmf.contains("&i") {
                    // le plus petit numéro dont aucun nom n'est déjà pris dans la map
                    let noms: std::collections::HashSet<String> = tx.doc().noms().into_iter().map(|n| n.to_ascii_lowercase()).collect();
                    let (frag, _) = crate::modele::Document::lire(vmf.as_bytes()).map_err(|e| e.0)?;
                    let modeles: Vec<String> = frag.entites().filter_map(|e| e.nom_donne()).filter(|n| n.contains("&i")).map(|n| n.to_ascii_lowercase()).collect();
                    let n = (1u64..).find(|n| modeles.iter().all(|m| !noms.contains(&m.replace("&i", &n.to_string())))).unwrap();
                    vmf.replace("&i", &n.to_string())
                } else {
                    vmf.clone()
                };
                coller(tx, &texte, DVec3::from(*origine))
            }
            Commande::CreerDisp { faces, puissance } => {
                if !(2..=4).contains(puissance) {
                    return Err(format!("puissance {puissance} : 2, 3 ou 4 seulement (wiki Valve)"));
                }
                for rf in faces {
                    let poly = polygone(tx.doc(), *rf)?;
                    if poly.len() != 4 {
                        return Err(format!("la face {} a {} côtés : un displacement demande une face à quatre côtés", rf.face, poly.len()));
                    }
                    let f = face_mut(tx, *rf)?;
                    let n = f.normale();
                    f.disp = Some(crate::disp::Disp::neuf(*puissance, poly[0], n).ecrire());
                }
                Ok(Resultat::default())
            }
            Commande::DetruireDisp { faces } => {
                for rf in faces {
                    let f = face_mut(tx, *rf)?;
                    if f.disp.take().is_none() {
                        return Err(format!("la face {} n'est pas un displacement", rf.face));
                    }
                }
                Ok(Resultat::default())
            }
            Commande::PeindreDisp { faces, centre, rayon, pinceau } => {
                if *rayon <= 0.0 {
                    return Err("le rayon du pinceau doit être positif".into());
                }
                let c = DVec3::from(*centre);
                // les faces displacement visées, ou toutes celles que le pinceau peut toucher
                let cibles: Vec<RefFace> = match faces {
                    Some(f) => f.clone(),
                    None => tx
                        .doc()
                        .tous_les_solides()
                        .flat_map(|(_, s)| s.faces.iter().filter(|f| f.disp.is_some()).map(move |f| RefFace { solide: s.id, face: f.id }))
                        .collect(),
                };
                let mut touchees = 0;
                for rf in cibles {
                    let Ok(poly) = polygone(tx.doc(), rf) else { continue };
                    let Some(f) = tx.doc().solide(rf.solide).and_then(|s| s.faces.iter().find(|x| x.id == rf.face)) else { continue };
                    let Some(bloc) = &f.disp else { continue };
                    let Some(mut d) = crate::disp::Disp::lire(bloc) else { continue };
                    let n = f.normale();
                    // rejet rapide : le pinceau ne touche pas la boîte des points
                    if let Some((_, monde)) = d.points(&poly, n) {
                        let (lo, hi) = monde.iter().fold((DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
                        if (c.clamp(lo, hi) - c).length() > *rayon {
                            continue;
                        }
                    }
                    if d.peindre(&poly, n, c, *rayon, *pinceau) {
                        face_mut(tx, rf)?.disp = Some(d.ecrire());
                        touchees += 1;
                    }
                }
                Ok(Resultat { crees: vec![], message: format!("{touchees} displacement(s) touché(s)") })
            }
            Commande::CoudreDisp { faces } => {
                // points de bord de chaque displacement : (face, indice, position à plat, position)
                let mut infos: Vec<(RefFace, crate::disp::Disp, Vec<DVec3>, Vec<DVec3>, Vec<DVec3>, DVec3)> = Vec::new();
                for rf in faces {
                    let poly = polygone(tx.doc(), *rf)?;
                    let f = tx.doc().solide(rf.solide).and_then(|s| s.faces.iter().find(|x| x.id == rf.face)).unwrap();
                    let d = f.disp.as_ref().and_then(crate::disp::Disp::lire).ok_or_else(|| format!("la face {} n'est pas un displacement", rf.face))?;
                    let n = f.normale();
                    let (plat, monde) = d.points(&poly, n).ok_or("displacement illisible")?;
                    infos.push((*rf, d, poly, plat, monde, n));
                }
                // regroupe les points de bord confondus à plat, et les met à leur position moyenne
                let bord = |k: usize, x: usize| {
                    let (i, j) = (x / k, x % k);
                    i == 0 || j == 0 || i == k - 1 || j == k - 1
                };
                let mut cibles: Vec<Vec<(usize, usize)>> = Vec::new();
                let mut vus = std::collections::HashSet::new();
                for (a, (_, d, _, plat, _, _)) in infos.iter().enumerate() {
                    let k = d.cote();
                    for x in 0..k * k {
                        if !bord(k, x) || vus.contains(&(a, x)) {
                            continue;
                        }
                        let mut groupe = vec![(a, x)];
                        for (b, (_, d2, _, plat2, _, _)) in infos.iter().enumerate().skip(a + 1) {
                            let k2 = d2.cote();
                            if let Some(y) = (0..k2 * k2).find(|y| bord(k2, *y) && (plat2[*y] - plat[x]).length() < 0.1) {
                                groupe.push((b, y));
                            }
                        }
                        for g in &groupe {
                            vus.insert(*g);
                        }
                        if groupe.len() > 1 {
                            cibles.push(groupe);
                        }
                    }
                }
                if cibles.is_empty() {
                    return Err("ces displacements n'ont pas de bord commun".into());
                }
                for groupe in &cibles {
                    let moy = groupe.iter().fold(DVec3::ZERO, |s, (a, x)| s + infos[*a].4[*x]) / groupe.len() as f64;
                    for (a, x) in groupe {
                        infos[*a].4[*x] = moy;
                    }
                }
                for (rf, mut d, poly, plat, monde, n) in infos {
                    let (_, avant) = d.points(&poly, n).unwrap();
                    let mut change = false;
                    for x in 0..monde.len() {
                        if (monde[x] - avant[x]).length() > 1e-6 {
                            d.placer_public(x, plat[x], n, monde[x]);
                            change = true;
                        }
                    }
                    if change {
                        d.etiqueter(&monde);
                        face_mut(tx, rf)?.disp = Some(d.ecrire());
                    }
                }
                Ok(Resultat { crees: vec![], message: format!("{} point(s) cousu(s)", cibles.len()) })
            }
            Commande::PoserDecal { point, materiau } => {
                if materiau.trim().is_empty() {
                    return Err("un décal sans matériau".into());
                }
                Commande::CreerEntite { classe: "infodecal".into(), origine: *point, cles: vec![("texture".into(), materiau.clone())] }.executer(tx)
            }
            Commande::PoserOverlay { face, point, materiau, taille } => {
                if materiau.trim().is_empty() || taille[0] <= 0.0 || taille[1] <= 0.0 {
                    return Err("un overlay demande un matériau et une taille positive".into());
                }
                let s = tx.doc().solide(face.solide).ok_or_else(|| format!("solide {} introuvable", face.solide))?;
                let f = s.faces.iter().find(|x| x.id == face.face).ok_or_else(|| format!("face {} introuvable", face.face))?.clone();
                let cles = crate::decals::cles_overlay(DVec3::from(*point), &f, materiau, *taille);
                Commande::CreerEntite { classe: "info_overlay".into(), origine: *point, cles }.executer(tx)
            }
            Commande::Sorties { entite, sorties } => {
                let e = tx.doc().entite(*entite).ok_or_else(|| format!("entité {entite} introuvable"))?;
                for x in sorties {
                    if x.sortie.trim().is_empty() {
                        return Err("une sortie sans nom".into());
                    }
                    for (champ, v) in [("sortie", &x.sortie), ("cible", &x.cible), ("entrée", &x.entree), ("paramètre", &x.parametre)] {
                        if v.contains('"') || v.contains(crate::io::ESC) {
                            return Err(format!("{champ} « {v} » : pas de guillemet ni de caractère ESC"));
                        }
                    }
                    if !(x.delai >= 0.0) {
                        return Err(format!("délai {} : doit être positif ou nul", x.delai));
                    }
                    if x.fois == 0 || x.fois < -1 {
                        return Err(format!("{} fois : −1 (toujours) ou au moins 1", x.fois));
                    }
                }
                let origine: Vec<(String, String)> = e.sorties.clone().unwrap_or_default();
                // le séparateur : celui de l'entité, sinon celui de la map ; une virgule dans un paramètre impose ESC
                let a_esc = origine.iter().any(|(_, v)| v.contains(crate::io::ESC));
                let a_virgule = origine.iter().any(|(_, v)| !v.contains(crate::io::ESC));
                let mut esc = if a_esc { true } else if a_virgule { false } else { tx.doc().sorties_en_esc() };
                if sorties.iter().any(|x| x.parametre.contains(',') || x.cible.contains(',') || x.entree.contains(',')) {
                    esc = true;
                }
                let mut libres: Vec<bool> = vec![true; origine.len()];
                let mut neuves = Vec::new();
                for x in sorties {
                    let reprise = origine.iter().enumerate().find(|(i, (k, v))| libres[*i] && crate::io::lire(k, v) == *x && v.contains(crate::io::ESC) == esc);
                    match reprise {
                        Some((i, kv)) => {
                            libres[i] = false;
                            neuves.push(kv.clone());
                        }
                        None => neuves.push((x.sortie.clone(), crate::io::ecrire(x, esc))),
                    }
                }
                let e = tx.entite_mut(*entite).unwrap();
                e.sorties = if neuves.is_empty() && e.sorties.is_none() { None } else { Some(neuves) };
                Ok(Resultat::default())
            }
            Commande::ModifierSommets { solide, operations } => {
                let s = tx.doc().solide(*solide).ok_or_else(|| format!("solide {solide} introuvable"))?.clone();
                let mut m = crate::sommets::Maillage::de(&s);
                for op in operations {
                    m.appliquer(op)?;
                }
                let neuf = m.solide(*solide, &s.editeur, &mut || tx.nouvelle_face())?;
                *tx.solide_mut(*solide).unwrap() = neuf;
                Ok(Resultat { crees: selection_de(tx.doc(), vec![Ref::Solide(*solide)]), message: String::new() })
            }
            Commande::Decouper { objets, plan, garder, materiau } => {
                let p = crate::brosses::Plan::de_points(plan.map(DVec3::from)).ok_or("le plan de coupe est dégénéré (points alignés)")?;
                let texture = crate::brosses::Gabarit::neuf(p, materiau).texture;
                let mut crees = Vec::new();
                let mut coupes = 0;
                for id in solides_de(tx.doc(), objets)? {
                    let s = tx.doc().solide(id).unwrap().clone();
                    if !crate::brosses::coupe(&s, &p) {
                        crees.push(Ref::Solide(id));
                        continue;
                    }
                    let proprio = tx.doc().proprio(id).unwrap();
                    let (derriere, devant) = crate::brosses::couper(&s, &p, &texture);
                    let parts: Vec<Vec<crate::brosses::Gabarit>> = match garder {
                        Garder::Derriere => vec![derriere],
                        Garder::Devant => vec![devant],
                        Garder::LesDeux => vec![derriere, devant],
                    };
                    let mut neufs = Vec::new();
                    for (k, g) in parts.iter().enumerate() {
                        let nid = if k == 0 { id } else { tx.nouvel_id() };
                        let sol = crate::brosses::solide_de(nid, g, &mut || tx.nouvelle_face(), &s.editeur, k == 0).ok_or_else(|| format!("le solide {id} coupé donnerait un morceau sans volume"))?;
                        neufs.push(sol);
                    }
                    let mut neufs = neufs.into_iter();
                    *tx.solide_mut(id).unwrap() = neufs.next().unwrap();
                    crees.push(Ref::Solide(id));
                    for sol in neufs {
                        crees.push(Ref::Solide(sol.id));
                        tx.ajouter_solide(proprio, sol);
                    }
                    coupes += 1;
                }
                if coupes == 0 {
                    return Err("le plan ne coupe aucun des solides".into());
                }
                Ok(Resultat { crees: selection_de(tx.doc(), crees), message: format!("{coupes} solide(s) coupé(s)") })
            }
            Commande::Creuser { outils, cibles } => {
                let outils_ids = solides_de(tx.doc(), outils)?;
                let cibles_ids: Vec<u64> = match cibles {
                    Some(c) => solides_de(tx.doc(), c)?,
                    None => tx.doc().tous_les_solides().map(|(_, s)| s.id).collect(),
                };
                let mut creuses = 0;
                for o in &outils_ids {
                    let b = tx.doc().solide(*o).unwrap().clone();
                    let faces_b: Vec<Face> = b.faces.iter().filter(|f| crate::brosses::Plan::de_face(f).is_some()).cloned().collect();
                    let plans: Vec<crate::brosses::Plan> = faces_b.iter().filter_map(crate::brosses::Plan::de_face).collect();
                    let (blo, bhi) = b.boite().ok_or("outil sans volume")?;
                    for c in &cibles_ids {
                        if outils_ids.contains(c) || tx.doc().solide(*c).is_none() {
                            continue;
                        }
                        let a = tx.doc().solide(*c).unwrap().clone();
                        let Some((alo, ahi)) = a.boite() else { continue };
                        if alo.x >= bhi.x || ahi.x <= blo.x || alo.y >= bhi.y || ahi.y <= blo.y || alo.z >= bhi.z || ahi.z <= blo.z {
                            continue;
                        }
                        // se recouvrent-ils vraiment ?
                        let mut inter = crate::brosses::gabarits(&a);
                        inter.extend(plans.iter().zip(&faces_b).map(|(p, f)| crate::brosses::Gabarit { plan: *p, texture: f.clone() }));
                        if crate::brosses::solide_de(0, &inter, &mut || 0, &a.editeur, false).is_none() {
                            continue;
                        }
                        let morceaux = crate::brosses::soustraire(&crate::brosses::gabarits(&a), &plans, &|i| {
                            let mut f = faces_b[i].clone();
                            f.id = 0;
                            f
                        });
                        let proprio = tx.doc().proprio(*c).unwrap();
                        let mut premier = true;
                        tx.retirer_solide(*c);
                        for m in morceaux {
                            let nid = if premier { *c } else { tx.nouvel_id() };
                            if let Some(sol) = crate::brosses::solide_de(nid, &m, &mut || tx.nouvelle_face(), &a.editeur, premier) {
                                tx.ajouter_solide(proprio, sol);
                                premier = false;
                            }
                        }
                        creuses += 1;
                    }
                }
                if creuses == 0 {
                    return Err("les outils ne touchent aucun solide à creuser".into());
                }
                Ok(Resultat { crees: vec![], message: format!("{creuses} solide(s) creusé(s)") })
            }
            Commande::Fusionner { objets } => {
                let ids = solides_de(tx.doc(), objets)?;
                if ids.len() < 2 {
                    return Err("il faut au moins deux solides à fusionner".into());
                }
                let proprio = tx.doc().proprio(ids[0]).unwrap();
                if ids.iter().any(|i| tx.doc().proprio(*i) != Some(proprio)) {
                    return Err("les solides n'ont pas le même propriétaire (monde ou entité)".into());
                }
                let solides: Vec<Solide> = ids.iter().map(|i| tx.doc().solide(*i).unwrap().clone()).collect();
                let points: Vec<DVec3> = solides.iter().flat_map(crate::brosses::sommets).collect();
                if points.len() > 160 {
                    return Err(format!("{} sommets : trop pour une fusion (160 au plus)", points.len()));
                }
                let plans = crate::brosses::enveloppe(&points);
                if plans.is_empty() {
                    return Err("les solides n'ont pas de volume à eux tous".into());
                }
                // chaque face de l'enveloppe prend la texture de la face d'origine du même plan, sinon de la plus proche
                let faces: Vec<&Face> = solides.iter().flat_map(|s| s.faces.iter()).collect();
                let score = |f: &Face, p: &crate::brosses::Plan| crate::brosses::Plan::de_face(f).map_or(-9.0, |q| q.n.dot(p.n) - (q.d - p.d).abs() * 1e-3);
                let g: Vec<crate::brosses::Gabarit> = plans
                    .iter()
                    .map(|p| {
                        let f = faces.iter().max_by(|a, b| score(a, p).total_cmp(&score(b, p))).unwrap();
                        let mut t = (*f).clone();
                        t.id = 0;
                        crate::brosses::Gabarit { plan: *p, texture: t }
                    })
                    .collect();
                let somme: f64 = solides.iter().map(crate::brosses::volume).sum();
                let sol = crate::brosses::solide_de(ids[0], &g, &mut || tx.nouvelle_face(), &solides[0].editeur, false).ok_or("l'enveloppe n'a pas de volume")?;
                let ajout = crate::brosses::volume(&sol) - somme;
                for i in &ids[1..] {
                    tx.retirer_solide(*i);
                }
                *tx.solide_mut(ids[0]).unwrap() = sol;
                let message = if ajout > somme * 0.005 + 1.0 { format!("l'enveloppe ajoute {ajout:.0} unités³ : les solides ne formaient pas une forme convexe") } else { String::new() };
                Ok(Resultat { crees: selection_de(tx.doc(), vec![Ref::Solide(ids[0])]), message })
            }
            Commande::Primitive { forme, min, max, materiau, classe } => {
                let (lo, hi) = (DVec3::from(*min).min(DVec3::from(*max)), DVec3::from(*min).max(DVec3::from(*max)));
                if (hi - lo).min_element() < 1.0 {
                    return Err("la boîte d'une primitive doit avoir une épaisseur dans les trois directions".into());
                }
                let pieces = forme.pieces(lo, hi)?;
                let mut crees = Vec::new();
                for plans in pieces {
                    let g: Vec<crate::brosses::Gabarit> = plans.iter().map(|p| crate::brosses::Gabarit::neuf(*p, materiau)).collect();
                    let id = tx.nouvel_id();
                    let ed = geo::editeur_neuf(id);
                    let sol = crate::brosses::solide_de(id, &g, &mut || tx.nouvelle_face(), &ed, false).ok_or_else(|| format!("{} : une pièce n'a pas de volume (boîte trop petite pour tant de côtés ?)", forme.nom()))?;
                    tx.ajouter_solide(Proprio::Monde, sol);
                    crees.push(Ref::Solide(id));
                }
                let mut sortie = crees.clone();
                if let Some(c) = classe {
                    sortie = Commande::LierAEntite { objets: crees.clone(), classe: c.clone() }.executer(tx)?.crees;
                } else if crees.len() > 1 {
                    sortie = Commande::Grouper { objets: crees.clone() }.executer(tx)?.crees;
                }
                Ok(Resultat { crees: sortie, message: String::new() })
            }
            Commande::PoserAuSol { entites, bas, aligner, plafond } => {
                if bas.len() != entites.len() {
                    return Err(format!("{} entités pour {} valeurs de bas", entites.len(), bas.len()));
                }
                let mut posees = 0;
                for (id, b) in entites.iter().zip(bas) {
                    let e = tx.doc().entite(*id).ok_or_else(|| format!("entité {id} introuvable"))?;
                    let o = e.origine().ok_or_else(|| format!("entité {id} sans origine"))?;
                    let siens: Vec<u64> = e.solides.iter().map(|s| s.id).collect();
                    let angles = e.angles();
                    // depuis un peu au-dessus de son bas (au-dessous de son haut), pour ne pas manquer une surface
                    // qu'elle touche déjà
                    let (depart, sens) = if *plafond { (o + DVec3::Z * (b - 1.0), DVec3::Z) } else { (o + DVec3::Z * (1.0 - b).max(1.0), -DVec3::Z) };
                    let Some(imp) = crate::geo::rayon(tx.doc(), depart, sens, &|s| siens.contains(&s)) else { continue };
                    let (no, na) = if *plafond && !*aligner { (imp.point - DVec3::Z * *b, angles) } else { crate::geo::pose_sur(imp.point, imp.normale, *b, *aligner, angles) };
                    let e = tx.entite_mut(*id).unwrap();
                    e.set("origin", &crate::modele::texte_vec3(no));
                    if *aligner || e.get("angles").is_some() {
                        e.set("angles", &crate::modele::texte_vec3(na));
                    }
                    posees += 1;
                }
                Ok(Resultat { crees: vec![], message: format!("{posees} posée(s) sur {}", entites.len()) })
            }
            Commande::CreerVisgroup { nom, objets, cache } => {
                let id = tx.doc().visgroups().iter().map(|v| v.id).max().unwrap_or(0) + 1;
                let h = id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                let v = crate::visgroups::Visgroup { id, nom: nom.clone(), couleur: [(h >> 8) as u8 | 64, (h >> 16) as u8 | 64, (h >> 24) as u8 | 64], parent: 0 };
                crate::visgroups::creer(tx.bloc_haut_mut("visgroups").ok_or("la map n'a pas de bloc visgroups")?, &v);
                for r in membres(tx.doc(), objets)? {
                    let ed = editeur_de(tx, r).unwrap();
                    crate::visgroups::ajouter(ed, id);
                    if *cache {
                        ed.set("visgroupshown", "0");
                    }
                }
                if *cache {
                    mettre_a_jour_visibilite(tx);
                }
                Ok(Resultat { crees: vec![], message: format!("visgroup n° {id}") })
            }
            Commande::AjouterAuVisgroup { visgroup, objets } | Commande::RetirerDuVisgroup { visgroup, objets } => {
                if !tx.doc().visgroups().iter().any(|v| v.id == *visgroup) {
                    return Err(format!("visgroup {visgroup} introuvable"));
                }
                let ajout = matches!(self, Commande::AjouterAuVisgroup { .. });
                for r in membres(tx.doc(), objets)? {
                    let ed = editeur_de(tx, r).unwrap();
                    if ajout {
                        crate::visgroups::ajouter(ed, *visgroup);
                    } else {
                        crate::visgroups::retirer(ed, *visgroup);
                    }
                }
                mettre_a_jour_visibilite(tx);
                Ok(Resultat::default())
            }
            Commande::MontrerVisgroup { visgroup, visible } => {
                let vg = tx.doc().visgroups();
                if !vg.iter().any(|v| v.id == *visgroup) {
                    return Err(format!("visgroup {visgroup} introuvable"));
                }
                // le visgroup et ses enfants
                let mut ids = vec![*visgroup];
                let mut k = 0;
                while k < ids.len() {
                    let p = ids[k];
                    ids.extend(vg.iter().filter(|v| v.parent == p).map(|v| v.id));
                    k += 1;
                }
                for r in tous_les_objets(tx.doc()) {
                    let dans = editeur_lu(tx.doc(), r).map_or(false, |ed| crate::visgroups::de(ed).iter().any(|x| ids.contains(x)));
                    if dans {
                        editeur_de(tx, r).unwrap().set("visgroupshown", if *visible { "1" } else { "0" });
                    }
                }
                mettre_a_jour_visibilite(tx);
                Ok(Resultat::default())
            }
            Commande::RenommerVisgroup { visgroup, nom } => {
                if !crate::visgroups::renommer(tx.bloc_haut_mut("visgroups").ok_or("pas de bloc visgroups")?, *visgroup, nom) {
                    return Err(format!("visgroup {visgroup} introuvable"));
                }
                Ok(Resultat::default())
            }
            Commande::DeplacerVisgroup { visgroup, monter } => match crate::visgroups::deplacer(tx.bloc_haut_mut("visgroups").ok_or("pas de bloc visgroups")?, *visgroup, *monter) {
                None => Err(format!("visgroup {visgroup} introuvable")),
                Some(false) => Err(if *monter { "le visgroup est déjà le premier" } else { "le visgroup est déjà le dernier" }.into()),
                Some(true) => Ok(Resultat::default()),
            },
            Commande::SupprimerVisgroup { visgroup } => {
                let retires = crate::visgroups::supprimer(tx.bloc_haut_mut("visgroups").ok_or("pas de bloc visgroups")?, *visgroup);
                if retires.is_empty() {
                    return Err(format!("visgroup {visgroup} introuvable"));
                }
                for r in tous_les_objets(tx.doc()) {
                    let dans = editeur_lu(tx.doc(), r).map_or(false, |ed| crate::visgroups::de(ed).iter().any(|x| retires.contains(x)));
                    if dans {
                        let ed = editeur_de(tx, r).unwrap();
                        for id in &retires {
                            crate::visgroups::retirer(ed, *id);
                        }
                        ed.set("visgroupshown", "1");
                    }
                }
                mettre_a_jour_visibilite(tx);
                Ok(Resultat::default())
            }
            Commande::Cordon { min, max, actif } => {
                let mut c = tx.doc().cordons();
                if c.liste.is_empty() {
                    c.liste.push(crate::visgroups::Cordon { nom: "cordon".into(), actif: true, boites: Vec::new() });
                }
                let p = &mut c.liste[0];
                p.actif = true;
                match p.boites.first_mut() {
                    Some(b) => *b = [*min, *max],
                    None => p.boites.push([*min, *max]),
                }
                Commande::Cordons { actif: *actif, liste: c.liste }.executer(tx)
            }
            Commande::Cordons { actif, liste } => {
                let mut c = crate::visgroups::Cordons { actif: *actif, liste: liste.clone() };
                for k in &mut c.liste {
                    for b in &mut k.boites {
                        let (lo, hi) = (DVec3::from(b[0]).min(DVec3::from(b[1])), DVec3::from(b[0]).max(DVec3::from(b[1])));
                        if (hi - lo).min_element() < 1.0 {
                            return Err(format!("une boîte du cordon « {} » est plate", k.nom));
                        }
                        *b = [lo.to_array(), hi.to_array()];
                    }
                }
                crate::visgroups::ecrire_cordons(tx.bloc_haut_mut("cordons").ok_or("la map n'a pas de bloc cordons")?, &c);
                if let Some(v) = tx.bloc_haut_mut("cordon") {
                    crate::visgroups::ecrire_ancien_cordon(v, &c);
                }
                Ok(Resultat::default())
            }
            Commande::Evider { objets, epaisseur } => {
                let e = *epaisseur;
                if e <= 0.0 {
                    return Err("l'épaisseur des murs doit être positive".into());
                }
                let solides = solides_de(tx.doc(), objets)?;
                if solides.is_empty() {
                    return Err("aucun solide à évider".into());
                }
                let mut crees = Vec::new();
                for id in solides {
                    let s = tx.doc().solide(id).unwrap().clone();
                    let proprio = tx.doc().proprio(id).unwrap();
                    // l'intérieur : chaque plan rentré de l'épaisseur ; les murs : le solide moins l'intérieur (le
                    // « Make Hollow » de Hammer, par la même soustraction que le creusement)
                    let faces: Vec<Face> = s.faces.iter().filter(|f| crate::brosses::Plan::de_face(f).is_some()).cloned().collect();
                    let dedans: Vec<crate::brosses::Plan> = faces
                        .iter()
                        .map(|f| {
                            let p = crate::brosses::Plan::de_face(f).unwrap();
                            crate::brosses::Plan { n: p.n, d: p.d - e }
                        })
                        .collect();
                    let g_dedans: Vec<crate::brosses::Gabarit> = dedans.iter().zip(&faces).map(|(p, f)| crate::brosses::Gabarit { plan: *p, texture: f.clone() }).collect();
                    if crate::brosses::solide_de(0, &g_dedans, &mut || 0, &s.editeur, false).is_none() {
                        return Err(format!("le solide {id} est trop petit pour des murs de {e}"));
                    }
                    let morceaux = crate::brosses::soustraire(&crate::brosses::gabarits(&s), &dedans, &|i| {
                        let mut f = faces[i].clone();
                        f.id = 0;
                        f
                    });
                    tx.retirer_solide(id);
                    for m in morceaux {
                        let nid = tx.nouvel_id();
                        if let Some(sol) = crate::brosses::solide_de(nid, &m, &mut || tx.nouvelle_face(), &s.editeur, false) {
                            tx.ajouter_solide(proprio, sol);
                            crees.push(Ref::Solide(nid));
                        }
                    }
                }
                Ok(Resultat { crees, message: String::new() })
            }
            Commande::FondreInstance { entite, vmf } => fondre_instance(tx, *entite, vmf),
            Commande::CreerEnveloppe { points, materiau, classe } => {
                let pts: Vec<DVec3> = points.iter().map(|p| DVec3::from(*p)).collect();
                let plans = crate::brosses::enveloppe(&pts);
                let g: Vec<crate::brosses::Gabarit> = plans.iter().map(|p| crate::brosses::Gabarit::neuf(*p, materiau)).collect();
                let id = tx.nouvel_id();
                let ed = geo::editeur_neuf(id);
                let sol = (g.len() >= 4).then(|| crate::brosses::solide_de(id, &g, &mut || tx.nouvelle_face(), &ed, false)).flatten().ok_or("ces points n'enferment aucun volume (alignés ou dans un même plan)")?;
                tx.ajouter_solide(Proprio::Monde, sol);
                let crees = vec![Ref::Solide(id)];
                match classe {
                    Some(c) => Commande::LierAEntite { objets: crees, classe: c.clone() }.executer(tx),
                    None => Ok(Resultat { crees, message: String::new() }),
                }
            }
            Commande::CouleurEditeur { objets, couleur } => {
                let (solides, entites, _) = resoudre(tx.doc(), objets)?;
                let texte = format!("{} {} {}", couleur[0], couleur[1], couleur[2]);
                for r in solides.iter().map(|s| Ref::Solide(*s)).chain(entites.iter().map(|e| Ref::Entite(*e))) {
                    if let Some(b) = editeur_de(tx, r) {
                        b.set("color", &texte);
                    }
                }
                Ok(Resultat::default())
            }
            Commande::Lot { commandes, .. } => {
                let mut r = Resultat::default();
                for c in commandes {
                    let x = c.executer(tx)?;
                    r.crees.extend(x.crees);
                    if !x.message.is_empty() {
                        r.message = x.message;
                    }
                }
                Ok(r)
            }
        }
    }
}

fn editeur_de<'a>(tx: &'a mut Tx, r: Ref) -> Option<&'a mut Bloc> {
    match r {
        Ref::Solide(id) => Some(&mut tx.solide_mut(id)?.editeur),
        Ref::Entite(id) => {
            let e = tx.entite_mut(id)?;
            Some(e.editeur.get_or_insert_with(|| geo::editeur_neuf(id)))
        }
        Ref::Groupe(id) => Some(&mut tx.groupe_mut(id)?.editeur),
    }
}

/// Objets dont le bloc `editor` dit appartenir directement à ce groupe.
pub fn membres_directs(doc: &Document, g: u64) -> Vec<Ref> {
    let dans = |b: &Bloc| b.get("groupid").and_then(|v| v.trim().parse::<u64>().ok()) == Some(g);
    let mut out: Vec<Ref> = doc.solides.values().filter(|s| dans(&s.editeur)).map(|s| Ref::Solide(s.id)).collect();
    out.extend(doc.entites.values().filter(|e| e.editeur.as_ref().map_or(false, dans)).map(|e| Ref::Entite(e.id)));
    out.extend(doc.groupes.values().filter(|x| dans(&x.editeur)).map(|x| Ref::Groupe(x.id)));
    out
}

/// Déplie les désignations : solides, entités et groupes concernés (un groupe donne ses membres, récursivement).
pub fn resoudre(doc: &Document, objets: &[Ref]) -> Res<(Vec<u64>, Vec<u64>, Vec<u64>)> {
    let (mut s, mut e, mut g) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut pile: Vec<Ref> = objets.to_vec();
    while let Some(r) = pile.pop() {
        match r {
            Ref::Solide(id) => {
                if doc.solide(id).is_none() {
                    return Err(format!("solide {id} introuvable"));
                }
                s.insert(id);
            }
            Ref::Entite(id) => {
                if !doc.entites.contains_key(&id) {
                    return Err(format!("entité {id} introuvable"));
                }
                e.insert(id);
            }
            Ref::Groupe(id) => {
                if !doc.groupes.contains_key(&id) {
                    return Err(format!("groupe {id} introuvable"));
                }
                if g.insert(id) {
                    pile.extend(membres_directs(doc, id));
                }
            }
        }
    }
    // un solide d'une entité déjà désignée en entier n'est pas à traiter deux fois
    s.retain(|id| !matches!(doc.proprio(*id), Some(Proprio::Entite(x)) if e.contains(&x)));
    Ok((s.into_iter().collect(), e.into_iter().collect(), g.into_iter().collect()))
}

/// Boîte englobante des objets.
pub fn boite_de(doc: &Document, objets: &[Ref]) -> Option<(DVec3, DVec3)> {
    let (solides, entites, _) = resoudre(doc, objets).ok()?;
    let mut b: Option<(DVec3, DVec3)> = None;
    let mut ajouter = |lo: DVec3, hi: DVec3| b = Some(b.map_or((lo, hi), |(a, c)| (a.min(lo), c.max(hi))));
    for id in solides {
        if let Some((lo, hi)) = doc.solide(id).and_then(Solide::boite) {
            ajouter(lo, hi);
        }
    }
    for id in entites {
        let e = &doc.entites[&id];
        for s in &e.solides {
            if let Some((lo, hi)) = s.boite() {
                ajouter(lo, hi);
            }
        }
        if e.solides.is_empty() {
            if let Some(o) = e.origine() {
                ajouter(o - DVec3::splat(8.0), o + DVec3::splat(8.0));
            }
        }
    }
    b
}

fn transformer(tx: &mut Tx, objets: &[Ref], f: &Affine, verrou: bool) -> Res<Resultat> {
    let (solides, entites, _) = resoudre(tx.doc(), objets)?;
    if solides.is_empty() && entites.is_empty() {
        return Err("rien à transformer".into());
    }
    for id in solides {
        let s = tx.solide_mut(id).unwrap();
        let t = geo::transformer_solide(s, f, verrou);
        if !t.valide() {
            return Err(format!("le solide {id} deviendrait invalide : transformation refusée"));
        }
        *s = t;
    }
    for id in entites {
        let e = tx.entite_mut(id).unwrap();
        let t = geo::transformer_entite(e, f, verrou);
        if let Some(s) = t.solides.iter().find(|s| !s.valide()) {
            return Err(format!("le solide {} de l'entité {id} deviendrait invalide : transformation refusée", s.id));
        }
        *e = t;
    }
    Ok(Resultat::default())
}

/// Ajoute le contenu d'un morceau de VMF avec des numéros neufs, décalé de `d`.
fn coller(tx: &mut Tx, vmf: &str, d: DVec3) -> Res<Resultat> {
    let (frag, _) = Document::lire(vmf.as_bytes()).map_err(|e| format!("presse-papiers illisible : {e}"))?;
    coller_doc(tx, &frag, &Affine::translation(d))
}

#[allow(clippy::too_many_arguments)]
fn coller_special(tx: &mut Tx, vmf: &str, copies: u32, decalage: DVec3, rotation: DVec3, grouper: bool, prefixe: &str, noms_uniques: bool) -> Res<Resultat> {
    if copies == 0 || copies > 1000 {
        return Err("nombre de copies : de 1 à 1000".into());
    }
    let (frag, _) = Document::lire(vmf.as_bytes()).map_err(|e| format!("presse-papiers illisible : {e}"))?;
    let tout: Vec<Ref> = frag.solides.keys().map(|i| Ref::Solide(*i)).chain(frag.entites.keys().map(|i| Ref::Entite(*i))).collect();
    let (lo, hi) = boite_de(&frag, &tout).ok_or("le presse-papiers ne contient aucun objet")?;
    let centre = (lo + hi) * 0.5;
    let mut pris: std::collections::HashSet<String> = tx.doc().noms().into_iter().map(|n| n.to_lowercase()).collect();
    let mut crees = Vec::new();
    for k in 1..=copies {
        let k = k as f64;
        let r = glam::DMat3::from_rotation_z((rotation.z * k).to_radians()) * glam::DMat3::from_rotation_y((rotation.y * k).to_radians()) * glam::DMat3::from_rotation_x((rotation.x * k).to_radians());
        let f = Affine { l: r, t: centre - r * centre + decalage * k };
        let mut copie = frag.clone();
        renommer_copie(&mut copie, prefixe, noms_uniques, &mut pris);
        crees.extend(coller_doc(tx, &copie, &f)?.crees);
    }
    if grouper && crees.len() > 1 {
        crees = Commande::Grouper { objets: crees }.executer(tx)?.crees;
    }
    Ok(Resultat { crees, message: String::new() })
}

/// Les noms des entités de la copie : `prefixe` devant, puis un numéro libre si `uniques` ; les sorties de la copie
/// vers ces noms, `parentname` et `target` suivent. `pris` : les noms déjà employés (en minuscules), complété.
fn renommer_copie(copie: &mut Document, prefixe: &str, uniques: bool, pris: &mut std::collections::HashSet<String>) {
    if prefixe.is_empty() && !uniques {
        return;
    }
    let mut table: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for e in copie.entites.values() {
        let Some(n) = e.nom_donne() else { continue };
        if table.contains_key(&n.to_lowercase()) {
            continue;
        }
        let base = format!("{prefixe}{n}");
        let mut nouveau = base.clone();
        if uniques {
            let mut i = 1;
            while pris.contains(&format!("{base}_{i}").to_lowercase()) {
                i += 1;
            }
            nouveau = format!("{base}_{i}");
        }
        pris.insert(nouveau.to_lowercase());
        table.insert(n.to_lowercase(), nouveau);
    }
    appliquer_noms(copie, &table);
}

/// Change les noms d'entités selon `table` (ancien nom en minuscules → nouveau) : `targetname`, `parentname`,
/// `target` et les cibles des sorties.
fn appliquer_noms(copie: &mut Document, table: &std::collections::HashMap<String, String>) {
    let esc = copie.sorties_en_esc();
    for e in copie.entites.values_mut() {
        for cle in ["targetname", "parentname", "target"] {
            if let Some(n) = e.get(cle).and_then(|v| table.get(&v.to_lowercase())).cloned() {
                e.set(cle, &n);
            }
        }
        if let Some(sorties) = &mut e.sorties {
            for (k, v) in sorties.iter_mut() {
                let mut s = crate::io::lire(k, v);
                if let Some(n) = table.get(&s.cible.to_lowercase()) {
                    s.cible = n.clone();
                    *v = crate::io::ecrire(&s, esc || v.contains(crate::io::ESC));
                }
            }
        }
    }
}

fn fondre_instance(tx: &mut Tx, entite: u64, vmf: &str) -> Res<Resultat> {
    let e = tx.doc().entite(entite).ok_or_else(|| format!("entité {entite} introuvable"))?.clone();
    if !e.classe().eq_ignore_ascii_case("func_instance") {
        return Err(format!("l'entité {entite} n'est pas une func_instance"));
    }
    let (mut frag, _) = Document::lire(vmf.as_bytes()).map_err(|x| format!("fichier d'instance illisible : {x}"))?;
    // noms : préfixe ou suffixe du nom de l'instance, sauf ceux qui commencent par @
    let style = e.get("fixup_style").and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(0);
    let nom = e.nom().trim().to_string();
    if !nom.is_empty() && style < 2 {
        let table = frag
            .entites
            .values()
            .filter_map(|x| x.nom_donne())
            .filter(|n| !n.starts_with('@'))
            .map(|n| (n.to_lowercase(), if style == 0 { format!("{nom}-{n}") } else { format!("{n}-{nom}") }))
            .collect();
        appliquer_noms(&mut frag, &table);
    }
    // paramètres : « $variable valeur » dans replace01, replace02…
    let remplacements: Vec<(String, String)> = e
        .cles
        .iter()
        .filter(|(k, _)| k.to_ascii_lowercase().starts_with("replace"))
        .filter_map(|(_, v)| v.trim().split_once(' ').map(|(a, b)| (a.to_string(), b.trim().to_string())))
        .filter(|(a, _)| a.starts_with('$'))
        .collect();
    if !remplacements.is_empty() {
        for x in frag.entites.values_mut() {
            for (_, v) in x.cles.iter_mut().chain(x.sorties.iter_mut().flatten()) {
                for (a, b) in &remplacements {
                    if v.contains(a.as_str()) {
                        *v = v.replace(a.as_str(), b);
                    }
                }
            }
        }
    }
    let a = e.angles();
    let f = Affine { l: hm_geom::angles_to_mat(a.x, a.y, a.z), t: e.origine().unwrap_or(DVec3::ZERO) };
    let r = coller_doc(tx, &frag, &f)?;
    tx.retirer_entite(entite);
    Ok(r)
}

/// Colle les objets d'un morceau de VMF lu, transformés par `f`, sous de nouveaux numéros.
fn coller_doc(tx: &mut Tx, frag: &Document, f: &Affine) -> Res<Resultat> {
    let f = *f;
    let mut crees = Vec::new();
    // groupes : nouveaux numéros, appartenances suivies
    let mut table_groupes = std::collections::HashMap::new();
    for g in frag.groupes.values() {
        table_groupes.insert(g.id, tx.nouvel_id());
    }
    let regrouper = |b: &mut Bloc, table: &std::collections::HashMap<u64, u64>| {
        if let Some(g) = b.get("groupid").and_then(|v| v.trim().parse::<u64>().ok()) {
            match table.get(&g) {
                Some(n) => b.set("groupid", &n.to_string()),
                None => b.retirer("groupid"),
            }
        }
    };
    let neuf = |tx: &mut Tx, s: &Solide| -> Solide {
        let mut s = geo::transformer_solide(s, &f, true);
        s.id = tx.nouvel_id();
        for face in &mut s.faces {
            face.id = tx.nouvelle_face();
            face.gabarit.clear();
        }
        s.gabarit.clear();
        s
    };
    let racines: BTreeSet<u64> = frag.groupes.values().filter(|g| g.editeur.get("groupid").map_or(true, |p| !table_groupes.contains_key(&p.trim().parse::<u64>().unwrap_or(0)))).map(|g| g.id).collect();
    for g in frag.groupes.values() {
        let id = table_groupes[&g.id];
        let mut ng = Groupe { id, editeur: g.editeur.clone(), gabarit: Vec::new() };
        regrouper(&mut ng.editeur, &table_groupes);
        tx.ajouter_groupe(ng);
        if racines.contains(&g.id) {
            crees.push(Ref::Groupe(id));
        }
    }
    for s in frag.solides.values() {
        let mut n = neuf(tx, s);
        let groupe = n.groupe();
        regrouper(&mut n.editeur, &table_groupes);
        if groupe == 0 || !table_groupes.contains_key(&groupe) {
            crees.push(Ref::Solide(n.id));
        }
        tx.ajouter_solide(Proprio::Monde, n);
    }
    for e in frag.entites.values() {
        let mut nu = e.clone();
        nu.solides.clear();
        let mut n = geo::transformer_entite(&nu, &f, true);
        n.id = tx.nouvel_id();
        n.gabarit.clear();
        for s in &e.solides {
            n.solides.push(neuf(tx, s));
        }
        let groupe = n.groupe();
        if let Some(ed) = &mut n.editeur {
            regrouper(ed, &table_groupes);
        }
        if groupe == 0 || !table_groupes.contains_key(&groupe) {
            crees.push(Ref::Entite(n.id));
        }
        tx.ajouter_entite(n);
    }
    if crees.is_empty() {
        return Err("le presse-papiers ne contient aucun objet".into());
    }
    Ok(Resultat { crees, message: String::new() })
}

fn face_mut<'a>(tx: &'a mut Tx, rf: RefFace) -> Res<&'a mut crate::modele::Face> {
    let s = tx.solide_mut(rf.solide).ok_or_else(|| format!("solide {} introuvable", rf.solide))?;
    s.faces.iter_mut().find(|f| f.id == rf.face).ok_or_else(|| format!("face {} introuvable sur le solide {}", rf.face, rf.solide))
}

/// Le polygone d'une face.
pub fn polygone(doc: &Document, rf: RefFace) -> Res<Vec<DVec3>> {
    let s = doc.solide(rf.solide).ok_or_else(|| format!("solide {} introuvable", rf.solide))?;
    let i = s.faces.iter().position(|f| f.id == rf.face).ok_or_else(|| format!("face {} introuvable sur le solide {}", rf.face, rf.solide))?;
    Ok(s.polygones().swap_remove(i))
}

/// Les objets désignés, groupes dépliés (les membres, pas le groupe).
fn membres(doc: &Document, objets: &[Ref]) -> Res<Vec<Ref>> {
    let (s, e, _) = resoudre(doc, objets)?;
    Ok(s.into_iter().map(Ref::Solide).chain(e.into_iter().map(Ref::Entite)).collect())
}

/// Tous les objets de la map : solides du monde, entités, solides des entités.
fn tous_les_objets(doc: &Document) -> Vec<Ref> {
    doc.tous_les_solides().map(|(_, s)| Ref::Solide(s.id)).chain(doc.entites().map(|e| Ref::Entite(e.id))).collect()
}

fn editeur_lu(doc: &Document, r: Ref) -> Option<&Bloc> {
    match r {
        Ref::Solide(id) => doc.solide(id).map(|s| &s.editeur),
        Ref::Entite(id) => doc.entite(id).and_then(|e| e.editeur.as_ref()),
        Ref::Groupe(id) => doc.groupe(id).map(|g| &g.editeur),
    }
}

/// Un objet est caché (rangé dans un bloc `hidden`) quand il est dans un visgroup décoché.
fn mettre_a_jour_visibilite(tx: &mut Tx) {
    for r in tous_les_objets(tx.doc()) {
        let Some(ed) = editeur_lu(tx.doc(), r) else { continue };
        let cache = !crate::visgroups::de(ed).is_empty() && ed.get("visgroupshown").map_or(false, |v| v.trim() == "0");
        let actuel = match r {
            Ref::Solide(id) => tx.doc().solide(id).map(|s| s.cache),
            Ref::Entite(id) => tx.doc().entite(id).map(|e| e.cache),
            _ => None,
        };
        if actuel != Some(cache) {
            match r {
                Ref::Solide(id) => tx.solide_mut(id).unwrap().cache = cache,
                Ref::Entite(id) => tx.entite_mut(id).unwrap().cache = cache,
                _ => {}
            }
        }
    }
}

/// Ce qu'on garde d'une découpe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Garder {
    /// Le côté opposé à la normale du plan.
    Derriere,
    /// Le côté de la normale.
    Devant,
    LesDeux,
}

/// Les primitives de l'outil bloc.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Forme {
    Cylindre { cotes: usize },
    Pointe { cotes: usize },
    Sphere { cotes: usize },
    Coin,
    Arche(crate::brosses::Arche),
    Escalier {
        marches: usize,
        #[serde(default)]
        sens: u8,
    },
}

impl Forme {
    /// Les plans de chaque pièce de la primitive dans la boîte (une pièce sauf l'arche et l'escalier).
    pub fn pieces(&self, lo: DVec3, hi: DVec3) -> Res<Vec<Vec<crate::brosses::Plan>>> {
        use crate::brosses::{self as b, Plan};
        Ok(match self {
            Forme::Cylindre { cotes } => vec![b::cylindre(lo, hi, (*cotes).clamp(3, 64))],
            Forme::Pointe { cotes } => vec![b::pointe(lo, hi, (*cotes).clamp(3, 64))],
            Forme::Sphere { cotes } => vec![b::sphere(lo, hi, (*cotes).clamp(4, 32))],
            Forme::Coin => vec![b::coin(lo, hi)],
            Forme::Arche(a) => {
                if !(3..=100).contains(&a.cotes) || !(8.0..=360.0).contains(&a.arc) || a.largeur_mur <= 0.0 {
                    return Err("arche : de 3 à 100 côtés, un arc de 8 à 360°, une largeur de mur positive".into());
                }
                b::arche(lo, hi, a)
            }
            Forme::Escalier { marches, sens } => b::escalier(lo, hi, (*marches).clamp(1, 256), *sens)
                .into_iter()
                .map(|(a, c)| {
                    vec![
                        Plan { n: DVec3::X, d: c.x },
                        Plan { n: -DVec3::X, d: -a.x },
                        Plan { n: DVec3::Y, d: c.y },
                        Plan { n: -DVec3::Y, d: -a.y },
                        Plan { n: DVec3::Z, d: c.z },
                        Plan { n: -DVec3::Z, d: -a.z },
                    ]
                })
                .collect(),
        })
    }

    pub fn nom(&self) -> &'static str {
        match self {
            Forme::Cylindre { .. } => "un cylindre",
            Forme::Pointe { .. } => "une pointe",
            Forme::Sphere { .. } => "une sphère",
            Forme::Coin => "un coin",
            Forme::Arche(_) => "une arche",
            Forme::Escalier { .. } => "un escalier",
        }
    }
}

/// Les solides désignés : ceux du monde, et ceux des entités et des groupes désignés.
fn solides_de(doc: &Document, objets: &[Ref]) -> Res<Vec<u64>> {
    let (mut s, e, _) = resoudre(doc, objets)?;
    for id in e {
        if let Some(ent) = doc.entite(id) {
            for x in &ent.solides {
                if !s.contains(&x.id) {
                    s.push(x.id);
                }
            }
        }
    }
    Ok(s)
}

/// Ce que la sélection tient après une opération : un solide d'entité est sélectionné par son entité.
fn selection_de(doc: &Document, refs: Vec<Ref>) -> Vec<Ref> {
    let mut out: Vec<Ref> = Vec::new();
    for r in refs {
        let r = match r {
            Ref::Solide(id) => match doc.proprio(id) {
                Some(Proprio::Entite(e)) => Ref::Entite(e),
                _ => r,
            },
            _ => r,
        };
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}
