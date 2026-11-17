//! Textures : un VTF devient soit des blocs compressés envoyables tels quels au GPU, soit du RGBA.
use vformats::vtf::BcFormat;
use vformats::Limits;

use crate::material::normalize;
use crate::vfs::Vfs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// DXT1
    Bc1,
    /// DXT3
    Bc2,
    /// DXT5
    Bc3,
    Rgba8,
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub name: String,
    pub format: PixelFormat,
    /// Taille du premier mip gardé.
    pub width: u32,
    pub height: u32,
    /// Taille de la texture dans le fichier : c'est elle qui fixe l'échelle des coordonnées d'une face de solide.
    pub full_width: u32,
    pub full_height: u32,
    /// Du plus grand au plus petit.
    pub mips: Vec<Vec<u8>>,
    pub mount: usize,
}

/// Une cubemap décodée en RGBA : `faces[f][mip]`, faces dans l'ordre du VTF (+X, -X, +Y, -Y, +Z, -Z, celui des
/// cubemaps de Direct3D, que le moteur échantillonne avec le vecteur du monde tel quel).
pub struct Cubemap {
    pub taille: u32,
    pub faces: Vec<Vec<Vec<u8>>>,
}

/// Charge une cubemap des fichiers du jeu (`engine/defaultcubemap`).
pub fn load_cubemap(vfs: &Vfs, name: &str) -> Option<Cubemap> {
    decoder_cubemap(&vfs.read(&format!("materials/{}.vtf", normalize(name)))?.bytes)
}

/// La cubemap des reflets d'un éditeur sans cubemap compilée : celle que Hammer++ apporte
/// (`hammerplusplus/materials/dev_test/cubemap.vtf`) s'il est installé, sinon `engine/defaultcubemap` du jeu, qui
/// est noire dans Garry's Mod (vu le 15/11). `None` si elle est noire ou introuvable.
pub fn cubemap_editeur(vfs: &Vfs) -> Option<Cubemap> {
    let hpp = crate::jeu::garrysmod().map(|g| g.join("bin/win64/hammerplusplus/materials/dev_test/cubemap.vtf"));
    let c = hpp.and_then(|p| std::fs::read(p).ok()).and_then(|b| decoder_cubemap(&b)).or_else(|| load_cubemap(vfs, "engine/defaultcubemap"))?;
    c.faces.iter().any(|f| f[0].chunks_exact(4).any(|p| p[0] > 8 || p[1] > 8 || p[2] > 8)).then_some(c)
}

/// Six faces carrées d'une cubemap VTF, tous leurs mips d'au moins 1 pixel.
pub fn decoder_cubemap(octets: &[u8]) -> Option<Cubemap> {
    let limits = Limits::default();
    let vtf = vformats::vtf::parse(octets, &limits).ok()?;
    if vtf.face_count() < 6 || vtf.width() != vtf.height() {
        return None;
    }
    let mips = vtf.mip_count().max(1);
    let faces = (0..6)
        .map(|f| (0..mips).map(|m| vtf.decode_rgba(0, f, m, &limits).ok().map(|i| i.rgba)).collect::<Option<Vec<_>>>())
        .collect::<Option<Vec<_>>>()?;
    Some(Cubemap { taille: vtf.width(), faces })
}

/// Taille d'une texture sans la charger (pour l'échelle des coordonnées).
pub fn texture_size(vfs: &Vfs, name: &str) -> Option<(u32, u32)> {
    let found = vfs.read(&format!("materials/{}.vtf", normalize(name)))?;
    let vtf = vformats::vtf::parse(&found.bytes, &Limits::default()).ok()?;
    Some((vtf.width(), vtf.height()))
}

/// Charge une texture. Les mips de plus de `max_size` pixels de côté sont laissés de côté.
pub fn load_texture(vfs: &Vfs, name: &str, max_size: u32) -> Option<Texture> {
    let name = normalize(name);
    let found = vfs.read(&format!("materials/{name}.vtf"))?;
    let limits = Limits::default();
    let vtf = vformats::vtf::parse(&found.bytes, &limits).ok()?;
    let (fw, fh) = (vtf.width(), vtf.height());
    if let Some(raw) = vtf.raw_bc(0, 0) {
        // du plus petit au plus grand dans le fichier : on garde ceux d'au moins 4 pixels et d'au plus max_size
        let mut mips: Vec<_> = raw.mips.iter().rev().filter(|m| m.width.max(m.height) <= max_size.max(4) && m.width >= 4 && m.height >= 4).collect();
        if mips.is_empty() {
            mips = raw.mips.iter().rev().take(1).collect();
        }
        let top = mips[0];
        if top.width % 4 == 0 && top.height % 4 == 0 {
            let format = match raw.format {
                BcFormat::Bc1 => PixelFormat::Bc1,
                BcFormat::Bc2 => PixelFormat::Bc2,
                BcFormat::Bc3 => PixelFormat::Bc3,
            };
            return Some(Texture {
                name,
                format,
                width: top.width,
                height: top.height,
                full_width: fw,
                full_height: fh,
                mips: mips.iter().map(|m| m.data.to_vec()).collect(),
                mount: found.mount,
            });
        }
    }
    // autres formats, ou taille non multiple de 4 : décodage en RGBA
    let count = vtf.mip_count().max(1);
    let mut first = 0u8;
    while first + 1 < count && (fw >> first).max(fh >> first) > max_size {
        first += 1;
    }
    let mut mips = Vec::new();
    let (mut w, mut h) = (0, 0);
    for mip in first..count {
        match vtf.decode_rgba(0, 0, mip, &limits) {
            Ok(img) => {
                if mips.is_empty() {
                    (w, h) = (img.width, img.height);
                }
                mips.push(img.rgba);
            }
            Err(_) => break,
        }
    }
    if mips.is_empty() {
        return None;
    }
    Some(Texture { name, format: PixelFormat::Rgba8, width: w, height: h, full_width: fw, full_height: fh, mips, mount: found.mount })
}
