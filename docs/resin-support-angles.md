# Resin support angles

Each bundled resin carries `support_overhang_angle_deg`: overhangs flatter than
this (degrees from horizontal) get supports, and the island check lets a layer
reach sideways onto the one below only as far as this allows. Picking a resin
sets the Supports panel's overhang angle to it.

No manufacturer publishes an overhang angle. What decides how far a freshly
cured overhang can reach before it sags or peels away is the stiffness of the
cured resin, which datasheets do publish (Shore hardness, tensile/flexural
modulus, elongation at break). Each resin is placed in a band from those
numbers:

| Band | Published properties | Angle |
|---|---|---|
| Very stiff | modulus at least 2.5 GPa, elongation at most 15% | 40 |
| Rigid | everything between, and resins with no published data | 45 |
| Soft rigid | modulus at most 0.8 GPa, or at most 1.0 GPa with at least 40% elongation | 50 |
| Rubber, 76-85 Shore A | | 65 |
| Rubber, 61-75 Shore A (or no Shore value) | | 70 |
| Rubber, 60 Shore A or softer | | 75 |

45 degrees is the long-standing resin rule (one layer height sideways per
layer). The bands either side are estimates from stiffness, not measurements;
the Overhang test (Calibration menu) measures a resin on your own printer and
saves the result.

## Values used

Sources disagree for several resins (versions, retailers); the manufacturer's
own sheet is preferred where one was found.

| Resin | Published facts | Angle |
|---|---|---|
| Siraya Tech Sculpt | 90D, Young's modulus 3.5 GPa, 5% | 40 |
| Liqcreate Strong-X | 87D, tensile 2.5 GPa, flexural 3.25 GPa, 8% | 40 |
| Siraya Tech Blu | 80D, 1.8 GPa, 32% | 45 |
| Siraya Tech Build | 77D, flexural 1.45 GPa, 8% | 45 |
| Siraya Tech Fast | 75D, 1.1 GPa, flexural 1.4 GPa, 7% | 45 |
| Anycubic ABS-Like | 81.3D, flexural 1.15 GPa, 25% | 45 |
| Anycubic Standard | 80-83D, flexural 1.5-2.1 GPa, 25-30% | 45 |
| Anycubic Craftsman | 84D | 45 |
| Elegoo ABS-Like | about 80D, 14-24% | 45 |
| Liqcreate Premium Tough | 69D, 1.0 GPa, 15-25% | 45 |
| Phrozen TR250LV | 79D, tensile 0.9 GPa, 20% | 45 |
| eSUN PM200 (PMMA-like) | 32-40% elongation | 45 |
| eSUN Water Washable | 74-82D, 17-30% | 45 |
| Siraya Tech Tenacious | 65D, 0.8 GPa, 70% | 50 |
| Phrozen Nylon-Green Tough | 78D, tensile 0.6 GPa | 50 |
| Prusament Resin Flex80 | 82 Shore A, 60% | 65 |
| Resione F69 | 60-75 Shore A, 255% | 70 |
| Liqcreate Flexible-X | 160% elongation (no Shore value found) | 70 |
| Siraya Tech Rebound 55A | 55 Shore A, 170% | 75 |
| Resione F80 | 50-60 Shore A, 159% | 75 |

No mechanical data was found for the rest (the Elegoo, Sunlu, Monocure and
other Anycubic and Phrozen resins, Siraya Tech Simple, Craft and Magna, eSUN
Hard-Tough, Resione M68, Prusament Tough and Model, Liqcreate's other
grades): they use the rigid 45.
