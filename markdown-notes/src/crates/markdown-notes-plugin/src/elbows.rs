//! Right-angled edges for a diagram that was drawn with curved ones.
//!
//! merman draws the edges of a state diagram as splines and has no setting
//! for it. It does keep the points the layout gave each edge, base64 JSON in
//! a `data-points` attribute, and the nodes say where they are, so an edge
//! that runs down the page can be drawn again: out of the bottom of the node
//! it leaves, through its points in vertical and horizontal runs, and into
//! the top of the node it reaches. The label sits on one of those points, so
//! it stays on the line.
//!
//! What the picture has to say is which node is joined to which, so every
//! edge leaves its node and reaches its node at a place of its own, no two
//! horizontal runs lie along each other, and an edge crossing another one
//! goes over it in a bump.
//!
//! An edge this cannot place, because it runs up the page or because a node
//! at either end of it was not found, keeps the path it came with.

use base64::Engine;

/// How tightly a corner is rounded.
const CORNER: f64 = 5.0;

/// Two coordinates closer than this are the same coordinate.
const SAME: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Frame {
    fn holds(&self, p: Point) -> bool {
        p.x >= self.left - SAME
            && p.x <= self.right + SAME
            && p.y >= self.top - SAME
            && p.y <= self.bottom + SAME
    }
}

/// Redraw every edge of `svg` that can be, and hand the rest back as it was.
pub fn square_the_edges(svg: &str) -> String {
    let nodes = nodes(svg);
    let edges = edges(svg, &nodes);
    let sizes = label_sizes(svg);
    let throughs = throughs(&edges, &nodes, &sizes);
    let runs = runs(&edges, &throughs);
    let routes: Vec<Vec<Point>> = (0..edges.len())
        .map(|e| corners(e, &throughs[e], &runs))
        .collect();

    let mut out = String::with_capacity(svg.len());
    let mut done = 0;
    // Where the label of each redrawn edge belongs, by the edge's id.
    let mut labels: Vec<(String, Point)> = Vec::new();

    for (e, edge) in edges.iter().enumerate() {
        let tag = &svg[edge.at..edge.at + edge.len];
        let Some(d) = attribute(tag, "d") else {
            continue;
        };
        let Some(start) = tag.find(&format!(" d=\"{d}\"")).map(|s| s + " d=\"".len()) else {
            continue;
        };
        let redrawn = format!(
            "{}{}{}",
            &tag[..start],
            path(&routes[e], &hops(e, &routes)),
            &tag[start + d.len()..]
        );
        out.push_str(&svg[done..edge.at]);
        out.push_str(&with_a_fixed_size_arrowhead(&redrawn, svg));
        done = edge.at + edge.len;

        // Two points are the ends of the edge, and neither is a place for a
        // label.
        if let (Some(id), Some(label)) = (&edge.id, edge.label()) {
            let at = throughs[e][label];
            let reach = sizes.iter().find(|(edge, _)| edge == id).map_or(0.0, |(_, half)| half.x);
            labels.push((id.clone(), halfway(at, &routes[e], &crossings(e, &routes, at.x, reach))));
        }
    }
    out.push_str(&svg[done..]);
    with_labels_on_their_edges(&out, &labels)
}

/// Where a label goes: halfway along the upright stretch of `route` that runs
/// through `point`, between the bend above it and the bend below it.
///
/// The layout puts every label of a row at one height. The stretch a label is
/// on starts and ends wherever its edge happens to bend, so at that height
/// the label sits against one bend with a long bare line to the other.
///
/// `crossed` is the heights at which other edges cross that stretch. They cut
/// it into pieces, and the label goes halfway along the longest piece, so it
/// does not sit on top of a line that is not its own.
fn halfway(point: Point, route: &[Point], crossed: &[f64]) -> Point {
    // The upright stretch at the label's place across the page. Where an edge
    // has two of them, the one the label was nearest.
    let away = |stretch: &&[Point]| {
        let (top, bottom) = (stretch[0].y.min(stretch[1].y), stretch[0].y.max(stretch[1].y));
        (top - point.y).max(point.y - bottom).max(0.0)
    };
    let Some(stretch) = route
        .windows(2)
        .filter(|s| (s[0].x - point.x).abs() < SAME && (s[1].x - point.x).abs() < SAME)
        .min_by(|a, b| away(a).total_cmp(&away(b)))
    else {
        return point;
    };
    let (above, below) = (stretch[0], stretch[1]);

    let mut cuts: Vec<f64> = crossed
        .iter()
        .copied()
        .filter(|y| *y > above.y && *y < below.y)
        .collect();
    cuts.push(above.y);
    cuts.push(below.y);
    cuts.sort_by(f64::total_cmp);
    let (top, bottom) = cuts
        .windows(2)
        .map(|piece| (piece[0], piece[1]))
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
        .unwrap_or((above.y, below.y));
    Point { x: point.x, y: (top + bottom) / 2.0 }
}

/// The heights at which a horizontal run of any edge but `e` passes within
/// `reach` either side of `x`.
fn crossings(e: usize, routes: &[Vec<Point>], x: f64, reach: f64) -> Vec<f64> {
    routes
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != e)
        .flat_map(|(_, route)| route.windows(2))
        .filter(|run| (run[0].y - run[1].y).abs() < 0.01)
        .filter(|run| run[0].x.min(run[1].x) < x + reach && run[0].x.max(run[1].x) > x - reach)
        .map(|run| run[0].y)
        .collect()
}

/// Point the edge at the arrowhead that does not grow with the line.
///
/// In the `neo` look merman defines each arrowhead twice: one measured in
/// stroke widths, which a two pixel line doubles, and one named `-margin`
/// measured in pixels. The edges it draws use the first.
fn with_a_fixed_size_arrowhead(tag: &str, svg: &str) -> String {
    let Some(marker) = attribute(tag, "marker-end") else {
        return tag.to_string();
    };
    let Some(id) = marker.strip_prefix("url(#").and_then(|m| m.strip_suffix(')')) else {
        return tag.to_string();
    };
    if id.ends_with("-margin") || !svg.contains(&format!("<marker id=\"{id}-margin\"")) {
        return tag.to_string();
    }
    tag.replace(
        &format!("marker-end=\"{marker}\""),
        &format!("marker-end=\"url(#{id}-margin)\""),
    )
}

/// Put each label back on the point the layout gave it.
///
/// merman slides a label along the curve it drew, and the curve is gone. The
/// point the label was laid out on is the middle one of the edge's points,
/// which the redrawn edge passes through.
fn with_labels_on_their_edges(svg: &str, labels: &[(String, Point)]) -> String {
    const OPENING: &str = "<g class=\"edgeLabel\" transform=\"";
    let mut covered: Vec<Frame> = Vec::new();
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;

    while let Some(at) = rest.find(OPENING) {
        let moved = at + OPENING.len();
        let Some(len) = rest[moved..].find('"') else {
            break;
        };
        out.push_str(&rest[..moved]);
        let after = &rest[moved + len..];

        // The group inside says which edge the label belongs to, and is moved
        // up and left by half the label's size to centre it.
        let inner = after
            .find("<g ")
            .map(|g| &after[g..])
            .and_then(|g| Some(&g[..g.find('>')?]));
        let id = inner.and_then(|tag| attribute(tag, "data-id"));
        match id.and_then(|id| labels.iter().find(|(edge, _)| edge == id)) {
            Some((_, point)) => {
                out.push_str(&format!("translate({:.3}, {:.3})", point.x, point.y));
                let half = inner
                    .and_then(|tag| attribute(tag, "transform"))
                    .and_then(translation)
                    .unwrap_or(Point { x: 0.0, y: 0.0 });
                covered.push(Frame {
                    left: point.x - half.x.abs(),
                    top: point.y - half.y.abs(),
                    right: point.x + half.x.abs(),
                    bottom: point.y + half.y.abs(),
                });
            }
            None => out.push_str(&rest[moved..moved + len]),
        }
        rest = after;
    }
    out.push_str(rest);
    with_room_for(&out, &covered)
}

/// The two numbers of a `translate(x, y)`.
fn translation(transform: &str) -> Option<Point> {
    let inner = transform.strip_prefix("translate(")?.strip_suffix(')')?;
    let mut parts = inner.split(',').map(|n| n.trim().parse::<f64>());
    Some(Point { x: parts.next()?.ok()?, y: parts.next()?.ok()? })
}

/// Space kept between a moved label and the edge of the picture.
const MARGIN: f64 = 8.0;

/// Widen the picture to hold every frame in `covered`.
///
/// merman sized the picture around the labels where it had put them. A label
/// moved onto its edge can end up past that, and would be cut off.
fn with_room_for(svg: &str, covered: &[Frame]) -> String {
    let widened = (|| {
        let end = svg.find('>')?;
        let root = &svg[..end];
        let view = attribute(root, "viewBox")?;
        let numbers: Vec<f64> = view
            .split_whitespace()
            .map(|n| n.parse::<f64>())
            .collect::<Result<_, _>>()
            .ok()?;
        let [x, y, wide, tall] = numbers[..] else {
            return None;
        };

        let mut all = Frame { left: x, top: y, right: x + wide, bottom: y + tall };
        for frame in covered {
            all.left = all.left.min(frame.left - MARGIN);
            all.top = all.top.min(frame.top - MARGIN);
            all.right = all.right.max(frame.right + MARGIN);
            all.bottom = all.bottom.max(frame.bottom + MARGIN);
        }
        let (new_wide, new_tall) = (all.right - all.left, all.bottom - all.top);
        if new_wide <= wide && new_tall <= tall {
            return None;
        }

        let root = root.replace(
            &format!("viewBox=\"{view}\""),
            &format!("viewBox=\"{} {} {new_wide} {new_tall}\"", all.left, all.top),
        );
        let root = match root.find("max-width:") {
            Some(at) => {
                let from = at + "max-width:".len();
                let len = root[from..].find("px")?;
                format!("{}{new_wide}{}", &root[..from], &root[from + len..])
            }
            None => root,
        };
        Some(format!("{root}{}", &svg[end..]))
    })();
    widened.unwrap_or_else(|| svg.to_string())
}

/// An edge this can place: one that runs down the page, between two nodes
/// that were found.
struct Edge {
    /// Where its `<path` tag starts in the SVG, and how long the tag is.
    at: usize,
    len: usize,
    id: Option<String>,
    points: Vec<Point>,
    /// The nodes it leaves and reaches, as indices into the nodes.
    from: usize,
    to: usize,
}

impl Edge {
    /// Which of its points the label sits on. Two points are the ends of the
    /// edge, and neither is a place for a label.
    fn label(&self) -> Option<usize> {
        (self.points.len() > 2).then_some(self.points.len() / 2)
    }
}

fn edges(svg: &str, nodes: &[Frame]) -> Vec<Edge> {
    let mut found = Vec::new();
    let mut offset = 0;
    while let Some(at) = svg[offset..].find("<path ").map(|at| at + offset) {
        let Some(len) = svg[at..].find('>') else {
            break;
        };
        found.extend(edge(&svg[at..at + len], at, len, nodes));
        offset = at + len;
    }
    found
}

fn edge(tag: &str, at: usize, len: usize, nodes: &[Frame]) -> Option<Edge> {
    if !tag.contains("data-edge=\"true\"") {
        return None;
    }
    attribute(tag, "d")?;
    let points = decoded(attribute(tag, "data-points")?)?;
    let (first, last) = (*points.first()?, *points.last()?);
    if points.len() < 2 || last.y <= first.y {
        return None;
    }
    let from = nodes.iter().position(|n| n.holds(first))?;
    let to = nodes.iter().position(|n| n.holds(last))?;
    if nodes[to].top <= nodes[from].bottom {
        return None;
    }
    Some(Edge {
        at,
        len,
        id: attribute(tag, "data-id").map(str::to_string),
        points,
        from,
        to,
    })
}

/// Room an edge keeps from the end of its part of a node's side.
const PORT_MARGIN: f64 = 4.0;

/// Closest two horizontal runs sit to each other, where there is the height.
const TRACK_GAP: f64 = 14.0;

/// Radius of the bump one edge makes going over another.
const HOP: f64 = 6.0;

/// Room a run keeps from what is above and below it.
const BELOW_A_NODE: f64 = 16.0;
const ABOVE_AN_ARROWHEAD: f64 = 22.0;
const BESIDE_A_BEND: f64 = 4.0;

/// How much of a straight part a label needs: its own height and a little
/// bare line above and below it.
const ROOM_FOR_A_LABEL: f64 = 34.0;

/// Place edges along a side from `left` to `right`. The side is cut into as
/// many equal parts as there are edges, handed out in the order of `heading`,
/// which is where each edge ends up. An edge takes the place it `wanted` if
/// that is inside its part, which keeps an edge going straight down straight,
/// and the middle of its part otherwise, which balances the rest. Answers in
/// the order it was asked.
fn spread(heading: &[f64], wanted: &[f64], left: f64, right: f64) -> Vec<f64> {
    let n = wanted.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        heading[*a]
            .total_cmp(&heading[*b])
            .then(wanted[*a].total_cmp(&wanted[*b]))
    });

    let part = (right - left) / n.max(1) as f64;
    let margin = PORT_MARGIN.min(part / 2.0);
    let mut placed = vec![0.0; n];
    for (slot, &i) in order.iter().enumerate() {
        let (lo, hi) = (left + part * slot as f64 + margin, left + part * (slot + 1) as f64 - margin);
        placed[i] = if wanted[i] >= lo && wanted[i] <= hi {
            wanted[i]
        } else {
            (lo + hi) / 2.0
        };
    }
    placed
}

/// The points each edge goes through: a place of its own on the bottom of the
/// node it leaves, the points the layout gave it, and a place of its own on
/// the top of the node it reaches.
///
/// Every edge of a node leaves it separately, so that what branches is seen
/// to branch at the node and not somewhere below it.
fn throughs(edges: &[Edge], nodes: &[Frame], sizes: &[(String, Point)]) -> Vec<Vec<Point>> {
    let side = |frame: &Frame| {
        let inset = ((frame.right - frame.left) / 2.0).min(CORNER + 1.0);
        (frame.left + inset, frame.right - inset)
    };

    let mut exits = vec![0.0; edges.len()];
    let mut entries = vec![0.0; edges.len()];
    for (n, frame) in nodes.iter().enumerate() {
        let (left, right) = side(frame);
        // The order the layout put the labels in is the order it found with
        // the fewest crossings, so that is the order the edges leave in.
        let leaving: Vec<usize> = (0..edges.len()).filter(|e| edges[*e].from == n).collect();
        let wanted: Vec<f64> = leaving.iter().map(|e| edges[*e].points[1].x).collect();
        let heading: Vec<f64> = wanted.clone();
        for (e, x) in leaving.iter().zip(spread(&heading, &wanted, left, right)) {
            exits[*e] = x;
        }

        let reaching: Vec<usize> = (0..edges.len()).filter(|e| edges[*e].to == n).collect();
        let wanted: Vec<f64> = reaching
            .iter()
            .map(|e| edges[*e].points[edges[*e].points.len() - 2].x)
            .collect();
        let coming_from: Vec<f64> = wanted.clone();
        for (e, x) in reaching.iter().zip(spread(&coming_from, &wanted, left, right)) {
            entries[*e] = x;
        }
    }

    let mut throughs: Vec<Vec<Point>> = edges
        .iter()
        .enumerate()
        .map(|(e, edge)| {
            let mut through = vec![Point { x: exits[e], y: nodes[edge.from].bottom }];
            through.extend_from_slice(&edge.points[1..edge.points.len() - 1]);
            through.push(Point { x: entries[e], y: nodes[edge.to].top });
            through
        })
        .collect();
    straighten(edges, &mut throughs, sizes);
    throughs
}

/// How far the boxes of two labels side by side may run into each other.
/// A box is a little wider than the words in it, and two edges leaving the
/// halves of one node are just closer together than two boxes are wide.
const LABELS_MAY_OVERLAP: f64 = 3.0;

/// How many times every label is tried. One that was in the way of another
/// may have been slid out of it since.
const PASSES: usize = 3;

/// Slide the label of an edge over the place it reaches its node, or under
/// the place it leaves the other, wherever nothing is in the way.
///
/// The layout puts a label between the two nodes, under neither, and an edge
/// through it has to step sideways twice. Under one of its ends it steps
/// once, and the long part of it is one straight line with the label on it.
/// Only an edge with the one point between its ends is moved: the others bend
/// where the layout needs them to.
fn straighten(edges: &[Edge], throughs: &mut [Vec<Point>], sizes: &[(String, Point)]) {
    let half = |e: usize| {
        let id = edges[e].id.as_deref()?;
        sizes.iter().find(|(label, _)| label == id).map(|(_, half)| *half)
    };

    for e in (0..PASSES).flat_map(|_| 0..edges.len()) {
        let (Some(label), 3) = (edges[e].label(), throughs[e].len()) else {
            continue;
        };
        let (exit, at, entry) = (throughs[e][0].x, throughs[e][label], throughs[e][2].x);
        if (at.x - exit).abs() < SAME || (at.x - entry).abs() < SAME {
            continue;
        }
        let Some(size) = half(e) else {
            continue;
        };

        // Over the node it reaches first, as mermaid has it: the edge then
        // turns just under the node it leaves and comes down on the other
        // with its label. Under the node it leaves if that place is taken.
        let ends = [entry, exit];
        let free = |x: f64| {
            (0..edges.len()).filter(|other| *other != e).all(|other| {
                let (Some(theirs), Some(their_size)) = (edges[other].label(), half(other)) else {
                    return true;
                };
                let they = throughs[other][theirs.min(throughs[other].len() - 1)];
                (they.x - x).abs() >= size.x + their_size.x - LABELS_MAY_OVERLAP
                    || (they.y - at.y).abs() >= size.y + their_size.y
            })
        };
        if let Some(x) = ends.into_iter().find(|x| free(*x)) {
            throughs[e][label].x = x;
        }
    }
}

/// Half the width and half the height of every edge's label, by edge id.
fn label_sizes(svg: &str) -> Vec<(String, Point)> {
    const OPENING: &str = "<g class=\"edgeLabel\"";
    let mut found = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find(OPENING) {
        rest = &rest[at + OPENING.len()..];
        let Some(inner) = rest.find("<g ").map(|g| &rest[g..]) else {
            break;
        };
        let Some(tag) = inner.find('>').map(|end| &inner[..end]) else {
            break;
        };
        let size = attribute(tag, "transform").and_then(translation);
        if let (Some(id), Some(size)) = (attribute(tag, "data-id"), size) {
            found.push((id.to_string(), Point { x: size.x.abs(), y: size.y.abs() }));
        }
    }
    found
}

/// A horizontal run an edge needs, to get from under one of its points to
/// over the next.
struct Run {
    edge: usize,
    /// Which of the edge's points it starts under.
    segment: usize,
    left: f64,
    right: f64,
    /// The heights it may be drawn between.
    lo: f64,
    hi: f64,
    /// Whether it is the last run of its edge, the one that brings it over
    /// the node it reaches.
    entering: bool,
    y: f64,
}

/// Every horizontal run, each at a height of its own wherever two of them
/// would otherwise lie along each other.
///
/// A run is drawn beside the node its edge turns at: one out of a node just
/// under it, one into a node just over it, and the long upright part between
/// is where the label goes. The longest run is placed first and so nearest
/// its node: out of a node the edge going furthest turns first and the others
/// pass under it without crossing, and into a node the same the other way up.
/// The rest stack away from the node, a whole [`TRACK_GAP`] apart.
fn runs(edges: &[Edge], throughs: &[Vec<Point>]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (e, through) in throughs.iter().enumerate() {
        let last = through.len() - 1;
        let labelled = edges[e].label().is_some();

        // How many stretches of the edge step sideways, and so need a run.
        let mut x = through[0].x;
        let mut steps = 0;
        for point in &through[1..] {
            if (x - point.x).abs() >= SAME {
                steps += 1;
                x = point.x;
            }
        }

        let mut x = through[0].x;
        for k in 0..last {
            let (a, b) = (through[k], through[k + 1]);
            if (x - b.x).abs() < SAME {
                continue;
            }

            // The gap between two rows that this stretch is in. A point
            // between the ends of an edge sits halfway down a gap, where the
            // layout left it, and only how far across it is matters: the
            // label is put wherever on the edge there turns out to be room.
            let (leaving, reaching) = (k == 0, k + 1 == last);
            let (top, bottom) = match (leaving, reaching) {
                (true, true) => (a.y, b.y),
                (true, false) if last == 2 => (a.y, through[2].y),
                (false, true) if last == 2 => (through[0].y, b.y),
                (true, false) => (a.y, 2.0 * b.y - a.y),
                (false, true) => (2.0 * a.y - b.y, b.y),
                (false, false) => (a.y, b.y),
            };
            let (mut lo, mut hi) = if leaving || reaching {
                (top + BELOW_A_NODE, bottom - ABOVE_AN_ARROWHEAD)
            } else {
                (top + BESIDE_A_BEND, bottom - BESIDE_A_BEND)
            };

            // A labelled edge between two rows keeps a straight part long
            // enough for its label: all of what its one run leaves it, or
            // the middle of the gap between its two.
            if labelled && last == 2 {
                let middle = (top + bottom) / 2.0;
                match (steps, leaving) {
                    (1, true) => hi -= ROOM_FOR_A_LABEL,
                    (1, false) => lo += ROOM_FOR_A_LABEL,
                    (_, true) => hi = hi.min(middle - ROOM_FOR_A_LABEL / 2.0),
                    (_, false) => lo = lo.max(middle + ROOM_FOR_A_LABEL / 2.0),
                }
            }
            if hi < lo {
                lo = (a.y + b.y) / 2.0;
                hi = lo;
            }
            runs.push(Run {
                edge: e,
                segment: k,
                left: x.min(b.x),
                right: x.max(b.x),
                lo,
                hi,
                entering: k != 0 && k + 1 == last,
                y: lo,
            });
            x = b.x;
        }
    }

    // Runs out of a node first, then runs into one, the longest first in both.
    let length = |r: &Run| r.right - r.left;
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by(|a, b| {
        let (a, b) = (&runs[*a], &runs[*b]);
        a.entering.cmp(&b.entering).then(length(b).total_cmp(&length(a)))
    });

    // Each run takes the first height, counting away from the node it turns
    // beside, that keeps it a whole [`TRACK_GAP`] clear of every run already
    // placed beside it. Only a gap too full for that packs them closer.
    let mut placed: Vec<usize> = Vec::new();
    for r in order {
        let run = &runs[r];
        let beside = |other: &Run| run.right + HOP > other.left && run.left - HOP < other.right;
        let nearest = if run.entering { run.hi } else { run.lo };
        let height = [TRACK_GAP, TRACK_GAP / 2.0].iter().find_map(|gap| {
            let steps = ((run.hi - run.lo) / gap).floor() as usize;
            (0..=steps)
                .map(|s| {
                    if run.entering {
                        run.hi - gap * s as f64
                    } else {
                        run.lo + gap * s as f64
                    }
                })
                .find(|y| {
                    placed
                        .iter()
                        .all(|p| !beside(&runs[*p]) || (runs[*p].y - y).abs() >= gap - 0.01)
                })
        });
        let y = height.unwrap_or(nearest);
        runs[r].y = y;
        placed.push(r);
    }
    runs
}

/// The corners of edge `e`, every run between them vertical or horizontal.
fn corners(e: usize, through: &[Point], runs: &[Run]) -> Vec<Point> {
    // Only the ends of the edge and its runs are corners. A point between
    // says how far across the edge is at that stretch, and nothing about how
    // far down: a run may well be above the point it steps across to. A point
    // a hair to one side of the stretch above it is on that stretch, since the
    // layout places a label half a pixel off the node under it.
    let mut x = through[0].x;
    let mut corners = vec![through[0]];
    for (k, b) in through[1..].iter().enumerate() {
        if let Some(run) = runs.iter().find(|run| run.edge == e && run.segment == k) {
            corners.push(Point { x, y: run.y });
            corners.push(Point { x: b.x, y: run.y });
            x = b.x;
        }
    }
    corners.push(Point { x, y: through[through.len() - 1].y });
    corners.dedup();
    corners
}

/// Where edge `e` goes over another edge: for each of its upright stretches,
/// counted by the corner it starts at, the heights at which a horizontal run
/// of some other edge passes through it. It is the upright line that goes
/// over the level one, as mermaid draws it.
fn hops(e: usize, routes: &[Vec<Point>]) -> Vec<(usize, f64)> {
    let mut found = Vec::new();
    for (i, pair) in routes[e].windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if (a.x - b.x).abs() > 0.01 {
            continue;
        }
        // A bump needs a straight piece of line on either side of it, or it
        // reads as part of the corner beside it.
        let clear = CORNER + HOP + 2.0;
        let (top, bottom) = (a.y.min(b.y) + clear, a.y.max(b.y) - clear);

        let mut over: Vec<f64> = Vec::new();
        for (other, route) in routes.iter().enumerate() {
            if other == e {
                continue;
            }
            for run in route.windows(2) {
                let (left, right) = (run[0].x.min(run[1].x), run[0].x.max(run[1].x));
                let level = (run[0].y - run[1].y).abs() < 0.01;
                if level && run[0].y > top && run[0].y < bottom && a.x > left + 1.0 && a.x < right - 1.0 {
                    over.push(run[0].y);
                }
            }
        }
        over.sort_by(f64::total_cmp);
        over.dedup_by(|next, kept| *next - *kept < HOP * 2.0 + 1.0);
        found.extend(over.into_iter().map(|y| (i, y)));
    }
    found
}

/// Path data through `corners`, each of them rounded, with a bump at every
/// one of `hops`.
fn path(corners: &[Point], hops: &[(usize, f64)]) -> String {
    let mut d = format!("M{:.3},{:.3}", corners[0].x, corners[0].y);
    for i in 1..corners.len() {
        let here = corners[i];

        // An edge only ever runs down the page, so a bump is entered from
        // above, and bulges to the right.
        let x = corners[i - 1].x;
        let mut over: Vec<f64> = hops.iter().filter(|(c, _)| *c == i - 1).map(|(_, y)| *y).collect();
        over.sort_by(f64::total_cmp);
        for y in over {
            d.push_str(&format!(
                "L{x:.3},{:.3}A{HOP},{HOP} 0 0 1 {x:.3},{:.3}",
                y - HOP,
                y + HOP,
            ));
        }

        let Some(&next) = corners.get(i + 1) else {
            d.push_str(&format!("L{:.3},{:.3}", here.x, here.y));
            break;
        };
        let before = corners[i - 1];
        let reach = |a: Point, b: Point| ((a.x - b.x).abs() + (a.y - b.y).abs()) / 2.0;
        let r = CORNER.min(reach(before, here)).min(reach(here, next));
        let toward = |a: Point, b: Point| Point {
            x: a.x + (b.x - a.x).signum() * r * f64::from((a.x - b.x).abs() > 0.0),
            y: a.y + (b.y - a.y).signum() * r * f64::from((a.y - b.y).abs() > 0.0),
        };
        let (enter, leave) = (toward(here, before), toward(here, next));
        d.push_str(&format!(
            "L{:.3},{:.3}Q{:.3},{:.3},{:.3},{:.3}",
            enter.x, enter.y, here.x, here.y, leave.x, leave.y
        ));
    }
    d
}

/// The value of attribute `name` in `tag`.
pub(crate) fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let opening = format!(" {name}=\"");
    let start = tag.find(&opening)? + opening.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// The points in a `data-points` attribute.
fn decoded(points: &str) -> Option<Vec<Point>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(points).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .as_array()?
        .iter()
        .map(|p| {
            Some(Point {
                x: p.get("x")?.as_f64()?,
                y: p.get("y")?.as_f64()?,
            })
        })
        .collect()
}

/// Where every node of the diagram is.
///
/// A node is a group moved into place by a `translate`, holding an outline
/// drawn around its own origin: a path, or a circle for a start or end state.
fn nodes(svg: &str) -> Vec<Frame> {
    let mut found = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find("<g class=\"node") {
        let body = &rest[at..];
        let end = body[1..]
            .find("<g class=\"node")
            .map_or(body.len(), |next| next + 1);
        if let Some(frame) = node(&body[..end]) {
            found.push(frame);
        }
        rest = &body[end..];
    }
    found
}

fn node(body: &str) -> Option<Frame> {
    let tag = &body[..body.find('>')?];
    let moved = attribute(tag, "transform")?;
    let inner = moved.strip_prefix("translate(")?.strip_suffix(')')?;
    let mut centre = inner.split(',').map(|n| n.trim().parse::<f64>());
    let (cx, cy) = (centre.next()?.ok()?, centre.next()?.ok()?);

    // Whichever shape comes first is the outline: a state is a path, a start
    // or end state a circle, and a flowchart's node a rectangle.
    let shapes = ["<path ", "<circle ", "<rect "];
    let (shape, at) = shapes
        .iter()
        .filter_map(|shape| Some((*shape, body.find(shape)?)))
        .min_by_key(|(_, at)| *at)?;
    let tag = &body[at..at + body[at..].find('>')?];
    let number = |name: &str| attribute(tag, name)?.parse::<f64>().ok();
    let (half_wide, half_tall) = match shape {
        "<path " => extent(attribute(tag, "d")?)?,
        "<circle " => (number("r")?, number("r")?),
        _ => (number("width")? / 2.0, number("height")? / 2.0),
    };

    Some(Frame {
        left: cx - half_wide,
        top: cy - half_tall,
        right: cx + half_wide,
        bottom: cy + half_tall,
    })
}

/// How far an outline reaches from its origin, across and down.
///
/// The outline is made of moves, lines and curves, which are all pairs of
/// coordinates. One drawn with anything else is not measured.
pub(crate) fn extent(d: &str) -> Option<(f64, f64)> {
    if d.chars().any(|c| c.is_ascii_alphabetic() && !"MLCQZmlcqzeE".contains(c)) {
        return None;
    }
    let numbers: Vec<f64> = d
        .split(|c: char| !(c.is_ascii_digit() || ".-eE".contains(c)))
        .filter(|n| !n.is_empty())
        .map(|n| n.parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    let reach = |offset: usize| {
        numbers
            .iter()
            .skip(offset)
            .step_by(2)
            .fold(0.0_f64, |most, n| most.max(n.abs()))
    };
    let (wide, tall) = (reach(0), reach(1));
    (wide > 0.0 && tall > 0.0).then_some((wide, tall))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(points: &[(f64, f64)]) -> String {
        let json = serde_json::Value::Array(
            points
                .iter()
                .map(|(x, y)| serde_json::json!({ "x": x, "y": y }))
                .collect(),
        );
        base64::engine::general_purpose::STANDARD.encode(json.to_string())
    }

    /// Two nodes, one below and to the right of the other, and a curved edge
    /// between them.
    fn sample() -> String {
        format!(
            "<svg><g class=\"nodes\">\
             <g class=\"node a\" transform=\"translate(100, 20)\">\
             <path d=\"M-15 -20 L15 -20 L15 20 L-15 20 Z\"/></g>\
             <g class=\"node b\" transform=\"translate(300, 220)\">\
             <circle r=\"7\"/></g></g>\
             <g class=\"edgeLabel\" transform=\"translate(170, 95)\">\
             <g class=\"label\" data-id=\"e\" transform=\"translate(-10, -8)\"></g></g>\
             <path d=\"M110,40C150,80,250,120,300,213\" id=\"e\" data-edge=\"true\" \
             data-id=\"e\" data-points=\"{}\"/></svg>",
            encoded(&[(110.0, 40.0), (200.0, 120.0), (300.0, 213.0)])
        )
    }

    /// The corners of a path this module wrote: the end of every `L`, and the
    /// control point of every `Q`.
    fn corners_of(d: &str) -> Vec<Point> {
        let numbers: Vec<f64> = d
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|n| !n.is_empty())
            .map(|n| n.parse().unwrap())
            .collect();
        numbers.chunks(2).map(|p| Point { x: p[0], y: p[1] }).collect()
    }

    #[test]
    fn an_edge_is_redrawn_without_curves() {
        let svg = square_the_edges(&sample());
        let tag = &svg[svg.rfind("<path ").unwrap()..];
        let tag = &tag[..tag.find('>').unwrap()];
        let d = attribute(tag, "d").unwrap();
        assert!(!d.contains('C'), "the edge still has a spline in it: {d}");
        assert!(tag.contains("id=\"e\""), "the rest of the tag was lost: {tag}");
    }

    #[test]
    fn every_run_is_vertical_or_horizontal() {
        let svg = square_the_edges(&sample());
        let at = svg.rfind("<path ").unwrap();
        let d = attribute(&svg[at..], "d").unwrap().to_string();
        let corners = corners_of(&d);
        for pair in corners.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                (a.x - b.x).abs() < 0.01 || (a.y - b.y).abs() < 0.01,
                "{a:?} to {b:?} is neither vertical nor horizontal in {d}"
            );
        }
    }

    #[test]
    fn it_leaves_the_bottom_of_one_node_and_enters_the_top_of_the_other() {
        let svg = square_the_edges(&sample());
        let at = svg.rfind("<path ").unwrap();
        let corners = corners_of(attribute(&svg[at..], "d").unwrap());
        let (first, last) = (corners[0], corners[corners.len() - 1]);
        assert_eq!(first.y, 40.0, "it does not start at the bottom of the first node");
        assert_eq!(last.y, 213.0, "it does not end at the top of the second node");
        assert!((last.x - 300.0).abs() <= 7.0, "it ends beside the node, at {last:?}");
    }

    #[test]
    fn it_passes_through_the_point_the_label_is_on() {
        let svg = square_the_edges(&sample());
        let at = svg.rfind("<path ").unwrap();
        let corners = corners_of(attribute(&svg[at..], "d").unwrap());
        assert!(
            corners.windows(2).any(|pair| {
                let (a, b) = (pair[0], pair[1]);
                (a.x - 300.0).abs() < 0.01
                    && (b.x - 300.0).abs() < 0.01
                    && a.y.min(b.y) <= 134.5
                    && a.y.max(b.y) >= 134.5
            }),
            "no run of {corners:?} passes through (300, 134.5), where the label was slid to"
        );
    }

    #[test]
    fn the_label_is_put_on_the_point_the_edge_passes_through() {
        let svg = square_the_edges(&sample());
        assert!(
            svg.contains("<g class=\"edgeLabel\" transform=\"translate(300.000, 134.500)\">"),
            "the label was not put over the middle of the node its edge reaches, halfway \
             between the bend at 56, which is 16 under the node the edge leaves, and the \
             node at 213: {svg}"
        );
        assert!(
            svg.contains("data-id=\"e\" transform=\"translate(-10, -8)\""),
            "the label's own offset was touched: {svg}"
        );
    }

    #[test]
    fn the_picture_grows_to_hold_a_label_moved_past_its_edge() {
        let cramped = sample().replace(
            "<svg>",
            "<svg style=\"max-width:120px;\" viewBox=\"195 0 120 240\">",
        );
        let svg = square_the_edges(&cramped);
        // The label is 20 wide and ends up centred on 300, so it ends at 310,
        // and the margin is kept clear to the right of that.
        assert!(
            svg.starts_with("<svg style=\"max-width:123px;\" viewBox=\"195 0 123 240\">"),
            "the picture was not widened: {}",
            &svg[..svg.find('>').unwrap()]
        );
    }

    #[test]
    fn a_picture_with_room_for_its_labels_keeps_its_size() {
        let roomy = sample().replace("<svg>", "<svg viewBox=\"0 0 400 240\">");
        assert!(square_the_edges(&roomy).starts_with("<svg viewBox=\"0 0 400 240\">"));
    }

    /// A drawing of square nodes 40 across, and edges through the points
    /// given, the way merman writes them.
    fn drawing(nodes: &[(f64, f64)], edges: &[&[(f64, f64)]]) -> String {
        let mut svg = String::from("<svg><g class=\"nodes\">");
        for (i, (x, y)) in nodes.iter().enumerate() {
            svg.push_str(&format!(
                "<g class=\"node n{i}\" transform=\"translate({x}, {y})\">\
                 <path d=\"M-20 -20 L20 -20 L20 20 L-20 20 Z\"/></g>"
            ));
        }
        svg.push_str("</g>");
        for (i, points) in edges.iter().enumerate() {
            svg.push_str(&format!(
                "<path d=\"M0,0C1,1,2,2,3,3\" id=\"e{i}\" data-edge=\"true\" \
                 data-id=\"e{i}\" data-points=\"{}\"/>",
                encoded(points)
            ));
        }
        svg.push_str("</svg>");
        svg
    }

    /// The path data of every edge, in the order they were given.
    fn edge_paths(svg: &str) -> Vec<String> {
        svg.split("<path ")
            .filter(|tag| tag.contains("data-edge=\"true\""))
            .map(|tag| attribute(&format!(" {tag}"), "d").unwrap().to_string())
            .collect()
    }

    /// The height of the first horizontal run of a path with no bumps in it.
    fn first_turn(d: &str) -> f64 {
        corners_of(d)
            .windows(2)
            .find(|pair| (pair[0].x - pair[1].x).abs() > 1.0)
            .map(|pair| pair[0].y)
            .expect("the path never turns")
    }

    #[test]
    fn every_edge_of_a_node_leaves_it_at_a_place_of_its_own() {
        let svg = square_the_edges(&drawing(
            &[(200.0, 20.0), (50.0, 220.0), (200.0, 220.0), (350.0, 220.0)],
            &[
                &[(190.0, 40.0), (50.0, 120.0), (50.0, 200.0)],
                &[(200.0, 40.0), (200.0, 120.0), (200.0, 200.0)],
                &[(210.0, 40.0), (350.0, 120.0), (350.0, 200.0)],
            ],
        ));
        let starts: Vec<f64> = edge_paths(&svg).iter().map(|d| corners_of(d)[0].x).collect();
        assert_eq!(starts.len(), 3);
        assert!(
            starts[0] + 7.9 <= starts[1] && starts[1] + 7.9 <= starts[2],
            "the three edges leave at {starts:?}"
        );
        assert_eq!(starts[1], 200.0, "the edge going straight down was moved off its line");
    }

    #[test]
    fn every_edge_of_a_node_reaches_it_at_a_place_of_its_own() {
        let svg = square_the_edges(&drawing(
            &[(50.0, 20.0), (350.0, 20.0), (200.0, 220.0)],
            &[
                &[(50.0, 40.0), (50.0, 120.0), (190.0, 200.0)],
                &[(350.0, 40.0), (350.0, 120.0), (210.0, 200.0)],
            ],
        ));
        let ends: Vec<f64> = edge_paths(&svg)
            .iter()
            .map(|d| corners_of(d).last().unwrap().x)
            .collect();
        assert!(ends[0] + 7.9 <= ends[1], "the two edges arrive at {ends:?}");
    }

    #[test]
    fn two_runs_the_same_way_do_not_lie_along_each_other() {
        let svg = square_the_edges(&drawing(
            &[(100.0, 20.0), (300.0, 220.0), (500.0, 220.0)],
            &[
                &[(110.0, 40.0), (300.0, 120.0), (300.0, 200.0)],
                &[(115.0, 40.0), (500.0, 120.0), (500.0, 200.0)],
            ],
        ));
        let paths = edge_paths(&svg);
        let (near, far) = (first_turn(&paths[0]), first_turn(&paths[1]));
        assert!(
            far + 3.0 <= near,
            "the edge going furthest turns at {far} and the other at {near}: \
             it has to turn first, or the other one crosses it"
        );
    }

    #[test]
    fn an_edge_crossing_another_goes_over_it() {
        let svg = square_the_edges(&drawing(
            &[(100.0, 20.0), (400.0, 220.0), (250.0, 20.0), (250.0, 220.0)],
            &[
                &[(110.0, 40.0), (400.0, 120.0), (400.0, 200.0)],
                &[(250.0, 40.0), (250.0, 120.0), (250.0, 200.0)],
            ],
        ));
        // The first edge turns 16 under its node, at 56, and runs across the
        // second, which comes straight down at 250 and goes over it.
        let paths = edge_paths(&svg);
        assert!(
            paths[1].contains("L250.000,50.000A6,6 0 0 1 250.000,62.000"),
            "the upright edge does not go over the run crossing it at 56: {}",
            paths[1]
        );
        assert!(!paths[0].contains('A'), "the level run has a bump in it: {}", paths[0]);
    }

    #[test]
    fn a_label_sits_halfway_between_the_bends_either_side_of_it() {
        let route = [
            Point { x: 10.0, y: 0.0 },
            Point { x: 10.0, y: 20.0 },
            Point { x: 50.0, y: 20.0 },
            Point { x: 50.0, y: 120.0 },
            Point { x: 90.0, y: 120.0 },
            Point { x: 90.0, y: 140.0 },
        ];
        let label = halfway(Point { x: 50.0, y: 60.0 }, &route, &[]);
        assert_eq!(label, Point { x: 50.0, y: 70.0 });
    }

    #[test]
    fn a_label_keeps_off_a_line_crossing_its_stretch() {
        let route = [Point { x: 50.0, y: 20.0 }, Point { x: 50.0, y: 120.0 }];
        // Crossed at 90, the stretch is 70 long above the crossing and 30
        // below it. A crossing outside the stretch does not count.
        let label = halfway(Point { x: 50.0, y: 100.0 }, &route, &[90.0, 300.0]);
        assert_eq!(label, Point { x: 50.0, y: 55.0 });
    }

    #[test]
    fn a_label_on_no_stretch_of_the_route_stays_where_it_was() {
        let route = [Point { x: 10.0, y: 0.0 }, Point { x: 10.0, y: 100.0 }];
        let label = Point { x: 40.0, y: 50.0 };
        assert_eq!(halfway(label, &route, &[]), label);
    }

    #[test]
    fn a_flowchart_node_is_a_rectangle_and_is_found_too() {
        let flowchart = sample().replace(
            "<path d=\"M-15 -20 L15 -20 L15 20 L-15 20 Z\"/>",
            "<rect class=\"basic\" x=\"-15\" y=\"-20\" width=\"30\" height=\"40\"/>",
        );
        assert!(flowchart.contains("<rect "), "the sample has no such node to replace");
        let svg = square_the_edges(&flowchart);
        let at = svg.rfind("<path ").unwrap();
        let d = attribute(&svg[at..], "d").unwrap();
        assert!(!d.contains('C'), "the edge from a rectangle was not redrawn: {d}");
        assert_eq!(corners_of(d)[0].y, 40.0);
    }

    #[test]
    fn a_redrawn_edge_takes_the_arrowhead_measured_in_pixels() {
        let with_markers = sample()
            .replace(
                "<svg>",
                "<svg><defs><marker id=\"tip\"></marker>\
                 <marker id=\"tip-margin\"></marker></defs>",
            )
            .replace("data-edge=\"true\"", "data-edge=\"true\" marker-end=\"url(#tip)\"");
        let svg = square_the_edges(&with_markers);
        assert!(svg.contains("marker-end=\"url(#tip-margin)\""), "{svg}");
    }

    #[test]
    fn an_arrowhead_with_no_other_version_is_kept() {
        let one_marker = sample()
            .replace("<svg>", "<svg><defs><marker id=\"tip\"></marker></defs>")
            .replace("data-edge=\"true\"", "data-edge=\"true\" marker-end=\"url(#tip)\"");
        let svg = square_the_edges(&one_marker);
        assert!(svg.contains("marker-end=\"url(#tip)\""), "{svg}");
    }

    #[test]
    fn an_edge_running_up_the_page_is_left_alone() {
        let svg = sample().replace(
            &encoded(&[(110.0, 40.0), (200.0, 120.0), (300.0, 213.0)]),
            &encoded(&[(300.0, 213.0), (200.0, 120.0), (110.0, 40.0)]),
        );
        assert_eq!(square_the_edges(&svg), svg);
    }

    #[test]
    fn a_path_that_is_not_an_edge_is_left_alone() {
        let svg = "<svg><path d=\"M0,0C1,1,2,2,3,3\"/></svg>";
        assert_eq!(square_the_edges(svg), svg);
    }
}
