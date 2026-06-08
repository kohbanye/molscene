//! Color resolution — the Rust-side source of truth for palettes.
//!
//! The renderer is molecule-agnostic, so molscene resolves every atom to a
//! concrete RGB here: CPK by element, a per-chain cycling palette, a residue
//! "spectrum" rainbow, or a fixed named/hex color. Values follow PyMOL
//! (`layer1/Color.cpp`). RGB components are floats in 0–1.

use crate::structure::Element;

/// RGB color, components in 0–1.
pub type Rgb = [f32; 3];

/// A per-atom numeric field that can drive a colormap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyField {
    /// Crystallographic temperature factor.
    BFactor,
    /// Occupancy.
    Occupancy,
}

/// How a representation should be colored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorScheme {
    /// CPK coloring by element.
    Element,
    /// CPK coloring, but carbon atoms take a fixed color (the common
    /// "color by element, carbons in X" idiom).
    ElementCarbon(Rgb),
    /// Rainbow across residue order.
    Spectrum,
    /// Color by per-residue secondary structure (cartoon only). Other
    /// representations fall back to element coloring.
    SecondaryStructure,
    /// Cycling palette per chain.
    Chain,
    /// A numeric per-atom property mapped through a colormap. `range` is the
    /// `(min, max)` the field is normalized against; `None` means "auto" —
    /// resolved over the colored atoms at geometry time.
    ByProperty {
        field: PropertyField,
        map: Colormap,
        range: Option<(f32, f32)>,
    },
    /// A single fixed color.
    Fixed(Rgb),
}

impl ColorScheme {
    /// Parse a `color=` value. The grammar is `<base>[:<modifier>]`:
    ///
    /// - `element`/`cpk` → CPK; with a modifier color, carbons take it
    ///   (`element:cyan`).
    /// - `spectrum`/`rainbow`, `chain`/`bychain` → those schemes.
    /// - `bfactor`/`b`, `occupancy`/`q` → property coloring; the modifier picks
    ///   the colormap (`bfactor:plasma`), defaulting to viridis.
    /// - otherwise a named color or `#rrggbb`.
    ///
    /// Anything unknown falls back to [`ColorScheme::Element`] with a warning.
    pub fn parse(value: &str) -> ColorScheme {
        let v = value.trim().to_ascii_lowercase();
        let (base, modifier) = match v.split_once(':') {
            Some((b, m)) => (b.trim(), Some(m.trim())),
            None => (v.as_str(), None),
        };
        match base {
            "element" | "cpk" => match modifier {
                None => ColorScheme::Element,
                Some(carbon) => match named_color(carbon).or_else(|| parse_hex(carbon)) {
                    Some(rgb) => ColorScheme::ElementCarbon(rgb),
                    None => {
                        eprintln!(
                            "molscene: unknown carbon color {carbon:?}; using plain element coloring."
                        );
                        ColorScheme::Element
                    }
                },
            },
            "spectrum" | "rainbow" => ColorScheme::Spectrum,
            "secondary_structure" | "ss" | "sse" => ColorScheme::SecondaryStructure,
            "chain" | "bychain" => ColorScheme::Chain,
            "bfactor" | "b" => ColorScheme::ByProperty {
                field: PropertyField::BFactor,
                map: Colormap::parse(modifier),
                range: None,
            },
            "occupancy" | "q" => ColorScheme::ByProperty {
                field: PropertyField::Occupancy,
                map: Colormap::parse(modifier),
                range: None,
            },
            _ => {
                // No recognized base keyword; treat `base` as a fixed color. A
                // fixed color takes no modifier, so `red:plasma` is just `red`.
                if let Some(rgb) = named_color(base).or_else(|| parse_hex(base)) {
                    ColorScheme::Fixed(rgb)
                } else {
                    eprintln!("molscene: unknown color {value:?}; defaulting to element coloring.");
                    ColorScheme::Element
                }
            }
        }
    }
}

/// CPK color for an element (Jmol CPK / PyMOL-derived, desaturated slightly so
/// single colors read truer under the viewer's hemisphere+fill lighting);
/// carbon keeps the PyMOL-family green. Unknown → light grey.
pub fn element_color(element: &Element) -> Rgb {
    match element {
        Element::C => [0.30, 0.85, 0.30],
        Element::N => [0.20, 0.30, 0.85],
        Element::O => [0.90, 0.20, 0.20],
        Element::H => [0.92, 0.92, 0.92],
        Element::S => [1.0, 0.78, 0.20],
        Element::P => [1.0, 0.5, 0.0],
        Element::Fe => [0.88, 0.40, 0.20],
        Element::Cl => [0.12, 0.94, 0.12],
        Element::Na => [0.67, 0.36, 0.95],
        Element::Mg => [0.54, 1.0, 0.0],
        Element::Zn => [0.49, 0.50, 0.69],
        Element::Ca => [0.24, 1.0, 0.0],
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

/// A perceptual colormap for property-based coloring (maps 0–1 → RGB).
///
/// The lookup tables are sampled control points (interpolated linearly), not the
/// full 256-row originals: `viridis`/`plasma` from matplotlib (a CC0 dedication),
/// `RdYlGn` is ColorBrewer's 11-class diverging scheme (Cynthia Brewer, Apache-2.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colormap {
    Viridis,
    Plasma,
    RdYlGn,
}

impl Colormap {
    /// Parse a colormap modifier; `None` (and anything unknown) → viridis.
    pub fn parse(modifier: Option<&str>) -> Colormap {
        match modifier {
            None => Colormap::Viridis,
            Some(m) => match m.trim().to_ascii_lowercase().as_str() {
                "viridis" => Colormap::Viridis,
                "plasma" => Colormap::Plasma,
                "rdylgn" | "rd_yl_gn" => Colormap::RdYlGn,
                other => {
                    eprintln!("molscene: unknown colormap {other:?}; defaulting to viridis.");
                    Colormap::Viridis
                }
            },
        }
    }
}

/// Map `t` in 0–1 through `map`, linearly interpolating its control points.
pub fn colormap_color(map: Colormap, t: f32) -> Rgb {
    let lut: &[Rgb] = match map {
        Colormap::Viridis => &VIRIDIS,
        Colormap::Plasma => &PLASMA,
        Colormap::RdYlGn => &RDYLGN,
    };
    let t = t.clamp(0.0, 1.0);
    let last = lut.len() - 1;
    let scaled = t * last as f32;
    let i = scaled.floor() as usize;
    if i >= last {
        return lut[last];
    }
    let f = scaled - i as f32;
    let (a, b) = (lut[i], lut[i + 1]);
    [
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
    ]
}

/// viridis control points (matplotlib, CC0), sampled at 10 stops.
const VIRIDIS: [Rgb; 10] = [
    [0.2667, 0.0039, 0.3294],
    [0.2824, 0.1569, 0.4706],
    [0.2431, 0.2902, 0.5373],
    [0.1922, 0.4078, 0.5569],
    [0.1490, 0.5098, 0.5569],
    [0.1216, 0.6196, 0.5373],
    [0.2078, 0.7176, 0.4745],
    [0.4275, 0.8039, 0.3490],
    [0.7059, 0.8706, 0.1725],
    [0.9922, 0.9059, 0.1451],
];

/// plasma control points (matplotlib, CC0), sampled at 10 stops.
const PLASMA: [Rgb; 10] = [
    [0.0510, 0.0314, 0.5294],
    [0.2745, 0.0118, 0.6235],
    [0.4471, 0.0039, 0.6588],
    [0.6118, 0.0902, 0.6196],
    [0.7412, 0.2157, 0.5255],
    [0.8471, 0.3412, 0.4196],
    [0.9294, 0.4745, 0.3255],
    [0.9843, 0.6235, 0.2275],
    [0.9922, 0.7922, 0.1490],
    [0.9412, 0.9765, 0.1294],
];

/// ColorBrewer RdYlGn, 11-class diverging (red → yellow → green).
const RDYLGN: [Rgb; 11] = [
    [0.6471, 0.0000, 0.1490],
    [0.8431, 0.1882, 0.1529],
    [0.9569, 0.4275, 0.2627],
    [0.9922, 0.6824, 0.3804],
    [0.9961, 0.8784, 0.5451],
    [1.0000, 1.0000, 0.7490],
    [0.8510, 0.9373, 0.5451],
    [0.6510, 0.8510, 0.4157],
    [0.4000, 0.7412, 0.3882],
    [0.1020, 0.5961, 0.3137],
    [0.0000, 0.4078, 0.2157],
];

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
        assert_eq!(element_color(&Element::C), [0.30, 0.85, 0.30]);
        assert_eq!(
            element_color(&Element::from_symbol("o")),
            [0.90, 0.20, 0.20]
        );
        assert_eq!(element_color(&Element::from_symbol("Xx")), [0.8, 0.8, 0.8]);
        // fallback
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
        assert_eq!(
            ColorScheme::parse("secondary_structure"),
            ColorScheme::SecondaryStructure
        );
        assert_eq!(ColorScheme::parse("ss"), ColorScheme::SecondaryStructure);
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
        // a fixed color takes no modifier; an accidental one is ignored.
        assert_eq!(
            ColorScheme::parse("red:plasma"),
            ColorScheme::Fixed([1.0, 0.0, 0.0])
        );
        assert_eq!(
            ColorScheme::parse("#ff0000:nope"),
            ColorScheme::Fixed([1.0, 0.0, 0.0])
        );
    }

    #[test]
    fn element_carbon_parsing() {
        assert_eq!(
            ColorScheme::parse("element:cyan"),
            ColorScheme::ElementCarbon([0.0, 1.0, 1.0])
        );
        assert_eq!(
            ColorScheme::parse("cpk:#00ffff"),
            ColorScheme::ElementCarbon([0.0, 1.0, 1.0])
        );
        // unknown carbon color -> plain element coloring
        assert_eq!(ColorScheme::parse("element:nope"), ColorScheme::Element);
    }

    #[test]
    fn property_parsing() {
        assert_eq!(
            ColorScheme::parse("bfactor"),
            ColorScheme::ByProperty {
                field: PropertyField::BFactor,
                map: Colormap::Viridis,
                range: None,
            }
        );
        assert_eq!(
            ColorScheme::parse("b:plasma"),
            ColorScheme::ByProperty {
                field: PropertyField::BFactor,
                map: Colormap::Plasma,
                range: None,
            }
        );
        assert_eq!(
            ColorScheme::parse("occupancy:rdylgn"),
            ColorScheme::ByProperty {
                field: PropertyField::Occupancy,
                map: Colormap::RdYlGn,
                range: None,
            }
        );
        // unknown colormap -> viridis fallback
        assert_eq!(
            ColorScheme::parse("q:nope"),
            ColorScheme::ByProperty {
                field: PropertyField::Occupancy,
                map: Colormap::Viridis,
                range: None,
            }
        );
    }

    #[test]
    fn colormap_endpoints() {
        for map in [Colormap::Viridis, Colormap::Plasma, Colormap::RdYlGn] {
            // Endpoints land exactly on the first/last control point; the
            // midpoint stays inside the unit cube.
            let lo = colormap_color(map, 0.0);
            let hi = colormap_color(map, 1.0);
            let mid = colormap_color(map, 0.5);
            assert_ne!(lo, hi);
            for c in lo.iter().chain(hi.iter()).chain(mid.iter()) {
                assert!((0.0..=1.0).contains(c));
            }
            // Out-of-range clamps.
            assert_eq!(colormap_color(map, -1.0), lo);
            assert_eq!(colormap_color(map, 2.0), hi);
        }
        // viridis runs dark purple → yellow-green.
        assert!(close(
            colormap_color(Colormap::Viridis, 0.0),
            [0.2667, 0.0039, 0.3294]
        ));
        assert!(close(
            colormap_color(Colormap::Viridis, 1.0),
            [0.9922, 0.9059, 0.1451]
        ));
    }
}
