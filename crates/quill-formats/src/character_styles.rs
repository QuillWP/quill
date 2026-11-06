//! Character-style metadata for exports whose presentation is already expanded.
//! Readers validate every value and keep this separate from executable content.
use quill_core::CharProps;
use quill_core::character_styles::CharacterFormat;

pub fn encode(format: CharacterFormat) -> String {
    let c = format.props;
    let underline = match c.underline {
        quill_core::Underline::None => 0,
        quill_core::Underline::Single => 1,
        quill_core::Underline::Word => 2,
        quill_core::Underline::Double => 3,
        quill_core::Underline::Dotted => 4,
    };
    format!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        format.mask,
        c.size,
        u8::from(c.bold),
        u8::from(c.italic),
        underline,
        u8::from(c.strike),
        u8::from(c.outline),
        u8::from(c.shadow),
        u8::from(c.small_caps),
        u8::from(c.all_caps),
        u8::from(c.hidden),
        c.color.0,
        c.position,
        c.spacing
    )
}

pub fn decode(text: &str, font: u16) -> Option<CharacterFormat> {
    if text.len() > 256 {
        return None;
    }
    let values: Vec<i64> = text
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if values.len() != 14
        || [2, 3, 5, 6, 7, 8, 9, 10]
            .iter()
            .any(|&i| !(0..=1).contains(&values[i]))
    {
        return None;
    }
    let props = CharProps {
        font,
        size: u16::try_from(values[1]).ok()?,
        bold: values[2] != 0,
        italic: values[3] != 0,
        underline: match values[4] {
            0 => quill_core::Underline::None,
            1 => quill_core::Underline::Single,
            2 => quill_core::Underline::Word,
            3 => quill_core::Underline::Double,
            4 => quill_core::Underline::Dotted,
            _ => return None,
        },
        strike: values[5] != 0,
        outline: values[6] != 0,
        shadow: values[7] != 0,
        small_caps: values[8] != 0,
        all_caps: values[9] != 0,
        hidden: values[10] != 0,
        color: quill_core::Rgb(u32::try_from(values[11]).ok()? & 0xFF_FFFF),
        position: i32::try_from(values[12]).ok()?,
        spacing: i32::try_from(values[13]).ok()?,
        ..Default::default()
    };
    Some(CharacterFormat::new(props, u16::try_from(values[0]).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_metadata_is_rejected_and_valid_values_round_trip() {
        let format = CharacterFormat::new(
            CharProps {
                italic: true,
                size: 36,
                color: quill_core::Rgb(0x123456),
                ..Default::default()
            },
            CharacterFormat::ALL,
        );
        assert_eq!(decode(&encode(format), 0), Some(format));
        for value in [
            "".to_string(),
            "1,2".into(),
            "1,".repeat(1000),
            "-1,24,0,0,0,0,0,0,0,0,0,0,0,0".into(),
            "0,999999999999999999999,0,0,0,0,0,0,0,0,0,0,0,0".into(),
            "0,24,9,0,0,0,0,0,0,0,0,0,0,0".into(),
        ] {
            assert!(decode(&value, 0).is_none());
        }
    }
}
