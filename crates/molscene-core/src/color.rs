//! Color resolution — the Rust-side source of truth for palettes.
//!
//! The renderer is molecule-agnostic, so molscene resolves every atom to a
//! concrete RGB here: CPK by element, a per-chain cycling palette, a residue
//! "spectrum" rainbow, or a fixed named/hex color. Values follow PyMOL
//! (`layer1/Color.cpp`). RGB components are floats in 0–1.

/// RGB color, components in 0–1.
pub type Rgb = [f32; 3];

/// How a representation should be colored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorScheme {
    /// CPK coloring by element.
    Element,
    /// Rainbow across residue order.
    Spectrum,
    /// Cycling palette per chain.
    Chain,
    /// A single fixed color.
    Fixed(Rgb),
}

impl ColorScheme {
    /// Parse a `color=` value. Recognizes the scheme keywords, named colors, and
    /// `#rrggbb`; anything unknown falls back to [`ColorScheme::Element`].
    pub fn parse(value: &str) -> ColorScheme {
        let v = value.trim().to_ascii_lowercase();
        match v.as_str() {
            "element" | "cpk" => ColorScheme::Element,
            "spectrum" | "rainbow" => ColorScheme::Spectrum,
            "chain" | "bychain" => ColorScheme::Chain,
            _ => {
                if let Some(rgb) = named_color(&v).or_else(|| parse_hex(&v)) {
                    ColorScheme::Fixed(rgb)
                } else {
                    eprintln!("molscene: unknown color {value:?}; defaulting to element coloring.");
                    ColorScheme::Element
                }
            }
        }
    }
}

/// CPK color for an element symbol (PyMOL values); unknown → light grey.
pub fn element_color(element: &str) -> Rgb {
    match element.trim().to_ascii_uppercase().as_str() {
        "C" => [0.2, 1.0, 0.2],
        "N" => [0.2, 0.2, 1.0],
        "O" => [1.0, 0.3, 0.3],
        "H" => [0.9, 0.9, 0.9],
        "S" => [0.9, 0.775, 0.25],
        "P" => [1.0, 0.5, 0.0],
        "FE" => [0.88, 0.40, 0.20],
        "CL" => [0.1, 0.9, 0.1],
        "NA" => [0.67, 0.36, 0.95],
        "MG" => [0.54, 1.0, 0.0],
        "ZN" => [0.49, 0.50, 0.69],
        "CA" => [0.24, 1.0, 0.0],
        _ => [0.8, 0.8, 0.8],
    }
}

/// Cycling per-chain palette (PyMOL AutoColor-style), indexed by chain ordinal.
pub fn chain_color(chain_index: usize) -> Rgb {
    const PALETTE: [Rgb; 10] = [
        [0.0, 1.0, 1.0],    // cyan
        [1.0, 0.0, 1.0],    // magenta
        [1.0, 1.0, 0.0],    // yellow
        [1.0, 0.6, 0.6],    // salmon
        [0.5, 0.5, 1.0],    // slate
        [1.0, 0.5, 0.0],    // orange
        [0.5, 1.0, 0.5],    // lime
        [0.1, 0.6, 0.6],    // deepteal
        [1.0, 0.4, 0.7],    // hotpink
        [0.99, 0.82, 0.65], // wheat
    ];
    PALETTE[chain_index % PALETTE.len()]
}

/// Rainbow spectrum color for `t` in 0–1 (blue → green → red).
pub fn spectrum_color(t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    // Hue 240° (blue) down to 0° (red).
    hsv_to_rgb(240.0 * (1.0 - t), 1.0, 1.0)
}

fn hsv_to_rgb(h_deg: f32, s: f32, v: f32) -> Rgb {
    let h = (h_deg.rem_euclid(360.0)) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

/// Named colors (basic set + the chain palette names).
pub fn named_color(name: &str) -> Option<Rgb> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "white" => [1.0, 1.0, 1.0],
        "black" => [0.0, 0.0, 0.0],
        "red" => [1.0, 0.0, 0.0],
        "green" => [0.0, 1.0, 0.0],
        "blue" => [0.0, 0.0, 1.0],
        "yellow" => [1.0, 1.0, 0.0],
        "cyan" => [0.0, 1.0, 1.0],
        "magenta" => [1.0, 0.0, 1.0],
        "grey" | "gray" => [0.5, 0.5, 0.5],
        "salmon" => [1.0, 0.6, 0.6],
        "slate" => [0.5, 0.5, 1.0],
        "orange" => [1.0, 0.5, 0.0],
        "lime" => [0.5, 1.0, 0.5],
        "deepteal" => [0.1, 0.6, 0.6],
        "hotpink" => [1.0, 0.4, 0.7],
        "wheat" => [0.99, 0.82, 0.65],
        _ => return None,
    })
}

/// Parse `#rrggbb` (with or without the leading `#`).
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgb, b: Rgb) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn cpk_element_colors() {
        assert_eq!(element_color("C"), [0.2, 1.0, 0.2]);
        assert_eq!(element_color("o"), [1.0, 0.3, 0.3]);
        assert_eq!(element_color("Xx"), [0.8, 0.8, 0.8]); // fallback
    }

    #[test]
    fn chain_palette_cycles() {
        assert_eq!(chain_color(0), [0.0, 1.0, 1.0]);
        assert_eq!(chain_color(10), chain_color(0)); // wraps
    }

    #[test]
    fn spectrum_endpoints() {
        assert!(close(spectrum_color(0.0), [0.0, 0.0, 1.0])); // blue
        assert!(close(spectrum_color(0.5), [0.0, 1.0, 0.0])); // green
        assert!(close(spectrum_color(1.0), [1.0, 0.0, 0.0])); // red
    }

    #[test]
    fn scheme_parsing() {
        assert_eq!(ColorScheme::parse("element"), ColorScheme::Element);
        assert_eq!(ColorScheme::parse("spectrum"), ColorScheme::Spectrum);
        assert_eq!(ColorScheme::parse("chain"), ColorScheme::Chain);
        assert_eq!(
            ColorScheme::parse("red"),
            ColorScheme::Fixed([1.0, 0.0, 0.0])
        );
        assert_eq!(
            ColorScheme::parse("#ff0000"),
            ColorScheme::Fixed([1.0, 0.0, 0.0])
        );
        // unknown -> element fallback
        assert_eq!(ColorScheme::parse("not-a-color"), ColorScheme::Element);
    }
}
