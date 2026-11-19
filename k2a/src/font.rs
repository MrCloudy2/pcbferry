//! Widths of the stroke font KiCad draws text with (newstroke, ASCII), in 1/21 of the text
//! size: what KiCad's importer measures when it re-centres text, so ours lands where it was.
//! Generated from KiCad's common/newstroke_font.cpp (each glyph's first two characters).

const ADVANCE: [u8; 95] = [
    16, 10, 16, 21, 20, 24, 26, 10, 14, 14, 16, 26, 10, 26, 10, 22, 20, 20, 20, 20, 20, 20, 20, 20,
    20, 20, 10, 10, 26, 26, 26, 18, 27, 18, 21, 21, 21, 19, 18, 21, 22, 10, 16, 21, 17, 24, 22, 22,
    21, 22, 21, 20, 16, 22, 18, 24, 20, 18, 20, 14, 14, 14, 12, 16, 8, 19, 19, 18, 19, 18, 12, 19,
    19, 10, 10, 17, 11, 28, 19, 19, 19, 19, 13, 17, 12, 19, 16, 22, 17, 16, 17, 14, 20, 14, 15,
];

/// Advance of one character in 1/21 of the text width; others take an "M"-like width
fn advance(c: char) -> u32 {
    let i = c as u32;
    if (0x20..0x7f).contains(&i) {
        ADVANCE[(i - 0x20) as usize] as u32
    } else {
        ADVANCE[(b'M' - 0x20) as usize] as u32
    }
}

/// Width of the longest line of a text drawn KiCad's stroke font at this glyph width
pub fn text_width(text: &str, glyph_width: i64) -> i64 {
    text.split('\n')
        .map(|line| line.chars().map(advance).sum::<u32>())
        .max()
        .map(|units| (units as f64 * glyph_width as f64 / 21.0).round() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn widths() {
        // Space is 16/21 of the size, "M" 24/21 in newstroke
        assert_eq!(super::advance(' '), 16);
        assert!(super::text_width("MM", 2100) > super::text_width("ii", 2100));
    }
}
