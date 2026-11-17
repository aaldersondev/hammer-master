// Rendu de contrôle : texture de base, soleil, ambiante, lampe frontale. Deux sorties : la couleur et l'identifiant
// de l'objet sous chaque pixel.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,        // w = 1 en perspective, 0 en vue orthographique (le regard est alors `view_dir`)
    view_dir: vec4<f32>,   // w = 1 : vitrages sans voile de reflet, comme Hammer (l'éditeur)
    sun_dir: vec4<f32>,    // vers le soleil
    sun_color: vec4<f32>,
    ambient: vec4<f32>,    // a = force de la lampe frontale
    clip_min: vec4<f32>,   // w = mode de rendu de la vue 3D : 0 textures ombrées, 1 textures sans ombrage, 2 polygones pleins
    clip_max: vec4<f32>,   // w = pas de la grille 3D sur les surfaces (0 : pas de grille)
    fog: vec4<f32>,        // début, fin, densité maximale, 1 = brouillard (vue en perspective)
    fog_color: vec4<f32>,  // couleur du brouillard, linéaire ; w = distance d'affichage des modèles (0 : sans limite)
};
@group(0) @binding(0) var<uniform> g: Globals;

struct Mat {
    color: vec4<f32>,
    flags: u32,
    alpha_ref: f32,
    reflet: f32,   // force du reflet d'un vitrage, 0 = aucun
    pad1: f32,
    envmap: vec4<f32>,  // reflet de la cubemap : teinte ; w = 0 aucun, 1 ajouté, 2 eau (Fresnel)
};
@group(1) @binding(0) var<uniform> m: Mat;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
@group(1) @binding(3) var tex2: texture_2d<f32>;
@group(1) @binding(4) var cube: texture_cube<f32>;


// Aperçu de l'éclairage de la map (eclairage.rs) : même disposition que l'uniforme écrit par `Moteur::preparer`.
struct Lum {
    pos: vec4<f32>,      // w = première couche de ses ombres (6 par lumière), -1 sans ombre
    couleur: vec4<f32>,  // w = portée
    attn: vec4<f32>,     // constante, linéaire, quadratique ; w = début du fondu
    cone: vec4<f32>,     // direction ; w = cosinus extérieur (-2 : pas de cône)
    cone2: vec4<f32>,    // cosinus intérieur, exposant, fin du fondu
};
const N_CIEL: u32 = 16u;
const N_LUM: u32 = 96u;
const N_MATS: u32 = 209u;  // 6 × 32 + 1 + 16
const M_SOLEIL: u32 = 192u;
struct Eclairage {
    n: vec4<u32>,          // lumières, soleil, directions du ciel, aperçu actif
    soleil_dir: vec4<f32>, // vers le soleil ; w = 0 avec ombre, -1 sans
    soleil_col: vec4<f32>,
    ciel: vec4<f32>,       // w = 1 avec ombres
    ajout: vec4<f32>,
    texels: vec4<f32>,     // taille d'un texel : soleil, ciel, cube (par unité de distance)
    ciel_dirs: array<vec4<f32>, 16>,
    lum: array<Lum, 96>,
    mats: array<mat4x4<f32>, 209>,
};
@group(2) @binding(0) var<uniform> el: Eclairage;
@group(2) @binding(1) var t_cubes: texture_depth_2d_array;
@group(2) @binding(2) var t_soleil: texture_depth_2d_array;
@group(2) @binding(3) var t_ciel: texture_depth_2d_array;
@group(2) @binding(4) var s_cmp: sampler_comparison;

fn projeter(m: mat4x4<f32>, q: vec3<f32>) -> vec3<f32> {
    let c = m * vec4<f32>(q, 1.0);
    let ndc = c.xyz / c.w;
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}

fn ombre_cube(base: i32, p: vec3<f32>, n: vec3<f32>, lp: vec3<f32>, d: f32) -> f32 {
    let q = p + n * (el.texels.z * d * 1.5 + 0.5);
    let v = q - lp;
    let a = abs(v);
    var f = 0;
    if (a.x >= a.y && a.x >= a.z) {
        f = select(1, 0, v.x > 0.0);
    } else if (a.y >= a.z) {
        f = select(3, 2, v.y > 0.0);
    } else {
        f = select(5, 4, v.z > 0.0);
    }
    let c = projeter(el.mats[base + f], q);
    return textureSampleCompareLevel(t_cubes, s_cmp, c.xy, base + f, c.z);
}

fn ombre_dir(t: texture_depth_2d_array, couche: i32, m: mat4x4<f32>, p: vec3<f32>, n: vec3<f32>, texel: f32) -> f32 {
    let c = projeter(m, p + n * texel * 1.5);
    if (any(c.xy < vec2<f32>(0.0)) || any(c.xy > vec2<f32>(1.0))) {
        return 1.0;
    }
    return textureSampleCompareLevel(t, s_cmp, c.xy, couche, c.z);
}

// L'éclairement direct d'un point (formule de hm_doc::lumiere, vérifiée contre vrad).
fn eclairement_carte(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var e = el.ajout.rgb;
    let nl = min(el.n.x, N_LUM);
    for (var i = 0u; i < nl; i++) {
        let L = el.lum[i];
        let v = L.pos.xyz - p;
        let d = length(v);
        if (d > L.couleur.w || d < 0.001) {
            continue;
        }
        let l = v / d;
        let ndl = dot(n, l);
        if (ndl <= 0.0) {
            continue;
        }
        let den = L.attn.x + L.attn.y * d + L.attn.z * d * d;
        var k = ndl / max(den, 1e-9);
        if (L.cone2.z > L.attn.w && d > L.attn.w) {
            k = k * clamp(1.0 - (d - L.attn.w) / (L.cone2.z - L.attn.w), 0.0, 1.0);
        }
        if (L.cone.w > -1.5) {
            let axe = -dot(l, L.cone.xyz);
            if (axe <= L.cone.w) {
                continue;
            }
            k = k * axe;
            if (axe <= L.cone2.x) {
                let t = (axe - L.cone.w) / max(L.cone2.x - L.cone.w, 1e-6);
                if (L.cone2.y == 0.0 || L.cone2.y == 1.0) {
                    k = k * t;
                } else {
                    k = k * pow(t, L.cone2.y);
                }
            }
        }
        if (L.pos.w >= 0.0) {
            k = k * ombre_cube(i32(L.pos.w), p, n, L.pos.xyz, d);
        }
        e = e + L.couleur.rgb * k;
    }
    if (el.n.y != 0u) {
        let ndl = dot(n, el.soleil_dir.xyz);
        if (ndl > 0.0) {
            var o = 1.0;
            if (el.soleil_dir.w >= 0.0) {
                o = ombre_dir(t_soleil, 0, el.mats[M_SOLEIL], p, n, el.texels.x);
            }
            e = e + el.soleil_col.rgb * ndl * o;
        }
    }
    let nc = min(el.n.z, N_CIEL);
    if (nc > 0u) {
        var s = 0.0;
        for (var j = 0u; j < nc; j++) {
            let dj = el.ciel_dirs[j];
            let c = dot(n, dj.xyz);
            if (c > 0.0) {
                var o = 1.0;
                if (el.ciel.w > 0.5) {
                    o = ombre_dir(t_ciel, i32(j), el.mats[M_SOLEIL + 1u + j], p, n, el.texels.y);
                }
                s = s + dj.w * c * o;
            }
        }
        e = e + el.ciel.rgb * s;
    }
    return e;
}

const F_ALPHATEST: u32 = 1u;
const F_SELFILLUM: u32 = 2u;
const F_UNLIT: u32 = 4u;
const F_TEX_ALPHA: u32 = 8u;
const F_FLAT: u32 = 16u;
const F_BLEND: u32 = 32u;
const F_ENV_ALPHA_BASE: u32 = 64u;
const F_ENV_ALPHA_2: u32 = 128u;
const F_ENV_COULEUR_2: u32 = 256u;

struct V2F {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) object: u32,
    @location(4) tint: vec4<f32>,
    @location(5) blend: f32,
};

@vertex
fn vs_world(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) object: u32, @location(10) blend: f32) -> V2F {
    var o: V2F;
    var p = pos;
    var n = normal;
    o.blend = blend;
    if (blend < -1.5) {
        // icône (`iconsprite`) : panneau parallèle à l'écran, centré sur `pos` ; `normal.xy` = coin (droite, haut)
        let f = normalize(g.view_dir.xyz);
        var r = cross(f, vec3<f32>(0.0, 0.0, 1.0));
        if (length(r) < 1e-4) {
            r = vec3<f32>(0.0, -1.0, 0.0);
        }
        r = normalize(r);
        p = pos + r * normal.x + cross(r, f) * normal.y;
        n = -f;
        o.blend = 0.0;
    }
    o.clip = g.view_proj * vec4<f32>(p, 1.0);
    o.world = p;
    o.normal = n;
    o.uv = uv;
    o.object = object;
    o.tint = vec4<f32>(1.0);
    return o;
}

@vertex
fn vs_prop(
    @location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>,
    @location(4) m0: vec4<f32>, @location(5) m1: vec4<f32>, @location(6) m2: vec4<f32>, @location(7) m3: vec4<f32>,
    @location(8) tint: vec4<f32>, @location(9) object: u32,
) -> V2F {
    let model = mat4x4<f32>(m0, m1, m2, m3);
    let w = model * vec4<f32>(pos, 1.0);
    var o: V2F;
    o.clip = g.view_proj * w;
    o.world = w.xyz;
    o.normal = (model * vec4<f32>(normal, 0.0)).xyz;
    o.uv = uv;
    o.object = object;
    o.tint = tint;
    // marque des modèles (le mélange de textures ne sert qu'au monde) : la distance d'affichage les écarte
    o.blend = -1.0;
    return o;
}

struct Out {
    @location(0) color: vec4<f32>,
    @location(1) id: u32,
};

@fragment
fn fs(v: V2F, @builtin(front_facing) front: bool) -> Out {
    // pente de la position par pixel, avant tout discard (grille 3D)
    let fw = fwidth(v.world);
    var t = textureSample(tex, samp, v.uv);
    // WorldVertexTransition : alpha 0 → $basetexture, 255 → $basetexture2
    let t2 = textureSample(tex2, samp, v.uv);
    if ((m.flags & F_BLEND) != 0u) {
        t = mix(t, t2, clamp(v.blend, 0.0, 1.0));
    }
    if (v.blend < -0.5 && g.fog_color.w > 0.0 && length(v.world - g.eye.xyz) > g.fog_color.w) {
        discard;
    }
    if (any(v.world < g.clip_min.xyz) || any(v.world > g.clip_max.xyz)) {
        discard;
    }
    var albedo = m.color.rgb * v.tint.rgb;
    var alpha = m.color.a;
    let mode = u32(g.clip_min.w + 0.5);
    if (mode == 2u) {
        // polygones pleins (« 3D Filled Polygons » de Hammer) : une couleur par objet, sans texture
        let h = v.object * 2654435761u;
        albedo = vec3<f32>(f32((h >> 8u) & 255u), f32((h >> 16u) & 255u), f32((h >> 24u) & 255u)) / 255.0 * 0.6 + vec3<f32>(0.3);
        alpha = 1.0;
    } else if ((m.flags & F_FLAT) == 0u) {
        albedo = albedo * t.rgb;
        if ((m.flags & F_TEX_ALPHA) != 0u) {
            alpha = alpha * t.a;
        }
        if ((m.flags & F_ALPHATEST) != 0u && t.a < m.alpha_ref) {
            discard;
        }
    }
    var n = normalize(v.normal);
    if (!front) {
        n = -n;
    }
    var to_eye = -g.view_dir.xyz;
    if (g.eye.w > 0.5) {
        to_eye = normalize(g.eye.xyz - v.world);
    }
    var lit = vec3<f32>(0.0);
    if (el.n.w != 0u) {
        // l'éclairage de la map : ce que donneraient les lightmaps (sans les rebonds)
        lit = albedo * eclairement_carte(v.world, n);
    } else {
        let ndl = max(dot(n, g.sun_dir.xyz), 0.0);
        let hemi = mix(0.62, 1.0, n.z * 0.5 + 0.5);
        let head = max(dot(n, to_eye), 0.0) * g.ambient.a;
        lit = albedo * (g.ambient.rgb * hemi + g.sun_color.rgb * ndl + vec3<f32>(head));
    }
    if ((m.flags & F_UNLIT) != 0u || mode == 1u) {
        lit = albedo;
    }
    if ((m.flags & F_SELFILLUM) != 0u && (m.flags & F_FLAT) == 0u) {
        if (el.n.w != 0u) {
            lit = lit + albedo * t.a;
        } else {
            lit = max(lit, albedo * t.a);
        }
    }
    if (m.envmap.w > 0.5 && mode != 2u) {
        // reflet de la cubemap (`$envmap`), échantillonnée avec le vecteur du monde comme dans le moteur
        var e = textureSample(cube, samp, reflect(-to_eye, n)).rgb * m.envmap.rgb;
        if (m.envmap.w > 1.5) {
            // eau : le reflet l'emporte en vue rasante (Fresnel de Schlick, eau ≈ 0,02 de face)
            let f = 0.02 + 0.98 * pow(1.0 - clamp(abs(dot(n, to_eye)), 0.0, 1.0), 5.0);
            lit = mix(lit, e, f);
        } else {
            var masque = 1.0;
            if ((m.flags & F_ENV_ALPHA_BASE) != 0u) {
                // inversé sur les brosses, pas sur les modèles (wiki Valve, « $envmapmask »)
                masque = select(1.0 - t.a, t.a, v.blend < -0.5);
            }
            if ((m.flags & F_ENV_ALPHA_2) != 0u) {
                masque = t2.a;
            }
            if ((m.flags & F_ENV_COULEUR_2) != 0u) {
                e = e * t2.rgb;
            }
            lit = lit + e * masque;
        }
    }
    let pas = g.clip_max.w;
    if (pas > 0.0) {
        // grille 3D : les plans de la grille qui coupent la face, en traits d'un pixel
        let q = abs(fract(v.world / pas + vec3<f32>(0.5)) - vec3<f32>(0.5)) * pas;
        let e = q / max(fw, vec3<f32>(1e-4));
        let an = abs(n);
        var d = 1e9;
        if (an.x < 0.95) { d = min(d, e.x); }
        if (an.y < 0.95) { d = min(d, e.y); }
        if (an.z < 0.95) { d = min(d, e.z); }
        let l = 1.0 - clamp(d - 0.5, 0.0, 1.0);
        lit = mix(lit, lit * 0.4, l * 0.8);
    }
    if (g.fog.w > 0.5 && g.eye.w > 0.5) {
        // brouillard linéaire de env_fog_controller : de fogstart à fogend, plafonné à fogmaxdensity
        let dist = length(v.world - g.eye.xyz);
        let f = clamp((dist - g.fog.x) / max(g.fog.y - g.fog.x, 1.0), 0.0, 1.0) * g.fog.z;
        lit = mix(lit, g.fog_color.rgb, f);
    }
    if (m.reflet > 0.0 && g.view_dir.w < 0.5) {
        // Vitrage : en jeu on le voit par le reflet de son envmap. Ici, un voile clair, plus fort en vue rasante,
        // pour qu'une vitre presque transparente ne passe pas pour un trou.
        let rasant = pow(1.0 - abs(dot(n, to_eye)), 3.0);
        let r = clamp(m.reflet * (0.55 + 0.45 * rasant), 0.0, 0.85) * (1.0 - alpha);
        let a = alpha + r;
        lit = (lit * alpha + vec3<f32>(0.80, 0.85, 0.92) * r) / max(a, 0.001);
        alpha = a;
    }
    var o: Out;
    o.color = vec4<f32>(lit, alpha);
    o.id = v.object;
    return o;
}
