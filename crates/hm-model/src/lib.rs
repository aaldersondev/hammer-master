//! Le modèle sémantique d'une map : ce que ses volumes veulent dire.
//!
//! Un VMF ne dit pas qu'une boîte est une salle de bain, qu'un drone est suspendu exprès ou que deux étages sont
//! reliés par un ascenseur piloté en Lua. Le modèle le dit, dans un fichier texte à côté de la map :
//! `<carte>.sem.json`. Pour une map générée par un script, c'est le générateur qui l'écrit (annotations) ; pour une
//! zone décrite directement dans le modèle, c'est la source dont le VMF est construit (lots E et F de la phase 4).
//!
//! Chaque objet a un identifiant stable d'une génération à l'autre : les règles, les retouches et les exceptions
//! s'y accrochent.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

pub mod amenagement;
pub mod construire;

/// Version du format lu et écrit par cette brique.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Modele {
    pub version: u32,
    /// Nom de la map (sans extension).
    pub carte: String,
    /// Qui a écrit ce fichier, et depuis quoi (« gen/hammer_master_sem.py, d'après out/debug_points.json »).
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub batiments: Vec<Batiment>,
    #[serde(default)]
    pub pieces: Vec<Piece>,
    #[serde(default)]
    pub liaisons: Vec<Liaison>,
    #[serde(default)]
    pub voies: Vec<Voie>,
    #[serde(default)]
    pub toits: Vec<Toit>,
    /// Modèles faits de plusieurs meubles fusionnés : ce que chacun contient.
    #[serde(default)]
    pub assemblages: Vec<Assemblage>,
    /// Objets faits de solides sans nom dans le VMF, que seul celui qui a construit la map sait nommer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objets: Vec<ObjetDeclare>,
}

/// Un objet fait de solides que le VMF ne nomme pas : un comptoir en `func_detail` n'a pas de `targetname`. Les
/// règles de rôle le comptent comme un modèle posé du même nom.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ObjetDeclare {
    /// Ce que c'est (« comptoir », « banc ») : comparé aux mots de la table des rôles.
    pub nom: String,
    /// Son centre.
    pub pos: [f32; 3],
}

/// Un modèle fusionné (plusieurs meubles compilés en un seul) et ses membres, dans le repère du modèle.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Assemblage {
    /// Chemin du modèle fusionné (`models/…/f_1234.mdl`).
    pub modele: String,
    pub membres: Vec<Membre>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Membre {
    pub modele: String,
    pub pos: [f32; 3],
    #[serde(default)]
    pub lacet: f32,
}

/// Un bâtiment : son emprise au sol. Les pièces qu'il contient le nomment.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Batiment {
    pub id: String,
    #[serde(default)]
    pub nom: String,
    /// Emprise `[xmin, ymin, xmax, ymax]`.
    pub emprise: [f32; 4],
}

/// Une pièce : un volume d'air et ce qu'il est censé être.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    pub id: String,
    /// Rôle, dans le vocabulaire de `data/roles.yaml` du skill (`wc`, `salle_de_bain`, `bureau`, `vide`…).
    pub role: String,
    /// Nom du rôle dans le générateur d'origine, s'il diffère (`bathroom`, `living_kitchen`…).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role_source: String,
    /// Emprise de l'air intérieur `[xmin, ymin, xmax, ymax]`.
    pub emprise: [f32; 4],
    /// Altitude du sol.
    pub sol: f32,
    /// Hauteur sous plafond, si le générateur la connaît.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hauteur: Option<f32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub batiment: String,
    /// Laissée vide exprès (logement que le joueur meuble, réserve, circulation).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vide_expres: bool,
    /// Nom du modèle d'aménagement appliqué à la pièce (`amenagements/<nom>.toml` du projet).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub amenagement: String,
}

impl Piece {
    pub fn largeur(&self) -> f32 {
        self.emprise[2] - self.emprise[0]
    }
    pub fn profondeur(&self) -> f32 {
        self.emprise[3] - self.emprise[1]
    }
    /// Surface de l'air intérieur, en unités carrées.
    pub fn surface(&self) -> f32 {
        self.largeur() * self.profondeur()
    }
    pub fn centre(&self) -> [f32; 3] {
        [(self.emprise[0] + self.emprise[2]) * 0.5, (self.emprise[1] + self.emprise[3]) * 0.5, self.sol]
    }
    /// Vrai si le point est dans la pièce (à `marge` près en plan, du sol à 128 au-dessus par défaut).
    pub fn contient(&self, p: [f32; 3], marge: f32) -> bool {
        p[0] >= self.emprise[0] - marge && p[0] <= self.emprise[2] + marge && p[1] >= self.emprise[1] - marge && p[1] <= self.emprise[3] + marge && p[2] >= self.sol - 8.0 && p[2] <= self.sol + self.hauteur.unwrap_or(128.0)
    }
}

/// Un moyen de passer d'un endroit à un autre que la géométrie seule ne montre pas.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Liaison {
    pub id: String,
    /// `ascenseur`, `escalier`, `echelle`, `teleporteur`, `metro`.
    pub genre: String,
    pub de: [f32; 3],
    pub vers: [f32; 3],
    #[serde(default = "vrai")]
    pub aller_retour: bool,
}

fn vrai() -> bool {
    true
}

/// Une voie ou un viaduc, et la manière dont chacune de ses extrémités se termine.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Voie {
    pub id: String,
    #[serde(default)]
    pub extremites: Vec<Extremite>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Extremite {
    pub en: [f32; 3],
    /// `terminus`, `tunnel`, `jonction`, `teleporteur`.
    pub genre: String,
}

/// Un toit, une dalle ou un auvent, et ce qui le porte.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Toit {
    pub id: String,
    /// Boîte `[xmin, ymin, zmin, xmax, ymax, zmax]`.
    pub boite: [f32; 6],
    /// Points d'appui déclarés (pied de chaque poteau, ou point de contact avec un mur porteur).
    #[serde(default)]
    pub appuis: Vec<[f32; 3]>,
}

pub const GENRES_LIAISON: [&str; 5] = ["ascenseur", "escalier", "echelle", "teleporteur", "metro"];
pub const GENRES_EXTREMITE: [&str; 4] = ["terminus", "tunnel", "jonction", "teleporteur"];

impl Modele {
    /// Le fichier du modèle d'une map : `<carte>.sem.json` à côté du VMF.
    pub fn chemin(vmf: &Path) -> PathBuf {
        vmf.with_extension("sem.json")
    }

    /// Le modèle d'une map, s'il existe. Un fichier présent mais faux est une erreur, pas une absence.
    pub fn du_vmf(vmf: &Path) -> Result<Option<Modele>> {
        let p = Modele::chemin(vmf);
        if !p.is_file() {
            return Ok(None);
        }
        Modele::lire(&p).map(Some)
    }

    pub fn lire(chemin: &Path) -> Result<Modele> {
        let octets = std::fs::read(chemin).with_context(|| format!("lecture de {}", chemin.display()))?;
        let m: Modele = serde_json::from_slice(&octets).with_context(|| format!("{}", chemin.display()))?;
        m.controler().with_context(|| format!("{}", chemin.display()))?;
        Ok(m)
    }

    pub fn ecrire(&self, chemin: &Path) -> Result<()> {
        self.controler()?;
        std::fs::write(chemin, self.en_json())?;
        Ok(())
    }

    /// Texte du fichier : JSON indenté, une valeur par ligne, fin de ligne Unix (lisible dans un diff).
    pub fn en_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("modèle sérialisable") + "\n"
    }

    /// Cohérence interne : version, identifiants uniques et non vides, volumes à l'endroit, genres connus,
    /// bâtiments nommés par les pièces existants.
    pub fn controler(&self) -> Result<()> {
        ensure!(self.version == VERSION, "version {} du modèle : cette brique lit la version {VERSION}", self.version);
        let mut vus: HashSet<String> = HashSet::new();
        let mut id = |genre: &str, i: &str| -> Result<()> {
            ensure!(!i.trim().is_empty(), "{genre} sans identifiant");
            ensure!(!i.contains(char::is_whitespace), "identifiant « {i} » : pas d'espace dans un identifiant");
            if !vus.insert(format!("{genre}:{i}")) {
                bail!("{genre} « {i} » déclaré deux fois");
            }
            Ok(())
        };
        for b in &self.batiments {
            id("batiment", &b.id)?;
            ensure!(b.emprise[0] < b.emprise[2] && b.emprise[1] < b.emprise[3], "bâtiment « {} » : emprise à l'envers ou vide", b.id);
        }
        for p in &self.pieces {
            id("piece", &p.id)?;
            ensure!(!p.role.trim().is_empty(), "pièce « {} » sans rôle (« vide » se dit aussi)", p.id);
            ensure!(p.emprise[0] < p.emprise[2] && p.emprise[1] < p.emprise[3], "pièce « {} » : emprise à l'envers ou vide", p.id);
            ensure!(p.hauteur.map_or(true, |h| h > 0.0), "pièce « {} » : hauteur nulle ou négative", p.id);
            ensure!(p.batiment.is_empty() || self.batiments.iter().any(|b| b.id == p.batiment), "pièce « {} » : bâtiment « {} » inconnu", p.id, p.batiment);
        }
        for l in &self.liaisons {
            id("liaison", &l.id)?;
            ensure!(GENRES_LIAISON.contains(&l.genre.as_str()), "liaison « {} » : genre « {} » inconnu ({})", l.id, l.genre, GENRES_LIAISON.join(", "));
            ensure!(l.de != l.vers, "liaison « {} » : départ et arrivée confondus", l.id);
        }
        for v in &self.voies {
            id("voie", &v.id)?;
            for e in &v.extremites {
                ensure!(GENRES_EXTREMITE.contains(&e.genre.as_str()), "voie « {} » : extrémité de genre « {} » inconnu ({})", v.id, e.genre, GENRES_EXTREMITE.join(", "));
            }
        }
        for t in &self.toits {
            id("toit", &t.id)?;
            ensure!((0..3).all(|a| t.boite[a] < t.boite[a + 3]), "toit « {} » : boîte à l'envers ou vide", t.id);
        }
        Ok(())
    }

    /// La pièce qui contient un point, s'il y en a une.
    pub fn piece_en(&self, p: [f32; 3]) -> Option<&Piece> {
        self.pieces.iter().find(|x| x.contient(p, 0.0))
    }

    /// Résumé en une ligne.
    pub fn resume(&self) -> String {
        format!("{} pièce(s), {} bâtiment(s), {} liaison(s), {} voie(s), {} toit(s), {} assemblage(s)", self.pieces.len(), self.batiments.len(), self.liaisons.len(), self.voies.len(), self.toits.len(), self.assemblages.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exemple() -> Modele {
        Modele {
            version: VERSION,
            carte: "essai".into(),
            source: "écrit à la main".into(),
            batiments: vec![Batiment { id: "gare".into(), nom: "Gare centrale".into(), emprise: [0.0, 0.0, 1024.0, 512.0] }],
            pieces: vec![
                Piece { id: "gare.wc_h".into(), role: "wc".into(), role_source: String::new(), emprise: [0.0, 0.0, 256.0, 320.0], sol: 8.0, hauteur: Some(192.0), batiment: "gare".into(), vide_expres: false, amenagement: "wc_cabines".into() },
                Piece { id: "gare.hall".into(), role: "vide".into(), role_source: "hall".into(), emprise: [256.0, 0.0, 1024.0, 512.0], sol: 8.0, hauteur: None, batiment: "gare".into(), vide_expres: true, amenagement: String::new() },
            ],
            liaisons: vec![Liaison { id: "gare.lift.0-1".into(), genre: "ascenseur".into(), de: [700.0, 300.0, 8.0], vers: [700.0, 300.0, 808.0], aller_retour: true }],
            voies: vec![Voie { id: "m1".into(), extremites: vec![Extremite { en: [2300.0, 2560.0, 768.0], genre: "terminus".into() }] }],
            toits: vec![Toit { id: "gare.auvent".into(), boite: [0.0, 0.0, 984.0, 648.0, 620.0, 996.0], appuis: vec![[16.0, 16.0, 808.0]] }],
            assemblages: vec![Assemblage { modele: "models/x/f_1.mdl".into(), membres: vec![Membre { modele: "models/x/bureau.mdl".into(), pos: [0.0, 42.0, 0.0], lacet: 180.0 }] }],
            objets: vec![ObjetDeclare { nom: "comptoir".into(), pos: [64.0, 64.0, 24.0] }],
        }
    }

    #[test]
    fn ecrit_puis_relu_sans_perte() {
        let m = exemple();
        let d = std::env::temp_dir().join(format!("hm_model_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let vmf = d.join("essai.vmf");
        assert!(Modele::du_vmf(&vmf).unwrap().is_none(), "pas de fichier : pas de modèle, sans erreur");
        m.ecrire(&Modele::chemin(&vmf)).unwrap();
        let relu = Modele::du_vmf(&vmf).unwrap().unwrap();
        assert_eq!(relu, m);
        // le texte relu puis réécrit est le même, octet pour octet
        assert_eq!(relu.en_json(), std::fs::read_to_string(Modele::chemin(&vmf)).unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn requetes() {
        let m = exemple();
        assert_eq!(m.piece_en([100.0, 100.0, 40.0]).unwrap().id, "gare.wc_h");
        assert_eq!(m.piece_en([600.0, 100.0, 40.0]).unwrap().id, "gare.hall");
        assert!(m.piece_en([100.0, 100.0, 900.0]).is_none());
        assert_eq!(m.pieces[0].surface(), 256.0 * 320.0);
    }

    #[test]
    fn les_fichiers_faux_sont_refuses_avec_leur_raison() {
        let refus = |f: &dyn Fn(&mut Modele)| {
            let mut m = exemple();
            f(&mut m);
            m.controler().unwrap_err().to_string()
        };
        assert!(refus(&|m| m.version = 7).contains("version 7"));
        assert!(refus(&|m| m.pieces[1].id = "gare.wc_h".into()).contains("deux fois"));
        assert!(refus(&|m| m.pieces[0].emprise = [256.0, 0.0, 0.0, 320.0]).contains("à l'envers"));
        assert!(refus(&|m| m.pieces[0].role = String::new()).contains("sans rôle"));
        assert!(refus(&|m| m.pieces[0].batiment = "mairie".into()).contains("inconnu"));
        assert!(refus(&|m| m.liaisons[0].genre = "tapis".into()).contains("genre « tapis »"));
        assert!(refus(&|m| m.voies[0].extremites[0].genre = "vide".into()).contains("inconnu"));
        assert!(refus(&|m| m.pieces[0].id = "gare wc".into()).contains("espace"));
        // une clé inconnue dans le fichier est une faute de frappe : refusée, pas ignorée
        assert!(serde_json::from_str::<Modele>(r#"{"version":1,"carte":"x","piecess":[]}"#).is_err());
    }
}
