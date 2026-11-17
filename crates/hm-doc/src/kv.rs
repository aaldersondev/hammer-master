//! Blocs génériques d'un VMF (`nom { "clé" "valeur" … sous-blocs … }`), lecture et écriture.
//!
//! Le texte d'un VMF n'a pas d'échappement. Il est en UTF-8, ou dans la page de code de Windows quand Hammer l'a
//! écrit sur un poste occidental : un fichier qui n'est pas de l'UTF-8 valide est lu octet pour caractère (Latin-1)
//! et réécrit de même, ce qui rend chaque octet à l'identique.
use serde::{Deserialize, Serialize};

/// Un élément d'un bloc, dans l'ordre du fichier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Element {
    Paire(String, String),
    Bloc(Bloc),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bloc {
    pub nom: String,
    pub elements: Vec<Element>,
}

impl Bloc {
    pub fn nouveau(nom: &str) -> Bloc {
        Bloc { nom: nom.to_string(), elements: Vec::new() }
    }

    pub fn get(&self, cle: &str) -> Option<&str> {
        self.elements.iter().find_map(|e| match e {
            Element::Paire(k, v) if k.eq_ignore_ascii_case(cle) => Some(v.as_str()),
            _ => None,
        })
    }

    /// Remplace la première valeur de la clé, ou ajoute la paire à la fin des paires.
    pub fn set(&mut self, cle: &str, valeur: &str) {
        for e in &mut self.elements {
            if let Element::Paire(k, v) = e {
                if k.eq_ignore_ascii_case(cle) {
                    *v = valeur.to_string();
                    return;
                }
            }
        }
        let fin = self.elements.iter().rposition(|e| matches!(e, Element::Paire(..))).map_or(0, |i| i + 1);
        self.elements.insert(fin, Element::Paire(cle.to_string(), valeur.to_string()));
    }

    pub fn retirer(&mut self, cle: &str) {
        self.elements.retain(|e| !matches!(e, Element::Paire(k, _) if k.eq_ignore_ascii_case(cle)));
    }

    pub fn paires(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.elements.iter().filter_map(|e| match e {
            Element::Paire(k, v) => Some((k.as_str(), v.as_str())),
            _ => None,
        })
    }

    pub fn enfants(&self) -> impl Iterator<Item = &Bloc> + '_ {
        self.elements.iter().filter_map(|e| match e {
            Element::Bloc(b) => Some(b),
            _ => None,
        })
    }

    pub fn enfant(&self, nom: &str) -> Option<&Bloc> {
        self.enfants().find(|b| b.nom.eq_ignore_ascii_case(nom))
    }
}

/// Façon d'écrire un fichier : fin de ligne et codage, relevés à la lecture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Format {
    pub crlf: bool,
    /// Le fichier n'était pas de l'UTF-8 valide : un caractère = un octet.
    pub latin1: bool,
}

impl Default for Format {
    /// Celui de Hammer : CRLF.
    fn default() -> Self {
        Format { crlf: true, latin1: false }
    }
}

#[derive(Debug)]
pub struct ErreurSyntaxe {
    pub ligne: usize,
    pub quoi: &'static str,
}

impl std::fmt::Display for ErreurSyntaxe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VMF invalide ligne {} : {}", self.ligne, self.quoi)
    }
}
impl std::error::Error for ErreurSyntaxe {}

#[derive(Clone, Copy, PartialEq)]
enum Jeton<'a> {
    Mot(&'a [u8]),
    Ouvre,
    Ferme,
}

/// Lit tout un fichier en une suite de blocs de premier niveau.
pub fn lire(src: &[u8]) -> Result<(Vec<Bloc>, Format), ErreurSyntaxe> {
    let latin1 = std::str::from_utf8(src).is_err();
    let crlf = match src.iter().position(|&b| b == b'\n') {
        Some(i) => i > 0 && src[i - 1] == b'\r',
        None => true,
    };
    let texte = |b: &[u8]| -> String {
        if latin1 {
            b.iter().map(|&c| c as char).collect()
        } else {
            // valide : vérifié sur le fichier entier, et un jeton ne coupe jamais un caractère
            String::from_utf8_lossy(b).into_owned()
        }
    };
    // jetons, avec la ligne de chacun pour les erreurs
    let mut jetons: Vec<(Jeton, usize)> = Vec::with_capacity(src.len() / 10);
    let (n, mut i, mut ligne) = (src.len(), 0usize, 1usize);
    while i < n {
        match src[i] {
            b'\n' => {
                ligne += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b'/' if i + 1 < n && src[i + 1] == b'/' => {
                while i < n && src[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => {
                jetons.push((Jeton::Ouvre, ligne));
                i += 1;
            }
            b'}' => {
                jetons.push((Jeton::Ferme, ligne));
                i += 1;
            }
            b'"' => {
                let debut = i + 1;
                let l0 = ligne;
                i += 1;
                while i < n && src[i] != b'"' {
                    if src[i] == b'\n' {
                        ligne += 1;
                    }
                    i += 1;
                }
                if i >= n {
                    return Err(ErreurSyntaxe { ligne: l0, quoi: hm_langue::t("guillemet non fermé") });
                }
                jetons.push((Jeton::Mot(&src[debut..i]), l0));
                i += 1;
            }
            _ => {
                let debut = i;
                while i < n && !matches!(src[i], b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}' | b'"') {
                    i += 1;
                }
                jetons.push((Jeton::Mot(&src[debut..i]), ligne));
            }
        }
    }
    // arbre, sans récursion (une map peut imbriquer profondément ses groupes de visibilité)
    let mut pile: Vec<Bloc> = vec![Bloc::default()];
    let mut k = 0;
    while k < jetons.len() {
        let (j, l) = jetons[k];
        match j {
            Jeton::Ferme => {
                if pile.len() == 1 {
                    return Err(ErreurSyntaxe { ligne: l, quoi: hm_langue::t("accolade fermante en trop") });
                }
                let b = pile.pop().unwrap();
                pile.last_mut().unwrap().elements.push(Element::Bloc(b));
                k += 1;
            }
            Jeton::Ouvre => return Err(ErreurSyntaxe { ligne: l, quoi: hm_langue::t("accolade sans nom de bloc") }),
            Jeton::Mot(m) => match jetons.get(k + 1).map(|x| x.0) {
                Some(Jeton::Ouvre) => {
                    pile.push(Bloc { nom: texte(m), elements: Vec::new() });
                    k += 2;
                }
                Some(Jeton::Mot(v)) => {
                    pile.last_mut().unwrap().elements.push(Element::Paire(texte(m), texte(v)));
                    k += 2;
                }
                Some(Jeton::Ferme) => return Err(ErreurSyntaxe { ligne: l, quoi: hm_langue::t("clé sans valeur") }),
                None => return Err(ErreurSyntaxe { ligne: l, quoi: hm_langue::t("clé sans valeur en fin de fichier") }),
            },
        }
    }
    if pile.len() != 1 {
        return Err(ErreurSyntaxe { ligne, quoi: hm_langue::t("accolade fermante manquante") });
    }
    let racine = pile.pop().unwrap();
    let mut blocs = Vec::new();
    for e in racine.elements {
        match e {
            Element::Bloc(b) => blocs.push(b),
            Element::Paire(..) => {} // une paire hors de tout bloc : Hammer l'ignore aussi
        }
    }
    Ok((blocs, Format { crlf, latin1 }))
}

/// Écrit comme Hammer : nom du bloc seul sur sa ligne, accolades seules, paires `"clé" "valeur"`, une tabulation
/// par niveau.
pub struct Ecrivain {
    pub out: Vec<u8>,
    format: Format,
}

impl Ecrivain {
    pub fn new(format: Format) -> Ecrivain {
        Ecrivain { out: Vec::with_capacity(1 << 16), format }
    }

    fn texte(&mut self, s: &str) {
        if self.format.latin1 {
            self.out.extend(s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }));
        } else {
            self.out.extend_from_slice(s.as_bytes());
        }
    }

    fn fin(&mut self) {
        if self.format.crlf {
            self.out.extend_from_slice(b"\r\n");
        } else {
            self.out.push(b'\n');
        }
    }

    fn retrait(&mut self, n: usize) {
        self.out.extend(std::iter::repeat(b'\t').take(n));
    }

    pub fn ouvrir(&mut self, nom: &str, niveau: usize) {
        self.retrait(niveau);
        self.texte(nom);
        self.fin();
        self.retrait(niveau);
        self.out.push(b'{');
        self.fin();
    }

    pub fn fermer(&mut self, niveau: usize) {
        self.retrait(niveau);
        self.out.push(b'}');
        self.fin();
    }

    pub fn paire(&mut self, cle: &str, valeur: &str, niveau: usize) {
        self.retrait(niveau);
        self.out.push(b'"');
        self.texte(cle);
        self.out.extend_from_slice(b"\" \"");
        self.texte(valeur);
        self.out.push(b'"');
        self.fin();
    }

    pub fn bloc(&mut self, b: &Bloc, niveau: usize) {
        self.ouvrir(&b.nom, niveau);
        for e in &b.elements {
            match e {
                Element::Paire(k, v) => self.paire(k, v, niveau + 1),
                Element::Bloc(c) => self.bloc(c, niveau + 1),
            }
        }
        self.fermer(niveau);
    }
}

/// Un nombre comme Hammer++ l'écrit : entier quand il l'est, sinon au plus six décimales, sans zéros inutiles.
pub fn nombre(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let r = v.round();
    if (v - r).abs() < 1e-9 {
        let r = if r == 0.0 { 0.0 } else { r };
        return format!("{}", r as i64);
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// Nombres d'une chaîne, les parenthèses et crochets pris pour des blancs.
pub fn nombres(s: &str) -> Vec<f64> {
    s.split(|c: char| c.is_ascii_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '{' | '}')).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_comme_hammer() {
        assert_eq!(nombre(64.0), "64");
        assert_eq!(nombre(-0.0), "0");
        assert_eq!(nombre(0.25), "0.25");
        assert_eq!(nombre(-1.5), "-1.5");
        assert_eq!(nombre(1.0 / 3.0), "0.333333");
        assert_eq!(nombre(-0.0000001), "0");
        assert_eq!(nombres("[1 0 0 -16] 0.25"), vec![1.0, 0.0, 0.0, -16.0, 0.25]);
    }

    #[test]
    fn aller_retour() {
        let src = b"versioninfo\r\n{\r\n\t\"editorversion\" \"400\"\r\n}\r\nworld\r\n{\r\n\t\"id\" \"1\"\r\n\tsolid\r\n\t{\r\n\t\t\"id\" \"2\"\r\n\t}\r\n}\r\n";
        let (blocs, f) = lire(src).unwrap();
        assert!(f.crlf && !f.latin1);
        let mut e = Ecrivain::new(f);
        for b in &blocs {
            e.bloc(b, 0);
        }
        assert_eq!(e.out, src);
    }

    #[test]
    fn latin1_garde_les_octets() {
        let mut src = b"entity\n{\n\t\"message\" \"caf".to_vec();
        src.push(0xE9);
        src.extend_from_slice(b"\"\n}\n");
        let (blocs, f) = lire(&src).unwrap();
        assert!(f.latin1 && !f.crlf);
        assert_eq!(blocs[0].get("message"), Some("café"));
        let mut e = Ecrivain::new(f);
        e.bloc(&blocs[0], 0);
        assert_eq!(e.out, src);
    }

    #[test]
    fn erreurs() {
        assert!(lire(b"world { \"a\" \"b\"").is_err());
        assert!(lire(b"}").is_err());
        assert!(lire(b"world { \"a }").is_err());
        assert!(lire(b"world { \"a\" }").is_err());
    }
}
