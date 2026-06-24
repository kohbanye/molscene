"""Tests for the native GPU rasterizer (``Scene.to_png`` / ``save_png``).

These need a GPU (or a software Vulkan/GL fallback such as SwiftShader or
llvmpipe). Where none is available — common in CI — the render raises
``RuntimeError`` and the test skips rather than fails, mirroring the Rust
``molscene-render`` test. Offline: uses a local fixture, no network.
"""

import os
import struct

import molscene as ms
import pytest

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "helix.pdb")

PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def _png_size(data: bytes) -> tuple[int, int]:
    """Width/height from a PNG's IHDR chunk (bytes 16..24, big-endian)."""
    assert data[:8] == PNG_MAGIC
    width, height = struct.unpack(">II", data[16:24])
    return width, height


def _render(scene, **kw):
    """Render, skipping the test only when no GPU adapter is available.

    Other render failures (device creation, readback, encode) are real
    regressions and must propagate, not be silently skipped.
    """
    try:
        return scene.to_png(**kw)
    except RuntimeError as e:
        if "no GPU adapter" in str(e):
            pytest.skip(f"no GPU available for rendering: {e}")
        raise


def test_to_png_returns_a_sized_png():
    scene = ms.load(FIXTURE).cartoon(color="spectrum")
    data = _render(scene, width=120, height=90, ssaa=1)
    assert isinstance(data, bytes)
    assert _png_size(data) == (120, 90)


def test_save_png_writes_file(tmp_path):
    scene = ms.load(FIXTURE).cartoon()
    out = str(tmp_path / "out.png")
    # Probe once so we skip cleanly on machines without a GPU.
    _render(scene, width=64, height=64, ssaa=1)
    assert scene.save_png(out, width=64, height=64, ssaa=1) == out
    with open(out, "rb") as fh:
        assert _png_size(fh.read()) == (64, 64)


def test_ssaa_does_not_change_output_dimensions():
    scene = ms.load(FIXTURE).cartoon()
    data = _render(scene, width=80, height=60, ssaa=2)
    # Supersampling renders larger internally but the PNG is the requested size.
    assert _png_size(data) == (80, 60)
