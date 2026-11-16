use crate::RenderError;

/// The one shader the renderer uses for now: instanced, textured quads.
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
/// sRGB, and colours are multiplied in gamma space, which is what UI
/// toolkits such as egui expect.
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

fn linear_from_gamma(gamma: vec3<f32>) -> vec3<f32> {
    let low = gamma / 12.92;
    let high = pow((gamma + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, gamma < vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(mesh_texture, mesh_sampler, in.uv);
    let gamma = in.color * vec4<f32>(gamma_from_linear(texel.rgb), texel.a);
    return vec4<f32>(linear_from_gamma(gamma.rgb), gamma.a);
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
    fn shader_mistakes_are_reported_with_source() {
        let err = compile_wgsl("fn broken( {").unwrap_err().to_string();
        assert!(err.contains("broken"), "{err}");
    }
}
