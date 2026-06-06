"""Facade tests: fluent chaining, geometry compilation, and notebook display.

These use a local fixture (no network). The RCSB fetch path is covered
separately under the ``network`` marker.
"""

import os

import molscene as ms
import pytest

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "dipeptide.pdb")
# dipeptide.pdb: 10 atoms (ALA 5 + GLY 4 + HOH 1), 1 water (HETATM).
HELIX_FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "helix.pdb")
# helix.pdb: 12-residue ideal alpha-helix (N/CA/C/O) with a HELIX annotation.


def build_scene():
    return (
        ms.load(FIXTURE)
        .spheres(ms.select.protein(), color="element")
        .sticks(ms.select.protein())
    )


def test_chaining_returns_same_object():
    scene = ms.load(FIXTURE)
    assert scene.spheres(ms.select.all()) is scene
    assert scene.sticks(ms.select.all()) is scene


def test_spheres_geometry_one_per_selected_atom():
    geom = ms.load(FIXTURE).spheres(ms.select.protein(), color="element").to_geometry()
    # 9 protein atoms (ALA 5 + GLY 4); water excluded.
    assert len(geom["spheres"]["centers"]) == 9
    assert len(geom["spheres"]["radii"]) == 9
    assert len(geom["spheres"]["colors"]) == 9


def test_all_includes_water():
    geom = ms.load(FIXTURE).spheres(ms.select.all()).to_geometry()
    assert len(geom["spheres"]["centers"]) == 10


def test_default_selections_apply():
    # spheres defaults to all (10), sticks to ligand (none in this fixture).
    assert len(ms.load(FIXTURE).spheres().to_geometry()["spheres"]["centers"]) == 10


def test_sticks_generate_cylinders():
    geom = ms.load(FIXTURE).sticks(ms.select.protein()).to_geometry()
    # Bonds within the dipeptide -> at least one cylinder (two halves per bond).
    assert len(geom["cylinders"]["starts"]) > 0
    assert len(geom["cylinders"]["starts"]) % 2 == 0


def test_cartoon_emits_meshes():
    geom = (
        ms.load(HELIX_FIXTURE)
        .cartoon(ms.select.protein(), color="spectrum")
        .to_geometry()
    )
    assert len(geom["meshes"]) == 1
    mesh = geom["meshes"][0]
    pos = mesh["positions"]
    assert len(pos) > 0
    assert len(mesh["colors"]) == len(pos)
    assert len(mesh["normals"]) == len(pos)
    assert len(mesh["indices"]) % 3 == 0
    assert mesh["opacity"] == 1.0


def test_cartoon_secondary_structure_coloring():
    geom = ms.load(HELIX_FIXTURE).cartoon(ms.select.protein(), color="ss").to_geometry()
    colors = geom["meshes"][0]["colors"]
    assert len(colors) > 0
    # SS coloring uses only the helix/sheet/loop palette.
    palette = {(1.0, 0.0, 1.0), (1.0, 1.0, 0.0), (0.0, 1.0, 1.0)}
    assert all(tuple(c) in palette for c in colors)


def test_surface_emits_a_transparent_mesh():
    geom = ms.load(FIXTURE).surface(ms.select.protein(), opacity=0.3).to_geometry()
    assert len(geom["meshes"]) >= 1
    mesh = geom["meshes"][-1]
    pos = mesh["positions"]
    assert len(pos) > 0
    assert len(mesh["colors"]) == len(pos)
    assert len(mesh["normals"]) == len(pos)
    assert len(mesh["indices"]) % 3 == 0
    assert mesh["opacity"] == pytest.approx(0.3)


def test_surface_over_cartoon_keeps_two_groups():
    geom = (
        ms.load(HELIX_FIXTURE)
        .cartoon(ms.select.protein())
        .surface(ms.select.protein(), opacity=0.3)
        .to_geometry()
    )
    # One opaque cartoon group and one transparent surface group.
    assert len(geom["meshes"]) == 2
    assert geom["meshes"][0]["opacity"] == 1.0
    assert geom["meshes"][1]["opacity"] == pytest.approx(0.3)


def test_camera_is_bounding_sphere():
    geom = ms.load(FIXTURE).spheres(ms.select.all()).to_geometry()
    assert geom["camera"]["radius"] > 0
    assert len(geom["camera"]["center"]) == 3


def test_unknown_representation_kind_rejected():
    scene = ms.load(FIXTURE)
    with pytest.raises(ValueError):
        scene._core.representation("ribbon", ms.select.all())


def test_repr_counts_representations():
    assert repr(build_scene()) == "<molscene.Scene: 2 representation(s)>"


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
    geom = ms.load("1ubq").spheres(ms.select.protein()).to_geometry()
    assert len(geom["spheres"]["centers"]) > 100
