//! Cartoon (ribbon) geometry — the native protein cartoon representation.
//!
//! Traces each chain's Cα backbone, assigns per-residue secondary structure
//! (preferring file `HELIX`/`SHEET` annotations, else a Cα-only geometric
//! heuristic), threads a Catmull-Rom spline through the Cα atoms, and extrudes a
//! tube whose elliptical cross-section morphs by secondary structure: a round
//! tube for loops, a flat ribbon for helices, and a flat ribbon ending in a
//! tapered arrowhead for β-strands. Pure compute (no pdbtbx, no rendering), so
//! it is WASM-safe.

use std::collections::HashSet;

use crate::color::Rgb;
use crate::geometry::Mesh;
use crate::structure::{Atom, Ss, Structure};

// -- tunables ---------------------------------------------------------------

/// Spline subdivisions between consecutive Cα atoms.
const SUBDIV: usize = 10;
/// Vertices around each cross-section.
const RING: usize = 16;
/// Default loop tube radius (overridable via the representation's `radius`).
const R_TUBE: f32 = 0.3;
/// β-strand ribbon half-width and half-thickness.
const RIBBON_HW: f32 = 0.95;
const RIBBON_HT: f32 = 0.16;
/// α-helix ribbon half-width and half-thickness.
const HELIX_HW: f32 = 1.0;
const HELIX_HT: f32 = 0.22;
/// Cross-section flatness exponent: 2 → ellipse (loop tube), higher → flat
/// ribbon faces with rounded edges (helix/sheet).
const RIBBON_FLATNESS: f32 = 3.4;
/// β-strand arrowhead: a wide flared base tapering to a sharp tip, spanning the
/// last ~1.5 residues of a strand run.
const ARROW_BASE_HW: f32 = 1.55;
const ARROW_TIP_HW: f32 = 0.02;
const ARROW_SECTIONS: usize = SUBDIV * 3 / 2;
/// Backbone path-smoothing iterations and how hard each SS class is pulled
/// toward the local midpoint. Strands flatten and loops declutter; helices are
/// left at full coil radius (their smoothness comes from the spline and frame
/// passes, not from moving Cα).
const SMOOTH_ITERS: usize = 2;
const SMOOTH_HELIX: f32 = 0.0;
const SMOOTH_SHEET: f32 = 0.6;
const SMOOTH_LOOP: f32 = 0.2;

// -- small vec3 helpers (no deps, WASM-safe) --------------------------------

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn normalize(a: V3) -> V3 {
    let n = norm(a);
    if n < 1e-6 {
        [0.0, 0.0, 0.0]
    } else {
        scale(a, 1.0 / n)
    }
}
fn lerp(a: V3, b: V3, t: f32) -> V3 {
    add(scale(a, 1.0 - t), scale(b, t))
}
fn lerpf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// PyMOL-ish cartoon palette for secondary-structure coloring.
pub fn ss_color(ss: Ss) -> Rgb {
    match ss {
        Ss::Helix => [1.0, 0.0, 1.0], // magenta
        Ss::Sheet => [1.0, 1.0, 0.0], // yellow
        Ss::Loop => [0.0, 1.0, 1.0],  // cyan
    }
}

/// How to color cartoon vertices.
pub struct CartoonParams<'a> {
    /// Resolve a residue's color from its Cα atom index/atom and assigned
    /// secondary structure. The caller encapsulates the full precedence
    /// (explicit `set_color` override > `secondary_structure` palette > the
    /// representation's base scheme), so the cartoon just supplies the SS it
    /// computed and renders whatever color comes back.
    pub color_fn: &'a dyn Fn(usize, &Atom, Ss) -> Rgb,
}

// -- backbone tracing -------------------------------------------------------

/// One residue's backbone reference points.
#[derive(Clone)]
struct ResidueTrace {
    ca: V3,
    /// Carbonyl direction `normalize(O - C)` used to orient ribbons.
    co: V3,
    has_co: bool,
    /// Index of the Cα atom in `structure.atoms` (for coloring).
    atom_index: usize,
    chain_id: String,
    residue_seq: i32,
    ss: Ss,
}

/// A contiguous backbone run within one chain.
struct Segment {
    residues: Vec<ResidueTrace>,
}

fn pos(a: &Atom) -> V3 {
    [a.x as f32, a.y as f32, a.z as f32]
}

/// Group selected atoms into residues by `(chain_id, residue_seq)` in file
/// order, collect CA/C/O, and split into contiguous segments (a new segment
/// starts at a chain change, a sequence gap, or a missing Cα). Segments shorter
/// than two residues are dropped (nothing to trace).
fn backbone_segments(structure: &Structure, selected: &[usize]) -> Vec<Segment> {
    let sel: HashSet<usize> = selected.iter().copied().collect();

    let mut residues: Vec<ResidueTrace> = Vec::new();
    let mut cur_key: Option<(String, i32)> = None;
    let mut ca: Option<(usize, V3)> = None;
    let mut c: Option<V3> = None;
    let mut o: Option<V3> = None;

    fn flush(
        out: &mut Vec<ResidueTrace>,
        key: &Option<(String, i32)>,
        ca: Option<(usize, V3)>,
        c: Option<V3>,
        o: Option<V3>,
    ) {
        let (Some((chain, seq)), Some((idx, ca_pos))) = (key, ca) else {
            return;
        };
        let (co, has_co) = match (c, o) {
            (Some(c), Some(o)) => (normalize(sub(o, c)), true),
            _ => ([0.0, 0.0, 0.0], false),
        };
        out.push(ResidueTrace {
            ca: ca_pos,
            co,
            has_co,
            atom_index: idx,
            chain_id: chain.clone(),
            residue_seq: *seq,
            ss: Ss::Loop,
        });
    }

    for (i, a) in structure.atoms.iter().enumerate() {
        if !sel.contains(&i) {
            continue;
        }
        let key = (a.chain_id.clone(), a.residue_seq);
        if cur_key.as_ref() != Some(&key) {
            flush(&mut residues, &cur_key, ca, c, o);
            cur_key = Some(key);
            ca = None;
            c = None;
            o = None;
        }
        match a.name.trim() {
            "CA" => ca = Some((i, pos(a))),
            "C" => c = Some(pos(a)),
            "O" => o = Some(pos(a)),
            _ => {}
        }
    }
    flush(&mut residues, &cur_key, ca, c, o);

    let mut segments: Vec<Segment> = Vec::new();
    let mut current: Vec<ResidueTrace> = Vec::new();
    for r in residues {
        if let Some(last) = current.last() {
            let contiguous = last.chain_id == r.chain_id && r.residue_seq == last.residue_seq + 1;
            if !contiguous {
                if current.len() >= 2 {
                    segments.push(Segment {
                        residues: std::mem::take(&mut current),
                    });
                } else {
                    current.clear();
                }
            }
        }
        current.push(r);
    }
    if current.len() >= 2 {
        segments.push(Segment { residues: current });
    }
    segments
}

// -- secondary-structure assignment -----------------------------------------

fn ca_distance(a: V3, b: V3) -> f32 {
    norm(sub(a, b))
}

/// Virtual bond angle (degrees, 0..180) at `p1` formed by `p0`–`p1`–`p2`.
fn virtual_angle(p0: V3, p1: V3, p2: V3) -> f32 {
    let a = normalize(sub(p0, p1));
    let b = normalize(sub(p2, p1));
    dot(a, b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Virtual torsion (signed dihedral, degrees -180..180) about `p1`–`p2`.
fn virtual_torsion(p0: V3, p1: V3, p2: V3, p3: V3) -> f32 {
    let b1 = sub(p1, p0);
    let b2 = sub(p2, p1);
    let b3 = sub(p3, p2);
    let n1 = cross(b1, b2);
    let n2 = cross(b2, b3);
    let m = cross(n1, normalize(b2));
    dot(m, n2).atan2(dot(n1, n2)).to_degrees()
}

/// Cα-only secondary-structure heuristic (P-SEA / Labesse 1997 style): combine a
/// virtual-angle/torsion test with a Cα(i±n) distance test. `ca` are the
/// segment's Cα positions.
fn raw_ss(ca: &[V3]) -> Vec<Ss> {
    let n = ca.len();
    let mut out = vec![Ss::Loop; n];
    for (i, slot) in out.iter_mut().enumerate() {
        // Angle/torsion form needs i-1..i+2; distance form needs i-1..i+3.
        let (theta, tau) = if i >= 1 && i + 2 < n {
            (
                Some(virtual_angle(ca[i - 1], ca[i], ca[i + 1])),
                Some(virtual_torsion(ca[i - 1], ca[i], ca[i + 1], ca[i + 2])),
            )
        } else {
            (None, None)
        };
        let (d2, d3, d4) = if i >= 1 && i + 3 < n {
            (
                Some(ca_distance(ca[i - 1], ca[i + 1])),
                Some(ca_distance(ca[i - 1], ca[i + 2])),
                Some(ca_distance(ca[i - 1], ca[i + 3])),
            )
        } else {
            (None, None, None)
        };

        let helix_angle = matches!((theta, tau), (Some(t), Some(u))
            if (89.0..=100.0).contains(&t) && (40.0..=60.0).contains(&u));
        let helix_dist = matches!((d2, d3, d4), (Some(a), Some(b), Some(c))
            if (5.0..=5.5).contains(&a) && (5.0..=5.5).contains(&b) && (6.1..=6.5).contains(&c));
        let sheet_angle = matches!((theta, tau), (Some(t), Some(u))
            if (124.0..=145.0).contains(&t) && (125.0..=180.0).contains(&u.abs()));
        let sheet_dist = matches!((d2, d3, d4), (Some(a), Some(b), Some(c))
            if (6.4..=6.8).contains(&a) && (9.7..=10.4).contains(&b) && (12.6..=13.4).contains(&c));

        *slot = if helix_angle || helix_dist {
            Ss::Helix
        } else if sheet_angle || sheet_dist {
            Ss::Sheet
        } else {
            Ss::Loop
        };
    }
    out
}

/// Demote short helix (<4) / sheet (<3) runs to loop to remove single-residue
/// speckle.
fn smooth_ss(labels: &mut [Ss]) {
    let n = labels.len();
    let mut i = 0;
    while i < n {
        let s = labels[i];
        let mut j = i;
        while j < n && labels[j] == s {
            j += 1;
        }
        let min_len = match s {
            Ss::Helix => 4,
            Ss::Sheet => 3,
            Ss::Loop => 0,
        };
        if s != Ss::Loop && (j - i) < min_len {
            labels[i..j].fill(Ss::Loop);
        }
        i = j;
    }
}

/// Assign secondary structure to a segment: prefer file annotations, else the
/// geometric heuristic (smoothed).
fn assign_ss(structure: &Structure, seg: &mut Segment) {
    if structure.has_ss_annotations() {
        for r in &mut seg.residues {
            r.ss = structure
                .ss_at(&r.chain_id, r.residue_seq)
                .unwrap_or(Ss::Loop);
        }
        return;
    }
    let ca: Vec<V3> = seg.residues.iter().map(|r| r.ca).collect();
    let mut labels = raw_ss(&ca);
    smooth_ss(&mut labels);
    for (r, s) in seg.residues.iter_mut().zip(labels) {
        r.ss = s;
    }
}

// -- spline, frames, extrusion ----------------------------------------------

/// Uniform Catmull-Rom point on the `p1`→`p2` span (neighbors `p0`,`p3`).
fn catmull_rom(p0: V3, p1: V3, p2: V3, p3: V3, t: f32) -> V3 {
    let t2 = t * t;
    let t3 = t2 * t;
    // 0.5 * (2p1 + (-p0+p2)t + (2p0-5p1+4p2-p3)t² + (-p0+3p1-3p2+p3)t³)
    let mut r = scale(p1, 2.0);
    r = add(r, scale(sub(p2, p0), t));
    r = add(
        r,
        scale(
            add(
                add(scale(p0, 2.0), scale(p1, -5.0)),
                add(scale(p2, 4.0), scale(p3, -1.0)),
            ),
            t2,
        ),
    );
    r = add(
        r,
        scale(
            add(
                add(scale(p0, -1.0), scale(p1, 3.0)),
                add(scale(p2, -3.0), p3),
            ),
            t3,
        ),
    );
    scale(r, 0.5)
}

/// One extruded cross-section.
struct Section {
    center: V3,
    /// Carbonyl-derived reference normal (ribbon orientation); `None` → transport.
    co: Option<V3>,
    half_w: f32,
    half_t: f32,
    /// Superellipse exponent: 2 → round, higher → flat-faced ribbon.
    flat: f32,
    ss: Ss,
    atom_index: usize,
}

/// A residue's target cross-section: half-width, half-thickness, flatness.
fn residue_profile(ss: Ss, radius: f32) -> (f32, f32, f32) {
    match ss {
        Ss::Loop => (radius, radius, 2.0),
        Ss::Helix => (HELIX_HW, HELIX_HT, RIBBON_FLATNESS),
        Ss::Sheet => (RIBBON_HW, RIBBON_HT, RIBBON_FLATNESS),
    }
}

/// A point on the superellipse cross-section at angle `phi`, in the local
/// `(normal, binormal)` plane. `flat = 2` is an ellipse; larger flattens the
/// faces while keeping rounded edges.
fn section_offset(half_w: f32, half_t: f32, flat: f32, phi: f32) -> (f32, f32) {
    let e = 2.0 / flat;
    let cx = phi.cos();
    let cy = phi.sin();
    let px = cx.signum() * cx.abs().powf(e);
    let py = cy.signum() * cy.abs().powf(e);
    (half_w * px, half_t * py)
}

/// Replace each β-strand run's last `ARROW_SECTIONS` cross-sections with an
/// arrowhead: a flared base (the barb) tapering linearly to a sharp tip.
fn apply_arrowheads(sections: &mut [Section]) {
    let n = sections.len();
    let mut i = 0;
    while i < n {
        if sections[i].ss != Ss::Sheet {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && sections[j].ss == Ss::Sheet {
            j += 1;
        }
        let head = ARROW_SECTIONS.min(j - i);
        let head_start = j - head;
        for (k, s) in sections[head_start..j].iter_mut().enumerate() {
            let t = if head <= 1 {
                1.0
            } else {
                k as f32 / (head - 1) as f32
            };
            s.half_w = lerpf(ARROW_BASE_HW, ARROW_TIP_HW, t);
        }
        i = j;
    }
}

/// Build the cross-section list for a segment.
fn sections_for(seg: &Segment, radius: f32) -> Vec<Section> {
    // Make carbonyl directions consistent so interpolation never cancels (they
    // alternate ~180° between residues in strands/helices).
    let mut res = seg.residues.clone();
    for i in 1..res.len() {
        if res[i].has_co && res[i - 1].has_co && dot(res[i].co, res[i - 1].co) < 0.0 {
            res[i].co = scale(res[i].co, -1.0);
        }
    }
    let n = res.len();
    let co_of = |r: &ResidueTrace| -> Option<V3> { r.has_co.then_some(r.co) };
    let profiles: Vec<(f32, f32, f32)> =
        res.iter().map(|r| residue_profile(r.ss, radius)).collect();

    let mut sections = Vec::new();
    for i in 0..n - 1 {
        let p0 = res[i.saturating_sub(1)].ca;
        let p1 = res[i].ca;
        let p2 = res[i + 1].ca;
        let p3 = res[(i + 2).min(n - 1)].ca;
        for j in 0..SUBDIV {
            let t = j as f32 / SUBDIV as f32;
            let nearest = if t < 0.5 { i } else { i + 1 };
            let co = match (co_of(&res[i]), co_of(&res[i + 1])) {
                (Some(a), Some(b)) => Some(normalize(lerp(a, b, t))),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            sections.push(Section {
                center: catmull_rom(p0, p1, p2, p3, t),
                co,
                half_w: lerpf(profiles[i].0, profiles[i + 1].0, t),
                half_t: lerpf(profiles[i].1, profiles[i + 1].1, t),
                flat: lerpf(profiles[i].2, profiles[i + 1].2, t),
                ss: res[nearest].ss,
                atom_index: res[nearest].atom_index,
            });
        }
    }
    // Final endpoint at the last residue.
    let last = n - 1;
    sections.push(Section {
        center: res[last].ca,
        co: co_of(&res[last]),
        half_w: profiles[last].0,
        half_t: profiles[last].1,
        flat: profiles[last].2,
        ss: res[last].ss,
        atom_index: res[last].atom_index,
    });
    apply_arrowheads(&mut sections);
    sections
}

/// A perpendicular to `t`, transported from `prev` (or arbitrary if degenerate).
fn transport(prev: V3, t: V3) -> V3 {
    let p = sub(prev, scale(t, dot(prev, t)));
    if norm(p) > 1e-3 {
        normalize(p)
    } else {
        // Pick any axis not parallel to t.
        let axis = if t[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        normalize(sub(axis, scale(t, dot(axis, t))))
    }
}

/// Per-section orthonormal `(tangent, normal, binormal)` frames, with flip
/// correction so the ribbon does not self-twist. `normal` is the wide axis,
/// taken from the carbonyl direction projected perpendicular to the tangent.
fn frames(sections: &[Section]) -> Vec<(V3, V3, V3)> {
    let n = sections.len();
    let mut out = Vec::with_capacity(n);
    let mut prev_normal = [1.0, 0.0, 0.0];
    for i in 0..n {
        let prev = sections[i.saturating_sub(1)].center;
        let next = sections[(i + 1).min(n - 1)].center;
        let mut tangent = normalize(sub(next, prev));
        if norm(tangent) < 1e-6 {
            tangent = [0.0, 0.0, 1.0];
        }
        let wide_axis = sections[i].co.and_then(|co| {
            let proj = sub(co, scale(tangent, dot(co, tangent)));
            (norm(proj) > 1e-3).then(|| normalize(proj))
        });
        let mut normal = match wide_axis {
            Some(nrm) => nrm,
            None => transport(prev_normal, tangent),
        };
        if dot(normal, prev_normal) < 0.0 {
            normal = scale(normal, -1.0);
        }
        let binormal = normalize(cross(tangent, normal));
        normal = normalize(cross(binormal, tangent)); // re-orthogonalize
        prev_normal = normal;
        out.push((tangent, normal, binormal));
    }
    smooth_frames(&mut out);
    out
}

/// One pass of 1-2-1 smoothing on the frame normals (re-orthogonalized to each
/// tangent), which evens out the per-residue twist so ribbons read cleanly.
fn smooth_frames(frames: &mut [(V3, V3, V3)]) {
    let n = frames.len();
    if n < 3 {
        return;
    }
    let normals: Vec<V3> = frames.iter().map(|f| f.1).collect();
    for i in 1..n - 1 {
        let (tangent, _, _) = frames[i];
        let avg = add(add(normals[i - 1], scale(normals[i], 2.0)), normals[i + 1]);
        let proj = sub(avg, scale(tangent, dot(avg, tangent)));
        if norm(proj) > 1e-3 {
            let normal = normalize(proj);
            let binormal = normalize(cross(tangent, normal));
            frames[i] = (tangent, normalize(cross(binormal, tangent)), binormal);
        }
    }
}

/// Extrude one segment into `out`, appending vertices/normals/colors/indices.
fn extrude_segment(
    structure: &Structure,
    seg: &Segment,
    radius: f32,
    params: &CartoonParams,
    out: &mut Mesh,
) {
    let sections = sections_for(seg, radius);
    if sections.len() < 2 {
        return;
    }
    let frames = frames(&sections);

    // Segment-local buffers (index offsets are applied when merging into `out`).
    let mut positions: Vec<V3> = Vec::new();
    let mut colors: Vec<Rgb> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let color_of = |s: &Section| -> Rgb {
        (params.color_fn)(s.atom_index, &structure.atoms[s.atom_index], s.ss)
    };

    // Ring vertices.
    for (s, &(_, normal, binormal)) in sections.iter().zip(&frames) {
        let color = color_of(s);
        for k in 0..RING {
            let phi = std::f32::consts::TAU * (k as f32) / (RING as f32);
            let (ox, oy) = section_offset(s.half_w, s.half_t, s.flat, phi);
            let local = add(scale(normal, ox), scale(binormal, oy));
            positions.push(add(s.center, local));
            colors.push(color);
        }
    }

    // Stitch consecutive rings into a closed tube.
    for s in 0..sections.len() - 1 {
        let a = (s * RING) as u32;
        let b = ((s + 1) * RING) as u32;
        for k in 0..RING {
            let k1 = ((k + 1) % RING) as u32;
            let (k, k1u) = (k as u32, k1);
            indices.extend_from_slice(&[a + k, b + k, b + k1u]);
            indices.extend_from_slice(&[a + k, b + k1u, a + k1u]);
        }
    }

    // End caps (triangle fans to each ring's center).
    let cap = |positions: &mut Vec<V3>,
               colors: &mut Vec<Rgb>,
               indices: &mut Vec<u32>,
               ring_start: usize,
               center: V3,
               color: Rgb,
               front: bool| {
        let c = positions.len() as u32;
        positions.push(center);
        colors.push(color);
        let base = ring_start as u32;
        for k in 0..RING {
            let k1 = ((k + 1) % RING) as u32;
            let (k, k1) = (k as u32, k1);
            if front {
                indices.extend_from_slice(&[c, base + k1, base + k]);
            } else {
                indices.extend_from_slice(&[c, base + k, base + k1]);
            }
        }
    };
    cap(
        &mut positions,
        &mut colors,
        &mut indices,
        0,
        sections[0].center,
        color_of(&sections[0]),
        true,
    );
    let last = sections.len() - 1;
    cap(
        &mut positions,
        &mut colors,
        &mut indices,
        last * RING,
        sections[last].center,
        color_of(&sections[last]),
        false,
    );

    // Smooth vertex normals by accumulating face normals.
    let mut normals = vec![[0.0f32; 3]; positions.len()];
    for tri in indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let fn_ = cross(
            sub(positions[i1], positions[i0]),
            sub(positions[i2], positions[i0]),
        );
        for &idx in &[i0, i1, i2] {
            normals[idx] = add(normals[idx], fn_);
        }
    }
    for nrm in &mut normals {
        let nn = normalize(*nrm);
        *nrm = if norm(nn) < 1e-6 { [0.0, 0.0, 1.0] } else { nn };
    }

    // Merge into the shared mesh with an index offset.
    let offset = out.positions.len() as u32;
    out.positions.extend(positions);
    out.normals.extend(normals);
    out.colors.extend(colors);
    out.indices.extend(indices.into_iter().map(|i| i + offset));
}

/// Laplacian-smooth a segment's Cα path (and carbonyl directions) toward the
/// local midpoint, weighted by secondary structure. This straightens the
/// per-residue zig-zag so helices read as clean spirals and strands as flat
/// ribbons, while loops stay close to the true backbone. Endpoints are pinned.
fn smooth_segment(seg: &mut Segment) {
    let n = seg.residues.len();
    if n < 3 {
        return;
    }
    // Make carbonyl directions consistent before averaging (they alternate
    // ~180° per residue, which would otherwise cancel).
    for i in 1..n {
        if seg.residues[i].has_co
            && seg.residues[i - 1].has_co
            && dot(seg.residues[i].co, seg.residues[i - 1].co) < 0.0
        {
            seg.residues[i].co = scale(seg.residues[i].co, -1.0);
        }
    }
    for _ in 0..SMOOTH_ITERS {
        let ca: Vec<V3> = seg.residues.iter().map(|r| r.ca).collect();
        let co: Vec<V3> = seg.residues.iter().map(|r| r.co).collect();
        for i in 1..n - 1 {
            let w = match seg.residues[i].ss {
                Ss::Helix => SMOOTH_HELIX,
                Ss::Sheet => SMOOTH_SHEET,
                Ss::Loop => SMOOTH_LOOP,
            };
            let mid = scale(add(ca[i - 1], ca[i + 1]), 0.5);
            seg.residues[i].ca = lerp(ca[i], mid, w);
            if seg.residues[i].has_co {
                let cmid = scale(add(co[i - 1], co[i + 1]), 0.5);
                seg.residues[i].co = normalize(lerp(co[i], cmid, 0.5));
            }
        }
    }
}

/// Build cartoon meshes for `selected` atoms into `out`.
pub fn build_cartoon(
    structure: &Structure,
    selected: &[usize],
    style_radius: Option<f32>,
    params: &CartoonParams,
    out: &mut Mesh,
) {
    let radius = style_radius.unwrap_or(R_TUBE);
    let mut segments = backbone_segments(structure, selected);
    for seg in &mut segments {
        assign_ss(structure, seg);
        smooth_segment(seg);
        extrude_segment(structure, seg, radius, params, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(serial: usize, name: &str, seq: i32, x: f32, y: f32, z: f32) -> Atom {
        Atom {
            serial,
            name: name.into(),
            element: if name == "CA" {
                "C".into()
            } else {
                name.into()
            },
            residue_name: "ALA".into(),
            residue_seq: seq,
            chain_id: "A".into(),
            hetero: false,
            b_factor: 0.0,
            occupancy: 1.0,
            x: x as f64,
            y: y as f64,
            z: z as f64,
        }
    }

    /// Build a structure of CA/C/O atoms from per-residue Cα positions on an
    /// ideal α-helix (radius 2.3 Å, 1.5 Å rise, 100°/residue).
    fn ideal_helix(n: usize) -> Structure {
        let mut atoms = Vec::new();
        let mut serial = 1;
        for k in 0..n {
            let theta = (k as f32) * 100.0_f32.to_radians();
            let ca = [2.3 * theta.cos(), 2.3 * theta.sin(), 1.5 * k as f32];
            // Place a plausible carbonyl just off the Cα.
            let c = add(ca, [0.3, 0.0, 0.4]);
            let o = add(c, [0.2, 0.5, 0.0]);
            atoms.push(atom(serial, "CA", k as i32 + 1, ca[0], ca[1], ca[2]));
            atoms.push(atom(serial + 1, "C", k as i32 + 1, c[0], c[1], c[2]));
            atoms.push(atom(serial + 2, "O", k as i32 + 1, o[0], o[1], o[2]));
            serial += 3;
        }
        Structure::new(atoms)
    }

    /// Extended β-strand: planar zig-zag, Cα-Cα ~3.8 Å, virtual angle ~130°.
    fn ideal_strand(n: usize) -> Structure {
        let mut atoms = Vec::new();
        let mut serial = 1;
        // Step along x with an alternating y to make a zig-zag of angle ~130°.
        let dx = 3.8 * (65.0_f32.to_radians()).sin(); // half-angle 65° → angle 130°
        let dy = 3.8 * (65.0_f32.to_radians()).cos();
        for k in 0..n {
            let ca = [dx * k as f32, if k % 2 == 0 { 0.0 } else { dy }, 0.0];
            let c = add(ca, [0.0, 0.0, 0.4]);
            let o = add(c, [0.0, 0.5, 0.2]);
            atoms.push(atom(serial, "CA", k as i32 + 1, ca[0], ca[1], ca[2]));
            atoms.push(atom(serial + 1, "C", k as i32 + 1, c[0], c[1], c[2]));
            atoms.push(atom(serial + 2, "O", k as i32 + 1, o[0], o[1], o[2]));
            serial += 3;
        }
        Structure::new(atoms)
    }

    fn all(structure: &Structure) -> Vec<usize> {
        (0..structure.atoms.len()).collect()
    }

    #[test]
    fn virtual_angle_and_torsion_are_sane() {
        // Right angle.
        assert!(
            (virtual_angle([1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]) - 90.0).abs() < 1e-3
        );
        // Planar cis (0°) and a +90° dihedral.
        let tau = virtual_torsion(
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
        );
        assert!((tau.abs() - 90.0).abs() < 1e-3);
    }

    #[test]
    fn assigns_helix_on_ideal_helix() {
        let mut seg = Segment {
            residues: backbone_segments(&ideal_helix(12), &all(&ideal_helix(12)))
                .pop()
                .unwrap()
                .residues,
        };
        assign_ss(&ideal_helix(12), &mut seg);
        let helices = seg.residues.iter().filter(|r| r.ss == Ss::Helix).count();
        // Most interior residues should read as helix.
        assert!(helices >= 6, "expected mostly helix, got {helices}");
    }

    #[test]
    fn assigns_sheet_on_ideal_strand() {
        let s = ideal_strand(8);
        let mut seg = backbone_segments(&s, &all(&s)).pop().unwrap();
        assign_ss(&s, &mut seg);
        let sheets = seg.residues.iter().filter(|r| r.ss == Ss::Sheet).count();
        assert!(sheets >= 2, "expected some sheet, got {sheets}");
    }

    #[test]
    fn smoothing_drops_short_runs() {
        let mut labels = vec![
            Ss::Loop,
            Ss::Helix,
            Ss::Helix, // 2-residue helix island → dropped
            Ss::Loop,
            Ss::Sheet,
            Ss::Sheet,
            Ss::Sheet, // 3-residue sheet run → kept
        ];
        smooth_ss(&mut labels);
        assert_eq!(labels[1], Ss::Loop);
        assert_eq!(labels[2], Ss::Loop);
        assert_eq!(labels[4], Ss::Sheet);
    }

    #[test]
    fn annotations_override_geometry() {
        let mut s = ideal_helix(6); // geometry would say helix
        for seq in 1..=6 {
            s.set_ss("A", seq, Ss::Sheet); // but annotate as sheet
        }
        let mut seg = backbone_segments(&s, &all(&s)).pop().unwrap();
        assign_ss(&s, &mut seg);
        assert!(seg.residues.iter().all(|r| r.ss == Ss::Sheet));
    }

    #[test]
    fn backbone_splits_on_gap() {
        // Residues 1,2,3 then 10,11 → two segments.
        let mut atoms = Vec::new();
        for (serial, &seq) in (1..).zip([1, 2, 3, 10, 11].iter()) {
            atoms.push(atom(serial, "CA", seq, seq as f32, 0.0, 0.0));
        }
        let s = Structure::new(atoms);
        let segs = backbone_segments(&s, &all(&s));
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].residues.len(), 3);
        assert_eq!(segs[1].residues.len(), 2);
    }

    fn grey(_: usize, _: &Atom, _: Ss) -> Rgb {
        [0.5, 0.5, 0.5]
    }

    fn by_ss(_: usize, _: &Atom, ss: Ss) -> Rgb {
        ss_color(ss)
    }

    #[test]
    fn mesh_is_non_empty_and_consistent() {
        let s = ideal_helix(12);
        let mut out = Mesh::default();
        let params = CartoonParams { color_fn: &grey };
        build_cartoon(&s, &all(&s), None, &params, &mut out);
        assert!(!out.positions.is_empty());
        assert_eq!(out.positions.len(), out.normals.len());
        assert_eq!(out.positions.len(), out.colors.len());
        assert_eq!(out.indices.len() % 3, 0);
        let max = out.positions.len() as u32;
        assert!(out.indices.iter().all(|&i| i < max));
    }

    #[test]
    fn ss_coloring_uses_palette_only() {
        let s = ideal_helix(12);
        let mut out = Mesh::default();
        let params = CartoonParams { color_fn: &by_ss };
        build_cartoon(&s, &all(&s), None, &params, &mut out);
        let palette = [ss_color(Ss::Helix), ss_color(Ss::Sheet), ss_color(Ss::Loop)];
        assert!(out.colors.iter().all(|c| palette.contains(c)));
        // And not the grey a non-SS color_fn would produce.
        assert!(!out.colors.contains(&[0.5, 0.5, 0.5]));
    }

    #[test]
    fn no_mesh_without_backbone() {
        // Two bare carbons (no CA) → nothing to trace.
        let s = Structure::new(vec![
            atom(1, "C1", 1, 0.0, 0.0, 0.0),
            atom(2, "C2", 1, 1.5, 0.0, 0.0),
        ]);
        let mut out = Mesh::default();
        let params = CartoonParams { color_fn: &grey };
        build_cartoon(&s, &all(&s), None, &params, &mut out);
        assert!(out.positions.is_empty());
    }
}
