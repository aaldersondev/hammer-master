//! Ce que les vues montrent : masquage rapide (H, Ctrl+H, U), visgroups automatiques (catégories d'objets), cordon
//! actif, en plus des visgroups de la map décochés (objets `cache` du document).
//!
//! Le masquage rapide et les visgroups automatiques sont des réglages d'affichage de la session : ils ne touchent
//! ni le document ni la compilation (écart voulu avec Hammer, où un visgroup automatique oublié décoché change la
//! compilation : wiki Valve, « L4D Level Design/Level Organization »). Les visgroups de la map, eux, se compilent
//! comme dans Hammer : décochés, leurs objets ne sont pas compilés.
use std::collections::HashSet;

use glam::DVec3;
use hm_doc::{Document, Proprio, Ref};

use crate::scene::Designe;

/// Les visgroups automatiques : chaque objet a une catégorie, et ses parents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Auto {
    Monde,
    Geometrie,
    Displacements,
    Details,
    EntitesBrosses,
    Declencheurs,
    EntitesPonctuelles,
    Lumieres,
    Props,
    Noeuds,
    Outils,
    Clips,
    Ciel,
    Nodraw,
    Hints,
    Areaportals,
    Occluders,
}

/// L'arbre affiché dans le panneau : (catégorie, nom, enfants).
pub const ARBRE: &[(Auto, &str, &[Auto])] = &[
    (Auto::Monde, "Monde", &[Auto::Geometrie, Auto::Displacements]),
    (Auto::Details, "Détails (func_detail)", &[]),
    (Auto::EntitesBrosses, "Entités à brosses", &[Auto::Declencheurs]),
    (Auto::EntitesPonctuelles, "Entités ponctuelles", &[Auto::Lumieres, Auto::Props, Auto::Noeuds]),
    (Auto::Outils, "Brosses d'outils", &[Auto::Clips, Auto::Ciel, Auto::Nodraw, Auto::Hints, Auto::Areaportals, Auto::Occluders]),
];

pub fn nom(a: Auto) -> &'static str {
    match a {
        Auto::Monde => crate::langue::t("Monde"),
        Auto::Geometrie => crate::langue::t("Géométrie"),
        Auto::Displacements => crate::langue::t("Displacements"),
        Auto::Details => crate::langue::t("Détails (func_detail)"),
        Auto::EntitesBrosses => crate::langue::t("Entités à brosses"),
        Auto::Declencheurs => crate::langue::t("Déclencheurs (trigger_*)"),
        Auto::EntitesPonctuelles => crate::langue::t("Entités ponctuelles"),
        Auto::Lumieres => crate::langue::t("Lumières"),
        Auto::Props => "Props",
        Auto::Noeuds => crate::langue::t("Nœuds (info_node*)"),
        Auto::Outils => crate::langue::t("Brosses d'outils"),
        Auto::Clips => "Clips",
        Auto::Ciel => crate::langue::t("Ciel (toolsskybox)"),
        Auto::Nodraw => "Nodraw",
        Auto::Hints => crate::langue::t("Hint et skip"),
        Auto::Areaportals => "Areaportals",
        Auto::Occluders => "Occluders",
    }
}

/// La catégorie d'une brosse faite de textures d'outils, d'après ses faces.
fn categorie_outil(mats: &[String]) -> Option<Auto> {
    let m: Vec<String> = mats.iter().map(|m| m.to_ascii_lowercase().replace('\\', "/")).collect();
    if m.is_empty() || !m.iter().all(|x| x.starts_with("tools/")) {
        return None;
    }
    let toutes = |f: &dyn Fn(&str) -> bool| m.iter().all(|x| f(x));
    let une = |f: &dyn Fn(&str) -> bool| m.iter().any(|x| f(x));
    Some(if une(&|x| x.contains("clip")) {
        Auto::Clips
    } else if une(&|x| x.starts_with("tools/toolsskybox")) {
        Auto::Ciel
    } else if une(&|x| x == "tools/toolshint" || x == "tools/toolsskip") {
        Auto::Hints
    } else if toutes(&|x| x == "tools/toolsnodraw") {
        Auto::Nodraw
    } else {
        Auto::Outils
    })
}

/// Catégorie d'un objet et ses parents (la plus précise d'abord).
pub fn categories(doc: &Document, d: &Designe) -> Vec<Auto> {
    let parent = |a: Auto| -> Option<Auto> { ARBRE.iter().find(|(_, _, e)| e.contains(&a)).map(|(p, _, _)| *p) };
    let feuille = match (d.objet, d.solide) {
        (Ref::Solide(_), Some(id)) | (Ref::Groupe(_), Some(id)) => match doc.solide(id) {
            Some(s) if s.faces.iter().any(|f| f.disp.is_some()) => Auto::Displacements,
            Some(s) => categorie_outil(&s.faces.iter().map(|f| f.materiau.clone()).collect::<Vec<_>>()).unwrap_or(Auto::Geometrie),
            None => Auto::Geometrie,
        },
        (Ref::Entite(e), solide) => {
            let c = doc.entite(e).map(|e| e.classe().to_ascii_lowercase()).unwrap_or_default();
            match solide {
                Some(_) => match c.as_str() {
                    "func_detail" => Auto::Details,
                    "func_areaportal" | "func_areaportalwindow" => Auto::Areaportals,
                    "func_occluder" => Auto::Occluders,
                    c if c.starts_with("trigger_") => Auto::Declencheurs,
                    _ => Auto::EntitesBrosses,
                },
                None => match c.as_str() {
                    c if c.starts_with("light") || c == "env_sun" || c == "env_projectedtexture" => Auto::Lumieres,
                    c if c.starts_with("prop_") => Auto::Props,
                    c if c.starts_with("info_node") => Auto::Noeuds,
                    _ => Auto::EntitesPonctuelles,
                },
            }
        }
        _ => Auto::Geometrie,
    };
    let mut out = vec![feuille];
    while let Some(p) = parent(*out.last().unwrap()) {
        out.push(p);
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct Filtres {
    /// Masquage rapide : objets désignés comme les sélectionne un clic (solide du monde, entité, groupe).
    pub masques: HashSet<Ref>,
    /// Visgroups automatiques décochés.
    pub auto_caches: HashSet<Auto>,
    /// Montre tout, quel que soit l'état des visgroups (bouton « Show » de Hammer) : les objets restent rangés
    /// comme avant, seule la vue change.
    pub tout_montrer: bool,
    /// Change à chaque modification, pour refaire la géométrie.
    pub revision: u64,
}

impl Filtres {
    pub fn changer(&mut self) {
        self.revision += 1;
    }

    /// L'objet est montré dans les vues ; `lo`–`hi` : sa boîte (son origine pour une entité ponctuelle) ; `cordon` :
    /// les boîtes actives des cordons (`Cordons::boites_actives`), lues une fois pour toute la scène.
    pub fn visible(&self, doc: &Document, d: &Designe, lo: DVec3, hi: DVec3, cordon: &[(DVec3, DVec3)]) -> bool {
        if self.masques.contains(&d.objet) || d.solide.map_or(false, |s| self.masques.contains(&Ref::Solide(s))) {
            return false;
        }
        if !self.tout_montrer {
            let cache = match (d.objet, d.solide) {
                (Ref::Entite(e), _) if doc.entite(e).map_or(false, |e| e.cache) => true,
                (_, Some(s)) => doc.solide(s).map_or(false, |s| s.cache),
                _ => false,
            };
            if cache {
                return false;
            }
        }
        if !self.auto_caches.is_empty() && categories(doc, d).iter().any(|a| self.auto_caches.contains(a)) {
            return false;
        }
        // cordon actif : seul ce qui sera compilé est montré (wiki Valve, « Hammer Cordon Usage »)
        cordon.is_empty() || hm_doc::visgroups::touche(cordon, lo, hi)
    }

    /// Masque des objets (H) : la sélection dépliée en solides du monde et entités.
    pub fn masquer(&mut self, doc: &Document, objets: &[Ref]) -> usize {
        let Ok((s, e, _)) = hm_doc::resoudre(doc, objets) else { return 0 };
        let avant = self.masques.len();
        for id in s {
            match doc.proprio(id) {
                Some(Proprio::Entite(ent)) => self.masques.insert(Ref::Entite(ent)),
                _ => self.masques.insert(Ref::Solide(id)),
            };
        }
        for id in e {
            self.masques.insert(Ref::Entite(id));
        }
        self.changer();
        self.masques.len() - avant
    }

    /// Masque tout sauf ces objets (Ctrl+H).
    pub fn masquer_le_reste(&mut self, doc: &Document, objets: &[Ref]) -> usize {
        let (s, e, _) = hm_doc::resoudre(doc, objets).unwrap_or_default();
        let (s, e): (HashSet<u64>, HashSet<u64>) = (s.into_iter().collect(), e.into_iter().collect());
        let avant = self.masques.len();
        for (p, sol) in doc.tous_les_solides() {
            if p == Proprio::Monde && !s.contains(&sol.id) {
                self.masques.insert(Ref::Solide(sol.id));
            }
        }
        for ent in doc.entites() {
            if !e.contains(&ent.id) && !ent.solides.iter().any(|x| s.contains(&x.id)) {
                self.masques.insert(Ref::Entite(ent.id));
            }
        }
        self.changer();
        self.masques.len() - avant
    }

    /// Les objets masqués qui existent encore (pour en faire un visgroup).
    pub fn masques_existants(&self, doc: &Document) -> Vec<Ref> {
        let mut v: Vec<Ref> = self
            .masques
            .iter()
            .copied()
            .filter(|r| match r {
                Ref::Solide(i) => doc.solide(*i).is_some(),
                Ref::Entite(i) => doc.entite(*i).is_some(),
                Ref::Groupe(_) => false,
            })
            .collect();
        v.sort_by_key(|r| match r {
            Ref::Solide(i) | Ref::Entite(i) | Ref::Groupe(i) => *i,
        });
        v
    }
}
