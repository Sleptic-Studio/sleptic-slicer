# Resin support research

What the established resin slicers do, from their own documentation, and how
Sleptic compares. Other slicers are references for behaviour only; no code is
copied (see `AGENTS.md`). Numbers are the vendors' published defaults.

## 1. Where supports are needed

Lychee 7.5, PreForm (Supports V1) and PrusaSlicer 2.9.1 all sort automatic
support points into the same four kinds:

| Kind | Lychee | PreForm | PrusaSlicer | Rule |
|---|---|---|---|---|
| Islands / minima | Islands (small, medium, large) | Minima | Islands (small, medium, large) | Every region of a layer with nothing under it gets a support, at its lowest point. A small island gets one point at its centre of mass; a medium one two or more; a large one points along the centre line where it is thin and round the perimeter where it is thick. |
| Overhangs | Overhang (angle threshold, density, minimum area) | Overhangs (angle from horizontal, density) | Overhangs (density by height and angle) | Downward faces flatter than the critical angle get points on a grid. 45 degrees is the usual threshold; it is a resin property. |
| Reinforcement | Reinforcement (the volume above an island) | Reinforcements (along the body) | Extra density as the area above grows | More points where the cured area above a held region grows, so peel force does not tear the first points off. |
| Stabilisation | Stabilisation (spacing on tall structures) | Stabilization (around minima) | - | Extra points that stop a tall or flexible part swaying. |

What can go unsupported (Formlabs design guides): an overhang up to 1 mm
without deformation and up to 5 mm at most; faces steeper than 10 to 19 degrees
from level; a bridge between two supported ends up to 21 to 29 mm for a
5 x 3 mm beam. The goal every guide states: zero islands before slicing.

Typical spacing between overhang points is 2 to 3 mm for miniatures and up to
4 to 5 mm for engineering parts; PrusaSlicer never places two points closer
than 1 mm.

## 2. How a support touches the part

| Part | Lychee | Chitubox | PrusaSlicer | PreForm |
|---|---|---|---|---|
| Contact diameter | 0.3 / 0.4 / 0.6 mm (light / medium / heavy) | upper diameter, similar | head front diameter 0.4 mm | touchpoint size, down to 0.1 mm |
| Sinks into the part | penetration (0.1 to 0.3 mm in practice) | contact depth | head penetration 0.2 mm | - |
| Contact shape | optional ball ("break point") a little wider than the tip | sphere or none | ball | - |
| Tip | cone, 2.5 mm long, angled to meet the surface at about 90 degrees | cone, connection length about 3 mm | pinhead about 1 mm, at most the critical angle (45 degrees) from vertical | tooth, with a minimum length so the pillar stays off the surface |
| Shaft | 1.1 / 1.3 / 1.5 mm | 1.0 to 1.2 mm | pillar 1.0 mm | trussed pillars |
| Base | foot with a 45 degree join cone; "adaptive base" keeps it off the model | bottom plate | cone 4 mm wide, 1 mm high, 1 mm from the model | raft |

Rules that follow:
- The tip is the only thin part. It narrows to the contact so the mark is
  small and the support snaps off at the part.
- The tip leans to meet the surface square, never more than about 45 degrees
  from vertical; the pillar under it is vertical.
- When the part is in the way below, the support bends away from the part
  with an angled section and then drops straight ("search for ground",
  1 to 30 mm sideways in Lychee). Standing a support on the model is a
  separate, optional mode ("internal supports", "support on build plate only").
- Fewer shafts by "parenting": several tips share one shaft (Lychee's
  Optimize Supports, PrusaSlicer's bridges and branching tree). Lychee turns
  it off for brittle resins, since one shaft failing drops several tips.

## 3. Bracing and lattices

- Lychee: any support taller than 20 mm needs bracing. Bracing is a grid of
  horizontal links with none, one ("simple") or two ("double") diagonals per
  cell, thinner than the shafts, starting a set distance above the base. An
  isolated tall support gets a companion support placed next to it so there
  is something to brace it to ("first reinforcement"), and a second above a
  further height.
- PrusaSlicer: pillars within 10 mm are linked zig-zag, cross (double
  zig-zag) or dynamic (chosen by distance); bridges up to 15 mm.
- Chitubox / Creality: a cross brace between neighbouring pillars closer
  than the "cross width", above a "cross start height".
- PreForm: pillars joined by trusses (truss width and height), or a full
  lattice (cell size in XY and Z); thin pillars can be bundled.
- Every brace is self-supporting: diagonals at about 45 degrees, never a
  long horizontal bar printed in mid-air.

## 4. Raft

- PreForm: full raft by default (best adhesion, recommended under 30 mm
  parts); mini rafts (one per support, merged when close) save 20 to 30%
  resin; a raised or bevelled edge so a removal tool can slip under it.
- PrusaSlicer pad: 2 mm thick, 45 degree wall slope, parts lifted 5 mm.
- Chitubox skate raft: thin plate with a sloped rim.
- Lift between plate and part: 3 to 10 mm, 5 mm by default nearly everywhere.

## 5. Sleptic against this

| Standard | R86 | R87 |
|---|---|---|
| Islands first, then overhang grid | yes | unchanged |
| Tip along the normal, at most 45 degrees | yes | unchanged |
| Thin tip, slim branches, thicker trunk | branches as thick as the trunk | branches 0.7 x shaft; trunk grows 15% per extra contact up to the largest column; junction ball 1.15 x trunk |
| Bend away then drop when the part is below | old leaning columns (thick, near-horizontal arms) or the contact was dropped | slim angled arm at the branch angle (then 55, 65 degrees), reaching 1 to 30 mm over 16 directions, then a vertical pillar; old lean code only when no arm fits |
| Isolated tall support gets a companion | lone poles unbraced | companion pillar 7 mm away, 75% as tall, zigzag braced, its own raft foot |
| Nothing below the plate | old lean feet dipped 0.08 mm under it | invariant test over fixtures, raft on/off, trees on/off |
| Raft scraper edge | 0.6 mm, small top chamfer | see R87 notes in `docs/STATUS.md` |
