"""Facade tests: fluent chaining, geometry compilation, and notebook display.

These use a local fixture (no network). The RCSB fetch path is covered
separately under the ``network`` marker.
"""

import os

import molscene as ms
import pytest

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "dipeptide.pdb")
# dipeptide.pdb: 10 atoms (ALA 5 + GLY 4 + HOH 1), 1 water (HETATM).


def build_scene():
    return ms.load(FIXTURE).spheres("protein", color="element").sticks("protein")


def test_chaining_returns_same_object():
    scene = ms.load(FIXTURE)
    assert scene.spheres("all") is scene
    assert scene.sticks("all") is scene


def test_load_local_file_records_inline_source():
    spec = ms.load(FIXTURE).to_dict()
    source = spec["structures"][0]["source"]
    assert source["type"] == "inline_pdb"
    assert "ATOM" in source["data"]


def test_spheres_geometry_one_per_selected_atom():
    geom = ms.load(FIXTURE).spheres("protein", color="element").to_geometry()
    # 9 protein atoms (ALA 5 + GLY 4); water excluded.
    assert len(geom["spheres"]["centers"]) == 9
    assert len(geom["spheres"]["radii"]) == 9
    assert len(geom["spheres"]["colors"]) == 9


def test_all_includes_water():
    geom = ms.load(FIXTURE).spheres("all").to_geometry()
    assert len(geom["spheres"]["centers"]) == 10


def test_sticks_generate_cylinders():
    geom = ms.load(FIXTURE).sticks("protein").to_geometry()
    # Bonds within the dipeptide -> at least one cylinder (two halves per bond).
    assert len(geom["cylinders"]["starts"]) > 0
    assert len(geom["cylinders"]["starts"]) % 2 == 0


def test_camera_is_bounding_sphere():
    geom = ms.load(FIXTURE).spheres("all").to_geometry()
    assert geom["camera"]["radius"] > 0
    assert len(geom["camera"]["center"]) == 3


def test_unknown_representation_kind_rejected():
    scene = ms.load(FIXTURE)
    with pytest.raises(ValueError):
        scene._core.representation("ribbon", "all", "")


def test_repr_html_contains_iframe_and_geometry():
    html = build_scene()._repr_html_()
    assert "<iframe" in html
    assert "srcdoc=" in html
    assert "molscene-geometry" in html
    # No 3Dmol anywhere — Three.js is the sole renderer.
    assert "3Dmol" not in html and "3dmol" not in html


def test_export_html_is_self_contained(tmp_path):
    out = tmp_path / "scene.html"
    build_scene().export_html(str(out))
    text = out.read_text()
    assert text.startswith("<iframe")
    # Fully offline: nothing is loaded from the network. (URL strings may appear
    # inside the inlined Three.js bundle's license comments — that's fine; what
    # matters is no external <script src>/<link href>.)
    assert 'src="http' not in text and "src=&quot;http" not in text
    assert "<link" not in text


@pytest.mark.network
def test_load_rcsb_fetches_and_parses():
    geom = ms.load("1ubq").spheres("protein").to_geometry()
    assert len(geom["spheres"]["centers"]) > 100
