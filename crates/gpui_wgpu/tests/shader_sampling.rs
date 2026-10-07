//! Fragment shaders that sample a texture never return early after the sample: some GL ES
//! drivers (Android emulators') drop every fragment of a shader that does, so text and images
//! vanish there. Clip and mask tests fold into the result instead.

use std::path::Path;

/// Each `@fragment` function in `source`, as `(name, body)`.
fn fragment_functions(source: &str) -> Vec<(String, String)> {
    let mut functions = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find("@fragment") {
        rest = &rest[start + "@fragment".len()..];
        let Some(name_start) = rest.find("fn ") else {
            break;
        };
        let name: String = rest[name_start + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let Some(open) = rest.find('{') else {
            break;
        };
        let mut depth = 0;
        let mut end = open;
        for (index, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + index;
                        break;
                    }
                }
                _ => {}
            }
        }
        functions.push((name, rest[open + 1..end].to_string()));
        rest = &rest[end..];
    }
    functions
}

#[test]
fn no_fragment_shader_returns_early_after_sampling() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for name in ["shaders.wgsl", "shaders_subpixel.wgsl"] {
        let source = std::fs::read_to_string(dir.join(name)).unwrap();
        for (function, body) in fragment_functions(&source) {
            // `textureSampleLevel` needs no derivatives and is not affected.
            let Some(sample) = body.find("textureSample(") else {
                continue;
            };
            checked += 1;
            let returns = body[sample..].matches("return ").count();
            assert_eq!(
                returns, 1,
                "{name}: `{function}` returns {returns} times after sampling; fold early exits \
                 into the result with `visible_if`"
            );
        }
    }
    // The sprite, path, HTML island and subpixel shaders sample.
    assert!(
        checked >= 5,
        "only {checked} sampling fragment shaders were found"
    );
}
