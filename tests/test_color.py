"""Coloring tests for v0.3: property colormaps, carbon overrides, and
explicit per-selection color (``set_color``). All offline, using fixtures.
"""

import os

import molscene as ms

DIPEPTIDE = os.path.join(os.path.dirname(__file__), "fixtures", "dipeptide.pdb")
BFACTORS = os.path.join(os.path.dirname(__file__), "fixtures", "bfactors.pdb")
# bfactors.pdb: 4 bonded carbons with B = 10/30/50/90.


def test_bfactor_coloring_spreads_a_gradient():
    geom = ms.load(BFACTORS).spheres("all", color="bfactor").to_geometry()
    colors = geom["spheres"]["colors"]
    assert len(colors) == 4
    # Auto-ranged over [10, 90]: the four atoms get four distinct colors,
    # lowest and highest at the colormap's ends.
    assert len({tuple(c) for c in colors}) == 4
    assert colors[0] != colors[-1]


def test_occupancy_colormap_keyword_is_accepted():
    # All occupancies are 1.0 -> degenerate range -> a single flat color.
    geom = ms.load(BFACTORS).spheres("all", color="occupancy:plasma").to_geometry()
    colors = geom["spheres"]["colors"]
    assert len(colors) == 4  # all atoms produced, not a partial/empty geometry
    assert len({tuple(c) for c in colors}) == 1


def test_element_coloring_keeps_carbon_in_chosen_color():
    geom = ms.load(DIPEPTIDE).spheres("protein", color="element:cyan").to_geometry()
    colors = [tuple(c) for c in geom["spheres"]["colors"]]
    cyan = (0.0, 1.0, 1.0)
    # Carbons are cyan; the backbone N/O atoms keep their CPK colors.
    assert cyan in colors
    assert any(c != cyan for c in colors)


def test_set_color_overrides_a_subselection():
    scene = ms.load(DIPEPTIDE).spheres("all", color="grey")
    assert scene.set_color("resn HOH", "red") is scene  # chains, returns self
    geom = scene.to_geometry()
    colors = [tuple(c) for c in geom["spheres"]["colors"]]
    red = (1.0, 0.0, 0.0)
    grey = (0.5, 0.5, 0.5)
    assert red in colors  # the water atom was repainted
    assert grey in colors  # everything else stayed grey


def test_set_color_is_recorded_in_the_scene_spec():
    spec = ms.load(DIPEPTIDE).set_color("resi 1", "red").to_dict()
    assert spec["colors"] == [{"selection": "resi 1", "color": "red"}]
