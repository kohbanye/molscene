//! wasm-bindgen bindings over molscene-core for the browser scene engine.
//! Fleshed out in the WASM milestone.

use wasm_bindgen::prelude::*;

/// Smoke export to validate the WASM build wiring.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
