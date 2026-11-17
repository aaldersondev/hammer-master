//! Lecture et écriture **sans perte** d'un fichier VMF (Valve Map Format).
//!
//! Le fichier est gardé en octets. L'analyse produit des jetons (mot, chaîne entre guillemets, accolades) et un
//! arbre de blocs qui renvoie à ces jetons. L'écriture repart de l'arbre et recopie, pour chaque jeton, les blancs
//! et commentaires qui le précèdent : un fichier non modifié ressort **octet pour octet** ; un fichier modifié ne
//! diffère que par les jetons remplacés.
//!
//! Le texte n'est jamais supposé être de l'UTF-8 : les valeurs sont décodées avec tolérance pour la lecture, et
//! les octets d'origine servent à l'écriture.
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// Erreur de syntaxe, avec le numéro de ligne (à partir de 1).
    Syntax { line: usize, what: &'static str },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "lecture impossible : {e}"),
            Error::Syntax { line, what } => write!(f, "VMF invalide ligne {line} : {what}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Word,
    Quoted,
    Open,
    Close,
}

#[derive(Clone, Copy, Debug)]
struct Tok {
    start: u32,
    end: u32,
    kind: Kind,
}

/// Ce que contient un bloc, dans l'ordre du fichier.
#[derive(Clone, Copy, Debug)]
enum Item {
    /// Indice dans `Node::pairs`.
    Pair(u32),
    /// Indice dans `Node::children`.
    Child(u32),
}

/// Une paire `"clé" "valeur"`.
#[derive(Clone, Debug)]
pub struct Pair {
    pub key: String,
    pub value: String,
    key_tok: u32,
    value_tok: u32,
}

impl Pair {
    /// Identifiant du jeton de la valeur, à passer à [`Vmf::set_value`].
    pub fn value_token(&self) -> u32 {
        self.value_tok
    }
}

/// Un bloc `nom { ... }` : ses paires et ses sous-blocs.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub pairs: Vec<Pair>,
    pub children: Vec<Node>,
    items: Vec<Item>,
    name_tok: u32,
    open_tok: u32,
    close_tok: u32,
}

impl Node {
    /// Première valeur de la clé (sans tenir compte de la casse).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.iter().find(|p| p.key.eq_ignore_ascii_case(key)).map(|p| p.value.as_str())
    }

    /// Toutes les valeurs d'une clé répétée.
    pub fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.pairs.iter().filter(move |p| p.key.eq_ignore_ascii_case(key)).map(|p| p.value.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    pub fn id(&self) -> Option<u64> {
        self.get("id").and_then(|v| v.trim().parse().ok())
    }

    pub fn f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.trim().parse().ok())
    }

    pub fn vec3(&self, key: &str) -> Option<[f64; 3]> {
        self.get(key).and_then(parse_vec3)
    }

    /// Les solides du bloc, y compris ceux rangés dans un sous-bloc `hidden`.
    pub fn solids(&self) -> impl Iterator<Item = &Node> + '_ {
        self.children_named("solid").chain(self.children_named("hidden").flat_map(|h| h.children_named("solid")))
    }
}

/// Un fichier VMF chargé.
pub struct Vmf {
    src: Vec<u8>,
    toks: Vec<Tok>,
    /// Bloc sans nom qui contient tout le fichier.
    pub root: Node,
    overrides: HashMap<u32, Vec<u8>>,
}

impl Vmf {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::parse(std::fs::read(path)?)
    }

    pub fn parse(src: Vec<u8>) -> Result<Self, Error> {
        let toks = tokenize(&src)?;
        let mut pos = 0usize;
        let mut root = Node { name_tok: u32::MAX, open_tok: u32::MAX, close_tok: u32::MAX, ..Default::default() };
        parse_items(&src, &toks, &mut pos, &mut root, true)?;
        Ok(Self { src, toks, root, overrides: HashMap::new() })
    }

    /// Octets du fichier d'origine.
    pub fn source(&self) -> &[u8] {
        &self.src
    }

    pub fn world(&self) -> Option<&Node> {
        self.root.child("world")
    }

    /// Les entités, y compris celles rangées dans un bloc `hidden` de premier niveau.
    pub fn entities(&self) -> impl Iterator<Item = &Node> + '_ {
        self.root
            .children_named("entity")
            .chain(self.root.children_named("hidden").flat_map(|h| h.children_named("entity")))
    }

    /// Étendue en octets d'un bloc dans la source : du début de son nom à la fin de son accolade fermante.
    pub fn etendue(&self, n: &Node) -> (usize, usize) {
        (self.toks[n.name_tok as usize].start as usize, self.toks[n.close_tok as usize].end as usize)
    }

    /// Remplace le texte d'une valeur (sans les guillemets). Seul ce jeton changera à l'écriture.
    pub fn set_value(&mut self, value_token: u32, text: &str) {
        let mut b = Vec::with_capacity(text.len() + 2);
        b.push(b'"');
        b.extend_from_slice(text.as_bytes());
        b.push(b'"');
        self.overrides.insert(value_token, b);
    }

    /// Réécrit le fichier à partir de l'arbre. Sans modification, le résultat est identique à la source.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.src.len() + 64);
        let mut cursor = 0usize; // fin du dernier jeton écrit, dans la source
        self.emit(&self.root, &mut out, &mut cursor);
        out.extend_from_slice(&self.src[cursor..]);
        out
    }

    fn emit_tok(&self, i: u32, out: &mut Vec<u8>, cursor: &mut usize) {
        let t = self.toks[i as usize];
        out.extend_from_slice(&self.src[*cursor..t.start as usize]);
        match self.overrides.get(&i) {
            Some(b) => out.extend_from_slice(b),
            None => out.extend_from_slice(&self.src[t.start as usize..t.end as usize]),
        }
        *cursor = t.end as usize;
    }

    fn emit(&self, n: &Node, out: &mut Vec<u8>, cursor: &mut usize) {
        if n.name_tok != u32::MAX {
            self.emit_tok(n.name_tok, out, cursor);
            self.emit_tok(n.open_tok, out, cursor);
        }
        for it in &n.items {
            match *it {
                Item::Pair(i) => {
                    let p = &n.pairs[i as usize];
                    self.emit_tok(p.key_tok, out, cursor);
                    self.emit_tok(p.value_tok, out, cursor);
                }
                Item::Child(i) => self.emit(&n.children[i as usize], out, cursor),
            }
        }
        if n.close_tok != u32::MAX {
            self.emit_tok(n.close_tok, out, cursor);
        }
    }
}

fn line_of(src: &[u8], at: usize) -> usize {
    1 + src[..at.min(src.len())].iter().filter(|&&b| b == b'\n').count()
}

fn tokenize(src: &[u8]) -> Result<Vec<Tok>, Error> {
    let mut toks = Vec::with_capacity(src.len() / 12);
    let n = src.len();
    let mut i = 0usize;
    while i < n {
        let b = src[i];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if i + 1 < n && src[i + 1] == b'/' => {
                while i < n && src[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => {
                toks.push(Tok { start: i as u32, end: i as u32 + 1, kind: Kind::Open });
                i += 1;
            }
            b'}' => {
                toks.push(Tok { start: i as u32, end: i as u32 + 1, kind: Kind::Close });
                i += 1;
            }
            b'"' => {
                let s = i;
                i += 1;
                while i < n && src[i] != b'"' {
                    i += 1;
                }
                if i >= n {
                    return Err(Error::Syntax { line: line_of(src, s), what: "guillemet non fermé" });
                }
                i += 1;
                toks.push(Tok { start: s as u32, end: i as u32, kind: Kind::Quoted });
            }
            _ => {
                let s = i;
                while i < n && !matches!(src[i], b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}' | b'"') {
                    i += 1;
                }
                toks.push(Tok { start: s as u32, end: i as u32, kind: Kind::Word });
            }
        }
    }
    Ok(toks)
}

fn text(src: &[u8], t: Tok) -> String {
    let (a, b) = match t.kind {
        Kind::Quoted => (t.start as usize + 1, t.end as usize - 1),
        _ => (t.start as usize, t.end as usize),
    };
    match std::str::from_utf8(&src[a..b]) {
        Ok(s) => s.to_owned(),
        Err(_) => String::from_utf8_lossy(&src[a..b]).into_owned(),
    }
}

fn parse_items(src: &[u8], toks: &[Tok], pos: &mut usize, node: &mut Node, top: bool) -> Result<(), Error> {
    loop {
        let Some(&t) = toks.get(*pos) else {
            if top {
                return Ok(());
            }
            return Err(Error::Syntax { line: line_of(src, src.len()), what: "accolade fermante manquante" });
        };
        match t.kind {
            Kind::Close => {
                if top {
                    return Err(Error::Syntax { line: line_of(src, t.start as usize), what: "accolade fermante en trop" });
                }
                node.close_tok = *pos as u32;
                *pos += 1;
                return Ok(());
            }
            Kind::Open => return Err(Error::Syntax { line: line_of(src, t.start as usize), what: "accolade sans nom de bloc" }),
            Kind::Word | Kind::Quoted => {
                let Some(&next) = toks.get(*pos + 1) else {
                    return Err(Error::Syntax { line: line_of(src, t.start as usize), what: "clé sans valeur en fin de fichier" });
                };
                match next.kind {
                    Kind::Open => {
                        let mut child = Node { name: text(src, t), name_tok: *pos as u32, open_tok: *pos as u32 + 1, close_tok: u32::MAX, ..Default::default() };
                        *pos += 2;
                        parse_items(src, toks, pos, &mut child, false)?;
                        node.items.push(Item::Child(node.children.len() as u32));
                        node.children.push(child);
                    }
                    Kind::Word | Kind::Quoted => {
                        node.items.push(Item::Pair(node.pairs.len() as u32));
                        node.pairs.push(Pair { key: text(src, t), value: text(src, next), key_tok: *pos as u32, value_tok: *pos as u32 + 1 });
                        *pos += 2;
                    }
                    Kind::Close => return Err(Error::Syntax { line: line_of(src, t.start as usize), what: "clé sans valeur" }),
                }
            }
        }
    }
}

// ------------------------------------------------------------------ valeurs typées

/// Nombres d'une chaîne, les parenthèses et crochets étant pris pour des blancs.
pub fn parse_floats(s: &str, out: &mut Vec<f64>) {
    out.clear();
    for part in s.split(|c: char| c.is_ascii_whitespace() || matches!(c, '(' | ')' | '[' | ']')) {
        if !part.is_empty() {
            if let Ok(v) = part.parse::<f64>() {
                out.push(v);
            }
        }
    }
}

pub fn parse_vec3(s: &str) -> Option<[f64; 3]> {
    let mut it = s.split(|c: char| c.is_ascii_whitespace() || matches!(c, '(' | ')' | '[' | ']')).filter(|p| !p.is_empty());
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    Some([a, b, c])
}

/// `"(x y z) (x y z) (x y z)"` : les trois points d'un plan, en sens horaire vus de l'extérieur du solide.
pub fn parse_plane(s: &str) -> Option<[[f64; 3]; 3]> {
    let mut v = Vec::with_capacity(9);
    parse_floats(s, &mut v);
    (v.len() == 9).then(|| [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]])
}

/// Axe de texture `"[x y z décalage] échelle"`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexAxis {
    pub dir: [f64; 3],
    pub shift: f64,
    pub scale: f64,
}

pub fn parse_axis(s: &str) -> Option<TexAxis> {
    let mut v = Vec::with_capacity(5);
    parse_floats(s, &mut v);
    (v.len() == 5).then(|| TexAxis { dir: [v[0], v[1], v[2]], shift: v[3], scale: v[4] })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PETIT: &str = "versioninfo\r\n{\r\n\t\"editorversion\" \"400\"\r\n}\r\n// commentaire\r\nworld\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n\tsolid\n\t{\n\t\t\"id\" \"2\"\n\t\tside\n\t\t{\n\t\t\t\"plane\" \"(0 0 0) (0 64 0) (64 64 0)\"\n\t\t\t\"uaxis\" \"[1 0 0 16] 0.25\"\n\t\t}\n\t}\n}\nentity\n{\n\t\"classname\" \"light\"\n\t\"origin\" \"1 2 3\"\n\tconnections\n\t{\n\t\t\"OnUser1\" \"a,b,,0,-1\"\n\t\t\"OnUser1\" \"c,d,,0,-1\"\n\t}\n}\n  \n";

    #[test]
    fn aller_retour_identique() {
        let v = Vmf::parse(PETIT.as_bytes().to_vec()).unwrap();
        assert_eq!(v.to_bytes(), PETIT.as_bytes());
    }

    #[test]
    fn lecture_typee() {
        let v = Vmf::parse(PETIT.as_bytes().to_vec()).unwrap();
        let w = v.world().unwrap();
        assert_eq!(w.classname(), "worldspawn");
        let side = w.solids().next().unwrap().child("side").unwrap();
        assert_eq!(parse_plane(side.get("plane").unwrap()).unwrap()[2], [64.0, 64.0, 0.0]);
        let ax = parse_axis(side.get("uaxis").unwrap()).unwrap();
        assert_eq!((ax.shift, ax.scale), (16.0, 0.25));
        let e = v.entities().next().unwrap();
        assert_eq!(e.vec3("origin"), Some([1.0, 2.0, 3.0]));
        assert_eq!(e.child("connections").unwrap().get_all("OnUser1").count(), 2);
    }

    #[test]
    fn modification_locale() {
        let mut v = Vmf::parse(PETIT.as_bytes().to_vec()).unwrap();
        let tok = v.entities().next().unwrap().pairs.iter().find(|p| p.key == "origin").unwrap().value_token();
        v.set_value(tok, "10 20 30");
        let out = String::from_utf8(v.to_bytes()).unwrap();
        assert_eq!(out, PETIT.replace("\"1 2 3\"", "\"10 20 30\""));
    }

    #[test]
    fn octets_non_utf8_conserves() {
        let mut b = b"entity\n{\n\t\"message\" \"caf".to_vec();
        b.push(0xE9); // é en Windows-1252
        b.extend_from_slice(b"\"\n}\n");
        let v = Vmf::parse(b.clone()).unwrap();
        assert_eq!(v.to_bytes(), b);
    }

    #[test]
    fn erreurs() {
        assert!(Vmf::parse(b"world { \"a\" \"b\"".to_vec()).is_err());
        assert!(Vmf::parse(b"}".to_vec()).is_err());
        assert!(Vmf::parse(b"world { \"a }".to_vec()).is_err());
    }
}

// ------------------------------------------------------------------------------------------------ révision

/// Préfixe du nom de l'entité qui porte la révision dans une map estampillée : `hm_rev_<révision>`.
pub const PREFIXE_REVISION: &str = "hm_rev_";

/// Étendue en octets du bloc `entity { … }` qui porte la révision, s'il existe.
fn bloc_revision(src: &[u8]) -> Option<(usize, usize)> {
    let motif = format!("\"targetname\" \"{PREFIXE_REVISION}");
    let p = src.windows(motif.len()).position(|w| w == motif.as_bytes())?;
    // début : la ligne `entity` qui précède ; fin : la première accolade fermante, avec sa fin de ligne
    let debut = src[..p].windows(6).rposition(|w| w == b"entity")?;
    let fin = p + src[p..].iter().position(|&c| c == b'}')? + 1;
    let fin = fin + src[fin..].iter().take_while(|&&c| c == b'\r' || c == b'\n').count();
    Some((debut, fin))
}

/// Révision inscrite dans la map par `estampiller`, s'il y en a une.
pub fn revision_inscrite(src: &[u8]) -> Option<String> {
    let (a, b) = bloc_revision(src)?;
    let bloc = std::str::from_utf8(&src[a..b]).ok()?;
    let i = bloc.find(PREFIXE_REVISION)? + PREFIXE_REVISION.len();
    Some(bloc[i..].chars().take_while(|c| c.is_ascii_hexdigit()).collect())
}

/// Révision d'un VMF : empreinte de son contenu, l'entité de révision mise à part. Une map estampillée garde
/// donc la révision qu'elle avait avant de l'être, et c'est cette révision-là qu'elle porte.
pub fn revision(src: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    match bloc_revision(src) {
        Some((a, b)) => {
            h.update(&src[..a]);
            h.update(&src[b..]);
        }
        None => {
            h.update(src);
        }
    }
    h.finalize().to_hex()[..16].to_string()
}

/// Rend le VMF avec sa révision inscrite dans une entité `info_target` nommée `hm_rev_<révision>`, que l'addon de
/// signalement relit en jeu. L'entité est posée sur un point d'apparition de joueur : une entité hors de la map
/// ferait une fuite. Rend `None` si le VMF n'a aucune entité ponctuelle où la poser.
pub fn estampiller(src: &[u8]) -> Result<Option<Vec<u8>>, Error> {
    let nu = match bloc_revision(src) {
        Some((a, b)) => [&src[..a], &src[b..]].concat(),
        None => src.to_vec(),
    };
    let rev = revision(&nu);
    let vmf = Vmf::parse(nu.clone())?;
    let ponctuelle = |e: &&Node| e.solids().next().is_none() && e.vec3("origin").is_some();
    let Some(appui) = vmf.entities().filter(|e| e.classname().starts_with("info_player")).find(ponctuelle).or_else(|| vmf.entities().find(ponctuelle)) else {
        return Ok(None);
    };
    let origine = appui.get("origin").unwrap_or("0 0 0").to_string();
    let id = vmf.entities().filter_map(|e| e.id()).max().unwrap_or(0) + 1;
    let nl = if nu.windows(2).any(|w| w == b"\r\n") { "\r\n" } else { "\n" };
    let bloc = format!("entity{nl}{{{nl}\t\"id\" \"{id}\"{nl}\t\"classname\" \"info_target\"{nl}\t\"targetname\" \"{PREFIXE_REVISION}{rev}\"{nl}\t\"origin\" \"{origine}\"{nl}}}{nl}");
    // avant le bloc `cameras` s'il existe (là où Hammer range les entités), sinon en fin de fichier
    let motif = format!("{nl}cameras{nl}");
    let ou = nu.windows(motif.len()).rposition(|w| w == motif.as_bytes()).map(|p| p + nl.len()).unwrap_or(nu.len());
    Ok(Some([&nu[..ou], bloc.as_bytes(), &nu[ou..]].concat()))
}

#[cfg(test)]
mod tests_revision {
    use super::*;

    const CARTE: &str = "world\r\n{\r\n\t\"id\" \"1\"\r\n\t\"classname\" \"worldspawn\"\r\n}\r\nentity\r\n{\r\n\t\"id\" \"7\"\r\n\t\"classname\" \"info_player_start\"\r\n\t\"origin\" \"64 -32 8\"\r\n}\r\ncameras\r\n{\r\n\t\"activecamera\" \"-1\"\r\n}\r\n";

    #[test]
    fn estampille_sans_changer_la_revision() {
        let avant = revision(CARTE.as_bytes());
        let est = estampiller(CARTE.as_bytes()).unwrap().unwrap();
        assert_ne!(est, CARTE.as_bytes());
        assert_eq!(revision(&est), avant, "l'entité de révision ne compte pas dans la révision");
        assert_eq!(revision_inscrite(&est).as_deref(), Some(avant.as_str()));
        // l'entité est posée sur le point d'apparition, avec un identifiant neuf, et le fichier reste lisible
        let v = Vmf::parse(est.clone()).unwrap();
        let e = v.entities().find(|e| e.get("targetname").map_or(false, |n| n.starts_with(PREFIXE_REVISION))).unwrap();
        assert_eq!(e.get("origin"), Some("64 -32 8"));
        assert_eq!(e.id(), Some(8));
        // estampiller deux fois ne change plus rien ; retirer l'entité rend le fichier d'origine
        assert_eq!(estampiller(&est).unwrap().unwrap(), est);
        let (a, b) = bloc_revision(&est).unwrap();
        assert_eq!([&est[..a], &est[b..]].concat(), CARTE.as_bytes());
    }

    #[test]
    fn un_changement_change_la_revision_malgre_l_estampille() {
        let est = estampiller(CARTE.as_bytes()).unwrap().unwrap();
        let modifie = String::from_utf8(est).unwrap().replace("info_player_start", "info_player_deathmatch");
        // la map a changé mais porte encore l'ancienne révision : c'est ce que l'outil doit savoir détecter
        assert_ne!(revision(modifie.as_bytes()), revision_inscrite(modifie.as_bytes()).unwrap());
    }

    #[test]
    fn sans_entite_ponctuelle_rien_a_poser() {
        assert!(estampiller(b"world\n{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n}\n").unwrap().is_none());
    }
}
