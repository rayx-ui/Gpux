// WebGL2 has no storage buffers. Each storage binding is uploaded as an
// Rgba32Uint texture so the exact Rust record bytes remain available to both
// shader stages. Binding 0 carries primary records and binding 1 carries the
// auxiliary array used by gradients and visual effects.

@group(1) @binding(0) var t_instances: texture_2d<u32>;
@group(1) @binding(5) var t_aux_instances: texture_2d<u32>;

struct InstanceCursor {
    word_index: u32,
    texel_index: u32,
    texel: vec4<u32>,
    width: u32,
    binding: u32,
}

fn fetch_instance_texel(texel_index: u32, width: u32, binding: u32) -> vec4<u32> {
    let coordinate = vec2<i32>(i32(texel_index % width), i32(texel_index / width));
    if (binding == 0u) {
        return textureLoad(t_instances, coordinate, 0);
    }
    return textureLoad(t_aux_instances, coordinate, 0);
}

fn instance_cursor(word_index: u32, binding: u32) -> InstanceCursor {
    let width = select(textureDimensions(t_aux_instances).x, textureDimensions(t_instances).x, binding == 0u);
    let texel_index = word_index / 4u;
    return InstanceCursor(
        word_index,
        texel_index,
        fetch_instance_texel(texel_index, width, binding),
        width,
        binding,
    );
}

fn read_word(cursor: ptr<function, InstanceCursor>) -> u32 {
    let word_index = (*cursor).word_index;
    let texel_index = word_index / 4u;
    if (texel_index != (*cursor).texel_index) {
        (*cursor).texel = fetch_instance_texel(texel_index, (*cursor).width, (*cursor).binding);
        (*cursor).texel_index = texel_index;
    }
    (*cursor).word_index = word_index + 1u;
    return (*cursor).texel[word_index % 4u];
}

fn skip_words(cursor: ptr<function, InstanceCursor>, count: u32) {
    (*cursor).word_index += count;
}

fn read_f32(cursor: ptr<function, InstanceCursor>) -> f32 {
    return bitcast<f32>(read_word(cursor));
}

fn read_i32(cursor: ptr<function, InstanceCursor>) -> i32 {
    return bitcast<i32>(read_word(cursor));
}

fn read_vec2_f32(cursor: ptr<function, InstanceCursor>) -> vec2<f32> {
    return vec2<f32>(read_f32(cursor), read_f32(cursor));
}

fn read_vec2_i32(cursor: ptr<function, InstanceCursor>) -> vec2<i32> {
    return vec2<i32>(read_i32(cursor), read_i32(cursor));
}

fn read_vec4_f32(cursor: ptr<function, InstanceCursor>) -> vec4<f32> {
    return vec4<f32>(read_f32(cursor), read_f32(cursor), read_f32(cursor), read_f32(cursor));
}

fn read_bounds(cursor: ptr<function, InstanceCursor>) -> Bounds {
    return Bounds(read_vec2_f32(cursor), read_vec2_f32(cursor));
}

fn read_corners(cursor: ptr<function, InstanceCursor>) -> Corners {
    return Corners(read_f32(cursor), read_f32(cursor), read_f32(cursor), read_f32(cursor));
}

fn read_content_mask(cursor: ptr<function, InstanceCursor>) -> ContentMask {
    return ContentMask(read_bounds(cursor), read_corners(cursor));
}

fn read_edges(cursor: ptr<function, InstanceCursor>) -> Edges {
    return Edges(read_f32(cursor), read_f32(cursor), read_f32(cursor), read_f32(cursor));
}

fn read_paint_color(cursor: ptr<function, InstanceCursor>) -> PaintColor {
    return PaintColor(read_f32(cursor), read_f32(cursor), read_f32(cursor), read_f32(cursor));
}

fn read_gradient_stop(cursor: ptr<function, InstanceCursor>) -> GradientStop {
    let stop = GradientStop(read_vec4_f32(cursor), read_f32(cursor));
    skip_words(cursor, 3u);
    return stop;
}

fn read_background(cursor: ptr<function, InstanceCursor>) -> Background {
    let tag = read_word(cursor);
    let color_space = read_word(cursor);
    let solid = read_paint_color(cursor);
    let value = read_f32(cursor);
    let stop_offset = read_word(cursor);
    let stop_count = read_word(cursor);
    skip_words(cursor, 3u);
    return Background(tag, color_space, solid, value, stop_offset, stop_count, vec2<u32>(0u));
}

fn read_atlas_tile(cursor: ptr<function, InstanceCursor>) -> AtlasTile {
    return AtlasTile(
        AtlasTextureId(read_word(cursor), read_word(cursor)),
        read_word(cursor),
        read_word(cursor),
        AtlasBounds(read_vec2_i32(cursor), read_vec2_i32(cursor)),
    );
}

fn read_transformation(cursor: ptr<function, InstanceCursor>) -> TransformationMatrix {
    return TransformationMatrix(
        mat2x2<f32>(read_vec2_f32(cursor), read_vec2_f32(cursor)),
        read_vec2_f32(cursor),
    );
}

fn load_gradient_stop(index: u32) -> GradientStop {
    var cursor = instance_cursor(index * 8u, 1u);
    return read_gradient_stop(&cursor);
}

fn load_quad(index: u32) -> Quad {
    var cursor = instance_cursor(index * 52u, 0u);
    return Quad(
        read_word(&cursor),
        read_word(&cursor),
        read_bounds(&cursor),
        read_content_mask(&cursor),
        read_background(&cursor),
        read_background(&cursor),
        read_corners(&cursor),
        read_edges(&cursor),
        read_transformation(&cursor),
    );
}

fn load_shadow(index: u32) -> Shadow {
    var cursor = instance_cursor(index * 38u, 0u);
    return Shadow(
        read_word(&cursor),
        read_f32(&cursor),
        read_bounds(&cursor),
        read_corners(&cursor),
        read_content_mask(&cursor),
        read_paint_color(&cursor),
        read_bounds(&cursor),
        read_corners(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_transformation(&cursor),
    );
}

fn load_path_vertex(index: u32) -> PathRasterizationVertex {
    var cursor = instance_cursor(index * 24u, 0u);
    return PathRasterizationVertex(
        read_vec2_f32(&cursor),
        read_vec2_f32(&cursor),
        read_background(&cursor),
        read_content_mask(&cursor),
    );
}

fn load_path_sprite(index: u32) -> PathSprite {
    var cursor = instance_cursor(index * 4u, 0u);
    return PathSprite(read_bounds(&cursor));
}

fn load_underline(index: u32) -> Underline {
    var cursor = instance_cursor(index * 26u, 0u);
    return Underline(
        read_word(&cursor),
        read_word(&cursor),
        read_bounds(&cursor),
        read_content_mask(&cursor),
        read_paint_color(&cursor),
        read_f32(&cursor),
        read_word(&cursor),
        read_transformation(&cursor),
    );
}

fn load_mono_sprite(index: u32) -> MonochromeSprite {
    var cursor = instance_cursor(index * 32u, 0u);
    return MonochromeSprite(
        read_word(&cursor),
        read_word(&cursor),
        read_bounds(&cursor),
        read_content_mask(&cursor),
        read_paint_color(&cursor),
        read_atlas_tile(&cursor),
        read_transformation(&cursor),
    );
}

fn load_poly_sprite(index: u32) -> PolychromeSprite {
    var cursor = instance_cursor(index * 34u, 0u);
    return PolychromeSprite(
        read_word(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_f32(&cursor),
        read_bounds(&cursor),
        read_content_mask(&cursor),
        read_corners(&cursor),
        read_atlas_tile(&cursor),
        read_transformation(&cursor),
    );
}

fn load_html_island(index: u32) -> HtmlIsland {
    var cursor = instance_cursor(index * 16u, 0u);
    let island = HtmlIsland(
        read_bounds(&cursor),
        read_content_mask(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
    );
    return island;
}

fn load_visual_effect_primitive(index: u32) -> VisualEffectPrimitiveParams {
    var cursor = instance_cursor(index * 26u, 0u);
    return VisualEffectPrimitiveParams(
        read_bounds(&cursor),
        read_content_mask(&cursor),
        read_corners(&cursor),
        read_f32(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_transformation(&cursor),
    );
}

fn load_element_effect(index: u32) -> ElementEffectParams {
    var cursor = instance_cursor(index * 20u, 1u);
    return ElementEffectParams(
        read_word(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_vec4_f32(&cursor),
        read_vec4_f32(&cursor),
        read_vec4_f32(&cursor),
        read_vec4_f32(&cursor),
    );
}

fn load_screen_effect(index: u32) -> ScreenEffectParams {
    var cursor = instance_cursor(index * 24u, 0u);
    return ScreenEffectParams(
        read_word(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_word(&cursor),
        read_bounds(&cursor),
        read_vec4_f32(&cursor),
        read_vec2_f32(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
        read_f32(&cursor),
        read_vec4_f32(&cursor),
    );
}

fn screen_effect_count() -> u32 {
    let dimensions = textureDimensions(t_instances);
    return dimensions.x * dimensions.y * 4u / 24u;
}
