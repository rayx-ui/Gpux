use crate::color::{
    Color, ColorComponent, ColorError, ColorSpace, EncodedColor, HexRepresentation,
};

const MAX_CSS_COLOR_LENGTH: usize = 4096;

pub(crate) fn parse_css(input: &str) -> Result<Color, ColorError> {
    let text = input.trim();
    if text.len() > MAX_CSS_COLOR_LENGTH {
        return Err(syntax(MAX_CSS_COLOR_LENGTH));
    }
    if text.starts_with('#') {
        return Color::parse_hex(text, HexRepresentation::Rgba);
    }
    let open = text.find('(').ok_or_else(|| syntax(0))?;
    if !text.ends_with(')') {
        return Err(syntax(text.len()));
    }
    let name = text[..open].trim().to_ascii_lowercase();
    let body = &text[open + 1..text.len() - 1];
    match name.as_str() {
        "rgb" | "rgba" => parse_rgb(body),
        "hsl" | "hsla" => parse_function(ColorSpace::Hsl, body, ComponentSyntax::Hsl),
        "hwb" => parse_function(ColorSpace::Hwb, body, ComponentSyntax::Hwb),
        "lab" => parse_function(ColorSpace::Lab, body, ComponentSyntax::Lab),
        "lch" => parse_function(ColorSpace::Lch, body, ComponentSyntax::Lab),
        "oklab" => parse_function(ColorSpace::Oklab, body, ComponentSyntax::Oklab),
        "oklch" => parse_function(ColorSpace::Oklch, body, ComponentSyntax::Oklab),
        "color" => parse_color_function(body),
        _ => Err(syntax(0)),
    }
}

fn parse_rgb(body: &str) -> Result<Color, ColorError> {
    let (component_text, explicit_alpha) = split_alpha(body)?;
    let comma_legacy = component_text.contains(',');
    let mut tokens = tokens(component_text);
    let alpha = if explicit_alpha.is_some() {
        parse_alpha(explicit_alpha.unwrap_or(""))?
    } else if comma_legacy && tokens.len() == 4 {
        parse_alpha(tokens.pop().unwrap_or(""))?
    } else {
        1.0
    };
    if tokens.len() != 3 {
        return Err(syntax(0));
    }
    let components = [
        parse_component(tokens[0], NumberScale::Rgb)?,
        parse_component(tokens[1], NumberScale::Rgb)?,
        parse_component(tokens[2], NumberScale::Rgb)?,
    ];
    Color::from_encoded(EncodedColor::new(ColorSpace::Srgb, components, alpha)?)
}

#[derive(Clone, Copy)]
enum ComponentSyntax {
    Hsl,
    Hwb,
    Lab,
    Oklab,
}

fn parse_function(
    space: ColorSpace,
    body: &str,
    syntax_kind: ComponentSyntax,
) -> Result<Color, ColorError> {
    let (component_text, alpha_text) = split_alpha(body)?;
    let values = tokens(component_text);
    if values.len() != 3 {
        return Err(syntax(0));
    }
    let scales = match syntax_kind {
        ComponentSyntax::Hsl | ComponentSyntax::Hwb => [
            NumberScale::Angle,
            NumberScale::UnitPercent,
            NumberScale::UnitPercent,
        ],
        ComponentSyntax::Lab => [
            NumberScale::LabLightness,
            NumberScale::Raw,
            NumberScale::AngleOrRaw,
        ],
        ComponentSyntax::Oklab => [
            NumberScale::OklabLightness,
            NumberScale::Raw,
            NumberScale::AngleOrRaw,
        ],
    };
    let components = [
        parse_component(values[0], scales[0])?,
        parse_component(values[1], scales[1])?,
        parse_component(values[2], scales[2])?,
    ];
    let alpha = alpha_text.map(parse_alpha).transpose()?.unwrap_or(1.0);
    Color::from_encoded(EncodedColor::new(space, components, alpha)?)
}

fn parse_color_function(body: &str) -> Result<Color, ColorError> {
    let (component_text, alpha_text) = split_alpha(body)?;
    let values = tokens(component_text);
    if values.len() != 4 {
        return Err(syntax(0));
    }
    let space = match values[0].to_ascii_lowercase().as_str() {
        "srgb" => ColorSpace::Srgb,
        "srgb-linear" => ColorSpace::LinearSrgb,
        "display-p3" => ColorSpace::DisplayP3,
        "a98-rgb" => ColorSpace::A98Rgb,
        "prophoto-rgb" => ColorSpace::ProPhotoRgb,
        "rec2020" => ColorSpace::Rec2020,
        "xyz" | "xyz-d65" => ColorSpace::XyzD65,
        "xyz-d50" => ColorSpace::XyzD50,
        _ => return Err(syntax(0)),
    };
    let components = [
        parse_component(values[1], NumberScale::Raw)?,
        parse_component(values[2], NumberScale::Raw)?,
        parse_component(values[3], NumberScale::Raw)?,
    ];
    let alpha = alpha_text.map(parse_alpha).transpose()?.unwrap_or(1.0);
    Color::from_encoded(EncodedColor::new(space, components, alpha)?)
}

fn split_alpha(body: &str) -> Result<(&str, Option<&str>), ColorError> {
    let mut parts = body.split('/');
    let components = parts.next().unwrap_or("");
    let alpha = parts.next().map(str::trim);
    if parts.next().is_some() {
        return Err(syntax(0));
    }
    Ok((components, alpha))
}

fn tokens(text: &str) -> Vec<&str> {
    text.split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
        .collect()
}

#[derive(Clone, Copy)]
enum NumberScale {
    Raw,
    Rgb,
    UnitPercent,
    LabLightness,
    OklabLightness,
    Angle,
    AngleOrRaw,
}

fn parse_component(text: &str, scale: NumberScale) -> Result<ColorComponent, ColorError> {
    if text.eq_ignore_ascii_case("none") {
        return Ok(ColorComponent::None);
    }
    let value = match scale {
        NumberScale::Angle | NumberScale::AngleOrRaw => parse_angle(text)?,
        NumberScale::Rgb => {
            if let Some(percent) = text.strip_suffix('%') {
                parse_number(percent)? / 100.0
            } else {
                parse_number(text)? / 255.0
            }
        }
        NumberScale::UnitPercent => parse_percentage(text, 1.0)?,
        NumberScale::LabLightness => parse_percentage(text, 100.0)?,
        NumberScale::OklabLightness => parse_percentage(text, 1.0)?,
        NumberScale::Raw => parse_number(text)?,
    };
    Ok(ColorComponent::Number(value))
}

fn parse_percentage(text: &str, percent_max: f64) -> Result<f64, ColorError> {
    if let Some(value) = text.strip_suffix('%') {
        Ok(parse_number(value)? * percent_max / 100.0)
    } else {
        parse_number(text)
    }
}

fn parse_alpha(text: &str) -> Result<f64, ColorError> {
    parse_percentage(text.trim(), 1.0)
}

fn parse_angle(text: &str) -> Result<f64, ColorError> {
    if let Some(value) = text.strip_suffix("deg") {
        return parse_number(value);
    }
    if let Some(value) = text.strip_suffix("grad") {
        return Ok(parse_number(value)? * 0.9);
    }
    if let Some(value) = text.strip_suffix("rad") {
        return Ok(parse_number(value)?.to_degrees());
    }
    if let Some(value) = text.strip_suffix("turn") {
        return Ok(parse_number(value)? * 360.0);
    }
    parse_number(text)
}

fn parse_number(text: &str) -> Result<f64, ColorError> {
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| syntax(0))
}

const fn syntax(offset: usize) -> ColorError {
    ColorError::InvalidCssSyntax { offset }
}
