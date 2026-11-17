//! Système de fichiers du jeu : une liste ordonnée de dossiers et d'archives VPK, comme les `SearchPaths` d'un
//! `gameinfo.txt`. Le premier point de montage qui contient le fichier gagne.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use vformats::Limits;

enum Kind {
    Dir(PathBuf),
    Vpk(source_vpk::Vpk),
    /// Quelques fichiers d'un dossier, et rien d'autre (chemins relatifs normalisés).
    Fichiers(PathBuf, std::collections::HashSet<String>),
    /// Contenu embarqué d'un `.bsp` (son « pakfile »).
    Pak { zip: vformats::bsp::ZipReader<'static>, index: std::collections::HashMap<String, usize> },
}

pub struct Mount {
    /// Nom affiché dans les rapports (chemin du dossier ou du `_dir.vpk`).
    pub label: String,
    kind: Kind,
}

/// Un fichier trouvé et l'indice du point de montage qui l'a fourni.
pub struct Found {
    pub bytes: Vec<u8>,
    pub mount: usize,
}

#[derive(Default)]
pub struct Vfs {
    mounts: Vec<Mount>,
}

fn norm(rel: &str) -> String {
    let mut s = rel.replace('\\', "/");
    while s.starts_with('/') {
        s.remove(0);
    }
    s.make_ascii_lowercase();
    s
}

impl Vfs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mounts(&self) -> &[Mount] {
        &self.mounts
    }

    pub fn add_dir(&mut self, path: impl AsRef<Path>) {
        let p = path.as_ref().to_path_buf();
        if p.is_dir() && !self.mounts.iter().any(|m| matches!(&m.kind, Kind::Dir(d) if *d == p)) {
            self.mounts.push(Mount { label: p.display().to_string(), kind: Kind::Dir(p) });
        }
    }

    /// Monte seulement ces fichiers d'un dossier (ceux qui existent), en dernier : ce qu'un outil apporte pour lui, comme
    /// le projecteur de Hammer++, sans le reste de son dossier.
    pub fn add_fichiers(&mut self, dossier: impl AsRef<Path>, noms: &[&str]) {
        let d = dossier.as_ref().to_path_buf();
        let noms: std::collections::HashSet<String> = noms.iter().map(|n| norm(n)).filter(|n| d.join(n).is_file()).collect();
        if !noms.is_empty() {
            self.mounts.push(Mount { label: d.display().to_string(), kind: Kind::Fichiers(d, noms) });
        }
    }

    /// Ajoute une archive. `chose.vpk` désigne `chose_dir.vpk` quand ce fichier existe (convention du moteur).
    pub fn add_vpk(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let mut p = path.as_ref().to_path_buf();
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if !stem.ends_with("_dir") {
            let dir = p.with_file_name(format!("{stem}_dir.vpk"));
            if dir.is_file() {
                p = dir;
            }
        }
        if !p.is_file() {
            return Err(anyhow!("archive introuvable : {}", p.display()));
        }
        let vpk = source_vpk::Vpk::open(&p).map_err(|e| anyhow!("{} : {e}", p.display()))?;
        self.mounts.push(Mount { label: p.display().to_string(), kind: Kind::Vpk(vpk) });
        Ok(())
    }

    /// Monte le contenu embarqué d'une map compilée : les fichiers exacts avec lesquels elle a été faite. Utile
    /// pour revoir un ancien VMF avec ses modèles et textures d'alors.
    ///
    /// Le fichier reste en mémoire jusqu'à la fin du processus.
    pub fn add_bsp_pak(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let bytes: &'static [u8] = Box::leak(std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?.into_boxed_slice());
        let limits = Limits { max_input_bytes: 4 << 30, ..Limits::default() };
        let bsp = vformats::bsp::parse(bytes, &limits).map_err(|e| anyhow!("{} : {e}", path.display()))?;
        let zip = bsp.pakfile().map_err(|e| anyhow!("{} : contenu embarqué illisible : {e}", path.display()))?;
        let index = zip.entries().iter().enumerate().map(|(i, e)| (norm(&e.path), i)).collect();
        self.mounts.push(Mount { label: format!("{} (contenu embarqué)", path.display()), kind: Kind::Pak { zip, index } });
        Ok(())
    }

    /// Monte les `SearchPaths` d'un `gameinfo.txt`, dans l'ordre du fichier.
    pub fn add_gameinfo(&mut self, gameinfo: impl AsRef<Path>) -> Result<()> {
        let gameinfo = gameinfo.as_ref();
        let text = std::fs::read(gameinfo).with_context(|| format!("lecture de {}", gameinfo.display()))?;
        let text = String::from_utf8_lossy(&text);
        let doc = vformats::keyvalues::parse(&text, &Limits::default()).map_err(|e| anyhow!("{} : {e}", gameinfo.display()))?;
        let paths = doc
            .get("GameInfo")
            .and_then(|v| v.as_block())
            .and_then(|b| b.get("FileSystem"))
            .and_then(|v| v.as_block())
            .and_then(|b| b.get("SearchPaths"))
            .and_then(|v| v.as_block())
            .ok_or_else(|| anyhow!("{} : pas de bloc GameInfo/FileSystem/SearchPaths", gameinfo.display()))?;
        let here = gameinfo.parent().unwrap_or(Path::new("."));
        let root = here.parent().unwrap_or(here);
        for pair in &paths.pairs {
            let Some(raw) = pair.value.as_str() else { continue };
            let p = if let Some(r) = raw.strip_prefix("|gameinfo_path|") {
                here.join(r)
            } else if let Some(r) = raw.strip_prefix("|all_source_engine_paths|") {
                root.join(r)
            } else if Path::new(raw).is_absolute() {
                PathBuf::from(raw)
            } else {
                root.join(raw)
            };
            let s = p.to_string_lossy().to_string();
            if s.to_ascii_lowercase().ends_with(".vpk") {
                let _ = self.add_vpk(&p); // une archive absente (jeu non installé) n'est pas une erreur
            } else if let Some(parent) = s.strip_suffix("/*").or_else(|| s.strip_suffix("\\*")) {
                if let Ok(rd) = std::fs::read_dir(parent) {
                    let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
                    subs.sort();
                    for d in subs {
                        self.add_dir(d);
                    }
                }
            } else {
                self.add_dir(&p);
            }
        }
        // Garry's Mod : les jeux qu'il monte lui-même (son gameinfo renvoie à cfg/mount.cfg), après son contenu
        if here.join("cfg/mountdepots.txt").is_file() {
            for p in crate::jeu::jeux_montes(root, &crate::jeu::dossiers_communs(root)) {
                if p.is_dir() {
                    self.add_dir(&p);
                } else {
                    let _ = self.add_vpk(&p);
                }
            }
        }
        Ok(())
    }

    /// Lit un fichier par son chemin relatif (`materials/x/y.vmt`), sans tenir compte de la casse.
    pub fn read(&self, rel: &str) -> Option<Found> {
        let rel = norm(rel);
        for (i, m) in self.mounts.iter().enumerate() {
            match &m.kind {
                Kind::Dir(d) => {
                    if let Ok(bytes) = std::fs::read(d.join(&rel)) {
                        return Some(Found { bytes, mount: i });
                    }
                }
                Kind::Fichiers(d, noms) => {
                    if noms.contains(&rel) {
                        if let Ok(bytes) = std::fs::read(d.join(&rel)) {
                            return Some(Found { bytes, mount: i });
                        }
                    }
                }
                Kind::Vpk(v) => {
                    if v.contains(&rel) {
                        if let Ok(bytes) = v.read(&rel) {
                            return Some(Found { bytes, mount: i });
                        }
                    }
                }
                Kind::Pak { zip, index } => {
                    if let Some(&k) = index.get(&rel) {
                        if let Ok(bytes) = zip.entry_bytes(&zip.entries()[k], &Limits::default()) {
                            return Some(Found { bytes: bytes.into_owned(), mount: i });
                        }
                    }
                }
            }
        }
        None
    }

    /// Tous les fichiers d'une extension (`.mdl`), avec l'indice de leur point de montage. Un fichier présent dans
    /// plusieurs points de montage n'est donné qu'une fois, pour le premier.
    pub fn list(&self, ext: &str) -> Vec<(String, usize)> {
        fn walk(root: &Path, dir: &Path, ext: &str, out: &mut Vec<String>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, ext, out);
                } else if let Ok(rel) = p.strip_prefix(root) {
                    let rel = norm(&rel.to_string_lossy());
                    if rel.ends_with(ext) {
                        out.push(rel);
                    }
                }
            }
        }
        let ext = ext.to_ascii_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (i, m) in self.mounts.iter().enumerate() {
            let mut names = Vec::new();
            match &m.kind {
                Kind::Dir(d) => walk(d, d, &ext, &mut names),
                Kind::Fichiers(_, noms) => names.extend(noms.iter().filter(|p| p.ends_with(&ext)).cloned()),
                Kind::Vpk(v) => names.extend(v.entries().map(|(p, _)| p.to_string()).filter(|p| p.ends_with(&ext))),
                Kind::Pak { index, .. } => names.extend(index.keys().filter(|p| p.ends_with(&ext)).cloned()),
            }
            for n in names {
                if seen.insert(n.clone()) {
                    out.push((n, i));
                }
            }
        }
        out
    }

    /// Comme [`Vfs::list`], seulement sous un dossier (`materials/`) : ne parcourt pas le reste des dossiers montés.
    pub fn list_dans(&self, dossier: &str, ext: &str) -> Vec<(String, usize)> {
        fn walk(root: &Path, dir: &Path, ext: &str, out: &mut Vec<String>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, ext, out);
                } else if let Ok(rel) = p.strip_prefix(root) {
                    let rel = norm(&rel.to_string_lossy());
                    if rel.ends_with(ext) {
                        out.push(rel);
                    }
                }
            }
        }
        let (ext, dossier) = (ext.to_ascii_lowercase(), norm(dossier));
        let prefixe = format!("{}/", dossier.trim_end_matches('/'));
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (i, m) in self.mounts.iter().enumerate() {
            let mut names = Vec::new();
            match &m.kind {
                Kind::Dir(d) => walk(d, &d.join(&dossier), &ext, &mut names),
                Kind::Fichiers(_, noms) => names.extend(noms.iter().filter(|p| p.starts_with(&prefixe) && p.ends_with(&ext)).cloned()),
                Kind::Vpk(v) => names.extend(v.entries().map(|(p, _)| p.to_string()).filter(|p| p.starts_with(&prefixe) && p.ends_with(&ext))),
                Kind::Pak { index, .. } => names.extend(index.keys().filter(|p| p.starts_with(&prefixe) && p.ends_with(&ext)).cloned()),
            }
            for n in names {
                if seen.insert(n.clone()) {
                    out.push((n, i));
                }
            }
        }
        out
    }

    /// Indice du premier point de montage qui contient le fichier.
    pub fn locate(&self, rel: &str) -> Option<usize> {
        let rel = norm(rel);
        self.mounts.iter().position(|m| match &m.kind {
            Kind::Dir(d) => d.join(&rel).is_file(),
            Kind::Fichiers(_, noms) => noms.contains(&rel),
            Kind::Vpk(v) => v.contains(&rel),
            Kind::Pak { index, .. } => index.contains_key(&rel),
        })
    }
}
