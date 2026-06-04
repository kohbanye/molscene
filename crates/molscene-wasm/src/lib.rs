//! wasm-bindgen bindings over molscene-core for the browser scene engine.
//! Fleshed out in the v0.4 WASM milestone.

use wasm_bindgen::prelude::*;

/// Smoke export to validate the WASM build wiring.
#[wasm_bindgen]
pub fn spec_version() -> String {
    molscene_core::spec::SPEC_VERSION.to_string()
}
