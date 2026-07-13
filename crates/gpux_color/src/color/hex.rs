use crate::color::{Color, ColorError, HexRepresentation};

pub(crate) fn parse_hex(
    input: &str,
    representation: HexRepresentation,
) -> Result<Color, ColorError> {
    let text = input.trim().strip_prefix('#').unwrap_or(input.trim());
    let bytes = match text.len() {
        3 => {
            let r = repeat_nibble(parse_nibble(text, 0)?);
            let g = repeat_nibble(parse_nibble(text, 1)?);
            let b = repeat_nibble(parse_nibble(text, 2)?);
            [r, g, b, 0xff]
        }
        4 => {
            let r = repeat_nibble(parse_nibble(text, 0)?);
            let g = repeat_nibble(parse_nibble(text, 1)?);
            let b = repeat_nibble(parse_nibble(text, 2)?);
            let a = repeat_nibble(parse_nibble(text, 3)?);
            [r, g, b, a]
        }
        6 => [
            parse_byte(text, 0)?,
            parse_byte(text, 2)?,
            parse_byte(text, 4)?,
            0xff,
        ],
        8 => match representation {
            HexRepresentation::Rgba => [
                parse_byte(text, 0)?,
                parse_byte(text, 2)?,
                parse_byte(text, 4)?,
                parse_byte(text, 6)?,
            ],
            HexRepresentation::Argb => [
                parse_byte(text, 2)?,
                parse_byte(text, 4)?,
                parse_byte(text, 6)?,
                parse_byte(text, 0)?,
            ],
        },
        _ => return Err(ColorError::InvalidHex),
    };

    Color::from_encoded_srgb(
        f64::from(bytes[0]) / 255.0,
        f64::from(bytes[1]) / 255.0,
        f64::from(bytes[2]) / 255.0,
        f64::from(bytes[3]) / 255.0,
    )
}

fn parse_byte(text: &str, index: usize) -> Result<u8, ColorError> {
    let high = parse_nibble(text, index)?;
    let low = parse_nibble(text, index + 1)?;
    Ok((high << 4) | low)
}

fn parse_nibble(text: &str, index: usize) -> Result<u8, ColorError> {
    text.as_bytes()
        .get(index)
        .copied()
        .and_then(|byte| char::from(byte).to_digit(16))
        .map(|digit| digit as u8)
        .ok_or(ColorError::InvalidHex)
}

fn repeat_nibble(nibble: u8) -> u8 {
    (nibble << 4) | nibble
}
