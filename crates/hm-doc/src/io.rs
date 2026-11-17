//! Entrées et sorties des entités (bloc `connections`). Une sortie s'écrit `"OnTrigger" "cible,Entrée,param,délai,fois"`
//! avec des virgules dans les anciennes versions, ou avec le caractère ESC (0x1B) dans les récentes (wiki Valve, « VMF
//! (Valve Map Format) », section Connections) ; les maps livrées avec Garry's Mod et celles de Hammer++ emploient ESC.
use serde::{Deserialize, Serialize};

use crate::modele::{Document, Entite};

pub const ESC: char = '\u{1b}';

fn moins_un() -> i32 {
    -1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sortie {
    /// La sortie de l'entité (`OnTrigger`).
    pub sortie: String,
    /// Nom de l'entité visée (ou classe, ou `!activator`…).
    pub cible: String,
    /// L'entrée de la cible (`Toggle`).
    pub entree: String,
    #[serde(default)]
    pub parametre: String,
    /// Délai en secondes.
    #[serde(default)]
    pub delai: f64,
    /// Nombre de fois (−1 : toujours).
    #[serde(default = "moins_un")]
    pub fois: i32,
}

/// Une sortie lue (séparateur ESC s'il y en a un, sinon virgules ; un paramètre à virgules est recollé).
pub fn lire(nom: &str, valeur: &str) -> Sortie {
    let morceaux: Vec<&str> = if valeur.contains(ESC) { valeur.split(ESC).collect() } else { valeur.split(',').collect() };
    let n = morceaux.len();
    let (cible, entree, parametre, delai, fois) = if n >= 5 {
        let sep = if valeur.contains(ESC) { ESC.to_string() } else { ",".into() };
        (morceaux[0], morceaux[1], morceaux[2..n - 2].join(&sep), morceaux[n - 2], morceaux[n - 1])
    } else {
        (morceaux.first().copied().unwrap_or(""), morceaux.get(1).copied().unwrap_or(""), morceaux.get(2).map(|s| s.to_string()).unwrap_or_default(), morceaux.get(3).copied().unwrap_or("0"), morceaux.get(4).copied().unwrap_or("-1"))
    };
    Sortie {
        sortie: nom.to_string(),
        cible: cible.to_string(),
        entree: entree.to_string(),
        parametre,
        delai: delai.trim().parse().unwrap_or(0.0),
        fois: fois.trim().parse().unwrap_or(-1),
    }
}

/// La valeur écrite d'une sortie.
pub fn ecrire(s: &Sortie, esc: bool) -> String {
    let sep = if esc { ESC.to_string() } else { ",".to_string() };
    [s.cible.clone(), s.entree.clone(), s.parametre.clone(), crate::kv::nombre(s.delai), s.fois.to_string()].join(&sep)
}

impl Entite {
    pub fn sorties_lues(&self) -> Vec<Sortie> {
        self.sorties.as_ref().map(|v| v.iter().map(|(k, x)| lire(k, x)).collect()).unwrap_or_default()
    }

    /// Le nom (`targetname`) s'il n'est pas vide.
    pub fn nom_donne(&self) -> Option<&str> {
        Some(self.nom()).filter(|n| !n.trim().is_empty())
    }
}

impl Document {
    /// Le séparateur de la map : ESC si une sortie l'emploie, ou s'il n'y a aucune sortie ; sinon la virgule.
    pub fn sorties_en_esc(&self) -> bool {
        let mut vu = false;
        for e in self.entites() {
            for (_, v) in e.sorties.iter().flatten() {
                if v.contains(ESC) {
                    return true;
                }
                vu = true;
            }
        }
        !vu
    }

    /// Les noms d'entités de la map (`targetname`), triés, sans doublon.
    pub fn noms(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entites().filter_map(|e| e.nom_donne().map(String::from)).collect();
        v.sort_by_key(|x| x.to_ascii_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// Les entités qui portent ce nom (comparaison sans casse, `*` en fin de nom accepté comme le moteur).
    pub fn entites_nommees(&self, nom: &str) -> Vec<u64> {
        let n = nom.to_ascii_lowercase();
        self.entites()
            .filter(|e| {
                let Some(x) = e.nom_donne() else { return false };
                let x = x.to_ascii_lowercase();
                match n.strip_suffix('*') {
                    Some(p) => x.starts_with(p),
                    None => x == n,
                }
            })
            .map(|e| e.id)
            .collect()
    }

    /// Les sorties d'autres entités qui visent celle-ci (par son nom) : (source, sortie).
    pub fn entrees_vers(&self, entite: u64) -> Vec<(u64, Sortie)> {
        let Some(nom) = self.entite(entite).and_then(|e| e.nom_donne()).map(|n| n.to_ascii_lowercase()) else { return Vec::new() };
        let mut out = Vec::new();
        for e in self.entites() {
            for s in e.sorties_lues() {
                let c = s.cible.to_ascii_lowercase();
                let vise = match c.strip_suffix('*') {
                    Some(p) => nom.starts_with(p),
                    None => c == nom,
                };
                if vise {
                    out.push((e.id, s));
                }
            }
        }
        out
    }
}
