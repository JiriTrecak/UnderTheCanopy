// Player colour on a masked albedo (old `maskedTeamColor`): the luminance-like peak of
// the authored colour, normalised by the material's source peak, shades the player
// colour; the mask chooses between them.
fn team_color(albedo: vec3f, mask: f32, player: vec3f, source_max: f32) -> vec3f {
    let shade = sqrt(max(max(albedo.r, max(albedo.g, albedo.b)) / max(source_max, 1e-4), 0.0));
    return mix(albedo, vec3f(shade) * player, clamp(mask, 0.0, 1.0));
}
