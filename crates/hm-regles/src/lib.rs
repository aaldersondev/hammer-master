//! Les règles de vérification d'une map (phase 3).
//!
//! Chaque règle a un identifiant stable, un niveau, une explication et une source. Un **constat** désigne l'objet
//! fautif, sa position et une boîte à regarder : l'outil en ligne de commande et le serveur MCP en tirent une vue.
//! Une exception se déclare dans le projet (`hammer-master.toml`), avec une raison écrite ; un constat excepté
//! reste dans le rapport, marqué comme tel.
use std::path::{Path, PathBuf};
use std::time::Instant;

use glam::Vec3;
use hm_assets::Vfs;
use hm_scene::Scene;
use serde::{Deserialize, Serialize};

pub mod assets;
pub mod budgets;
pub mod geometrie;
pub mod joueur;
pub mod monde;
pub mod physique;
pub mod scintillement;
pub mod reseau;
pub mod semantique;

pub use monde::Monde;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Niveau {
    /// La map ne compile pas, plante, ou montre un défaut que tout joueur voit.
    Erreur,
    /// Défaut probable, à regarder.
    Avertissement,
    /// Écart à une habitude ; souvent voulu.
    Information,
}

impl Niveau {
    pub fn nom(self) -> &'static str {
        match self {
            Niveau::Erreur => "erreur",
            Niveau::Avertissement => "avertissement",
            Niveau::Information => "information",
        }
    }
}

/// Une règle du catalogue.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Regle {
    pub id: &'static str,
    pub niveau: Niveau,
    pub titre: &'static str,
    /// Ce que la règle contrôle, et pourquoi c'est un défaut.
    pub explication: &'static str,
    /// D'où vient la règle ou sa valeur (code du SDK, wiki, mesure).
    pub source: &'static str,
}

/// Le catalogue complet, dans l'ordre du rapport.
pub fn catalogue() -> Vec<Regle> {
    let mut v = Vec::new();
    v.extend_from_slice(geometrie::REGLES);
    v.extend_from_slice(assets::REGLES);
    v.extend_from_slice(physique::REGLES);
    v.extend_from_slice(scintillement::REGLES);
    v.extend_from_slice(reseau::REGLES);
    v.extend_from_slice(budgets::REGLES);
    v.extend_from_slice(joueur::REGLES);
    v.extend_from_slice(semantique::REGLES);
    v
}

pub fn regle(id: &str) -> Option<Regle> {
    catalogue().into_iter().find(|r| r.id == id)
}

#[derive(Clone, Debug, Serialize)]
pub struct Constat {
    pub regle: &'static str,
    pub niveau: Niveau,
    pub message: String,
    /// Rang de l'objet fautif dans la scène (0 : aucun objet précis).
    pub objet: u32,
    pub vmf_id: u64,
    pub classe: String,
    pub nom: String,
    /// Modèle d'un prop, ou matériau dominant d'un solide.
    pub modele: String,
    pub materiau: String,
    /// Où regarder.
    pub pos: [f32; 3],
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Grandeur qui mesure le défaut (profondeur, distance, nombre…), pour trier.
    pub mesure: f32,
    /// Autres objets en cause (rangs dans la scène).
    pub autres: Vec<u32>,
    /// Raison de l'exception déclarée dans le projet, si le constat est excepté.
    pub excepte: Option<String>,
    /// Normale du plan en cause, quand le défaut tient à un plan (scintillement) : de quel côté décaler.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normale: Option<[f32; 3]>,
    /// `id` dans le VMF des autres objets en cause, dans l'ordre de `autres`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub autres_vmf: Vec<u64>,
}

impl Constat {
    /// Constat sur un objet de la scène.
    pub fn objet(scene: &Scene, regle: &'static str, objet: u32, message: String) -> Constat {
        let o = &scene.objects[objet as usize];
        let c = (o.min + o.max) * 0.5;
        Constat {
            regle,
            niveau: crate::regle(regle).map_or(Niveau::Avertissement, |r| r.niveau),
            message,
            objet,
            vmf_id: o.vmf_id,
            classe: o.class.clone(),
            nom: o.name.clone(),
            modele: o.model.clone(),
            materiau: o.material.clone(),
            pos: c.into(),
            min: o.min.into(),
            max: o.max.into(),
            mesure: 0.0,
            autres: Vec::new(),
            excepte: None,
            normale: None,
            autres_vmf: Vec::new(),
        }
    }

    /// Constat sur un endroit, sans objet précis.
    pub fn lieu(regle: &'static str, min: Vec3, max: Vec3, message: String) -> Constat {
        Constat {
            regle,
            niveau: crate::regle(regle).map_or(Niveau::Avertissement, |r| r.niveau),
            message,
            objet: 0,
            vmf_id: 0,
            classe: String::new(),
            nom: String::new(),
            modele: String::new(),
            materiau: String::new(),
            pos: ((min + max) * 0.5).into(),
            min: min.into(),
            max: max.into(),
            mesure: 0.0,
            autres: Vec::new(),
            excepte: None,
            normale: None,
            autres_vmf: Vec::new(),
        }
    }

    pub fn mesure(mut self, m: f32) -> Self {
        self.mesure = m;
        self
    }
    pub fn boite(mut self, min: Vec3, max: Vec3) -> Self {
        self.min = min.into();
        self.max = max.into();
        self.pos = ((min + max) * 0.5).into();
        self
    }
    pub fn autres(mut self, a: Vec<u32>) -> Self {
        self.autres = a;
        self
    }
    /// Les autres objets en cause, avec leur `id` du VMF (pour un script qui corrige d'après le rapport).
    pub fn autres_de(mut self, scene: &Scene, a: Vec<u32>) -> Self {
        self.autres_vmf = a.iter().map(|o| scene.objects[*o as usize].vmf_id).collect();
        self.autres = a;
        self
    }
    pub fn plan(mut self, n: Vec3) -> Self {
        self.normale = Some(n.into());
        self
    }
    pub fn niveau(mut self, n: Niveau) -> Self {
        self.niveau = n;
        self
    }
}

/// Une exception déclarée dans le projet. Tous les critères donnés doivent correspondre ; `raison` est obligatoire.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    /// Identifiant de règle, ou préfixe terminé par `*` (`phys.*`).
    pub regle: String,
    pub raison: String,
    /// `id` du solide ou de l'entité dans le VMF.
    #[serde(default)]
    pub id: Option<u64>,
    /// Motifs avec `*` : nom (`targetname`), modèle, classe, matériau.
    #[serde(default)]
    pub nom: Option<String>,
    #[serde(default)]
    pub modele: Option<String>,
    #[serde(default)]
    pub classe: Option<String>,
    #[serde(default)]
    pub materiau: Option<String>,
    /// Boîte `[xmin, ymin, zmin, xmax, ymax, zmax]` où le constat doit se trouver.
    #[serde(default)]
    pub zone: Option<[f32; 6]>,
}

/// Motif à jokers `*`, insensible à la casse.
pub fn motif(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.to_ascii_lowercase(), text.to_ascii_lowercase());
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == t;
    }
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !t.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            return t.len() >= pos + part.len() && t.ends_with(part);
        } else {
            match t[pos..].find(part) {
                Some(k) => pos += k + part.len(),
                None => return false,
            }
        }
    }
    true
}

impl Exception {
    pub fn couvre(&self, c: &Constat) -> bool {
        let regle_ok = match self.regle.strip_suffix('*') {
            Some(prefixe) => c.regle.starts_with(prefixe),
            None => self.regle == c.regle,
        };
        regle_ok
            && self.id.map_or(true, |i| i == c.vmf_id)
            && self.nom.as_deref().map_or(true, |m| motif(m, &c.nom))
            && self.modele.as_deref().map_or(true, |m| motif(m, &c.modele))
            && self.classe.as_deref().map_or(true, |m| motif(m, &c.classe))
            && self.materiau.as_deref().map_or(true, |m| motif(m, &c.materiau))
            && self.zone.map_or(true, |z| (0..3).all(|a| c.pos[a] >= z[a] && c.pos[a] <= z[a + 3]))
    }
}

/// Réglages d'un projet : `hammer-master.toml`, cherché près du VMF puis dans les dossiers au-dessus.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub exception: Vec<Exception>,
    /// Motifs de modèles et de noms d'entités suspendus exprès (drones, enseignes holographiques, nuages) : ils
    /// n'ont pas à toucher le sol.
    pub suspendus: Vec<String>,
    /// Motifs de noms de modèles qui forment un réseau linéaire (voies, viaducs, routes).
    pub reseau: Vec<String>,
    /// Motifs de modèles qui terminent un réseau (butoir, heurtoir).
    pub terminus: Vec<String>,
    /// Motifs de modèles de cuvettes de WC.
    pub cuvettes: Vec<String>,
    /// Pas de grille attendu pour les faces droites des solides du monde et du détail.
    pub grille_monde: f64,
    pub grille_detail: f64,
    /// Jeu admis entre deux objets qui se touchent.
    pub jeu_contact: f32,
    /// Montages dont le contenu n'arrive pas chez les joueurs (motifs sur le chemin du point de montage).
    pub montages_locaux: Vec<String>,
    /// Montages dont le contenu est embarqué dans la map à la compilation (motifs sur le chemin).
    pub montages_embarques: Vec<String>,
    /// Liaisons de déplacement déclarées (ascenseurs et téléporteurs faits en Lua) : `de` et `vers` en coordonnées.
    pub liaison: Vec<Liaison>,
    /// Ce que sont les modèles du projet dont le nom ne le dit pas : morceau du nom de fichier → mots de
    /// `regles/roles.yaml` (`poste_cellule = ["lit", "toilet", "lavabo"]`).
    pub vocabulaire: std::collections::BTreeMap<String, Vec<String>>,
    /// Zones que Hammer Master construit lui-même à partir d'une spec et fusionne à la map du générateur.
    pub zone_native: Vec<ZoneNative>,
}

/// Une zone native : une spec de module, posée et tournée dans la map (brique `hm-edit`, module `zones`).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneNative {
    pub nom: String,
    /// Chemin de la spec, depuis le dossier de `hammer-master.toml`.
    pub spec: String,
    #[serde(default)]
    pub origine: [f64; 3],
    /// Multiple de 90.
    #[serde(default)]
    pub angle: i32,
    /// Remplacements de texte faits dans la spec avant de la lire : ce qui change d'une pose à l'autre.
    #[serde(default)]
    pub remplace: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Liaison {
    pub de: [f32; 3],
    pub vers: [f32; 3],
    #[serde(default)]
    pub nom: String,
    /// Vrai si la liaison se prend dans les deux sens.
    #[serde(default)]
    pub aller_retour: bool,
}

impl Default for Config {
    fn default() -> Self {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Config {
            exception: Vec::new(),
            suspendus: Vec::new(),
            reseau: v(&["voie", "rail", "track", "viaduc", "pont_", "bridge", "road", "route_"]),
            terminus: v(&["butoir", "heurtoir", "bumper", "buffer", "terminus"]),
            cuvettes: v(&["toilet", "wc_cuvette", "cuvette"]),
            grille_monde: 1.0,
            grille_detail: 1.0,
            jeu_contact: 2.0,
            montages_locaux: Vec::new(),
            montages_embarques: Vec::new(),
            liaison: Vec::new(),
            vocabulaire: Default::default(),
            zone_native: Vec::new(),
        }
    }
}

impl Config {
    /// Cherche `hammer-master.toml` dans le dossier du VMF et jusqu'à trois dossiers au-dessus.
    pub fn trouver(vmf: &Path) -> Option<PathBuf> {
        let mut d = vmf.parent();
        for _ in 0..4 {
            let dir = d?;
            let p = dir.join("hammer-master.toml");
            if p.is_file() {
                return Some(p);
            }
            d = dir.parent();
        }
        None
    }

    pub fn lire(chemin: &Path) -> anyhow::Result<Config> {
        let texte = std::fs::read_to_string(chemin)?;
        let c: Config = toml::from_str(&texte).map_err(|e| anyhow::anyhow!("{} : {e}", chemin.display()))?;
        for e in &c.exception {
            anyhow::ensure!(e.raison.trim().len() >= 8, hm_langue::tr!("{} : l'exception sur « {} » n'a pas de raison écrite", "{}: the exception on \"{}\" has no written reason", chemin.display(), e.regle));
            anyhow::ensure!(e.regle.ends_with('*') || regle(&e.regle).is_some(), hm_langue::tr!("{} : règle inconnue « {} »", "{}: unknown rule \"{}\"", chemin.display(), e.regle));
        }
        Ok(c)
    }

    /// Les réglages du projet d'un VMF, ou ceux par défaut. Rend aussi le fichier lu.
    pub fn du_projet(vmf: &Path) -> anyhow::Result<(Config, Option<PathBuf>)> {
        match Config::trouver(vmf) {
            Some(p) => Ok((Config::lire(&p)?, Some(p))),
            None => Ok((Config::default(), None)),
        }
    }
}

/// Familles de règles, pour n'en lancer qu'une partie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Famille {
    Geometrie,
    Assets,
    Physique,
    Budgets,
    Joueur,
    /// Règles sur le modèle sémantique : elles ne tournent que si la map en a un.
    Semantique,
}

impl Famille {
    pub const TOUTES: [Famille; 6] = [Famille::Geometrie, Famille::Assets, Famille::Physique, Famille::Budgets, Famille::Joueur, Famille::Semantique];
    pub fn de(nom: &str) -> Option<Famille> {
        Some(match nom {
            "geometrie" | "geo" => Famille::Geometrie,
            "assets" | "asset" => Famille::Assets,
            "physique" | "phys" => Famille::Physique,
            "budgets" | "budget" => Famille::Budgets,
            "joueur" => Famille::Joueur,
            "semantique" | "sem" => Famille::Semantique,
            _ => return None,
        })
    }
}

#[derive(Default, Serialize)]
pub struct Rapport {
    pub revision: String,
    pub constats: Vec<Constat>,
    pub durees: Vec<(String, f32)>,
    pub budgets: Option<budgets::Budgets>,
    pub joueur: Option<joueur::Bilan>,
    /// D'où viennent les fichiers : (montage, provenance, matériaux, modèles, poses).
    pub provenances: Vec<(String, assets::Provenance, usize, usize, usize)>,
    /// Résumé du modèle sémantique lu à côté du VMF (`<carte>.sem.json`), s'il existe.
    pub modele: Option<String>,
    pub config: Option<String>,
}

impl Rapport {
    /// Constats non exceptés d'un niveau.
    pub fn compte(&self, n: Niveau) -> usize {
        self.constats.iter().filter(|c| c.niveau == n && c.excepte.is_none()).count()
    }
    pub fn exceptes(&self) -> usize {
        self.constats.iter().filter(|c| c.excepte.is_some()).count()
    }
    /// Constats non exceptés d'une règle.
    pub fn de(&self, regle: &str) -> Vec<&Constat> {
        self.constats.iter().filter(|c| c.regle == regle && c.excepte.is_none()).collect()
    }
}

/// Lance les règles des familles demandées.
pub fn valider(scene: &Scene, vfs: &Vfs, config: &Config, familles: &[Famille]) -> Rapport {
    let mut r = Rapport { revision: scene.revision.clone(), ..Default::default() };
    let mut t = Instant::now();
    let mut lap = |r: &mut Rapport, quoi: &str| {
        r.durees.push((quoi.to_string(), t.elapsed().as_secs_f32()));
        t = Instant::now();
    };
    let veut = |f: Famille| familles.contains(&f);
    // le modèle sémantique de la map, s'il existe ; un fichier faux est une erreur du rapport, pas une absence
    let modele = match hm_model::Modele::du_vmf(std::path::Path::new(&scene.path)) {
        Ok(m) => m,
        Err(e) => {
            r.constats.push(Constat::lieu("sem.modele_invalide", Vec3::ZERO, Vec3::ZERO, hm_langue::tr!("modèle sémantique illisible : {e:#}", "unreadable semantic model: {e:#}")));
            None
        }
    };
    r.modele = modele.as_ref().map(|m| m.resume());
    let monde = Monde::construire(scene, config);
    lap(&mut r, "index des objets");
    if veut(Famille::Geometrie) {
        geometrie::verifier(scene, &monde, config, &mut r.constats);
        lap(&mut r, "géométrie");
    }
    if veut(Famille::Assets) {
        assets::verifier(scene, vfs, config, &mut r.constats);
        r.provenances = assets::provenances(scene, vfs, config);
        lap(&mut r, "assets");
    }
    if veut(Famille::Physique) {
        physique::verifier(scene, &monde, config, &mut r.constats);
        lap(&mut r, "cohérence physique");
        scintillement::verifier(scene, &monde, &mut r.constats);
        lap(&mut r, "scintillement");
        reseau::verifier(scene, &monde, config, &mut r.constats);
        lap(&mut r, "réseaux");
    }
    if veut(Famille::Budgets) {
        let mut b = budgets::compter(scene, &monde);
        budgets::avec_log(&mut b, std::path::Path::new(&scene.path), &scene.revision);
        budgets::caler_lumiere(&mut b, std::path::Path::new(&scene.path), &scene.revision);
        budgets::verifier(&b, &mut r.constats);
        r.budgets = Some(b);
        lap(&mut r, "budgets");
    }
    if veut(Famille::Joueur) {
        let (bilan, constats, sim) = joueur::simuler(scene, config, modele.as_ref());
        if let Some(sim) = &sim {
            joueur::nodraw_visible(scene, &monde, sim, &mut r.constats);
        }
        r.constats.extend(constats);
        r.joueur = Some(bilan);
        lap(&mut r, "joueur simulé");
    }
    if veut(Famille::Semantique) {
        if let Some(m) = &modele {
            semantique::verifier(scene, &monde, m, config, &mut r.constats);
            lap(&mut r, "règles sémantiques");
        }
    }
    for c in &mut r.constats {
        if let Some(e) = config.exception.iter().find(|e| e.couvre(c)) {
            c.excepte = Some(e.raison.clone());
        }
    }
    // erreurs d'abord, puis par règle, puis les plus fortes mesures
    r.constats.sort_by(|a, b| a.niveau.cmp(&b.niveau).then(a.regle.cmp(b.regle)).then(b.mesure.total_cmp(&a.mesure)));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motifs() {
        assert!(motif("*lampe*", "models/kit/Lampe_boutique.mdl"));
        assert!(motif("func_door*", "func_door_rotating"));
        assert!(!motif("func_door", "func_door_rotating"));
        assert!(motif("*.mdl", "a/b.mdl") && !motif("*.mdl", "a/b.vmt"));
        assert!(motif("a*b*c", "aXXbYYc") && !motif("a*b*c", "aXXcYYb"));
    }

    #[test]
    fn une_exception_sans_raison_est_refusee() {
        let d = std::env::temp_dir().join(format!("hm_regles_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("hammer-master.toml");
        std::fs::write(&p, "[[exception]]\nregle = \"phys.flotte\"\nraison = \"\"\n").unwrap();
        assert!(Config::lire(&p).unwrap_err().to_string().contains("raison"));
        std::fs::write(&p, "[[exception]]\nregle = \"phys.inconnue\"\nraison = \"drone suspendu exprès\"\n").unwrap();
        assert!(Config::lire(&p).unwrap_err().to_string().contains("inconnue"));
        std::fs::write(&p, "[[exception]]\nregle = \"phys.flotte\"\nmodele = \"*drone*\"\nraison = \"drone suspendu exprès\"\n").unwrap();
        assert_eq!(Config::lire(&p).unwrap().exception.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
