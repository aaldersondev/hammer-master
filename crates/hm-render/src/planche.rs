//! Images côté processeur : légendes incrustées, planches de plusieurs vues, encodage JPEG.
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use anyhow::Result;

/// Image RGB simple.
#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, fill: [u8; 3]) -> Self {
        Self { width, height, rgb: fill.iter().copied().cycle().take((width * height * 3) as usize).collect() }
    }

    pub fn from_rgb(width: u32, height: u32, rgb: Vec<u8>) -> Self {
        Self { width, height, rgb }
    }

    pub fn blit(&mut self, src: &Image, x0: u32, y0: u32) {
        for y in 0..src.height.min(self.height.saturating_sub(y0)) {
            let w = src.width.min(self.width.saturating_sub(x0)) as usize;
            let d = ((y0 + y) * self.width + x0) as usize * 3;
            let s = (y * src.width) as usize * 3;
            self.rgb[d..d + w * 3].copy_from_slice(&src.rgb[s..s + w * 3]);
        }
    }

    pub fn fill_rect(&mut self, x0: i32, y0: i32, w: i32, h: i32, color: [u8; 3], alpha: f32) {
        for y in y0.max(0)..(y0 + h).min(self.height as i32) {
            for x in x0.max(0)..(x0 + w).min(self.width as i32) {
                let i = (y as u32 * self.width + x as u32) as usize * 3;
                for c in 0..3 {
                    self.rgb[i + c] = (self.rgb[i + c] as f32 * (1.0 - alpha) + color[c] as f32 * alpha) as u8;
                }
            }
        }
    }

    pub fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>> {
        jpeg(&self.rgb, self.width, self.height, quality)
    }
}

pub fn jpeg(rgb: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(rgb.len() / 8);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode(rgb, width, height, image::ExtendedColorType::Rgb8)?;
    Ok(out)
}

/// Police du système (Segoe UI, sinon Arial ou Consolas). Sans police, les légendes ne sont pas écrites.
pub struct Police(Option<FontVec>);

impl Police {
    pub fn systeme() -> Self {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".into());
        for f in ["segoeuib.ttf", "segoeui.ttf", "arialbd.ttf", "arial.ttf", "consola.ttf"] {
            if let Ok(bytes) = std::fs::read(format!("{windir}/Fonts/{f}")) {
                if let Ok(font) = FontVec::try_from_vec(bytes) {
                    return Self(Some(font));
                }
            }
        }
        Self(None)
    }

    pub fn largeur(&self, text: &str, px: f32) -> f32 {
        let Some(font) = &self.0 else { return 0.0 };
        let s = font.as_scaled(PxScale::from(px));
        text.chars().map(|c| s.h_advance(font.glyph_id(c))).sum()
    }

    /// Écrit une ligne ; `(x, y)` est le coin haut gauche.
    pub fn ecrire(&self, img: &mut Image, x: f32, y: f32, px: f32, text: &str, color: [u8; 3]) {
        let Some(font) = &self.0 else { return };
        let s = font.as_scaled(PxScale::from(px));
        let mut pen = x;
        let base = y + s.ascent();
        for c in text.chars() {
            let id = font.glyph_id(c);
            let glyph = id.with_scale_and_position(PxScale::from(px), ab_glyph::point(pen, base));
            pen += s.h_advance(id);
            if let Some(o) = font.outline_glyph(glyph) {
                let bb = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px, py) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < img.width && (py as u32) < img.height {
                        let i = (py as u32 * img.width + px as u32) as usize * 3;
                        for k in 0..3 {
                            img.rgb[i + k] = (img.rgb[i + k] as f32 * (1.0 - cov) + color[k] as f32 * cov) as u8;
                        }
                    }
                });
            }
        }
    }

    /// Écrit une étiquette sur un bandeau sombre.
    pub fn etiquette(&self, img: &mut Image, x: i32, y: i32, px: f32, text: &str, color: [u8; 3]) {
        let w = self.largeur(text, px).ceil() as i32 + 8;
        img.fill_rect(x, y, w, (px * 1.3) as i32, [0, 0, 0], 0.72);
        self.ecrire(img, x as f32 + 4.0, y as f32 + px * 0.08, px, text, color);
    }
}

/// Une vue d'une planche : son image et ses lignes de légende.
pub struct Case {
    pub image: Image,
    pub legende: Vec<String>,
}

/// Assemble des vues de même taille en une planche de `cols` colonnes, avec un titre.
pub fn planche(cases: &[Case], cols: u32, titre: &str, police: &Police) -> Image {
    let (cw, ch) = cases.first().map(|c| (c.image.width, c.image.height)).unwrap_or((512, 288));
    let rows = (cases.len() as u32).div_ceil(cols).max(1);
    let bandeau = 22;
    let mut out = Image::new(cols * cw, rows * ch + bandeau, [14, 14, 16]);
    police.ecrire(&mut out, 6.0, 2.0, 15.0, titre, [235, 235, 235]);
    for (i, c) in cases.iter().enumerate() {
        let (x, y) = ((i as u32 % cols) * cw, bandeau + (i as u32 / cols) * ch);
        out.blit(&c.image, x, y);
        for (k, line) in c.legende.iter().enumerate() {
            police.etiquette(&mut out, x as i32, y as i32 + k as i32 * 15, 11.5, line, if k == 0 { [255, 232, 80] } else { [230, 230, 230] });
        }
        // filet entre les vues
        out.fill_rect(x as i32, y as i32, cw as i32, 1, [0, 0, 0], 1.0);
        out.fill_rect(x as i32, y as i32, 1, ch as i32, [0, 0, 0], 1.0);
    }
    out
}
