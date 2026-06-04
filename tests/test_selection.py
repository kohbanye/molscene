"""Tests for the ``ms.select`` DSL, its Rust-backed boolean operators, and the
native selection evaluator (boolean / spatial / aggregation / numeric)."""

import os

import pytest

import molscene as ms

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "dipeptide.pdb")


def n_spheres(scene: ms.Scene) -> int:
    """Number of atoms a representation resolved to (one sphere per atom)."""
    return len(scene.to_geometry()["spheres"]["centers"])


# -- DSL string building ----------------------------------------------------


def test_basic_macros():
    assert str(ms.select.protein()) == "protein"
    assert str(ms.select.water()) == "water"
    assert str(ms.select.sidechain()) == "sidechain"


def test_predicates():
    assert str(ms.select.chain("A")) == "chain A"
    assert str(ms.select.resi(10, 30)) == "resi 10-30"
    assert str(ms.select.resi(42)) == "resi 42"
    assert str(ms.select.element("Fe")) == "element Fe"


def test_numeric_builders():
    assert str(ms.select.b(">", 30)) == "b > 30"
    assert str(ms.select.q("=", 1)) == "q = 1"


def test_aggregation_builders():
    assert str(ms.select.byres(ms.select.element("N"))) == "byres (element N)"
    assert str(ms.select.bychain(ms.select.resi(1))) == "bychain (resi 1)"
    assert str(ms.select.bymol(ms.select.ligand())) == "bymol (ligand)"


def test_spatial_builders():
    assert str(ms.select.around(ms.select.ligand(), 4.0)) == "around 4.0 of (ligand)"
    assert str(ms.select.within(ms.select.ligand(), 4.0)) == "within 4.0 of (ligand)"
    assert str(ms.select.expand(ms.select.ligand(), 4.0)) == "expand 4.0 of (ligand)"
    assert str(ms.select.beyond(ms.select.ligand(), 4.0)) == "beyond 4.0 of (ligand)"


def test_and_operator():
    s = ms.select.chain("A") & ms.select.protein()
    assert str(s) == "(chain A) and (protein)"


def test_or_operator():
    s = ms.select.water() | ms.select.ligand()
    assert str(s) == "(water) or (ligand)"


def test_invert_operator():
    s = ~ms.select.hydrogen()
    assert str(s) == "not (hydrogen)"


def test_composed_operators():
    s = ms.select.chain("A") & ms.select.resi(10, 30) & ~ms.select.hydrogen()
    assert str(s) == "((chain A) and (resi 10-30)) and (not (hydrogen))"


# -- native evaluation (dipeptide: ALA 1, GLY 2, HOH 101; chain A; 10 atoms) --


def test_selection_is_recorded_on_scene():
    scene = ms.load(FIXTURE).sticks(ms.select.chain("A") & ms.select.ligand())
    sel_str = scene.to_dict()["representations"][0]["selection"]
    assert sel_str == "(chain A) and (ligand)"


def test_single_clause_evaluates():
    # backbone: N/CA/C/O of ALA + GLY = 8 (CB excluded).
    assert n_spheres(ms.load(FIXTURE).spheres("backbone")) == 8
    # water: the single HOH oxygen.
    assert n_spheres(ms.load(FIXTURE).spheres(ms.select.water())) == 1


def test_composed_selection_is_evaluated_not_fallback():
    # chain A & water -> just the water atom (was a fallback-to-all in v0.1).
    sel = ms.select.chain("A") & ms.select.water()
    assert n_spheres(ms.load(FIXTURE).spheres(sel)) == 1
    # protein & ~water -> all 9 non-hetero atoms.
    sel = ms.select.protein() & ~ms.select.water()
    assert n_spheres(ms.load(FIXTURE).spheres(sel)) == 9


def test_aggregation_evaluates():
    # byres of one ALA atom expands to the whole ALA residue (5 atoms).
    sel = ms.select.byres(ms.select.resn("ALA") & ms.select.element("N"))
    assert n_spheres(ms.load(FIXTURE).spheres(sel)) == 5


def test_numeric_evaluates():
    # every atom in the fixture has B-factor 20.0.
    assert n_spheres(ms.load(FIXTURE).spheres(ms.select.b(">", 10))) == 10
    assert n_spheres(ms.load(FIXTURE).spheres(ms.select.b("<", 10))) == 0


# -- validation -------------------------------------------------------------


def test_invalid_selection_raises_value_error():
    with pytest.raises(ValueError):
        ms.load(FIXTURE).spheres("frobnicate")
    with pytest.raises(ValueError):
        ms.load(FIXTURE).spheres("chain")  # missing argument
    with pytest.raises(ValueError):
        ms.load(FIXTURE).center("around 5 (ligand)")  # missing "of"
