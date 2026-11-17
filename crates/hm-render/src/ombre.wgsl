// Cartes d'ombre de l'aperçu de l'éclairage : seulement la profondeur de ce qui fait de l'ombre.

struct M { m: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: M;

@vertex
fn vs_monde(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> {
    return u.m * vec4<f32>(pos, 1.0);
}

@vertex
fn vs_prop(
    @location(0) pos: vec3<f32>,
    @location(4) m0: vec4<f32>, @location(5) m1: vec4<f32>, @location(6) m2: vec4<f32>, @location(7) m3: vec4<f32>,
    @location(10) drapeaux: u32,
) -> @builtin(position) vec4<f32> {
    // un modèle qui ne fait pas d'ombre (prop_dynamic, disableshadows…) : hors du volume, le triangle disparaît
    if ((drapeaux & 1u) == 0u) {
        return vec4<f32>(0.0, 0.0, -2.0, 1.0);
    }
    let model = mat4x4<f32>(m0, m1, m2, m3);
    return u.m * (model * vec4<f32>(pos, 1.0));
}
