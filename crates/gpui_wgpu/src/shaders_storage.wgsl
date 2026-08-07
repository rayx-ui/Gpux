// Storage-buffer transport used by WebGPU and native backends.
// Resources intentionally share bindings because each render pipeline only reaches
// the declarations used by its own entry points.

@group(1) @binding(0) var<storage, read> b_quads: array<Quad>;
@group(1) @binding(5) var<storage, read> b_gradient_stops: array<GradientStop>;
@group(1) @binding(0) var<storage, read> b_shadows: array<Shadow>;
@group(1) @binding(0) var<storage, read> b_path_vertices: array<PathRasterizationVertex>;
@group(1) @binding(0) var<storage, read> b_path_sprites: array<PathSprite>;
@group(1) @binding(0) var<storage, read> b_underlines: array<Underline>;
@group(1) @binding(0) var<storage, read> b_mono_sprites: array<MonochromeSprite>;
@group(1) @binding(0) var<storage, read> b_poly_sprites: array<PolychromeSprite>;
@group(1) @binding(0) var<storage, read> b_html_islands: array<HtmlIsland>;
@group(1) @binding(0) var<storage, read> b_visual_effect_primitives: array<VisualEffectPrimitiveParams>;
@group(1) @binding(5) var<storage, read> b_element_effects: array<ElementEffectParams>;
@group(1) @binding(0) var<storage, read> b_screen_effects: array<ScreenEffectParams>;

fn load_gradient_stop(index: u32) -> GradientStop {
    return b_gradient_stops[index];
}

fn load_quad(index: u32) -> Quad {
    return b_quads[index];
}

fn load_shadow(index: u32) -> Shadow {
    return b_shadows[index];
}

fn load_path_vertex(index: u32) -> PathRasterizationVertex {
    return b_path_vertices[index];
}

fn load_path_sprite(index: u32) -> PathSprite {
    return b_path_sprites[index];
}

fn load_underline(index: u32) -> Underline {
    return b_underlines[index];
}

fn load_mono_sprite(index: u32) -> MonochromeSprite {
    return b_mono_sprites[index];
}

fn load_poly_sprite(index: u32) -> PolychromeSprite {
    return b_poly_sprites[index];
}

fn load_html_island(index: u32) -> HtmlIsland {
    return b_html_islands[index];
}

fn load_visual_effect_primitive(index: u32) -> VisualEffectPrimitiveParams {
    return b_visual_effect_primitives[index];
}

fn load_element_effect(index: u32) -> ElementEffectParams {
    return b_element_effects[index];
}

fn load_screen_effect(index: u32) -> ScreenEffectParams {
    return b_screen_effects[index];
}

fn screen_effect_count() -> u32 {
    return arrayLength(&b_screen_effects);
}
