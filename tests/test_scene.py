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
SOLVATED_FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "solvated.pdb")
# solvated.pdb: 9 protein atoms + 1 HOH water + 1 NA ion (11 total).


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


def test_background_defaults_to_white_and_is_configurable():
    scene = ms.load(FIXTURE).spheres(ms.select.all())
    assert scene.to_geometry()["background"] == [1.0, 1.0, 1.0]
    assert scene.background("black") is scene
    assert scene.to_geometry()["background"] == [0.0, 0.0, 0.0]
    scene.background("#ff0000")
    assert scene.to_geometry()["background"] == [1.0, 0.0, 0.0]


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
    # spheres defaults to "everything but solvent": 9 protein atoms, the HOH water
    # excluded (sticks defaults to ligand — none in this fixture).
    assert len(ms.load(FIXTURE).spheres().to_geometry()["spheres"]["centers"]) == 9


def test_spheres_default_excludes_water_and_ions():
    # solvated.pdb: 9 protein + 1 water + 1 ion. The default drops water and ions.
    geom = ms.load(SOLVATED_FIXTURE).spheres().to_geometry()
    assert len(geom["spheres"]["centers"]) == 9
    # ...but an explicit all() still shows every atom (defaults never override intent).
    all_geom = ms.load(SOLVATED_FIXTURE).spheres(ms.select.all()).to_geometry()
    assert len(all_geom["spheres"]["centers"]) == 11


def test_sticks_generate_cylinders():
    geom = ms.load(FIXTURE).sticks(ms.select.protein()).to_geometry()
    # Bonds within the dipeptide -> at least one cylinder (two halves per bond).
    assert len(geom["cylinders"]["starts"]) > 0
    assert len(geom["cylinders"]["starts"]) % 2 == 0


def test_lines_generate_cylinders_without_caps():
    geom = ms.load(FIXTURE).lines(ms.select.protein()).to_geometry()
    # Bonds within the dipeptide -> cylinders (two halves per bond)...
    assert len(geom["cylinders"]["starts"]) > 0
    assert len(geom["cylinders"]["starts"]) % 2 == 0
    # ...but no ball-and-stick atom caps (unlike sticks).
    assert len(geom["spheres"]["centers"]) == 0


def test_dots_one_sphere_per_atom_no_cylinders():
    geom = ms.load(FIXTURE).dots(ms.select.protein()).to_geometry()
    # 9 protein atoms (ALA 5 + GLY 4); water excluded.
    assert len(geom["spheres"]["centers"]) == 9
    assert len(geom["cylinders"]["starts"]) == 0


def test_label_one_per_residue_by_default():
    geom = ms.load(FIXTURE).label(ms.select.protein()).to_geometry()
    # 2 protein residues (ALA + GLY) -> one residue label each, black by default.
    labels = geom["labels"]
    assert len(labels) == 2
    assert all(label["color"] == [0.0, 0.0, 0.0] for label in labels)
    texts = {label["text"] for label in labels}
    assert any(t.startswith("ALA") for t in texts)
    assert any(t.startswith("GLY") for t in texts)


def test_label_atom_mode_is_one_per_atom():
    geom = ms.load(FIXTURE).label(ms.select.protein(), text="atom").to_geometry()
    # 9 protein atoms (ALA 5 + GLY 4) -> one label each.
    assert len(geom["labels"]) == 9


def test_label_defaults_to_ligand():
    # solvated.pdb has an NA ion (hetero, non-water) = the only ligand residue.
    geom = ms.load(SOLVATED_FIXTURE).label().to_geometry()
    assert len(geom["labels"]) == 1


def test_label_color_and_size_apply():
    geom = (
        ms.load(FIXTURE).label(ms.select.protein(), color="red", size=2.0).to_geometry()
    )
    labels = geom["labels"]
    assert len(labels) == 2
    assert all(label["color"] == [1.0, 0.0, 0.0] for label in labels)
    assert all(label["size"] == pytest.approx(2.0) for label in labels)


def test_label_returns_same_object():
    scene = ms.load(FIXTURE)
    assert scene.label(ms.select.protein()) is scene


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


def test_camera_is_oriented_box():
    cam = ms.load(FIXTURE).spheres(ms.select.all()).to_geometry()["camera"]
    assert len(cam["center"]) == 3
    # Default orientation: identity screen basis.
    assert cam["right"] == [1.0, 0.0, 0.0]
    assert cam["up"] == [0.0, 1.0, 0.0]
    assert len(cam["extent"]) == 3
    assert all(e > 0 for e in cam["extent"])


def test_center_returns_same_object():
    scene = ms.load(FIXTURE)
    assert scene.center(ms.select.protein()) is scene
    assert scene.orient(ms.select.protein()) is scene


def test_center_frames_only_the_selection():
    full = ms.load(FIXTURE).spheres(ms.select.all()).to_geometry()["camera"]
    # Centering on a single residue frames a tighter box than the whole scene.
    one = (
        ms.load(FIXTURE)
        .spheres(ms.select.all())
        .center(ms.select.resi(5))
        .to_geometry()["camera"]
    )
    assert max(one["extent"]) <= max(full["extent"])


def test_orient_rotates_the_screen_basis():
    cam = (
        ms.load(FIXTURE)
        .spheres(ms.select.all())
        .orient(ms.select.protein())
        .to_geometry()["camera"]
    )
    # A non-identity orientation: the basis is no longer the world axes.
    assert cam["right"] != [1.0, 0.0, 0.0] or cam["up"] != [0.0, 1.0, 0.0]
    # Basis vectors stay unit length.
    import math

    for axis in (cam["right"], cam["up"]):
        assert math.isclose(math.hypot(*axis), 1.0, abs_tol=1e-4)


def test_representations_expose_editable_handles():
    scene = ms.load(FIXTURE).cartoon(ms.select.protein()).surface(ms.select.protein())
    reps = scene.representations
    assert len(reps) == 2
    assert [r.kind for r in reps] == ["cartoon", "surface"]
    assert isinstance(reps[0], ms.Representation)


def test_representation_opacity_edited_in_place_affects_geometry():
    scene = (
        ms.load(HELIX_FIXTURE).cartoon(ms.select.protein()).surface(ms.select.protein())
    )
    # The surface starts opaque...
    assert scene.to_geometry()["meshes"][-1]["opacity"] == 1.0
    # ...edit it in place and the next compile picks it up.
    scene.representations[1].opacity = 0.3
    assert scene.representations[1].opacity == pytest.approx(0.3)
    assert scene.to_geometry()["meshes"][-1]["opacity"] == pytest.approx(0.3)


def test_representation_color_edit_preserves_other_fields():
    scene = ms.load(FIXTURE).spheres(ms.select.protein(), scale=1.5)
    rep = scene.representations[0]
    rep.color = "red"
    # Editing one field leaves the others untouched (read-modify-write).
    assert rep.color == "red"
    assert rep.scale == pytest.approx(1.5)


def test_representation_selection_retarget_changes_geometry():
    scene = ms.load(FIXTURE).spheres(ms.select.protein())
    assert len(scene.to_geometry()["spheres"]["centers"]) == 9
    # Re-target the existing representation at everything (incl. the water).
    scene.representations[0].selection = ms.select.all()
    assert len(scene.to_geometry()["spheres"]["centers"]) == 10


def test_representation_clear_field_with_none():
    scene = ms.load(FIXTURE).surface(ms.select.protein(), opacity=0.3)
    rep = scene.representations[0]
    assert rep.opacity == pytest.approx(0.3)
    rep.opacity = None
    assert rep.opacity is None
    # Cleared back to the opaque default.
    assert scene.to_geometry()["meshes"][-1]["opacity"] == 1.0


def test_representation_index_out_of_range_raises():
    scene = ms.load(FIXTURE).spheres(ms.select.protein())
    with pytest.raises(IndexError):
        scene._core.rep_kind(5)


def test_representation_string_selection_rejected():
    scene = ms.load(FIXTURE).spheres(ms.select.protein())
    with pytest.raises(TypeError):
        scene.representations[0].selection = "protein"


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
