//! Render a PDB file (cartoon + translucent surface) to PNG. Usage:
//!   cargo run -p molscene-render --example pdb -- tests/fixtures/helix.pdb out.png

use molscene_core::scene::Scene;
use molscene_core::selection::Expr;
use molscene_core::spec::{Source, Style};
use molscene_render::{render_png, RenderOptions};

fn main() {
    let pdb_path = std::env::args()
        .nth(1)
        .expect("usage: pdb <file> <out.png>");
    let out = std::env::args().nth(2).unwrap_or_else(|| "pdb.png".into());
    let text = std::fs::read_to_string(&pdb_path).unwrap();
    let mut scene = Scene::from_pdb(&text, Source::InlinePdb { data: text.clone() }).unwrap();
    scene
        .cartoon(
            Expr::All,
            Style {
                color: Some("spectrum".into()),
                ..Default::default()
            },
        )
        .surface(
            Expr::All,
            Style {
                opacity: Some(0.4),
                ..Default::default()
            },
        );
    let spec = scene.to_geometry();
    let opts = RenderOptions {
        width: 500,
        height: 500,
        ssaa: 2,
    };
    let bytes = render_png(&spec, &opts).unwrap();
    std::fs::write(&out, &bytes).unwrap();
    eprintln!("wrote {out} ({} bytes)", bytes.len());
}
