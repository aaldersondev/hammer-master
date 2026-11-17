//! Options de l'éditeur (Outils → Options), gardées dans `%LOCALAPPDATA%\HammerMaster\options.txt`, une ligne
//! `clé = valeur` chacune ; une clé absente ou illisible garde sa valeur par défaut.
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Champ de vision horizontal de la vue 3D, en degrés (comme `fov` du jeu).
    pub fov: f32,
    /// Vitesse de déplacement dans la vue 3D, en unités par seconde ; `vitesse_rapide` avec Maj.
    pub vitesse: f32,
    pub vitesse_rapide: f32,
    /// Degrés de rotation de la caméra par point de souris.
    pub sensibilite: f32,
    pub inverser_y: bool,
    /// Sauvegarde automatique (copie de récupération) : toutes les `sauvegarde_min` minutes.
    pub sauvegarde_min: u32,
    /// Distance de rendu de la vue 3D (« Back clipping plane » de Hammer).
    pub distance: f32,
    /// Vues 2D : lignes de 64 unités plus claires, une ligne sur `surligner_n` plus claire (0 : non), grille de moins
    /// de 4 pixels cachée (la grille affichée double alors), les flèches déplacent la sélection.
    pub surligner_64: bool,
    pub surligner_n: u32,
    pub cacher_petite_grille: bool,
    pub fleches: bool,
    pub grille_pointillee: bool,
    /// Textures lissées (sinon pixels nets) ; distance d'affichage des modèles dans la 3D (0 : sans limite).
    pub filtrer_textures: bool,
    pub distance_modeles: f32,
    /// Débuts de chemins de matériaux absents du navigateur de textures (« Material directory exclusion list »).
    pub exclus: Vec<String>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            fov: 90.0,
            vitesse: 500.0,
            vitesse_rapide: 1600.0,
            sensibilite: 0.25,
            inverser_y: false,
            sauvegarde_min: 1,
            distance: 80000.0,
            surligner_64: true,
            surligner_n: 8,
            cacher_petite_grille: true,
            fleches: true,
            grille_pointillee: false,
            filtrer_textures: true,
            distance_modeles: 0.0,
            exclus: ["models/", "vgui/", "console/", "hud/"].map(String::from).to_vec(),
        }
    }
}

fn oui(b: bool) -> &'static str {
    if b {
        "oui"
    } else {
        "non"
    }
}

fn fichier() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("HammerMaster").join("options.txt")
}

impl Options {
    pub fn charger() -> Options {
        Options::lire(&std::fs::read_to_string(fichier()).unwrap_or_default())
    }

    pub fn enregistrer(&self) -> Result<(), String> {
        let f = fichier();
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&f, self.ecrire()).map_err(|e| format!("{} : {e}", f.display()))
    }

    fn lire(texte: &str) -> Options {
        let mut o = Options::default();
        for l in texte.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            let v = v.trim();
            let f = |defaut: f32, lo: f32, hi: f32| v.parse::<f32>().ok().filter(|x| x.is_finite()).map_or(defaut, |x| x.clamp(lo, hi));
            match k.trim() {
                "fov" => o.fov = f(o.fov, 30.0, 150.0),
                "vitesse" => o.vitesse = f(o.vitesse, 50.0, 10000.0),
                "vitesse_rapide" => o.vitesse_rapide = f(o.vitesse_rapide, 50.0, 20000.0),
                "sensibilite" => o.sensibilite = f(o.sensibilite, 0.02, 2.0),
                "inverser_y" => o.inverser_y = v == "oui",
                "sauvegarde_min" => o.sauvegarde_min = v.parse::<u32>().map_or(o.sauvegarde_min, |m| m.clamp(1, 60)),
                "distance" => o.distance = f(o.distance, 1000.0, 200000.0),
                "surligner_64" => o.surligner_64 = v == "oui",
                "surligner_n" => o.surligner_n = v.parse::<u32>().map_or(o.surligner_n, |m| m.min(64)),
                "cacher_petite_grille" => o.cacher_petite_grille = v == "oui",
                "fleches" => o.fleches = v == "oui",
                "grille_pointillee" => o.grille_pointillee = v == "oui",
                "filtrer_textures" => o.filtrer_textures = v == "oui",
                "distance_modeles" => o.distance_modeles = f(o.distance_modeles, 0.0, 200000.0),
                "exclus" => o.exclus = v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                _ => {}
            }
        }
        o
    }

    fn ecrire(&self) -> String {
        format!(
            "fov = {}\nvitesse = {}\nvitesse_rapide = {}\nsensibilite = {}\ninverser_y = {}\nsauvegarde_min = {}\ndistance = {}\nsurligner_64 = {}\nsurligner_n = {}\ncacher_petite_grille = {}\nfleches = {}\ngrille_pointillee = {}\nfiltrer_textures = {}\ndistance_modeles = {}\nexclus = {}\n",
            self.fov,
            self.vitesse,
            self.vitesse_rapide,
            self.sensibilite,
            oui(self.inverser_y),
            self.sauvegarde_min,
            self.distance,
            oui(self.surligner_64),
            self.surligner_n,
            oui(self.cacher_petite_grille),
            oui(self.fleches),
            oui(self.grille_pointillee),
            oui(self.filtrer_textures),
            self.distance_modeles,
            self.exclus.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relire_ce_qui_est_ecrit() {
        let o = Options { fov: 75.0, vitesse: 800.0, vitesse_rapide: 3000.0, sensibilite: 0.4, inverser_y: true, sauvegarde_min: 5, distance: 32768.0, surligner_64: false, surligner_n: 0, cacher_petite_grille: false, fleches: false, grille_pointillee: true, filtrer_textures: false, distance_modeles: 3000.0, exclus: vec!["models/".into(), "effects/".into()] };
        assert_eq!(Options::lire(&o.ecrire()), o);
        // clés inconnues ignorées, valeurs illisibles ou hors bornes : défaut ou bornée
        let l = Options::lire("fov = beaucoup\nvitesse = 1e9\nautre = 3\nsauvegarde_min = 0");
        assert_eq!((l.fov, l.vitesse, l.sauvegarde_min), (90.0, 10000.0, 1));
        assert_eq!(Options::lire(""), Options::default());
    }
}
