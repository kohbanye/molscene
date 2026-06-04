"""Facade tests: fluent chaining, serialization, and notebook display."""

import json
import os

import pytest

import molscene as ms

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "dipeptide.pdb")


def build_scene():
    return (
        ms.load("1ubq")
        .cartoon("protein", color="spectrum")
        .surface("protein", opacity=0.25)
        .sticks("ligand", color="element")
    )


def test_chaining_returns_same_object():
    scene = ms.load("1ubq")
    assert scene.cartoon("protein") is scene
    assert scene.surface("protein") is scene


def test_to_dict_matches_spec():
    spec = build_scene().to_dict()
    assert spec["spec_version"] == "0.1"
    assert spec["structures"][0]["source"] == {"type": "rcsb", "id": "1ubq"}
    kinds = [r["kind"] for r in spec["representations"]]
    assert kinds == ["cartoon", "surface", "sticks"]
    assert spec["representations"][0]["style"] == {"color": "spectrum"}
    assert spec["representations"][1]["style"] == {"opacity": 0.25}
    assert spec["camera"] == {"auto": True}


def test_center_sets_camera():
    spec = ms.load("1ubq").cartoon("protein").center("ligand").to_dict()
    assert spec["camera"]["center"] == "ligand"


def test_load_pdb_id_is_lowercased():
    assert ms.load("1UBQ").to_dict()["structures"][0]["source"]["id"] == "1ubq"


def test_load_local_file_inlines_pdb():
    spec = ms.load(FIXTURE).to_dict()
    source = spec["structures"][0]["source"]
    assert source["type"] == "inline_pdb"
    assert "ATOM" in source["data"]


def test_unknown_representation_kind_rejected():
    scene = ms.load("1ubq")
    with pytest.raises(ValueError):
        scene._core.representation("ribbon", "all", "")


def test_repr_html_contains_iframe_and_spec():
    scene = build_scene()
    html = scene._repr_html_()
    assert "<iframe" in html
    assert "srcdoc=" in html
    # The spec JSON is embedded (HTML-escaped) in the srcdoc.
    assert "spec_version" in html
    assert "1ubq" in html


def test_export_html_writes_standalone_file(tmp_path):
    out = tmp_path / "scene.html"
    build_scene().export_html(str(out))
    text = out.read_text()
    assert text.startswith("<iframe")
    assert "3Dmol" in text  # the pinned CDN script is referenced
