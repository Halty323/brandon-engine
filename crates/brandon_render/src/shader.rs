use crate::RenderError;

/// The shader for instanced, textured quads: sprites and text.
pub(crate) const QUAD_WGSL: &str = r#"
struct Globals {
    clip_from_world: mat4x4<f32>,
}
var<push_constant> globals: Globals;

struct Instance {
    @location(0) axes: vec4<f32>,
    @location(1) origin: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
}

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let corner = corners[index];
    let world = instance.origin + instance.axes.xy * corner.x + instance.axes.zw * corner.y;
    var out: VsOut;
    out.position = globals.clip_from_world * vec4<f32>(world, 0.0, 1.0);
    out.uv = mix(instance.uv.xy, instance.uv.zw, corner);
    out.color = instance.color;
    return out;
}

@group(0) @binding(0) var quad_sampler: sampler;
@group(1) @binding(0) var quad_texture: texture_2d<f32>;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Textures and colours are premultiplied by alpha.
    return textureSample(quad_texture, quad_sampler, in.uv) * in.color;
}
"#;

/// The shader for UI meshes. Vertex colours and textures are premultiplied
/// sRGB. It draws into a gamma-space image, so colours are multiplied and
/// blended in gamma space, which is what UI toolkits such as egui expect.
pub(crate) const MESH_WGSL: &str = r#"
struct Globals {
    clip_from_world: mat4x4<f32>,
}
var<push_constant> globals: Globals;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.position = globals.clip_from_world * vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@group(0) @binding(0) var mesh_sampler: sampler;
@group(1) @binding(0) var mesh_texture: texture_2d<f32>;

fn gamma_from_linear(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, linear < vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // sRGB textures are read as linear values; turn them back into gamma.
    let texel = textureSample(mesh_texture, mesh_sampler, in.uv);
    return in.color * vec4<f32>(gamma_from_linear(texel.rgb), texel.a);
}
"#;

/// Full-window passes for the backdrop: shrinking and growing the image to
/// blur it (the "dual Kawase" filter), and copying it to the window.
pub(crate) const FULLSCREEN_WGSL: &str = r#"
struct Params {
    // One texel of the image being read, in texture coordinates.
    texel: vec2<f32>,
    // How far apart the samples are, in texels.
    spread: f32,
    // 0 shrinks, 1 grows, 2 copies, 3 copies from linear to gamma, 4 copies
    // from gamma to linear.
    mode: u32,
    // 1.0 leaves colours as they are; more makes them more vivid.
    saturation: f32,
}
var<push_constant> params: Params;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    // One triangle that covers the whole target.
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VsOut;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@group(0) @binding(0) var source_sampler: sampler;
@group(1) @binding(0) var source: texture_2d<f32>;

fn at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(source, source_sampler, uv, 0.0);
}

fn gamma_from_linear(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, linear < vec3<f32>(0.0031308));
}

fn linear_from_gamma(gamma: vec3<f32>) -> vec3<f32> {
    let low = gamma / 12.92;
    let high = pow((gamma + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, gamma < vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if params.mode == 3u {
        let c = at(in.uv);
        return vec4<f32>(gamma_from_linear(max(c.rgb, vec3<f32>(0.0))), 1.0);
    }
    if params.mode == 4u {
        let c = at(in.uv);
        return vec4<f32>(linear_from_gamma(c.rgb), 1.0);
    }
    let o = params.texel * params.spread;
    let uv = in.uv;
    var color: vec4<f32>;
    if params.mode == 0u {
        color = at(uv) * 4.0
            + at(uv - o)
            + at(uv + o)
            + at(uv + vec2<f32>(o.x, -o.y))
            + at(uv - vec2<f32>(o.x, -o.y));
        color = color / 8.0;
    } else if params.mode == 1u {
        color = at(uv + vec2<f32>(-o.x * 2.0, 0.0))
            + at(uv + vec2<f32>(-o.x, o.y)) * 2.0
            + at(uv + vec2<f32>(0.0, o.y * 2.0))
            + at(uv + vec2<f32>(o.x, o.y)) * 2.0
            + at(uv + vec2<f32>(o.x * 2.0, 0.0))
            + at(uv + vec2<f32>(o.x, -o.y)) * 2.0
            + at(uv + vec2<f32>(0.0, -o.y * 2.0))
            + at(uv + vec2<f32>(-o.x, -o.y)) * 2.0;
        color = color / 12.0;
    } else {
        color = at(uv);
    }
    let luma = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let vivid = mix(vec3<f32>(luma), color.rgb, params.saturation);
    return vec4<f32>(max(vivid, vec3<f32>(0.0)), color.a);
}
"#;

/// Compiles WGSL to SPIR-V.
pub(crate) fn compile_wgsl(source: &str) -> Result<Vec<u32>, RenderError> {
    use naga::back::spv;
    use naga::valid::{Capabilities, ValidationFlags, Validator};

    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| RenderError::Shader(e.emit_to_string(source)))?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| RenderError::Shader(e.emit_to_string(source)))?;
    // No coordinate adjustment: the renderer works in Vulkan's own clip space,
    // where `y` points down, like the engine's world.
    let options = spv::Options {
        flags: spv::WriterFlags::empty(),
        ..Default::default()
    };
    spv::write_vec(&module, &info, &options, None).map_err(|e| RenderError::Shader(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_shader_compiles() {
        let words = compile_wgsl(QUAD_WGSL).unwrap();
        assert_eq!(words[0], 0x0723_0203, "SPIR-V magic number");
    }

    #[test]
    fn mesh_shader_compiles() {
        compile_wgsl(MESH_WGSL).unwrap();
    }

    #[test]
    fn fullscreen_shader_compiles() {
        compile_wgsl(FULLSCREEN_WGSL).unwrap();
    }

    #[test]
    fn shader_mistakes_are_reported_with_source() {
        let err = compile_wgsl("fn broken( {").unwrap_err().to_string();
        assert!(err.contains("broken"), "{err}");
    }
}
