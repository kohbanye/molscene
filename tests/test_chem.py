"""Chemistry tests: SDF import with explicit bond orders, and the multi-bond /
aromatic geometry it produces.

These use local SDF fixtures (no network).
"""

import os

import molscene as ms

FIXDIR = os.path.join(os.path.dirname(__file__), "fixtures")
BENZENE = os.path.join(FIXDIR, "benzene.sdf")
ETHYLENE = os.path.join(FIXDIR, "ethylene.sdf")
ACETIC = os.path.join(FIXDIR, "acetic_acid.sdf")


def test_load_sdf_atoms_are_ligand():
    # SDF atoms are tagged as a ligand, so the sticks default (ligand) selects
    # them all. Ethylene = 2 C + 4 H = 6 atoms.
    geom = ms.load(ETHYLENE).spheres(ms.select.all()).to_geometry()
    assert len(geom["spheres"]["centers"]) == 6
    geom_lig = ms.load(ETHYLENE).spheres(ms.select.ligand()).to_geometry()
    assert len(geom_lig["spheres"]["centers"]) == 6


def test_ethylene_double_bond_cylinders():
    # 5 bonds: one C=C double (2 lines) + four C–H single (1 line each).
    # Each line is two half-cylinders → (2 + 4) lines × 2 = 12 cylinders.
    geom = ms.load(ETHYLENE).sticks().to_geometry()
    assert len(geom["cylinders"]["starts"]) == 12


def test_benzene_aromatic_inner_ring_cylinders():
    # 6 aromatic ring bonds (each: full line + inner line = 2 lines) +
    # 6 C–H single (1 line). (12 + 6) lines × 2 half-cylinders = 36 cylinders.
    geom = ms.load(BENZENE).sticks().to_geometry()
    assert len(geom["cylinders"]["starts"]) == 36


def test_acetic_acid_has_a_double_bond():
    # The carbonyl C=O is a double bond, so more cylinders than a pure-single
    # molecule with the same bond count would give.
    geom = ms.load(ACETIC).sticks().to_geometry()
    # 7 bonds, one of them double: (6 single + 1 double→2 lines) = 8 lines × 2.
    assert len(geom["cylinders"]["starts"]) == 16
