//! Render a small molecule to a PNG file. Usage:
//!   cargo run -p molscene-render --example render -- out.png

use molscene_core::scene::Scene;
use molscene_core::selection::Expr;
use molscene_core::spec::{Source, Style};
use molscene_render::{render_png, RenderOptions};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "out.png".into());
    let mode = std::env::args().nth(2).unwrap_or_else(|| "all".into());

    let sdf = "ethylene\n  molscene\n\n  6  5  0  0  0  0  0  0  0  0999 V2000\n\
        0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n\
        1.3300    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n\
       -0.5000    0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
       -0.5000   -0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
        1.8300    0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
        1.8300   -0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
        1  2  2  0  0  0  0\n  1  3  1  0  0  0  0\n  1  4  1  0  0  0  0\n\
        2  5  1  0  0  0  0\n  2  6  1  0  0  0  0\nM  END\n";
    let mut scene = Scene::from_sdf(sdf, Source::InlineSdf { data: sdf.into() }).unwrap();
    match mode.as_str() {
        "clear" => {}
        "spheres" => {
            scene.spheres(Expr::All, Style::default());
        }
        "sticks" => {
            scene.sticks(Expr::All, Style::default());
        }
        _ => {
            scene
                .spheres(Expr::All, Style::default())
                .sticks(Expr::All, Style::default());
        }
    }
    let spec = scene.to_geometry();
    let opts = RenderOptions {
        width: 400,
        height: 300,
        ssaa: 2,
    };
    match render_png(&spec, &opts) {
        Ok(bytes) => {
            std::fs::write(&path, &bytes).unwrap();
            eprintln!("wrote {} ({} bytes)", path, bytes.len());
        }
        Err(e) => {
            eprintln!("render failed: {e}");
            std::process::exit(1);
        }
    }
}
