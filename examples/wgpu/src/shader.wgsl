// One full-screen triangle; everything happens in the fragment shader.

struct Uniforms {
    // Surface size in physical pixels.
    resolution: vec2<f32>,
    // Seconds since start.
    time: f32,
    // Touch influence: eases to 1.0 while a finger is down, decays after.
    strength: f32,
    // Safe area: left, top, right, bottom edge reservations in pixels.
    safe: vec4<f32>,
    // Last touch position in pixels.
    touch: vec2<f32>,
    // Sound level in 0..1 (the audiocapture example); 0 draws the plain plasma.
    level: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // (-1,-1), (3,-1), (-1,3): covers the viewport with one triangle.
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// Sound as a second source in the field: rings spreading fast from the
// centre of the screen, as strong as the level. Zero when silent.
fn sound(q: vec2<f32>) -> f32 {
    let centre = u.resolution * 0.5 / min(u.resolution.x, u.resolution.y) * 6.0;
    let d = length(q - centre);
    return sin(d * 3.0 - u.time * 9.0) * 3.0 * exp(-d * 0.15) * u.level;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let inside = p.x >= u.safe.x && p.y >= u.safe.y
        && p.x < u.resolution.x - u.safe.z && p.y < u.resolution.y - u.safe.w;

    // Plasma, in coordinates normalised to the shorter side so it is not
    // stretched on a tall phone.
    let q = p / min(u.resolution.x, u.resolution.y) * 6.0;
    let t = u.time;
    // The touch is a source in the field itself: ripples spread from it
    // across the screen (well beyond the fingertip), falling off with
    // distance, and fade out after release.
    let tq = u.touch / min(u.resolution.x, u.resolution.y) * 6.0;
    let dt = length(q - tq);
    let source = sin(dt * 4.0 - t * 6.0) * 2.5 * exp(-dt * 0.5) * u.strength;
    let v = sin(q.x + t) + sin(q.y * 0.8 - t * 1.3)
        + sin((q.x + q.y) * 0.6 + t * 0.7)
        + sin(length(q - vec2<f32>(3.0, 6.0)) * 1.5 - t * 2.0)
        + source
        + sound(q);
    var color = 0.5 + 0.5 * cos(vec3<f32>(0.0, 2.1, 4.2) + v * 1.2 + t * 0.2);
    // Louder is brighter.
    color = color * (1.0 + 0.8 * u.level);

    // A ripple ring expanding from the last touch.
    let d = length(p - u.touch);
    let ring = exp(-abs(d - 90.0 - 40.0 * sin(t * 6.0)) * 0.08) * u.strength;
    color = color + vec3<f32>(ring);

    if (!inside) {
        // Reserved edges: dimmed and desaturated, so the bars and cutout are
        // visibly not app content.
        let grey = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        color = vec3<f32>(grey * 0.25);
    }
    return vec4<f32>(color, 1.0);
}
