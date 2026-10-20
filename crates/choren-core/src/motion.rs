//! Motion curves: number formatting, automatic tangents, left/right mirroring and the `.qianim`
//! animation format (`Animation typeVersion="2.0"`, editor namespace).

use crate::xml::{escape_attr, Doc, NodeId};
use std::collections::BTreeMap;
use std::ops::Bound;
use choren_robot::Robot;

/// Choregraphe writes key values with `%g` (6 significant digits).
pub fn fmt_val(v: f64) -> String {
    if !v.is_finite() || v == 0.0 {
        return "0".into();
    }
    let decimals = (5 - v.abs().log10().floor() as i64).clamp(0, 15) as usize;
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

/// Negates a raw number textually, so mirroring twice restores the exact bytes.
pub fn negate(raw: &str) -> String {
    match raw.strip_prefix('-') {
        Some(rest) => rest.to_string(),
        None if raw.trim_start_matches(['0', '.']).is_empty() => raw.to_string(),
        None => format!("-{raw}"),
    }
}

/// Mirroring across the sagittal plane flips roll and yaw joints (pure x or z axis); pitch,
/// HipYawPitch and the hands keep their sign.
pub fn mirror_negates(robot: &Robot, joint: &str) -> bool {
    robot.joints.iter().find(|j| j.name == joint).is_some_and(|j| j.axis[1] == 0.0 && (j.axis[0] != 0.0 || j.axis[2] != 0.0))
}

/// Joints of a dope-sheet group: "Whole body", "Head", "Arms", "Legs", or a chain name.
pub fn group(robot: &Robot, name: &str) -> Result<Vec<String>, String> {
    let chains: &[&str] = match name {
        "Whole body" => return Ok(robot.joints.iter().map(|j| j.name.clone()).collect()),
        "Head" => &["Head"],
        "Arms" => &["LArm", "RArm"],
        "Legs" => &["LLeg", "RLeg"],
        other if robot.joint_index(other).is_some() => return Ok(vec![other.to_string()]),
        other => return robot.chains.get(other).cloned().ok_or_else(|| format!("Unknown joint or group {other}")),
    };
    Ok(chains.iter().flat_map(|c| robot.chains.get(*c).cloned().unwrap_or_default()).collect())
}

/// A handle reaches a third of the way to its neighbour (`0.33333334f`).
pub const THIRD: f32 = 1.0 / 3.0;

/// The automatic slope of a key from its neighbours' (frame, value), also used by the simplifier:
/// zero unless the key lies strictly between them and every frame is non-negative, else their
/// secant, scaled down so neither third-way handle overshoots a neighbour's value.
pub fn auto_slope(prev: (i32, f32), cur: (i32, f32), next: (i32, f32)) -> f32 {
    let ((ip, vp), (i, v), (inx, vn)) = (prev, cur, next);
    if !((vn > v || vp > v) && (v > vn || v > vp)) || (ip | i | inx) < 0 {
        return 0.0;
    }
    let mut slope = (vn - vp) / (inx - ip) as f32;
    let reach = (inx - i) as f32 * THIRD * slope;
    if reach.abs() > (vn - v).abs() {
        slope *= (vn - v) / reach;
    }
    let reach = (i - ip) as f32 * THIRD * slope;
    if reach.abs() > (v - vp).abs() {
        slope *= (v - vp) / reach;
    }
    slope
}

/// Tangent params (fraction of the way to the neighbour, slope) → handle offset in frames and units. `neighbour` is the neighbour's frame on that side;
/// pass 0 for a missing left neighbour and -1 for a missing right one, which gives `(0, 0)`.
pub fn tangent_offset(frame: i32, neighbour: i32, (a, s): (f32, f32)) -> (f32, f32) {
    if neighbour < 0 || frame < 0 {
        return (0.0, 0.0);
    }
    let dx = (neighbour - frame) as f32 * a;
    (dx, dx * s)
}

/// The inverse of [`tangent_offset`]: a XAR `Tangent`'s handle offset → tangent params.
pub fn tangent_params(frame: i32, neighbour: i32, (dx, dy): (f32, f32)) -> (f32, f32) {
    if neighbour < 0 || frame < 0 {
        return (0.0, 0.0);
    }
    (dx / (neighbour - frame) as f32, if dx != 0.0 || dy != 0.0 { dy / dx } else { 0.0 })
}

/// Automatic Bezier handles (left, right) as (abscissa, ordinate) offsets in frames and units,
/// computed in `f32` so saved values match existing projects. Both sides share [`auto_slope`] at a third of the way to the
/// neighbour; a first key's left handle reaches towards frame 0 and a last key's right handle is
/// empty.
pub fn auto_tangents(prev: Option<(f64, f64)>, cur: (f64, f64), next: Option<(f64, f64)>) -> ((f64, f64), (f64, f64)) {
    let key = |(f, v): (f64, f64)| (f as i32, v as f32);
    let (prev, cur, next) = (prev.map(key), key(cur), next.map(key));
    let slope = match (prev, next) {
        (Some(p), Some(n)) => auto_slope(p, cur, n),
        _ => 0.0,
    };
    let handle = |neighbour: i32| {
        let (dx, dy) = tangent_offset(cur.0, neighbour, (THIRD, slope));
        (dx as f64, dy as f64)
    };
    (handle(prev.map_or(0, |p| p.0)), handle(next.map_or(-1, |n| n.0)))
}

/// Choren's own simplifier, Ramer–Douglas–Peucker: which of `pts` (frame, value) to keep so linear
/// interpolation between kept keys stays within `tolerance` of every dropped one. The ends are
/// always kept.
pub fn rdp_simplify(pts: &[(f64, f64)], tolerance: f64) -> Vec<bool> {
    let mut keep = vec![pts.len() <= 2; pts.len()];
    if pts.len() > 2 {
        let (first, last) = (0, pts.len() - 1);
        keep[first] = true;
        keep[last] = true;
        let mut stack = vec![(first, last)];
        while let Some((a, b)) = stack.pop() {
            let ((x0, y0), (x1, y1)) = (pts[a], pts[b]);
            let err = |&(x, y): &(f64, f64)| (y - (y0 + (y1 - y0) * (x - x0) / (x1 - x0))).abs();
            if let Some(i) = (a + 1..b).max_by(|&i, &j| err(&pts[i]).total_cmp(&err(&pts[j]))).filter(|&i| err(&pts[i]) > tolerance) {
                keep[i] = true;
                stack.extend([(a, i), (i, b)]);
            }
        }
    }
    keep
}

/// A curve key as the simplifier sees it: frame, value and each side's tangent params
/// (fraction of the way to the neighbour, slope), whatever the side's interpolation type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimplifyKey {
    pub frame: i32,
    pub value: f32,
    pub left: (f32, f32),
    pub right: (f32, f32),
}

/// Basic curve simplification over the keys of a frame range, in `f32`. Walking from the second key, each interior key `k` is
/// tried out: its neighbours `p` and `n` get new params (a third of the way, [`auto_slope`] against
/// their own new neighbours; an end of the range keeps its inner side's params), and the curve
/// from `p`'s left neighbour to `n`'s right one is resampled and scored against the original curve
/// by [`compare_curves`]. A score under `margin` removes `k` and stores the new params on both
/// sides of `p` and `n`; the walk then retries at `n`. `fps` sets the sampling step (20 ms).
/// Returns which keys stay and, for each key whose params were rewritten, its new params.
pub fn simplify(keys: &[SimplifyKey], margin: f32, fps: i32) -> (Vec<bool>, Vec<Option<(f32, f32)>>) {
    let (mut keep, mut params) = (vec![true; keys.len()], vec![None; keys.len()]);
    if keys.len() <= 2 {
        return (keep, params);
    }
    let original = whole_curve(keys, fps);
    let mut cur: Vec<(usize, SimplifyKey)> = keys.iter().copied().enumerate().collect();
    let mut i = 1;
    while i + 1 < cur.len() {
        let (p, n) = (cur[i - 1].1, cur[i + 1].1);
        let mut samples = vec![];
        let (from, p_params) = if i == 1 {
            (p.frame, p.right)
        } else {
            let pp = cur[i - 2].1;
            let p_params = (THIRD, auto_slope((pp.frame, pp.value), (p.frame, p.value), (n.frame, n.value)));
            samples.extend(segment(&pp, pp.right, &p, p_params, fps));
            (pp.frame, p_params)
        };
        let nn = cur.get(i + 2).map(|&(_, k)| k);
        let n_params = match nn {
            None => n.left,
            Some(nn) => (THIRD, auto_slope((p.frame, p.value), (n.frame, n.value), (nn.frame, nn.value))),
        };
        samples.extend(segment(&p, p_params, &n, n_params, fps));
        if let Some(nn) = nn {
            samples.extend(segment(&n, n_params, &nn, nn.left, fps));
        }
        let to = nn.map_or(n.frame, |nn| nn.frame);
        // `margin <= score` keeps the key, so a NaN score keeps it too
        if margin > compare_curves(&original, from, to, &samples) {
            for (at, ps) in [(i - 1, p_params), (i + 1, n_params)] {
                cur[at].1.left = ps;
                cur[at].1.right = ps;
                params[cur[at].0] = Some(ps);
            }
            let (gone, _) = cur.remove(i);
            (keep[gone], params[gone]) = (false, None);
        } else {
            i += 1;
        }
    }
    (keep, params)
}

/// Advanced curve simplification: from each kept key `s`, one Bezier segment is stretched over the following
/// keys for as long as [`explore_params`] finds params for it within `margin`; the keys it passes over
/// are dropped. Returns the new key for each key that stays (`None` for a dropped one): values
/// stay, the sides between kept keys take the params found (and become Bezier).
pub fn advanced_simplify(keys: &[SimplifyKey], margin: f32, angularity: f32, granularity: f32, fps: i32) -> Vec<Option<SimplifyKey>> {
    let original = whole_curve(keys, fps);
    let mut out: Vec<Option<SimplifyKey>> = keys.iter().enumerate().map(|(i, &k)| (i == 0).then_some(k)).collect();
    let mut s = 0;
    while s + 1 < keys.len() {
        let (mut m, mut right, mut left) = (s + 1, keys[s].right, keys[s + 1].left);
        // each try starts again from `s`'s and the new end's original params
        while let Some(n) = keys.get(m + 1) {
            let mut ps = [keys[s].right.0, keys[s].right.1, n.left.0, n.left.1];
            if !explore_params(&original, &keys[s], n, &mut ps, margin, angularity, granularity, fps) {
                break;
            }
            (m, right, left) = (m + 1, (ps[0], ps[1]), (ps[2], ps[3]));
        }
        if let Some(k) = out[s].as_mut() {
            k.right = right;
        }
        out[m] = Some(SimplifyKey { left, ..keys[m] });
        s = m;
    }
    out
}

/// A search for the params `ps` = `[a0, s0, a1, s1]` of
/// one Bezier segment from `a` to `b`, scored by [`compare_curves`] against the original curve
/// between them. One param after the other is tried a step down and a step up, strictly inside
/// its bounds: abscissas within (0, 1.1) by steps of 1.1 / `granularity`, slopes within
/// ± `angularity` / fps of where they started by a `granularity`-th of that span. A better score
/// keeps the candidate; the search ends on a score under `margin` (true) or after four tries in a
/// row without a better one (false), leaving the best params in `ps`.
fn explore_params(original: &BTreeMap<i32, Vec<f32>>, a: &SimplifyKey, b: &SimplifyKey, ps: &mut [f32; 4], margin: f32, angularity: f32, granularity: f32, fps: i32) -> bool {
    // the candidate runs from frame 0 to the span of the original samples
    let count: i32 = original.range(a.frame..b.frame).map(|(_, s)| s.len() as i32).sum();
    let span = ((count * fps * THREAD_DELAY_MS) as f32 / 1000.0) as i32;
    let (from, to) = (a.frame, b.frame);
    let (a, b) = (SimplifyKey { frame: 0, ..*a }, SimplifyKey { frame: span, ..*b });
    let samples = |p: &[f32; 4]| segment(&a, (p[0], p[1]), &b, (p[2], p[3]), fps);
    // the first score compares with the segments from frame 0 to 2, not `from` to `to`: mostly a
    // sample count mismatch (f32::MAX), so the search runs
    let mut score = compare_curves(original, 0, 2, &samples(ps));
    if margin > score {
        return true;
    }
    let reach = angularity / fps as f32;
    let lo = [0.0, ps[1] - reach, 0.0, ps[3] - reach];
    let hi = [1.1, ps[1] + reach, 1.1, ps[3] + reach];
    let step = [1.1 / granularity, (hi[1] - lo[1]) / granularity, 1.1 / granularity, (hi[3] - lo[3]) / granularity];
    let (mut i, mut tries, mut found) = (0, 4, false);
    loop {
        tries -= 1;
        let (mut down, mut up) = (*ps, *ps);
        down[i] -= step[i];
        up[i] += step[i];
        for (p, inside) in [(down, down[i] > lo[i]), (up, hi[i] > up[i])] {
            if !inside {
                continue;
            }
            let sc = compare_curves(original, from, to, &samples(&p));
            if score > sc {
                (*ps, tries, found, score) = (p, 4, margin > sc, sc);
            }
        }
        if found || tries <= 0 {
            return found;
        }
        i = (i + 1) % 4;
    }
}

/// Each segment's samples by its first frame.
fn whole_curve(keys: &[SimplifyKey], fps: i32) -> BTreeMap<i32, Vec<f32>> {
    keys.windows(2).map(|w| (w[0].frame, segment(&w[0], w[0].right, &w[1], w[1].left, fps))).collect()
}

/// The simplifier samples every 20 ms of motion.
const THREAD_DELAY_MS: i32 = 20;

/// The Bezier segment from key `a` (handle params `ra`) to key `b` (handle params `lb`) sampled
/// every `fps * THREAD_DELAY_MS / 1000` frames, the ends
/// snapped to that grid.
fn segment(a: &SimplifyKey, ra: (f32, f32), b: &SimplifyKey, lb: (f32, f32), fps: i32) -> Vec<f32> {
    let step = fps as f32 * (THREAD_DELAY_MS as f32 / 1000.0);
    let snap = |frame: i32| (frame as f32 / step + 0.5).floor() * step;
    let (p0, p3) = ((snap(a.frame), a.value), (snap(b.frame), b.value));
    let dx = (b.frame - a.frame) as f32 * ra.0;
    let p1 = (p0.0 + dx, p0.1 + ra.1 * dx);
    let dx = (a.frame - b.frame) as f32 * lb.0;
    let p2 = (p3.0 + dx, p3.1 + lb.1 * dx);
    bezier_samples(p0, p1, p2, p3, step)
}

/// The cubic Bezier's ordinate at `p0.x + step`, `p0.x + 2 step`, … up to `p3.x` (within 1e-4), the abscissa inverted
/// per sample. Handles past the other end are cut back along their line; control points that run
/// backwards give no samples.
fn bezier_samples(p0: (f32, f32), mut p1: (f32, f32), mut p2: (f32, f32), p3: (f32, f32), step: f32) -> Vec<f32> {
    if p3.0 < p0.0 || p1.0 < p0.0 || p3.0 < p2.0 {
        return vec![];
    }
    if p1.0 > p3.0 {
        p1 = (p3.0, (p3.0 - p0.0) * (p1.1 - p0.1) / (p1.0 - p0.0) + p0.1);
    }
    if p2.0 < p0.0 {
        p2 = (p0.0, (p0.0 - p3.0) * (p2.1 - p3.1) / (p2.0 - p3.0) + p3.1);
    }
    let a = p2.0 * -3.0 + (p1.0 * 3.0 - p0.0) + p3.0;
    let b = p2.0 * 3.0 + p0.0 * 3.0 + p1.0 * -6.0;
    let c = p0.0 * -3.0 + p1.0 * 3.0;
    let mut out = vec![];
    let mut x = p0.0 + step;
    if p3.0 + 1e-4 <= x {
        return out;
    }
    while {
        let t = invert_bezier_x(x, p0.0, a, b, c);
        let (t2, t3) = (t * t, t * (t * t));
        out.push(t3 * p3.1 + (t3 * -3.0 + t2 * 3.0) * p2.1 + p1.1 * (t * 3.0 + t3 * 3.0 + t2 * -6.0) + (t * -3.0 + (t2 * 3.0 - t3) + 1.0) * p0.1);
        x += step;
        x < p3.0 + 1e-4
    } {}
    out
}

/// The parameter `t` in [0, 1] where `a t³ + b t² + c t + x0` equals `x`. Coefficients are scaled
/// by the largest one and a leading coefficient under 1e-4 drops the degree; the smallest real root
/// from -1e-4 on is taken.
// Solved in f64 (Cardano), so `t` may differ from an f32 solver's in the last bits when the
// handles are not a third of the way (x not linear in t).
fn invert_bezier_x(x: f32, x0: f32, a: f32, b: f32, c: f32) -> f32 {
    if (x0 - x).abs() < 1e-4 {
        return 0.0;
    }
    if (a + b + c + (x0 - x)).abs() < 1e-4 {
        return 1.0;
    }
    let mut k = [a, b, c, x0 - x];
    let max = k.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if max >= 1e-4 {
        k.iter_mut().for_each(|v| *v /= max);
    }
    let [a, b, c, d] = k.map(f64::from);
    let roots: Vec<f64> = if a.abs() >= 1e-4 {
        cubic_roots(b / a, c / a, d / a)
    } else if b.abs() / b.abs().max(c.abs()).max(d.abs()) >= 1e-4 {
        // `deuxiemedegre`: a complex pair whose imaginary part is under 1e-4 counts as real
        let disc = c * c - 4.0 * b * d;
        if disc >= 0.0 {
            vec![(-c - disc.sqrt()) / (2.0 * b), (-c + disc.sqrt()) / (2.0 * b)]
        } else if (-disc).sqrt() / (2.0 * b).abs() <= 1e-4 {
            vec![-c / (2.0 * b)]
        } else {
            vec![]
        }
    } else if c.abs() >= 1e-4 {
        vec![-d / c]
    } else if d.abs() > 1e-4 {
        vec![]
    } else {
        vec![0.0]
    };
    roots.into_iter().filter(|&t| t >= -1e-4).min_by(f64::total_cmp).map_or(0.0, |t| t.clamp(0.0, 1.0) as f32)
}

/// Real roots of `t³ + b t² + c t + d`.
fn cubic_roots(b: f64, c: f64, d: f64) -> Vec<f64> {
    let (p, q) = (c - b * b / 3.0, 2.0 * b * b * b / 27.0 - b * c / 3.0 + d);
    let disc = q * q / 4.0 + p * p * p / 27.0;
    if disc > 1e-12 {
        let r = disc.sqrt();
        vec![(-q / 2.0 + r).cbrt() + (-q / 2.0 - r).cbrt() - b / 3.0]
    } else if p.abs() < 1e-12 {
        vec![-b / 3.0]
    } else {
        let m = 2.0 * (-p / 3.0).sqrt();
        let phi = (3.0 * q / (p * m)).clamp(-1.0, 1.0).acos() / 3.0;
        (0..3).map(|k| m * (phi - 2.0 * std::f64::consts::PI * k as f64 / 3.0).cos() - b / 3.0).collect()
    }
}

/// The error between `samples` and the original samples of
/// the segments starting at `from` up to (not including) `to`. Per sample the difference `e` is
/// paired with the previous one: `|e₀ + e₁|` when they share a sign, else `(e₀² + e₁²) / (|e₀| +
/// |e₁|)` (the area of a trapezoid that crosses zero, doubled); the sum is divided by twice the
/// sample count. A different sample count scores `f32::MAX`.
pub fn compare_curves(original: &BTreeMap<i32, Vec<f32>>, from: i32, to: i32, samples: &[f32]) -> f32 {
    // a frame no segment starts at stands for the map's end (`from` there: nothing)
    let end = if original.contains_key(&to) { Bound::Excluded(to) } else { Bound::Unbounded };
    let reference: Vec<f32> = if original.contains_key(&from) { original.range((Bound::Included(from), end)).flat_map(|(_, s)| s.iter().copied()).collect() } else { vec![] };
    if reference.len() != samples.len() {
        return f32::MAX;
    }
    let (mut sum, mut prev) = (0.0f32, 0.0f32);
    for (o, n) in reference.iter().zip(samples) {
        let e = n - o;
        sum += if prev * e >= 0.0 { (prev + e).abs() } else { (prev * prev + e * e) / (e.abs() + prev.abs()) };
        prev = e;
    }
    sum / (samples.len() * 2) as f32
}

/// Linear handles: one third of the way towards each neighbour.
fn linear_tangents(prev: Option<(f64, f64)>, cur: (f64, f64), next: Option<(f64, f64)>) -> ((f64, f64), (f64, f64)) {
    let left = prev.map_or((0.0, 0.0), |p| ((p.0 - cur.0) / 3.0, (p.1 - cur.1) / 3.0));
    let right = next.map_or((0.0, 0.0), |n| ((n.0 - cur.0) / 3.0, (n.1 - cur.1) / 3.0));
    (left, right)
}

const EDITOR_NS: &str = "http://www.aldebaran.com/animation/editor";

/// Exports a timeline's motion curves as a `.qianim` document. Keys without tangents (automatic
/// Bezier) and linear keys become Bezier handles tagged with `editor:interpType`.
pub fn to_qianim(d: &Doc, timeline: NodeId) -> Result<String, String> {
    let fps = d.attr_num(timeline, "fps", 25.0);
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Animation typeVersion=\"2.0\" xmlns:editor=\"{EDITOR_NS}\">\n");
    for list in d.kids(timeline, "ActuatorList") {
        for c in d.kids(list, "ActuatorCurve") {
            let actuator = d.attr_or(c, "actuator");
            let unit = if d.attr_raw(c, "unit") == Some("1") || (actuator.ends_with("Hand") && d.attr_raw(c, "unit") != Some("0")) { "dimensionless" } else { "degree" };
            let mute = if d.attr_raw(c, "mute") == Some("1") { "true" } else { "false" };
            out += &format!("    <ActuatorCurve fps=\"{}\" actuator=\"{}\" mute=\"{mute}\" unit=\"{unit}\">\n", fmt_val(fps), escape_attr(&actuator));
            let keys: Vec<NodeId> = d.kids(c, "Key").collect();
            let pt = |i: usize| (d.attr_num(keys[i], "frame", 0.0), d.attr_num(keys[i], "value", 0.0));
            for (i, &k) in keys.iter().enumerate() {
                let (prev, cur, next) = (i.checked_sub(1).map(pt), pt(i), keys.get(i + 1).map(|_| pt(i + 1)));
                let flag = |a: &str| if d.attr_raw(k, a) == Some("1") { format!(" editor:{a}=\"true\"") } else { String::new() };
                out += &format!("        <Key frame=\"{}\" value=\"{}\"{}{}>\n", d.attr_or(k, "frame"), d.attr_or(k, "value"), flag("smooth"), flag("symmetrical"));
                let auto = auto_tangents(prev, cur, next);
                let linear = linear_tangents(prev, cur, next);
                for (side, a, l) in [("left", auto.0, linear.0), ("right", auto.1, linear.1)] {
                    let t = d.kids(k, "Tangent").find(|&t| d.attr_raw(t, "side") == Some(side));
                    let (params, editor) = match t.map(|t| d.attr_or(t, "interpType")).as_deref() {
                        None => (a, " editor:interpType=\"bezier_auto\""),
                        Some("linear") => (l, " editor:interpType=\"linear\""),
                        Some("bezier") => ((d.attr_num(t.unwrap(), "abscissaParam", 0.0), d.attr_num(t.unwrap(), "ordinateParam", 0.0)), ""),
                        Some(other) => return Err(format!("unsupported Tangent type \"{other}\" ({actuator}, frame {})", cur.0)),
                    };
                    out += &format!("            <Tangent side=\"{side}\" interpType=\"bezier\" abscissaParam=\"{}\" ordinateParam=\"{}\"{editor}/>\n", fmt_val(params.0), fmt_val(params.1));
                }
                out += "        </Key>\n";
            }
            out += "    </ActuatorCurve>\n";
        }
    }
    out += "</Animation>\n";
    Ok(out)
}

/// `%g` with 6 significant digits, trailing zeros dropped, exponent form
/// (`1e-05`) below 1e-4 or from 1e6. Negative zero keeps its sign, as printf's `%g` does [inf].
pub fn fmt_g6(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return if v.is_nan() { "nan".into() } else if v.is_infinite() { if v > 0.0 { "inf".into() } else { "-inf".into() } } else if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let trim = |s: String| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    let e = format!("{v:.5e}");
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if !(-4..6).contains(&exp) {
        return format!("{}e{}{:02}", trim(mantissa.to_string()), if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    trim(format!("{v:.prec$}", prec = (5 - exp).max(0) as usize))
}

/// "Export motion to clipboard › Python": the timeline's curves with keys, by actuator name,
/// unmuted first and muted ones inside `"""`. Times are `(frame − 1) / fps`; joint values are
/// radians. Bezier keys are `[value, [type, dx / fps, dy], [type, dx / fps, dy]]` with the
/// handles' absolute offsets (types 0 constant, 1 linear, 2 bezier, 3 automatic); simplified
/// keys are the value alone, for `angleInterpolation`.
pub fn to_python(d: &Doc, timeline: NodeId, bezier: bool) -> String {
    let fps = d.attr_num(timeline, "fps", 25.0) as i32 as f32;
    let mut curves: Vec<NodeId> = d.kids(timeline, "ActuatorList").flat_map(|l| d.kids(l, "ActuatorCurve").collect::<Vec<_>>()).filter(|&c| d.kid(c, "Key").is_some()).collect();
    curves.sort_by_key(|&c| (d.attr_raw(c, "mute") == Some("1"), d.attr_or(c, "actuator")));
    let mut out = format!("# Choregraphe {} export in Python.
from naoqi import ALProxy
names = list()
times = list()
keys = list()

", if bezier { "bezier" } else { "simplified" });
    for c in curves {
        let actuator = d.attr_or(c, "actuator");
        let degrees = !(d.attr_raw(c, "unit") == Some("1") || (actuator.ends_with("Hand") && d.attr_raw(c, "unit") != Some("0")));
        let motion = |v: f64| if degrees { (v as f32).to_radians() as f64 } else { v as f32 as f64 };
        let keys: Vec<NodeId> = d.kids(c, "Key").collect();
        let pt = |i: usize| (d.attr_num(keys[i], "frame", 0.0), d.attr_num(keys[i], "value", 0.0));
        let times: Vec<String> = keys.iter().map(|&k| fmt_g6(((d.attr_num(k, "frame", 0.0) as i32 as f32 - 1.0) / fps) as f64)).collect();
        let values: Vec<String> = (0..keys.len())
            .map(|i| {
                let value = fmt_g6(motion(pt(i).1));
                if !bezier {
                    return value;
                }
                let auto = auto_tangents(i.checked_sub(1).map(pt), pt(i), keys.get(i + 1).map(|_| pt(i + 1)));
                let side = |name: &str, auto: (f64, f64)| {
                    let t = d.kids(keys[i], "Tangent").find(|&t| d.attr_raw(t, "side") == Some(name));
                    let kind = match t.and_then(|t| d.attr_raw(t, "interpType")) {
                        None => 3,
                        Some("constant") => 0,
                        Some("bezier") => 2,
                        _ => 1,
                    };
                    // A last key's right handle is empty (getAbsoluteTangentOffset).
                    let (dx, dy) = match t {
                        _ if name == "right" && i + 1 == keys.len() => (0.0, 0.0),
                        Some(t) => (d.attr_num(t, "abscissaParam", 0.0), d.attr_num(t, "ordinateParam", 0.0)),
                        None => auto,
                    };
                    format!("[{kind}, {}, {}]", fmt_g6((dx as f32 / fps) as f64), fmt_g6(motion(dy)))
                };
                format!("[{value}, {}, {}]", side("left", auto.0), side("right", auto.1))
            })
            .collect();
        let muted = d.attr_raw(c, "mute") == Some("1");
        let quote = if muted { "\"\"\"\n" } else { "" };
        out += &format!("{quote}names.append(\"{actuator}\")\ntimes.append([{}])\nkeys.append([{}])\n{quote}\n", times.join(", "), values.join(", "));
    }
    out += "try:\n  # uncomment the following line and modify the IP if you use this script outside Choregraphe.\n  # motion = ALProxy(\"ALMotion\", IP, 9559)\n  motion = ALProxy(\"ALMotion\")\n";
    out += if bezier { "  motion.angleInterpolationBezier(names, times, keys)\n" } else { "  motion.angleInterpolation(names, keys, times, True)\n" };
    out + "except BaseException, err:\n  print err\n"
}

/// Converts a `.qianim` document into a XAR `<ActuatorList>` for a timeline at `fps`.
pub fn from_qianim(xml: &str, fps: f64) -> Result<String, String> {
    let d = Doc::parse(xml)?;
    let root = d.root().unwrap();
    if d.name(root) != "Animation" {
        return Err("Animation: found no \"Animation\" node".into());
    }
    if d.attr_raw(root, "typeVersion") != Some("2.0") {
        return Err("Animation typeVersion is unsupported".into());
    }
    let mut out = String::from("<ActuatorList model=\"\">\n");
    for c in d.kids(root, "ActuatorCurve") {
        let actuator = d.attr_or(c, "actuator");
        let (unit, scale) = match d.attr_or(c, "unit").as_str() {
            "degree" => ("0", 1.0),
            "radian" => ("0", 180.0 / std::f64::consts::PI),
            "dimensionless" => ("1", 1.0),
            _ => return Err("ActuatorCurve unit is invalid".into()),
        };
        let ratio = fps / d.attr_num(c, "fps", fps).max(1e-9);
        let mute = if d.attr_raw(c, "mute") == Some("true") { "1" } else { "0" };
        out += &format!("    <ActuatorCurve name=\"value\" actuator=\"{}\" recordable=\"1\" mute=\"{mute}\" unit=\"{unit}\">\n", escape_attr(&actuator));
        let mut last = f64::NEG_INFINITY;
        for k in d.kids(c, "Key") {
            let frame = (d.attr_num(k, "frame", 0.0) * ratio).round();
            if frame <= last {
                return Err("successive Key elements shall have increasing frame attributes".into());
            }
            last = frame;
            let value = fmt_val(d.attr_num(k, "value", 0.0) * scale);
            let flags: String = ["smooth", "symmetrical"].iter().filter(|a| d.attr_raw(k, &format!("editor:{a}")) == Some("true")).map(|a| format!(" {a}=\"1\"")).collect();
            let tangents: Vec<NodeId> = d.kids(k, "Tangent").collect();
            let auto = tangents.iter().all(|&t| d.attr_raw(t, "editor:interpType") == Some("bezier_auto"));
            if auto {
                out += &format!("        <Key frame=\"{frame}\" value=\"{value}\"{flags} />\n");
                continue;
            }
            out += &format!("        <Key frame=\"{frame}\" value=\"{value}\"{flags}>\n");
            for t in tangents {
                let side = d.attr_or(t, "side");
                let (interp, x, y) = match d.attr_raw(t, "editor:interpType") {
                    Some("linear") => ("linear", 0.0, 0.0),
                    _ => ("bezier", d.attr_num(t, "abscissaParam", 0.0) * ratio, d.attr_num(t, "ordinateParam", 0.0) * scale),
                };
                if (side == "left" && x > 0.0) || (side == "right" && x < 0.0) {
                    return Err(format!("Key has {side} Tangent with out of bound abscissa attribute"));
                }
                out += &format!("            <Tangent side=\"{}\" interpType=\"{interp}\" abscissaParam=\"{}\" ordinateParam=\"{}\" />\n", escape_attr(&side), fmt_val(x), fmt_val(y));
            }
            out += "        </Key>\n";
        }
        out += "    </ActuatorCurve>\n";
    }
    out += "</ActuatorList>";
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_negation() {
        assert_eq!(fmt_val(-0.8813214), "-0.881321");
        assert_eq!(fmt_val(-13.0), "-13");
        assert_eq!(fmt_val(123.4567), "123.457");
        assert_eq!(negate("-4.37083"), "4.37083");
        assert_eq!(negate("7"), "-7");
        assert_eq!(negate("0"), "0");
    }

    #[test]
    fn groups_and_mirror_signs() {
        let r = Robot::load("nao6").unwrap();
        assert_eq!(group(&r, "Head").unwrap(), ["HeadYaw", "HeadPitch"]);
        assert_eq!(group(&r, "Arms").unwrap().len(), 12);
        assert_eq!(group(&r, "Whole body").unwrap().len(), 26);
        assert!(mirror_negates(&r, "LShoulderRoll") && mirror_negates(&r, "HeadYaw") && mirror_negates(&r, "RAnkleRoll"));
        assert!(!mirror_negates(&r, "LShoulderPitch") && !mirror_negates(&r, "LHipYawPitch") && !mirror_negates(&r, "LHand"));
    }

    fn key(frame: i32, value: f32, params: (f32, f32)) -> SimplifyKey {
        SimplifyKey { frame, value, left: params, right: params }
    }

    #[test]
    fn python_export_follows_choregraphes_layout() {
        let xml = r#"<Timeline fps="25"><ActuatorList model="">
<ActuatorCurve name="value" actuator="LHand" mute="1" unit="1"><Key frame="1" value="0.5" /></ActuatorCurve>
<ActuatorCurve name="value" actuator="HeadYaw" mute="0" unit="0"><Key frame="1" value="0" /><Key frame="26" value="30" /><Key frame="51" value="10"><Tangent side="left" interpType="bezier" abscissaParam="-5" ordinateParam="2" /><Tangent side="right" interpType="linear" abscissaParam="0" ordinateParam="0" /></Key></ActuatorCurve>
<ActuatorCurve name="value" actuator="HeadPitch" mute="0" unit="0" />
</ActuatorList></Timeline>"#;
        let d = Doc::parse(xml).unwrap();
        let t = d.root().unwrap();
        let simple = to_python(&d, t, false);
        assert_eq!(simple, "# Choregraphe simplified export in Python.\nfrom naoqi import ALProxy\nnames = list()\ntimes = list()\nkeys = list()\n\n\
names.append(\"HeadYaw\")\ntimes.append([0, 1, 2])\nkeys.append([0, 0.523599, 0.174533])\n\n\
\"\"\"\nnames.append(\"LHand\")\ntimes.append([0])\nkeys.append([0.5])\n\"\"\"\n\n\
try:\n  # uncomment the following line and modify the IP if you use this script outside Choregraphe.\n  # motion = ALProxy(\"ALMotion\", IP, 9559)\n  motion = ALProxy(\"ALMotion\")\n  motion.angleInterpolation(names, keys, times, True)\nexcept BaseException, err:\n  print err\n");
        let bezier = to_python(&d, t, true);
        // first key: the left handle reaches towards frame 0 (1/3 frame); a local maximum is flat;
        // stored handles are used as written; a last key's right handle is empty
        assert!(bezier.contains("keys.append([[0, [3, -0.0133333, -0], [3, 0.333333, 0]], [0.523599, [3, -0.333333, -0], [3, 0.333333, 0]], [0.174533, [2, -0.2, 0.0349066], [1, 0, 0]]])"), "{bezier}");
        assert!(bezier.contains("  motion.angleInterpolationBezier(names, times, keys)\n"));
    }

    #[test]
    fn g6_matches_qstring_number() {
        for (v, s) in [(0.96, "0.96"), (1.0, "1"), (-0.0872665, "-0.0872665"), (123456.0, "123456"), (1234567.0, "1.23457e+06"), (0.00001, "1e-05"), (0.000123, "0.000123"), (2.08566, "2.08566")] {
            assert_eq!(fmt_g6(v), s);
        }
    }

    #[test]
    fn rdp_simplify_keeps_corners_and_ends() {
        let pts = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.05), (3.0, 3.0), (4.0, 0.0)];
        assert_eq!(rdp_simplify(&pts, 0.1), [true, false, false, true, true]);
        assert_eq!(rdp_simplify(&pts, 0.03), [true, false, true, true, true]);
        assert_eq!(rdp_simplify(&pts, 0.01), [true; 5]);
        assert_eq!(rdp_simplify(&pts[..2], 5.0), [true, true]);
    }

    #[test]
    fn simplify_drops_keys_the_curve_does_not_need() {
        // flat: every interior key goes; the ends take a third of the way at slope 0
        let flat: Vec<_> = (0..4).map(|i| key(i * 10, 5.0, (THIRD, 0.0))).collect();
        let (keep, params) = simplify(&flat, 0.5, 25);
        assert_eq!(keep, [true, false, false, true]);
        assert_eq!(params, [Some((THIRD, 0.0)), None, None, Some((THIRD, 0.0))]);
        // a line drawn with slope-1 handles stays a line; a zero margin keeps everything
        let line: Vec<_> = (0..4).map(|i| key(i * 10, (i * 10) as f32, (THIRD, 1.0))).collect();
        assert_eq!(simplify(&line, 0.5, 25).0, [true, false, false, true]);
        assert_eq!(simplify(&line, 0.0, 25).0, [true; 4]);
        assert_eq!(simplify(&line[..2], 100.0, 25), (vec![true; 2], vec![None; 2]));
    }

    #[test]
    fn simplify_scores_the_mean_error() {
        // a peak with automatic tangents (flat at all three keys): without it the curve is flat at
        // 0, so the score is the mean of the original samples, 10·smoothstep at x = 0.5, 1, … 20,
        // which sums to 200 over 40 samples
        let peak = [key(0, 0.0, (THIRD, 0.0)), key(10, 10.0, (THIRD, 0.0)), key(20, 0.0, (THIRD, 0.0))];
        let original = BTreeMap::from([(0, segment(&peak[0], peak[0].right, &peak[1], peak[1].left, 25)), (10, segment(&peak[1], peak[1].right, &peak[2], peak[2].left, 25))]);
        let score = compare_curves(&original, 0, 20, &segment(&peak[0], peak[0].right, &peak[2], peak[2].left, 25));
        assert!((score - 5.0).abs() < 1e-5, "{score}");
        assert_eq!(simplify(&peak, 4.99, 25).0, [true; 3]);
        assert_eq!(simplify(&peak, 5.01, 25).0, [true, false, true]);
    }

    #[test]
    fn simplify_samples_every_20_ms() {
        let (a, b) = (key(10, 0.0, (THIRD, 0.0)), key(20, 1.0, (THIRD, 0.0)));
        // 25 fps: every half frame, the last sample on the key
        let s = segment(&a, a.right, &b, b.left, 25);
        assert_eq!((s.len(), s[9], s[19]), (20, 0.5, 1.0));
        // 30 fps: 0.6 frames from 10.2 to 19.8, the ends snapped to that grid
        assert_eq!(segment(&a, a.right, &b, b.left, 30).len(), 16);
        // a sign change counts the trapezoid area; a count mismatch is f32::MAX
        let original = BTreeMap::from([(0, vec![1.0, -1.0])]);
        assert_eq!(compare_curves(&original, 0, 10, &[0.0, 0.0]), 0.5);
        assert_eq!(compare_curves(&original, 0, 10, &[0.0]), f32::MAX);
    }

    #[test]
    fn advanced_simplify_wave() {
        // this wave with automatic tangents keeps 10, 50 and 70 with these params: one segment
        // per stretch, its handles a step or two off where they started (0.553333 = 1/3 + 1.1/5)
        let points = [(10, 0.0), (20, 8.0), (30, 10.0), (40, 6.0), (50, 0.0), (60, -4.0), (70, -3.0)];
        let slope = |i: usize| if i > 0 && i + 1 < points.len() { auto_slope(points[i - 1], points[i], points[i + 1]) } else { 0.0 };
        let wave: Vec<_> = (0..points.len()).map(|i| key(points[i].0, points[i].1, (THIRD, slope(i)))).collect();
        let out: Vec<_> = advanced_simplify(&wave, 0.5, 10.0, 5.0, 25).into_iter().flatten().map(|k| (k.frame, [k.left.0, k.left.1, k.right.0, k.right.1].map(f32::to_bits))).collect();
        // 0.32, 0.773333, -0.66, 0.553333, -0.5
        assert_eq!(out, [(10, [0x3eaaaaab, 0, 0x3eaaaaab, 0x3ea3d70a]), (50, [0x3f45f92c, 0xbf28f5c2, 0x3f0da741, 0xbf000000]), (70, [0x3eaaaaab, 0, 0x3eaaaaab, 0])]);
        // the first score looks at the segments from frame 0 to 2: a flat curve from frame 0 fits
        // at once and keeps its params, the same curve from frame 10 gets searched
        let flat = |from: i32| (0..4).map(|i| key(from + i * 10, 5.0, (THIRD, 0.0))).collect::<Vec<_>>();
        assert_eq!(advanced_simplify(&flat(0), 0.5, 10.0, 5.0, 25)[0].unwrap().right, (THIRD, 0.0));
        assert_eq!(advanced_simplify(&flat(10), 0.5, 10.0, 5.0, 25)[0].unwrap().right.0.to_bits(), 0x3f0da741);
    }

    #[test]
    fn auto_tangents_match_update_bezier_auto_tangents() {
        // local extremum: flat
        let (l, r) = auto_tangents(Some((0.0, 0.0)), (3.0, 10.0), Some((6.0, 0.0)));
        assert_eq!((l, r), ((-1.0, 0.0), (1.0, 0.0)));
        // straight line: the secant slope
        let (l, r) = auto_tangents(Some((0.0, 0.0)), (3.0, 3.0), Some((6.0, 6.0)));
        assert_eq!((l, r), ((-1.0, -1.0), (1.0, 1.0)));
        // the secant 10/12 would overshoot the next value (reach 9/3 * 10/12 = 2.5 > 1): scaled to 1/3
        let (l, r) = auto_tangents(Some((0.0, 0.0)), (3.0, 9.0), Some((12.0, 10.0)));
        assert_eq!((l.0, r.0), (-1.0, 3.0));
        assert!((r.1 - 1.0).abs() < 1e-6 && (l.1 + 1.0 / 3.0).abs() < 1e-6, "{l:?} {r:?}");
        // end keys: the first key's left handle reaches towards frame 0, the last key's right is empty
        assert_eq!(auto_tangents(None, (6.0, 5.0), Some((9.0, 0.0))), ((-2.0, 0.0), (1.0, 0.0)));
        assert_eq!(auto_tangents(Some((3.0, 0.0)), (6.0, 5.0), None), ((-1.0, 0.0), (0.0, 0.0)));
        assert_eq!(auto_tangents(None, (0.0, 5.0), None), ((0.0, 0.0), (0.0, 0.0)));
    }
}
