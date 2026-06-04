"""Tests for the ``ms.sel`` DSL and its Rust-backed boolean operators."""

import os

import molscene as ms

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "dipeptide.pdb")


def test_basic_macros():
    assert str(ms.sel.protein()) == "protein"
    assert str(ms.sel.water()) == "water"


def test_predicates():
    assert str(ms.sel.chain("A")) == "chain A"
    assert str(ms.sel.resi(10, 30)) == "resi 10-30"
    assert str(ms.sel.resi(42)) == "resi 42"
    assert str(ms.sel.element("Fe")) == "element Fe"


def test_and_operator():
    s = ms.sel.chain("A") & ms.sel.protein()
    assert str(s) == "(chain A) and (protein)"


def test_or_operator():
    s = ms.sel.water() | ms.sel.ligand()
    assert str(s) == "(water) or (ligand)"


def test_invert_operator():
    s = ~ms.sel.hydrogen()
    assert str(s) == "not (hydrogen)"


def test_composed_operators():
    s = ms.sel.chain("A") & ms.sel.resi(10, 30) & ~ms.sel.hydrogen()
    assert str(s) == "((chain A) and (resi 10-30)) and (not (hydrogen))"


def test_around_records_string():
    lig = ms.sel.ligand()
    near = ms.sel.around(lig, 4.0)
    assert str(near) == "around 4.0 of (ligand)"


def test_selection_is_accepted_by_scene():
    scene = ms.load(FIXTURE).sticks(ms.sel.chain("A") & ms.sel.ligand())
    sel_str = scene.to_dict()["representations"][0]["selection"]
    assert sel_str == "(chain A) and (ligand)"
