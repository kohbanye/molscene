//! Regenerates the `molscene._core` type stubs from the live PyO3 signatures.
//!
//! Run from the repo root so the `.pyi` lands next to the facade:
//!
//! ```sh
//! cargo run -p molscene-py --features stub-gen --bin stub_gen
//! ```
//!
//! Gated behind the `stub-gen` feature (`required-features`) so a plain
//! `cargo test/clippy --workspace` never builds this binary — it links
//! libpython without `extension-module`, which the lean Rust-core CI job does
//! not provide.

fn main() -> pyo3_stub_gen::Result<()> {
    let stub = _core::stub_info()?;
    stub.generate()?;
    Ok(())
}
