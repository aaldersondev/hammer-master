//! Lecture des FGD (Forge Game Data) : les classes d'entités, leurs clés, choix, drapeaux, entrées et sorties, et
//! les aides de l'éditeur (couleur, taille, modèle, icône). Syntaxe vérifiée sur la page « FGD » du wiki Valve
//! (14/11/2026) : `@Classe aides = nom : "description" [ corps ]`, descriptions continuées par `+`, `@include`,
//! `@ExtendClass` de Hammer++ (lu en dernier, ajoute des clés à une classe existante).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Genre {
    Base,
    Point,
    Solide,
    Npc,
    KeyFrame,
    Move,
    Filtre,
    Extension,
}

impl Genre {
    fn lire(s: &str) -> Option<Genre> {
        Some(match s.to_ascii_lowercase().as_str() {
            "baseclass" => Genre::Base,
            "pointclass" => Genre::Point,
            "solidclass" => Genre::Solide,
            "npcclass" => Genre::Npc,
            "keyframeclass" => Genre::KeyFrame,
            "moveclass" => Genre::Move,
            "filterclass" => Genre::Filtre,
            "extendclass" => Genre::Extension,
            _ => return None,
        })
    }

    /// Posée à un point (les autres classes, hors base et solide, sont des entités ponctuelles).
    pub fn ponctuelle(self) -> bool {
        !matches!(self, Genre::Base | Genre::Solide | Genre::Extension)
    }
}

/// Un choix d'une clé `choices` ou un drapeau d'une clé `flags`.
#[derive(Clone, Debug, PartialEq)]
pub struct Choix {
    pub valeur: String,
    pub libelle: String,
    /// Pour un drapeau : coché par défaut.
    pub defaut: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Propriete {
    pub nom: String,
    /// `string`, `integer`, `float`, `boolean`, `choices`, `flags`, `color255`, `studio`, `material`, `sound`,
    /// `target_destination`, `angle`, `vector`…
    pub genre: String,
    pub libelle: String,
    pub defaut: Option<String>,
    pub description: String,
    pub choix: Vec<Choix>,
    pub lecture_seule: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntreeSortie {
    pub nom: String,
    pub genre: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Classe {
    pub nom: String,
    pub genre: Genre,
    pub bases: Vec<String>,
    /// Aides de l'éditeur : `color`, `size`, `studio`, `iconsprite`, `sphere`, `line`… avec leurs arguments.
    pub aides: Vec<(String, Vec<String>)>,
    pub description: String,
    pub proprietes: Vec<Propriete>,
    pub entrees: Vec<EntreeSortie>,
    pub sorties: Vec<EntreeSortie>,
}

impl Classe {
    pub fn aide(&self, nom: &str) -> Option<&[String]> {
        self.aides.iter().find(|(n, _)| n.eq_ignore_ascii_case(nom)).map(|(_, a)| a.as_slice())
    }

    pub fn propriete(&self, nom: &str) -> Option<&Propriete> {
        self.proprietes.iter().find(|p| p.nom.eq_ignore_ascii_case(nom))
    }

    /// Couleur dans les vues (aide `color`), magenta (220 30 220) sans elle, comme Hammer.
    pub fn couleur(&self) -> [u8; 3] {
        self.aide("color")
            .and_then(|a| {
                let v: Vec<u8> = a.join(" ").split_whitespace().filter_map(|x| x.parse::<f32>().ok()).map(|x| x.clamp(0.0, 255.0) as u8).collect();
                (v.len() >= 3).then(|| [v[0], v[1], v[2]])
            })
            .unwrap_or([220, 30, 220])
    }

    /// Boîte de l'entité sans modèle (aide `size`), 16 de côté sans elle.
    pub fn taille(&self) -> ([f32; 3], [f32; 3]) {
        let Some(a) = self.aide("size") else { return ([-8.0; 3], [8.0; 3]) };
        let v: Vec<f32> = a.join(" ").split(|c: char| c.is_whitespace() || c == ',').filter_map(|x| x.parse().ok()).collect();
        match v.len() {
            3 => ([-v[0] / 2.0, -v[1] / 2.0, -v[2] / 2.0], [v[0] / 2.0, v[1] / 2.0, v[2] / 2.0]),
            6 => ([v[0], v[1], v[2]], [v[3], v[4], v[5]]),
            _ => ([-8.0; 3], [8.0; 3]),
        }
    }

    /// Matériau de l'icône `iconsprite("editor/light.vmt")`, sans son extension.
    pub fn icone(&self) -> Option<&str> {
        self.aide("iconsprite").and_then(|a| a.first()).map(|s| s.trim_end_matches(".vmt").trim_end_matches(".spr")).filter(|s| !s.is_empty())
    }

    /// Modèle montré par l'aide `studio("…")` / `studioprop("…")`, s'il est donné.
    pub fn modele(&self) -> Option<&str> {
        ["studio", "studioprop", "lightprop"].iter().find_map(|h| self.aide(h)).and_then(|a| a.first()).map(|s| s.as_str()).filter(|s| !s.is_empty())
    }

    /// Le modèle vient de `lightprop("…")` : « identique à studioprop(), sauf que le tangage du modèle est inversé »
    /// (wiki Valve, « FGD ») ; c'est le projecteur de `light_spot`.
    pub fn modele_tangage_inverse(&self) -> bool {
        self.aide("studio").is_none() && self.aide("studioprop").is_none() && self.aide("lightprop").is_some()
    }
}

#[derive(Debug)]
pub struct Erreur {
    pub fichier: PathBuf,
    pub ligne: usize,
    pub quoi: String,
}

impl std::fmt::Display for Erreur {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ligne {} : {}", self.fichier.display(), self.ligne, self.quoi)
    }
}

/// Les classes de tous les FGD chargés, résolues (bases fusionnées).
#[derive(Clone, Debug, Default)]
pub struct Fgd {
    brutes: HashMap<String, Classe>,
    ordre: Vec<String>,
    pub fichiers: Vec<PathBuf>,
    /// Ce qui n'a pas pu être lu (une classe mal écrite ne bloque pas le reste).
    pub avertissements: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Jeton {
    Mot(String),
    Chaine(String),
    Sym(char),
}

fn decouper(src: &str) -> Vec<(Jeton, usize)> {
    let mut out = Vec::new();
    let c: Vec<char> = src.chars().collect();
    let (mut i, mut ligne) = (0, 1);
    while i < c.len() {
        match c[i] {
            '\n' => {
                ligne += 1;
                i += 1;
            }
            x if x.is_whitespace() => i += 1,
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\n' {
                        ligne += 1;
                    }
                    s.push(c[i]);
                    i += 1;
                }
                i += 1;
                out.push((Jeton::Chaine(s), ligne));
            }
            '@' | '=' | ':' | '[' | ']' | '(' | ')' | ',' | '+' => {
                out.push((Jeton::Sym(c[i]), ligne));
                i += 1;
            }
            _ => {
                let mut s = String::new();
                while i < c.len() && !c[i].is_whitespace() && !matches!(c[i], '@' | '=' | ':' | '[' | ']' | '(' | ')' | ',' | '"') {
                    // un `+` collé à un nombre (« +16 ») fait partie du mot
                    if c[i] == '+' && !s.is_empty() {
                        break;
                    }
                    s.push(c[i]);
                    i += 1;
                }
                if s.is_empty() {
                    i += 1;
                    continue;
                }
                out.push((Jeton::Mot(s), ligne));
            }
        }
    }
    out
}

struct Lecteur<'a> {
    j: &'a [(Jeton, usize)],
    i: usize,
}

impl<'a> Lecteur<'a> {
    fn voir(&self) -> Option<&Jeton> {
        self.j.get(self.i).map(|x| &x.0)
    }
    fn ligne(&self) -> usize {
        self.j.get(self.i.min(self.j.len().saturating_sub(1))).map(|x| x.1).unwrap_or(0)
    }
    fn prendre(&mut self) -> Option<Jeton> {
        let t = self.j.get(self.i).map(|x| x.0.clone());
        self.i += 1;
        t
    }
    fn sym(&mut self, c: char) -> bool {
        if self.voir() == Some(&Jeton::Sym(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    /// Une chaîne, éventuellement continuée par `+ "…"`.
    fn chaine(&mut self) -> Option<String> {
        let mut s = match self.voir()? {
            Jeton::Chaine(s) => s.clone(),
            _ => return None,
        };
        self.i += 1;
        while self.voir() == Some(&Jeton::Sym('+')) {
            if let Some(Jeton::Chaine(x)) = self.j.get(self.i + 1).map(|x| &x.0) {
                s += x;
                self.i += 2;
            } else {
                break;
            }
        }
        Some(s)
    }
    /// Une valeur : chaîne ou mot.
    fn valeur(&mut self) -> Option<String> {
        match self.voir()? {
            Jeton::Chaine(_) => self.chaine(),
            Jeton::Mot(m) => {
                let m = m.clone();
                self.i += 1;
                Some(m)
            }
            Jeton::Sym(_) => None,
        }
    }
    /// Saute jusqu'au `]` qui ferme le bloc ouvert (pour sauter une section inconnue).
    fn sauter_bloc(&mut self) {
        let mut n = 0;
        while let Some(t) = self.prendre() {
            match t {
                Jeton::Sym('[') => n += 1,
                Jeton::Sym(']') => {
                    n -= 1;
                    if n <= 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

impl Fgd {
    /// Charge un FGD et ceux qu'il inclut (chemins relatifs à son dossier).
    pub fn charger(&mut self, chemin: &Path) -> Result<(), Erreur> {
        let src = std::fs::read(chemin).map_err(|e| Erreur { fichier: chemin.to_path_buf(), ligne: 0, quoi: e.to_string() })?;
        let texte = String::from_utf8_lossy(&src).into_owned();
        self.fichiers.push(chemin.to_path_buf());
        let jetons = decouper(&texte);
        let mut l = Lecteur { j: &jetons, i: 0 };
        let mut extensions = Vec::new();
        while l.voir().is_some() {
            if !l.sym('@') {
                l.i += 1;
                continue;
            }
            let Some(Jeton::Mot(mot)) = l.prendre() else { continue };
            let bas = mot.to_ascii_lowercase();
            match bas.as_str() {
                "include" => {
                    if let Some(f) = l.chaine() {
                        let p = chemin.parent().unwrap_or(Path::new(".")).join(&f);
                        if !self.fichiers.contains(&p) {
                            if let Err(e) = self.charger(&p) {
                                self.avertissements.push(e.to_string());
                            }
                        }
                    }
                }
                "mapsize" | "version" | "gridnav" => {
                    if l.sym('(') {
                        while l.voir().is_some() && !l.sym(')') {
                            l.i += 1;
                        }
                    }
                }
                "materialexclusion" | "autovisgroup" | "main" => {
                    // `@AutoVisGroup = "nom" [ … ]` : sauté pour l'instant
                    while l.voir().is_some() && l.voir() != Some(&Jeton::Sym('[')) {
                        l.i += 1;
                    }
                    l.sauter_bloc();
                }
                _ => match Genre::lire(&bas) {
                    Some(g) => {
                        let ligne = l.ligne();
                        match lire_classe(&mut l, g) {
                            Some(c) if g == Genre::Extension => extensions.push(c),
                            Some(c) => {
                                // les noms sont sensibles à la casse : base.fgd a la base « Light » et l'entité « light »
                                let k = c.nom.clone();
                                if !self.brutes.contains_key(&k) {
                                    self.ordre.push(k.clone());
                                }
                                // la dernière définition gagne (wiki)
                                self.brutes.insert(k, c);
                            }
                            None => self.avertissements.push(format!("{} ligne {ligne} : classe illisible", chemin.display())),
                        }
                    }
                    None => self.avertissements.push(format!("{} : @{mot} inconnu", chemin.display())),
                },
            }
        }
        for e in extensions {
            let k = self.cle(&e.nom, false);
            match k.and_then(|k| self.brutes.get_mut(&k)) {
                Some(c) => {
                    for p in e.proprietes {
                        c.proprietes.retain(|x| !x.nom.eq_ignore_ascii_case(&p.nom));
                        c.proprietes.push(p);
                    }
                    c.entrees.extend(e.entrees);
                    c.sorties.extend(e.sorties);
                    c.aides.extend(e.aides);
                }
                None => self.avertissements.push(format!("@ExtendClass {} : classe inconnue", e.nom)),
            }
        }
        Ok(())
    }

    /// Les FGD que le Hammer++ installé charge pour Garry's Mod (sa configuration de jeu), sinon `garrysmod.fgd`,
    /// puis celui de Hammer++ (toujours chargé par lui, en dernier).
    pub fn de_garrysmod() -> Fgd {
        let mut f = Fgd::default();
        let Some(gmod) = hm_assets::jeu::garrysmod() else { return f };
        let hpp = gmod.join("bin/win64/hammerplusplus");
        let mut liste: Vec<PathBuf> = Vec::new();
        if let Ok(src) = std::fs::read(hpp.join("hammerplusplus_gameconfig.txt")) {
            for l in String::from_utf8_lossy(&src).lines() {
                let m: Vec<&str> = l.split('"').filter(|x| !x.trim().is_empty()).collect();
                if m.len() >= 2 && m[0].starts_with("GameData") {
                    liste.push(PathBuf::from(m[1]));
                }
            }
        }
        if liste.is_empty() {
            liste.push(gmod.join("bin/garrysmod.fgd"));
        }
        liste.push(hpp.join("hammerplusplus_fgd.fgd"));
        for p in liste {
            if p.is_file() {
                if let Err(e) = f.charger(&p) {
                    f.avertissements.push(e.to_string());
                }
            }
        }
        f
    }

    /// La clé d'une classe : le nom exact, sinon le même sans tenir compte de la casse, parmi les bases si `base`,
    /// parmi les classes à poser sinon.
    fn cle(&self, nom: &str, base: bool) -> Option<String> {
        if let Some(c) = self.brutes.get(nom) {
            if (c.genre == Genre::Base) == base || !self.ordre.iter().any(|k| k.eq_ignore_ascii_case(nom) && k != nom) {
                return Some(nom.to_string());
            }
        }
        self.ordre.iter().find(|k| k.eq_ignore_ascii_case(nom) && self.brutes.get(*k).map_or(false, |c| (c.genre == Genre::Base) == base)).or_else(|| self.ordre.iter().find(|k| k.eq_ignore_ascii_case(nom))).cloned()
    }

    /// Une classe avec tout ce qu'elle hérite de ses bases (une clé redéfinie remplace celle de la base).
    pub fn classe(&self, nom: &str) -> Option<Classe> {
        let mut vus = Vec::new();
        let k = self.cle(nom, false)?;
        self.resoudre(&k, &mut vus)
    }

    fn resoudre(&self, nom: &str, vus: &mut Vec<String>) -> Option<Classe> {
        if vus.iter().any(|v| v == nom) || vus.len() > 32 {
            return None;
        }
        vus.push(nom.to_string());
        let c = self.brutes.get(nom)?;
        let mut out = Classe { nom: c.nom.clone(), genre: c.genre, bases: c.bases.clone(), aides: Vec::new(), description: c.description.clone(), proprietes: Vec::new(), entrees: Vec::new(), sorties: Vec::new() };
        for b in &c.bases {
            let Some(kb) = self.cle(b, true) else { continue };
            if let Some(bc) = self.resoudre(&kb, vus) {
                for p in bc.proprietes {
                    out.proprietes.retain(|x| !x.nom.eq_ignore_ascii_case(&p.nom));
                    out.proprietes.push(p);
                }
                for e in bc.entrees {
                    out.entrees.retain(|x| !x.nom.eq_ignore_ascii_case(&e.nom));
                    out.entrees.push(e);
                }
                for s in bc.sorties {
                    out.sorties.retain(|x| !x.nom.eq_ignore_ascii_case(&s.nom));
                    out.sorties.push(s);
                }
                out.aides.extend(bc.aides);
            }
        }
        for p in &c.proprietes {
            out.proprietes.retain(|x| !x.nom.eq_ignore_ascii_case(&p.nom));
            out.proprietes.push(p.clone());
        }
        for e in &c.entrees {
            out.entrees.retain(|x| !x.nom.eq_ignore_ascii_case(&e.nom));
            out.entrees.push(e.clone());
        }
        for s in &c.sorties {
            out.sorties.retain(|x| !x.nom.eq_ignore_ascii_case(&s.nom));
            out.sorties.push(s.clone());
        }
        // les aides de la classe passent devant celles des bases
        let mut aides = c.aides.clone();
        aides.extend(out.aides);
        out.aides = aides;
        vus.pop();
        Some(out)
    }

    /// Noms des classes qu'on peut poser (ni base ni extension), dans l'ordre des fichiers.
    pub fn classes(&self, genre_solide: bool) -> Vec<&str> {
        self.ordre.iter().filter_map(|k| self.brutes.get(k)).filter(|c| if genre_solide { c.genre == Genre::Solide } else { c.genre.ponctuelle() }).map(|c| c.nom.as_str()).collect()
    }

    pub fn nombre(&self) -> usize {
        self.brutes.len()
    }
}

fn lire_classe(l: &mut Lecteur, genre: Genre) -> Option<Classe> {
    let mut c = Classe { nom: String::new(), genre, bases: Vec::new(), aides: Vec::new(), description: String::new(), proprietes: Vec::new(), entrees: Vec::new(), sorties: Vec::new() };
    // aides jusqu'au `=`
    loop {
        match l.voir()? {
            Jeton::Sym('=') => {
                l.i += 1;
                break;
            }
            Jeton::Mot(m) => {
                let nom = m.clone();
                l.i += 1;
                let mut args = Vec::new();
                if l.sym('(') {
                    let mut courant = String::new();
                    loop {
                        match l.prendre()? {
                            Jeton::Sym(')') => break,
                            Jeton::Sym(',') => {
                                args.push(std::mem::take(&mut courant).trim().to_string());
                            }
                            Jeton::Mot(x) | Jeton::Chaine(x) => {
                                if !courant.is_empty() {
                                    courant.push(' ');
                                }
                                courant += &x;
                            }
                            Jeton::Sym(s) => courant.push(s),
                        }
                    }
                    if !courant.trim().is_empty() || !args.is_empty() {
                        args.push(courant.trim().to_string());
                    }
                }
                if nom.eq_ignore_ascii_case("base") {
                    c.bases.extend(args.into_iter().filter(|a| !a.is_empty()));
                } else {
                    c.aides.push((nom.to_ascii_lowercase(), args));
                }
            }
            _ => {
                l.i += 1;
            }
        }
    }
    c.nom = match l.prendre()? {
        Jeton::Mot(m) | Jeton::Chaine(m) => m,
        _ => return None,
    };
    if l.sym(':') {
        c.description = l.chaine().unwrap_or_default();
        // Hammer 5 : une URL après une seconde colonne
        if l.sym(':') {
            let _ = l.chaine();
        }
    }
    if !l.sym('[') {
        return Some(c);
    }
    // le corps
    loop {
        match l.voir()? {
            Jeton::Sym(']') => {
                l.i += 1;
                return Some(c);
            }
            Jeton::Mot(m) if (m.eq_ignore_ascii_case("input") || m.eq_ignore_ascii_case("output")) && matches!(l.j.get(l.i + 2).map(|x| &x.0), Some(Jeton::Sym('('))) => {
                let entree = m.eq_ignore_ascii_case("input");
                l.i += 1;
                let nom = match l.prendre()? {
                    Jeton::Mot(n) => n,
                    _ => return None,
                };
                let mut genre_es = String::new();
                if l.sym('(') {
                    while let Some(t) = l.prendre() {
                        match t {
                            Jeton::Sym(')') => break,
                            Jeton::Mot(x) => genre_es = x,
                            _ => {}
                        }
                    }
                }
                let description = if l.sym(':') { l.chaine().unwrap_or_default() } else { String::new() };
                let es = EntreeSortie { nom, genre: genre_es, description };
                if entree {
                    c.entrees.push(es);
                } else {
                    c.sorties.push(es);
                }
            }
            Jeton::Mot(_) => {
                let p = lire_propriete(l)?;
                c.proprietes.push(p);
            }
            _ => {
                l.i += 1;
            }
        }
    }
}

fn lire_propriete(l: &mut Lecteur) -> Option<Propriete> {
    let nom = match l.prendre()? {
        Jeton::Mot(n) => n,
        _ => return None,
    };
    let mut genre = String::new();
    if l.sym('(') {
        while let Some(t) = l.prendre() {
            match t {
                Jeton::Sym(')') => break,
                Jeton::Mot(x) => genre = x.trim_start_matches('*').to_ascii_lowercase(),
                _ => {}
            }
        }
    }
    let mut p = Propriete { nom, genre, libelle: String::new(), defaut: None, description: String::new(), choix: Vec::new(), lecture_seule: false };
    // modificateurs
    while let Some(Jeton::Mot(m)) = l.voir() {
        match m.to_ascii_lowercase().as_str() {
            "readonly" => p.lecture_seule = true,
            "report" => {}
            _ => break,
        }
        l.i += 1;
    }
    if l.sym(':') {
        if let Some(x) = l.chaine().or_else(|| if matches!(l.voir(), Some(Jeton::Mot(_))) { l.valeur() } else { None }) {
            p.libelle = x;
        }
        if l.sym(':') {
            // valeur par défaut, peut-être vide
            if !matches!(l.voir(), Some(Jeton::Sym(':')) | Some(Jeton::Sym('=')) | Some(Jeton::Sym(']'))) {
                p.defaut = l.valeur().filter(|v| !v.is_empty());
            }
            if l.sym(':') {
                p.description = l.chaine().unwrap_or_default();
            }
        }
    }
    if l.sym('=') && l.sym('[') {
        // liste de choix ou de drapeaux : valeur : "libellé" [: défaut]
        loop {
            match l.voir()? {
                Jeton::Sym(']') => {
                    l.i += 1;
                    break;
                }
                Jeton::Mot(_) | Jeton::Chaine(_) => {
                    let valeur = l.valeur()?;
                    let mut ch = Choix { valeur, libelle: String::new(), defaut: false };
                    if l.sym(':') {
                        ch.libelle = l.chaine().unwrap_or_default();
                        if l.sym(':') {
                            ch.defaut = l.valeur().map_or(false, |v| v.trim() == "1");
                        }
                    }
                    p.choix.push(ch);
                }
                _ => {
                    l.i += 1;
                }
            }
        }
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXEMPLE: &str = r#"
// commentaire
@BaseClass = Targetname [ targetname(target_source) : "Name" : : "The name." ]
@BaseClass color(0 0 200) = Couleur []
@PointClass base(Targetname, Couleur) size(-4 -4 -4, 4 4 4) studio("models/x.mdl") = exemple : "Une " +
    "description." : "https://exemple"
[
    Property_name_1(string) : "Example String Name" : "Example" : "Keyvalue Description"
    health(integer) readonly : "Strength" : : "Not editable."
    skin(choices) : "Skin" : 0 : "La skin" =
    [
        0 : "Default"
        1 : "Something"
    ]
    spawnflags(flags) =
    [
        1 : "A flag" : 0
        2 : "Another flag" : 1
    ]
    input DoSomething(void) : "Do something"
    output OnSomethingHappened(void) : "Fires"
]
@SolidClass base(Targetname) = func_exemple : "Brosse" []
"#;

    fn charger_texte(t: &str) -> Fgd {
        let p = std::env::temp_dir().join(format!("hm_fgd_{}.fgd", std::process::id()));
        std::fs::write(&p, t).unwrap();
        let mut f = Fgd::default();
        f.charger(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        f
    }

    #[test]
    fn classe_complete() {
        let f = charger_texte(EXEMPLE);
        assert!(f.avertissements.is_empty(), "{:?}", f.avertissements);
        let c = f.classe("exemple").unwrap();
        assert_eq!(c.genre, Genre::Point);
        assert_eq!(c.description, "Une description.");
        assert_eq!(c.couleur(), [0, 0, 200], "couleur héritée de la base");
        assert_eq!(c.taille(), ([-4.0; 3], [4.0; 3]));
        assert_eq!(c.modele(), Some("models/x.mdl"));
        assert_eq!(c.propriete("targetname").unwrap().genre, "target_source", "clé héritée");
        let p = c.propriete("Property_name_1").unwrap();
        assert_eq!((p.libelle.as_str(), p.defaut.as_deref(), p.description.as_str()), ("Example String Name", Some("Example"), "Keyvalue Description"));
        let h = c.propriete("health").unwrap();
        assert!(h.lecture_seule && h.defaut.is_none());
        assert_eq!(c.propriete("skin").unwrap().choix.len(), 2);
        let fl = c.propriete("spawnflags").unwrap();
        assert_eq!(fl.genre, "flags");
        assert!(!fl.choix[0].defaut && fl.choix[1].defaut);
        assert_eq!(c.entrees[0].nom, "DoSomething");
        assert_eq!(c.sorties[0].genre, "void");
        assert_eq!(f.classes(true), vec!["func_exemple"]);
        assert_eq!(f.classes(false), vec!["exemple"]);
    }

    #[test]
    fn fgd_de_garrysmod() {
        let f = Fgd::de_garrysmod();
        if f.fichiers.is_empty() {
            return; // pas de Garry's Mod sur ce PC
        }
        assert!(f.nombre() > 300, "{} classes", f.nombre());
        let l = f.classe("light").unwrap();
        assert!(l.propriete("_light").is_some());
        let p = f.classe("prop_static").unwrap();
        assert!(p.propriete("model").is_some() && p.propriete("skin").is_some());
        assert!(f.classe("func_detail").map_or(false, |c| c.genre == Genre::Solide));
        assert!(f.classe("info_player_start").is_some());
        let t = f.classe("trigger_multiple").unwrap();
        assert!(t.sorties.iter().any(|s| s.nom == "OnTrigger"));
        assert!(t.entrees.iter().any(|s| s.nom == "Enable"));
        assert!(f.classe("lua_run").is_some(), "classe propre à GMod");
        eprintln!("{} classes, fichiers {:?}, avertissements {:?}", f.nombre(), f.fichiers, f.avertissements);
    }
}
