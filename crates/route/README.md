# diacraft-route

Right-angle connector routing for DiaCraft: a route round boxes, kept off other connectors, found within a work budget. It is a layer-2 crate that uses
the standard library alone and knows nothing of a document, so its names say nothing of DiaCraft and it can be carved into craft-libs for the other Craft apps'
connectors. The engine decides which shapes are obstacles and what the cells say; this crate finds the route. The window's routing preview calls it, and nothing in the engine does yet (
do).

## What it does

`route(&Obstacles, &Wires, &Request, &Settings, &mut Work) -> Outcome` takes the boxes a route goes round (`Obstacle`: an id, a box, and whether level or upright legs
may run through it), the legs of the other connectors (`Wire`: legs and the shapes their ends are glued to), two ends (`End`: a point, the ways it may leave, and the
obstacle it stands on) and the settings (clearance, spacing, the costs of a bend, a crossing, running near a wire and leaning against a flow). It gives the corners
of a route (`Outcome::Routed`), or `NoWay`, `OutOfWork` or a refusal in words.

A route is level and upright legs from the begin to the end. It leaves each end along a way the end allows, by a **stub** of the clearance across that side (shortened
to what is free when another box is nearer) and turns only after it; it arrives against the end's way; it enters no obstacle grown by the clearance unless the obstacle
lets it through or is exempt for the request. Its cost is its length, a bend cost for each turn, a cross cost for each wire's leg it crosses and a near cost for each unit
it runs closer than the spacing to a wire it is kept apart from. A search that finds no route at the full clearance is made again at half of it and then at none; one
that finds no way is made again on an area three times as wide. The route is then centred (the middle leg of a Z lies half way between the boxes, and a U keeps its leg
at the clearance) and tidied.

`enters_any(&Obstacles, &[Point], exempt, &mut Work) -> bool` is the test the engine's first try is kept by: whether a leg goes into a box that does not let it through.

## The algorithm, and where it comes from

The algorithm is ours, written from the published one: Wybrow, Marriott and Stuckey, *Orthogonal Connector Routing*, Graph Drawing 2009 (LNCS 5849). The graph is the
Hanan grid of the routing area: an upright line at the left and right edge of each obstacle grown by the clearance, a level line at its bottom and top, a line of each kind
through each end and the end of its stub, the area's edges and, for the wires a route is kept apart from, a line at the spacing on each side of each leg. The grid is
never built: its lines are two sorted lists, a vertex is a pair of indexes, and the search asks the obstacle buckets whether an edge is blocked when it reaches it. The
search is A\* over (vertex, the way the route arrived) with a heap of `(cost, depth, a key that turning and mirroring leave as it is, the order reached)`. Its heuristic
is the Manhattan distance to the nearest goal plus the bends the ends force (`forced_bends`: the fewest bends on an empty plane, from a table built once by trying every
sequence of at most six legs), a consistent lower bound, so the route found is the cheapest of the grid. The buckets are the uniform grid of the engine's snap index: a
`BTreeMap` of cells, a list of big boxes every query looks at, and each entry visited charged.

No code was copied from `ortho-router` or `connector-routing` (the two permissive routing crates the design surveyed), and nothing of the LGPL routing library their
surveys name was read: the algorithm is written from the paper. `docs/provenance.md` has no row for this crate.

## Limits (every input is hostile)

| Limit | Value | Past it |
|---|---|---|
| Coordinates | within 1e6 of the origin, finite | A box or a leg is left out and counted; an end is refused |
| Obstacles in an area / wires in an area | 2,048 / 4,096 | `OutOfWork` |
| Lines on one axis | 8,192 | `OutOfWork` |
| States one route's searches expand | 200,000 | `OutOfWork` |
| Steps of one route (states, pushes, bucket entries, lines) | 2,000,000, and no more than the work it is given | `OutOfWork` |
| Bucket entries one edge query visits | 4,096 | The edge counts as blocked and the answer's `cut` says so |
| Points of a route | 48 | `Refused` |

Every loop charges a `Work` before it runs, and a charge that would pass the budget is refused and leaves the work spent. The same input gives the same route, bit for bit,
on every machine: no hash map's order is in any result, and there is no clock, no thread and no recursion.

## What it does not do

It does not nudge or spread connectors that are already routed (a new route keeps its spacing from the others by its own cost), route Steiner trees, curve a route (the
engine draws a curve through the router's corners), or read a document. Straight and centre-to-centre connectors are not searched.

## Tests

`tests/it` holds the composed vectors (`vectors.rs`, each naming the wrong result it catches), two oracles written apart from the search (`oracle.rs`: a Dijkstra over the
whole materialized Hanan grid, and a search over the unit lattice) that the router's cost is held to on random scenes (`props.rs`, 2,000 cases each, and 60,000 each in the
ignored `long_` twins), the same scenes mirrored and turned (`symmetry.rs`), and the crafted hostile inputs, counted (`hostile.rs`, with ignored timed twins beside a
yardstick). `fuzz/` is the `route_scene` fuzz target; its input and the invariants a route must hold are `fuzz/src/lib.rs`, which `props.rs` includes.
