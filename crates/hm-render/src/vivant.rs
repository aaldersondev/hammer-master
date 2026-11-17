//! Rendu « vivant » pour la fenêtre de l'éditeur : les matériaux, textures et modèles sont chargés une fois et
//! gardés ; seule la géométrie du monde et la liste des modèles posés sont renvoyées à la carte graphique quand la
//! map change. Le dessin se fait dans des textures données par l'appelant (une par vue de la fenêtre), sans relecture.
use std::collections::HashMap;

use bytemuck::Zeroable;
use glam::{Mat4, Vec3};
use hm_assets::material::MasqueEnvmap;
use hm_assets::{load_material, load_texture, Material, PixelFormat, Tool, Vfs};
use wgpu::util::DeviceExt;

use crate::{pipe_index, srgb_to_linear, Globals, Instance, MVertex, MatU, Pass, Renderer, View, COLOR_FORMAT, DEPTH_FORMAT, F_ALPHATEST, F_BLEND, F_ENV_ALPHA_2, F_ENV_ALPHA_BASE, F_ENV_COULEUR_2, F_FLAT, F_SELFILLUM, F_TEX_ALPHA, F_UNLIT, ID_FORMAT};

/// Comment dessiner une vue.
#[derive(Clone, Copy, Debug, Default)]
pub struct OptionsVue {
    /// Avec l'éclairage de la map préparé par `preparer_eclairage` (sinon la lumière de contrôle de la vue).
    pub eclaire: bool,
    /// Le ciel a déjà été dessiné (`dessiner_ciel`) : garder sa couleur et ne pas dessiner les faces de ciel.
    pub ciel: bool,
    /// Mode de rendu : 0 textures ombrées, 1 textures sans ombrage, 2 polygones pleins (une couleur par objet).
    pub rendu: u32,
    /// Pas de la grille 3D tracée sur les surfaces (0 : aucune).
    pub grille: f32,
    /// Brouillard : couleur (sRGB, 0 à 1), début, fin, densité maximale.
    pub brouillard: Option<([f32; 3], f32, f32, f32)>,
    /// Distance d'affichage des modèles (0 : sans limite).
    pub distance_modeles: f32,
    /// Vitrages sans le voile de reflet : tels que Hammer les montre (l'éditeur). Le voile reste pour les vues de
    /// contrôle, où une vitre presque transparente passerait pour un trou.
    pub sans_voile: bool,
}

/// Ce que l'appelant sait d'un matériau après l'avoir demandé.
#[derive(Clone, Copy, Debug)]
pub struct InfoMateriau {
    pub indice: u32,
    /// Taille de la texture de base dans son fichier (échelle des coordonnées de texture d'une face).
    pub taille: (u32, u32),
    pub outil: Option<Tool>,
    pub trouve: bool,
    pub eau: bool,
    /// `$decalscale` : unités par texel d'un décal.
    pub decalscale: f32,
}

struct ModeleGpu {
    /// (début d'indices, nombre, sommet de base) de chaque maillage.
    maillages: Vec<(u32, u32, i32)>,
    /// Matériau de chaque maillage, par skin.
    skins: Vec<Vec<u32>>,
    pub bornes: ([f32; 3], [f32; 3]),
}

/// Les fichiers du jeu sur la carte graphique, gardés d'une modification à l'autre.
pub struct Bibliotheque {
    materiaux: HashMap<String, InfoMateriau>,
    pub(crate) groupes: Vec<wgpu::BindGroup>,
    passes: Vec<(Pass, bool)>,
    /// Le matériau est une face de ciel (`toolsskybox`).
    pub(crate) est_ciel: Vec<bool>,
    /// Le matériau est un outil (nodraw, clip, trigger…) : jamais vu en jeu.
    pub(crate) est_outil: Vec<bool>,
    /// Le cube du ciel 2D.
    pub(crate) ciel_vb: wgpu::Buffer,
    pub(crate) ciel_ib: wgpu::Buffer,
    textures: HashMap<String, Option<(wgpu::Texture, u32, u32)>>,
    modeles: HashMap<String, Option<ModeleGpu>>,
    erreurs_modeles: HashMap<String, String>,
    // modèles : copies en mémoire, pour refaire les tampons quand un modèle s'ajoute
    m_sommets: Vec<MVertex>,
    m_indices: Vec<u32>,
    pub(crate) model_vb: wgpu::Buffer,
    pub(crate) model_ib: wgpu::Buffer,
    sale: bool,
    cube: (u32, u32),
    alerte: u32,
    blanc: wgpu::Texture,
    /// Faces d'outil dessinées avec leur vraie texture, opaques, comme dans Hammer (sinon : voile de couleur codée).
    pub outils_reels: bool,
    /// La cubemap des reflets, chargée à la première demande (`None` intérieur : introuvable).
    cubemap: Option<Option<wgpu::TextureView>>,
}

/// Un sommet du monde (identique à `hm_scene::WVertex`).
pub use hm_scene::WVertex;

/// Un lot de triangles du monde d'un même matériau.
#[derive(Clone, Debug)]
pub struct LotMonde {
    pub materiau: u32,
    /// Faces d'outil ou entité masquée en jeu : dessinées seulement si les outils sont affichés.
    pub outil: bool,
    /// Fait de l'ombre dans l'aperçu de l'éclairage (monde, func_detail, displacements… comme dans vrad).
    pub ombre: bool,
    pub indices: Vec<u32>,
}

/// Un modèle posé.
#[derive(Clone, Debug)]
pub struct Pose {
    pub modele: String,
    pub skin: u32,
    /// Valeur `body` : les sous-modèles choisis (bodygroups).
    pub corps: u32,
    pub transform: Mat4,
    pub objet: u32,
    pub teinte: [f32; 4],
    /// Fait de l'ombre dans l'aperçu de l'éclairage (prop_static sans `disableshadows`).
    pub ombre: bool,
}

/// La géométrie de la map sur la carte graphique.
pub struct MondeGpu {
    pub(crate) world_vb: wgpu::Buffer,
    world_ib: wgpu::Buffer,
    pub(crate) instances: wgpu::Buffer,
    /// (passe, élimination des faces arrière, matériau, géométrie, face d'outil : cachée avec les textures d'outils)
    pub(crate) draws: Vec<(Pass, bool, u32, Geo, bool)>,
    /// Les triangles du monde qui font de l'ombre (indices dans `world_vb`).
    pub(crate) ombre_ib: wgpu::Buffer,
    pub(crate) ombre_n: u32,
}

#[derive(Clone, Copy)]
pub(crate) enum Geo {
    World { start: u32, count: u32 },
    Prop { index_start: u32, index_count: u32, base_vertex: i32, inst_start: u32, inst_count: u32 },
}

/// Les textures où une vue est dessinée.
pub struct Cibles {
    pub largeur: u32,
    pub hauteur: u32,
    pub couleur: wgpu::Texture,
    pub ids: wgpu::Texture,
    pub profondeur: wgpu::Texture,
}

impl Cibles {
    /// La couleur vue comme octets bruts : ce qu'une interface qui travaille en sRGB doit afficher.
    pub fn vue_brute(&self) -> wgpu::TextureView {
        self.couleur.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8Unorm), ..Default::default() })
    }

    pub fn nouvelles(device: &wgpu::Device, largeur: u32, hauteur: u32) -> Cibles {
        let (largeur, hauteur) = (largeur.max(8), hauteur.max(8));
        let mk = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vue"),
                size: wgpu::Extent3d { width: largeur, height: hauteur, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                // la fenêtre lit les octets tels quels (déjà en sRGB), sans seconde conversion
                view_formats: if format == COLOR_FORMAT { &[wgpu::TextureFormat::Rgba8Unorm] } else { &[] },
            })
        };
        Cibles {
            largeur,
            hauteur,
            couleur: mk(COLOR_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC),
            ids: mk(ID_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC),
            profondeur: mk(DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT),
        }
    }
}

impl Bibliotheque {
    pub fn nouvelle(r: &Renderer) -> Bibliotheque {
        let d = &r.gpu.device;
        let blanc = d.create_texture_with_data(
            &r.gpu.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[255, 255, 255, 255],
        );
        let mut m_sommets = Vec::new();
        let mut m_indices = Vec::new();
        let cube = crate::cube_mesh(&mut m_sommets, &mut m_indices);
        let vide = |label| d.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: 64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX, mapped_at_creation: false });
        let (cv, ci) = crate::ciel::cube();
        let mut b = Bibliotheque {
            materiaux: HashMap::new(),
            groupes: Vec::new(),
            passes: Vec::new(),
            est_ciel: Vec::new(),
            est_outil: Vec::new(),
            ciel_vb: d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ciel"), contents: bytemuck::cast_slice(&cv), usage: wgpu::BufferUsages::VERTEX }),
            ciel_ib: d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ciel"), contents: bytemuck::cast_slice(&ci), usage: wgpu::BufferUsages::INDEX }),
            textures: HashMap::new(),
            modeles: HashMap::new(),
            erreurs_modeles: HashMap::new(),
            m_sommets,
            m_indices,
            model_vb: vide("modèles"),
            model_ib: vide("modèles"),
            sale: true,
            cube,
            alerte: 0,
            blanc,
            outils_reels: false,
            cubemap: None,
        };
        b.alerte = b.groupe(r, MatU { color: [1.0, 0.05, 0.05, 1.0], flags: F_FLAT | F_UNLIT, alpha_ref: 0.5, reflet: 0.0, pad: 0.0, envmap: [0.0; 4] }, None, (Pass::Opaque, true));
        b
    }

    fn groupe(&mut self, r: &Renderer, u: MatU, tex: Option<&wgpu::Texture>, passe: (Pass, bool)) -> u32 {
        self.groupe_avec(r, u, tex, None, passe, false)
    }

    /// `bord` : texture serrée à ses bords (faces du ciel, sans couture).
    fn groupe_avec(&mut self, r: &Renderer, u: MatU, tex: Option<&wgpu::Texture>, tex2: Option<&wgpu::Texture>, passe: (Pass, bool), bord: bool) -> u32 {
        let d = &r.gpu.device;
        let buf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let vue = tex.unwrap_or(&self.blanc).create_view(&Default::default());
        let vue2 = tex2.or(tex).unwrap_or(&self.blanc).create_view(&Default::default());
        let cube = if u.envmap[3] > 0.0 { self.cubemap.as_ref().and_then(|c| c.as_ref()).unwrap_or(&r.cube_neutre) } else { &r.cube_neutre };
        let g = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &r.material_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&vue) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(if bord { &r.sampler_bord } else { &r.sampler }) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&vue2) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(cube) },
            ],
        });
        self.groupes.push(g);
        self.passes.push(passe);
        self.est_ciel.push(false);
        self.est_outil.push(false);
        self.groupes.len() as u32 - 1
    }

    /// La cubemap des reflets (`cubemap_editeur` : celle de Hammer++), chargée à la première demande ; `false` sans.
    fn cubemap(&mut self, r: &Renderer, vfs: &Vfs) -> bool {
        if self.cubemap.is_none() {
            self.cubemap = Some(hm_assets::texture::cubemap_editeur(vfs).map(|c| crate::cubemap(&r.gpu.device, &r.gpu.queue, c.taille, &c.faces)));
        }
        self.cubemap.as_ref().map_or(false, |c| c.is_some())
    }

    fn texture(&mut self, r: &Renderer, vfs: &Vfs, nom: &str) -> Option<(u32, u32)> {
        if !self.textures.contains_key(nom) {
            let t = load_texture(vfs, nom, 1024).map(|t| {
                let bc_ok = r.gpu.bc || t.format == PixelFormat::Rgba8;
                let format = match (t.format, bc_ok) {
                    (_, false) => wgpu::TextureFormat::Rgba8UnormSrgb,
                    (PixelFormat::Bc1, true) => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
                    (PixelFormat::Bc2, true) => wgpu::TextureFormat::Bc2RgbaUnormSrgb,
                    (PixelFormat::Bc3, true) => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
                    (PixelFormat::Rgba8, true) => wgpu::TextureFormat::Rgba8UnormSrgb,
                };
                let (w, h, mips) = if bc_ok { (t.width, t.height, t.mips.clone()) } else { (1, 1, vec![vec![160, 160, 160, 255]]) };
                let tex = r.gpu.device.create_texture_with_data(
                    &r.gpu.queue,
                    &wgpu::TextureDescriptor {
                        label: Some(nom),
                        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                        mip_level_count: mips.len() as u32,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &mips.concat(),
                );
                (tex, t.full_width, t.full_height)
            });
            self.textures.insert(nom.to_string(), t);
        }
        self.textures[nom].as_ref().map(|t| (t.1, t.2))
    }

    /// Le matériau, chargé à la première demande.
    pub fn materiau(&mut self, r: &Renderer, vfs: &Vfs, nom: &str) -> InfoMateriau {
        let cle = hm_assets::material::normalize(nom);
        if let Some(i) = self.materiaux.get(&cle) {
            return *i;
        }
        let m: Material = load_material(vfs, &cle);
        let taille = m.base.as_deref().and_then(|b| self.texture(r, vfs, b));
        let tex = m.base.as_deref().and_then(|b| self.textures.get(b)).and_then(|t| t.as_ref()).map(|t| t.0.clone());
        // seconde texture : `$basetexture2` (mélange), sinon le masque du reflet
        let masque = match m.envmap.as_ref().map(|e| &e.masque) {
            Some(MasqueEnvmap::AlphaNormales(t)) | Some(MasqueEnvmap::Texture(t)) if m.base2.is_none() => Some(t.clone()),
            _ => None,
        };
        let nom2 = m.base2.clone().or(masque);
        if let Some(b2) = nom2.as_deref() {
            self.texture(r, vfs, b2);
        }
        let tex2 = nom2.as_deref().and_then(|b| self.textures.get(b)).and_then(|t| t.as_ref()).map(|t| t.0.clone());
        let (u, passe) = if let Some(tool) = m.tool.filter(|_| !self.outils_reels || tex.is_none()) {
            let c = match tool {
                Tool::Nodraw => [0.90, 0.80, 0.20, 0.22],
                Tool::Sky => [0.30, 0.60, 1.00, 0.18],
                Tool::Clip => [0.85, 0.20, 0.85, 0.35],
                Tool::Trigger => [1.00, 0.55, 0.10, 0.35],
                Tool::Areaportal => [0.20, 0.90, 0.90, 0.40],
                Tool::Hint => [0.90, 0.90, 0.90, 0.12],
                Tool::Invisible => [0.50, 0.50, 0.95, 0.30],
                Tool::Other => [0.70, 0.70, 0.70, 0.30],
            };
            (MatU { color: c, flags: F_FLAT | F_UNLIT, alpha_ref: 0.5, reflet: 0.0, pad: 0.0, envmap: [0.0; 4] }, (Pass::Tools, true))
        } else if !m.found {
            (MatU { color: [1.0, 0.0, 1.0, 1.0], flags: F_FLAT | F_UNLIT, alpha_ref: 0.5, reflet: 0.0, pad: 0.0, envmap: [0.0; 4] }, (Pass::Opaque, true))
        } else {
            let mut flags = 0;
            if tex.is_none() {
                flags |= F_FLAT;
            }
            if m.alphatest {
                flags |= F_ALPHATEST;
            }
            if m.selfillum {
                flags |= F_SELFILLUM;
            }
            if m.unlit {
                flags |= F_UNLIT;
            }
            if m.translucent || m.additive {
                flags |= F_TEX_ALPHA;
            }
            if tex.is_some() && tex2.is_some() && m.base2.is_some() {
                flags |= F_BLEND;
            }
            // reflet de la cubemap par défaut du jeu, comme un éditeur sans cubemap compilée
            let mut envmap = [0.0; 4];
            if let Some(e) = m.envmap.as_ref().filter(|_| self.cubemap(r, vfs)) {
                envmap = [e.teinte[0], e.teinte[1], e.teinte[2], if m.shader.starts_with("water") { 2.0 } else { 1.0 }];
                flags |= match &e.masque {
                    MasqueEnvmap::AlphaBase => F_ENV_ALPHA_BASE,
                    MasqueEnvmap::AlphaNormales(_) if tex2.is_some() && m.base2.is_none() => F_ENV_ALPHA_2,
                    MasqueEnvmap::Texture(_) if tex2.is_some() && m.base2.is_none() => F_ENV_COULEUR_2,
                    _ => 0,
                };
            }
            let mut color = [m.color[0], m.color[1], m.color[2], m.alpha];
            if tex.is_none() && m.base.is_some() {
                color = [0.55, 0.55, 0.58, m.alpha];
            }
            let pass = if m.additive { Pass::Additive } else if m.translucent || m.alpha < 0.999 { Pass::Blend } else { Pass::Opaque };
            (MatU { color, flags, alpha_ref: m.alpha_ref, reflet: m.reflet, pad: 0.0, envmap }, (pass, !m.nocull))
        };
        let indice = self.groupe_avec(r, u, tex.as_ref(), tex2.as_ref(), passe, cle.starts_with("skybox/"));
        self.est_ciel[indice as usize] = m.tool == Some(Tool::Sky);
        self.est_outil[indice as usize] = m.tool.is_some();
        let info = InfoMateriau { indice, taille: taille.unwrap_or((512, 512)), outil: m.tool, trouve: m.found, eau: m.shader.starts_with("water"), decalscale: m.decalscale };
        self.materiaux.insert(cle, info);
        info
    }

    /// Le modèle, chargé à la première demande. `None` : illisible ou introuvable (dessiné en cube rouge).
    pub fn modele(&mut self, r: &Renderer, vfs: &Vfs, nom: &str) -> Option<([f32; 3], [f32; 3])> {
        self.modele_corps(r, vfs, nom, 0)
    }

    /// Un modèle avec les sous-modèles d'une valeur `body` (bodygroups).
    pub fn modele_corps(&mut self, r: &Renderer, vfs: &Vfs, nom: &str, corps: u32) -> Option<([f32; 3], [f32; 3])> {
        let cle = cle_modele(nom, corps);
        if !self.modeles.contains_key(&cle) {
            let m = match hm_assets::model::load_model_body(vfs, &hm_assets::model::normalize_model(nom), corps) {
                Ok(m) => {
                    let base = self.m_sommets.len() as i32;
                    self.m_sommets.extend(m.vertices.iter().map(|v| MVertex { pos: v.pos, normal: v.normal, uv: v.uv }));
                    let mut maillages = Vec::with_capacity(m.meshes.len());
                    for mesh in &m.meshes {
                        maillages.push((self.m_indices.len() as u32, mesh.indices.len() as u32, base));
                        self.m_indices.extend_from_slice(&mesh.indices);
                    }
                    let mut skins = Vec::new();
                    for skin in &m.skins {
                        let mut s = Vec::new();
                        for cands in skin {
                            // le premier candidat trouvé, comme le jeu qui essaie chaque dossier `$cdmaterials`
                            let mut choisi = None;
                            for c in cands {
                                let i = self.materiau(r, vfs, c);
                                if i.trouve {
                                    choisi = Some(i.indice);
                                    break;
                                }
                            }
                            s.push(choisi.unwrap_or_else(|| self.materiau(r, vfs, cands.first().map(|x| x.as_str()).unwrap_or("")).indice));
                        }
                        skins.push(s);
                    }
                    self.sale = true;
                    Some(ModeleGpu { maillages, skins, bornes: m.bounds })
                }
                Err(e) => {
                    self.erreurs_modeles.insert(cle.clone(), e);
                    None
                }
            };
            self.modeles.insert(cle.clone(), m);
        }
        self.modeles[&cle].as_ref().map(|m| m.bornes)
    }

    /// Un matériau de couleur unie éclairée (boîtes des entités sans modèle).
    pub fn couleur(&mut self, r: &Renderer, rgba: [u8; 4]) -> u32 {
        let cle = format!("#couleur {} {} {} {}", rgba[0], rgba[1], rgba[2], rgba[3]);
        if let Some(i) = self.materiaux.get(&cle) {
            return i.indice;
        }
        let c = rgba.map(|v| v as f32 / 255.0);
        let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        // une couleur translucide (calques) se mélange et se voit des deux côtés
        let passe = if rgba[3] < 255 { (Pass::Blend, false) } else { (Pass::Opaque, true) };
        let indice = self.groupe(r, MatU { color: [lin(c[0]), lin(c[1]), lin(c[2]), c[3]], flags: F_FLAT, alpha_ref: 0.5, reflet: 0.0, pad: 0.0, envmap: [0.0; 4] }, None, passe);
        self.materiaux.insert(cle, InfoMateriau { indice, taille: (1, 1), outil: None, trouve: true, eau: false, decalscale: 1.0 });
        indice
    }

    pub fn erreur_modele(&self, nom: &str) -> Option<&str> {
        self.erreurs_modeles.get(&hm_assets::model::normalize_model(nom)).map(|s| s.as_str())
    }

    fn envoyer_modeles(&mut self, r: &Renderer) {
        if !self.sale {
            return;
        }
        let d = &r.gpu.device;
        self.model_vb = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("modèles"), contents: bytemuck::cast_slice(&self.m_sommets), usage: wgpu::BufferUsages::VERTEX });
        self.model_ib = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("modèles"), contents: bytemuck::cast_slice(&self.m_indices), usage: wgpu::BufferUsages::INDEX });
        self.sale = false;
    }

    /// Envoie la géométrie de la map : triangles du monde par lot, modèles posés.
    pub fn monde(&mut self, r: &Renderer, sommets: &[WVertex], lots: &[LotMonde], poses: &[Pose]) -> MondeGpu {
        self.envoyer_modeles(r);
        let d = &r.gpu.device;
        let mut draws = Vec::new();
        let mut indices: Vec<u32> = Vec::with_capacity(lots.iter().map(|l| l.indices.len()).sum());
        let mut ombres: Vec<u32> = Vec::new();
        for l in lots {
            let (mut pass, cull) = self.passes[l.materiau as usize];
            // les surfaces translucides et additives ne font pas d'ombre dans vrad
            if l.ombre && !matches!(pass, Pass::Blend | Pass::Additive) {
                ombres.extend_from_slice(&l.indices);
            }
            if l.outil && !self.outils_reels {
                pass = Pass::Tools;
            }
            draws.push((pass, cull, l.materiau, Geo::World { start: indices.len() as u32, count: l.indices.len() as u32 }, l.outil));
            indices.extend_from_slice(&l.indices);
        }
        let vide = [WVertex::zeroed()];
        let world_vb = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("monde"), contents: bytemuck::cast_slice(if sommets.is_empty() { &vide[..] } else { sommets }), usage: wgpu::BufferUsages::VERTEX });
        if indices.is_empty() {
            indices.push(0);
        }
        let world_ib = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("monde"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX });
        let ombre_n = ombres.len() as u32;
        if ombres.is_empty() {
            ombres.push(0);
        }
        let ombre_ib = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ombres"), contents: bytemuck::cast_slice(&ombres), usage: wgpu::BufferUsages::INDEX });
        // modèles posés, groupés par (modèle, skin)
        let mut ordre: Vec<usize> = (0..poses.len()).collect();
        let cle = |p: &Pose| (cle_modele(&p.modele, p.corps), p.skin);
        ordre.sort_by_key(|&i| cle(&poses[i]));
        let mut inst: Vec<Instance> = Vec::with_capacity(poses.len().max(1));
        let mut k = 0;
        while k < ordre.len() {
            let c0 = cle(&poses[ordre[k]]);
            let start = inst.len() as u32;
            while k < ordre.len() && cle(&poses[ordre[k]]) == c0 {
                let p = &poses[ordre[k]];
                let mut m = p.transform;
                if self.modeles.get(&c0.0).map_or(true, |x| x.is_none()) {
                    m = m * Mat4::from_scale(Vec3::splat(16.0));
                }
                inst.push(Instance { m: m.to_cols_array_2d(), tint: p.teinte, object: p.objet, pad: [p.ombre as u32, 0, 0] });
                k += 1;
            }
            let n = inst.len() as u32 - start;
            match self.modeles.get(&c0.0).and_then(|x| x.as_ref()) {
                Some(mg) => {
                    let skin = (c0.1 as usize).min(mg.skins.len().saturating_sub(1));
                    for (j, &(index_start, index_count, base_vertex)) in mg.maillages.iter().enumerate() {
                        let materiau = mg.skins.get(skin).and_then(|s| s.get(j)).copied().unwrap_or(self.alerte);
                        let (pass, cull) = self.passes[materiau as usize];
                        draws.push((pass, cull, materiau, Geo::Prop { index_start, index_count, base_vertex, inst_start: start, inst_count: n }, false));
                    }
                }
                None => draws.push((Pass::Opaque, true, self.alerte, Geo::Prop { index_start: self.cube.0, index_count: self.cube.1, base_vertex: 0, inst_start: start, inst_count: n }, false)),
            }
        }
        if inst.is_empty() {
            inst.push(Instance::zeroed());
        }
        let instances = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("instances"), contents: bytemuck::cast_slice(&inst), usage: wgpu::BufferUsages::VERTEX });
        MondeGpu { world_vb, world_ib, instances, draws, ombre_ib, ombre_n }
    }
}

impl Renderer {
    /// Dessine une vue dans ses cibles. Le tampon de commandes est rendu à l'appelant (soumis avec le reste).
    /// Les constantes d'une vue : `vp` remplace la projection de la caméra, `clip` la boîte de découpe.
    pub(crate) fn globals_de(&self, view: &View, w: u32, h: u32, vp: Option<Mat4>, clip: Option<(Vec3, Vec3)>) -> Globals {
        let cam = &view.camera;
        let (cmin, cmax) = clip.or(view.clip).unwrap_or((Vec3::splat(-1e9), Vec3::splat(1e9)));
        let persp = matches!(cam.proj, crate::Projection::Perspective { .. });
        let l = &view.light;
        Globals {
            view_proj: vp.unwrap_or_else(|| cam.view_proj(w as f32 / h as f32)).to_cols_array_2d(),
            eye: cam.eye.extend(if persp { 1.0 } else { 0.0 }).to_array(),
            view_dir: cam.dir.extend(0.0).to_array(),
            sun_dir: l.sun_dir.normalize().extend(0.0).to_array(),
            sun_color: l.sun_color.extend(0.0).to_array(),
            ambient: l.ambient.extend(l.headlight).to_array(),
            clip_min: cmin.extend(0.0).to_array(),
            clip_max: cmax.extend(0.0).to_array(),
            fog: [0.0; 4],
            fog_color: [0.0; 4],
        }
    }

    /// Les lots de la map, passe par passe. `sans_ciel` : les faces de ciel en profondeur seulement, d'abord (on voit
    /// le ciel déjà dessiné au travers, mais rien de ce qui est derrière, comme en jeu : ni la salle de la skybox 3D
    /// au-dessus de la map ni le vide) ; `comme_en_jeu` : sans aucune face d'outil, même dessinée avec sa vraie
    /// texture (la skybox 3D).
    pub(crate) fn dessiner_lots(&self, rp: &mut wgpu::RenderPass<'_>, bib: &Bibliotheque, monde: &MondeGpu, outils: bool, sans_ciel: bool, comme_en_jeu: bool) {
        if sans_ciel {
            rp.set_vertex_buffer(0, monde.world_vb.slice(..));
            rp.set_index_buffer(monde.world_ib.slice(..), wgpu::IndexFormat::Uint32);
            for cull in [true, false] {
                rp.set_pipeline(&self.pipelines[crate::PIPE_CIEL + (!cull) as usize]);
                for (_, c, mat, geo, _) in &monde.draws {
                    if let Geo::World { start, count } = *geo {
                        if *c == cull && bib.est_ciel[*mat as usize] {
                            rp.set_bind_group(1, &bib.groupes[*mat as usize], &[]);
                            rp.draw_indexed(start..start + count, 0, 0..1);
                        }
                    }
                }
            }
        }
        for pass in [Pass::Opaque, Pass::Blend, Pass::Additive, Pass::Tools] {
            if pass == Pass::Tools && !outils {
                continue;
            }
            for prop in [false, true] {
                if prop {
                    rp.set_vertex_buffer(0, bib.model_vb.slice(..));
                    rp.set_vertex_buffer(1, monde.instances.slice(..));
                    rp.set_index_buffer(bib.model_ib.slice(..), wgpu::IndexFormat::Uint32);
                } else {
                    rp.set_vertex_buffer(0, monde.world_vb.slice(..));
                    rp.set_index_buffer(monde.world_ib.slice(..), wgpu::IndexFormat::Uint32);
                }
                for cull in [true, false] {
                    rp.set_pipeline(&self.pipelines[pipe_index(prop, pass, cull)]);
                    for (p, c, mat, geo, outil) in &monde.draws {
                        // textures d'outils masquées (Ctrl+Maj+F2) : aussi celles dessinées avec leur vraie texture
                        if *p != pass || *c != cull || (sans_ciel && bib.est_ciel[*mat as usize]) || (comme_en_jeu && bib.est_outil[*mat as usize]) || (*outil && !outils) {
                            continue;
                        }
                        match *geo {
                            Geo::World { start, count } if !prop => {
                                rp.set_bind_group(1, &bib.groupes[*mat as usize], &[]);
                                rp.draw_indexed(start..start + count, 0, 0..1);
                            }
                            Geo::Prop { index_start, index_count, base_vertex, inst_start, inst_count } if prop => {
                                rp.set_bind_group(1, &bib.groupes[*mat as usize], &[]);
                                rp.draw_indexed(index_start..index_start + index_count, base_vertex, inst_start..inst_start + inst_count);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Dessine une vue dans ses cibles. Le tampon de commandes est rendu à l'appelant (soumis avec le reste).
    pub fn dessiner_vivant(&self, bib: &Bibliotheque, monde: &MondeGpu, view: &View, cibles: &Cibles, globals: &wgpu::Buffer, globals_bg: &wgpu::BindGroup, opts: OptionsVue, enc: &mut wgpu::CommandEncoder) {
        let groupe_eclairage = if opts.eclaire { self.eclairage.groupe_actif() } else { None };
        let (w, h) = (cibles.largeur, cibles.hauteur);
        let l = &view.light;
        let mut g = self.globals_de(view, w, h, None, None);
        g.clip_min[3] = opts.rendu as f32;
        g.clip_max[3] = opts.grille;
        if let Some((c, debut, fin, max)) = opts.brouillard {
            g.fog = [debut, fin, max, 1.0];
            g.fog_color = [srgb_to_linear(c[0]) as f32, srgb_to_linear(c[1]) as f32, srgb_to_linear(c[2]) as f32, 1.0];
        }
        g.fog_color[3] = opts.distance_modeles;
        g.view_dir[3] = if opts.sans_voile { 1.0 } else { 0.0 };
        self.gpu.queue.write_buffer(globals, 0, bytemuck::bytes_of(&g));
        let (cv, iv, dv) = (cibles.couleur.create_view(&Default::default()), cibles.ids.create_view(&Default::default()), cibles.profondeur.create_view(&Default::default()));
        // après le ciel : on garde sa couleur
        let couleur = if opts.ciel { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color { r: srgb_to_linear(l.sky[0]), g: srgb_to_linear(l.sky[1]), b: srgb_to_linear(l.sky[2]), a: 1.0 }) };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vue 3D"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment { view: &cv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: couleur, store: wgpu::StoreOp::Store } }),
                Some(wgpu::RenderPassColorAttachment { view: &iv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
            ..Default::default()
        });
        rp.set_bind_group(0, globals_bg, &[]);
        rp.set_bind_group(2, groupe_eclairage.as_ref().unwrap_or(&self.eclairage.eteint), &[]);
        self.dessiner_lots(&mut rp, bib, monde, view.show_tools, opts.ciel, false);
    }

    /// Prépare l'aperçu de l'éclairage de la map : lumières, et cartes d'ombre de ce qui a changé depuis la dernière
    /// fois. `version` : celle de la géométrie de `monde` (une autre refait toutes les ombres).
    pub fn preparer_eclairage(&self, bib: &Bibliotheque, monde: &MondeGpu, version: u64, e: &crate::eclairage::Eclairage, enc: &mut wgpu::CommandEncoder) {
        self.eclairage.preparer(&self.gpu.device, &self.gpu.queue, bib, monde, version, e, enc);
    }

    /// Tampon et groupe des constantes d'une vue (un par vue : les vues se dessinent dans le même tampon de commandes).
    pub fn constantes_vue(&self) -> (wgpu::Buffer, wgpu::BindGroup) {
        let d = &self.gpu.device;
        let b = d.create_buffer(&wgpu::BufferDescriptor { label: Some("globals"), size: std::mem::size_of::<Globals>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let bg = d.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &self.globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: b.as_entire_binding() }] });
        (b, bg)
    }

    /// Identifiant de l'objet sous un pixel des cibles d'une vue (attend la carte graphique).
    pub fn id_sous(&self, cibles: &Cibles, x: u32, y: u32) -> u32 {
        u32::from_le_bytes(self.texel(cibles, &cibles.ids, x, y))
    }

    /// La couleur affichée au pixel (x, y) de la vue (sRGB, 0 à 255).
    pub fn couleur_sous(&self, cibles: &Cibles, x: u32, y: u32) -> [u8; 4] {
        self.texel(cibles, &cibles.couleur, x, y)
    }

    /// Les quatre octets d'un texel d'une cible de la vue.
    fn texel(&self, cibles: &Cibles, texture: &wgpu::Texture, x: u32, y: u32) -> [u8; 4] {
        let d = &self.gpu.device;
        let buf = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: 256, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = d.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x: x.min(cibles.largeur - 1), y: y.min(cibles.hauteur - 1), z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(1) } },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        self.gpu.queue.submit([enc.finish()]);
        let s = buf.slice(..);
        s.map_async(wgpu::MapMode::Read, |_| {});
        let _ = d.poll(wgpu::PollType::wait_indefinitely());
        let v = {
            let r = s.get_mapped_range().expect("lecture d'un texel");
            [r[0], r[1], r[2], r[3]]
        };
        buf.unmap();
        v
    }
}

/// Clé du cache des modèles : le chemin, suivi de la valeur `body` quand elle n'est pas nulle.
fn cle_modele(nom: &str, corps: u32) -> String {
    let n = hm_assets::model::normalize_model(nom);
    if corps == 0 { n } else { format!("{n}#{corps}") }
}
