//! Kinematic simulator, like Choregraphe's virtual robot (NAOqi's ALMotion with LocalSimulator in
//! place of the DCM): every motion cycle each running task computes one command per joint, and the
//! sensors take the commanded values. There is no physics.
//!
//! The interpolators reproduce ALMotion's observed behavior: bang-bang acceleration for speed-fraction moves, a
//! clamped cubic spline for timed keys and setAngles, and Bezier keys sampled each cycle under the
//! joint's speed and range for timelines and postures. A physics backend can replace `run_cycle`
//! later without changing the services.

use choren_robot::Robot;
use std::collections::BTreeMap;
use std::sync::Mutex;
use tokio::sync::{oneshot, watch};

/// ALMotion's joint acceleration limit for speed-fraction moves (rad/s², `jointMaxAcceleration` of
/// its control configuration).
pub const JOINT_MAX_ACCELERATION: f64 = 10.0;

/// A key at time `t` (s). Tangents are Bezier handle offsets `(dt, dv)` from the key.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub t: f64,
    pub v: f64,
    pub left: Option<(f64, f64)>,
    pub right: Option<(f64, f64)>,
    /// How the segment from this key to the next one is interpolated.
    pub after: Segment,
}

/// Timeline interpolation of a segment: Bezier (explicit or automatic handles), Linear, Constant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Segment {
    #[default]
    Bezier,
    Linear,
    Constant,
}

impl Key {
    pub fn new(t: f64, v: f64) -> Key {
        Key { t, v, left: None, right: None, after: Segment::Bezier }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Curve {
    pub keys: Vec<Key>,
}

impl Curve {
    pub fn end(&self) -> f64 {
        self.keys.last().map_or(0.0, |k| k.t)
    }

    /// Automatic slope at key `i` (NAOqi's auto tangents): zero at the ends and at extrema,
    /// otherwise the slope between the neighbours, reduced so that neither handle passes the
    /// neighbouring key's value. The curve never overshoots its keys.
    fn auto_slope(&self, i: usize) -> f64 {
        let k = &self.keys;
        if i == 0 || i + 1 >= k.len() {
            return 0.0;
        }
        let (a, b, c) = (&k[i - 1], &k[i], &k[i + 1]);
        let (dtp, dtn, dvp, dvn) = (b.t - a.t, c.t - b.t, b.v - a.v, c.v - b.v);
        if dvp * dvn <= 0.0 {
            return 0.0;
        }
        let mut s = (dvp + dvn) / (dtp + dtn);
        for (dt, dv) in [(dtn, dvn), (dtp, dvp)] {
            let reach = (s * dt / 3.0).abs();
            if reach > dv.abs() {
                s *= dv.abs() / reach;
            }
        }
        s
    }

    pub fn sample(&self, t: f64) -> f64 {
        let k = &self.keys;
        let Some(first) = k.first() else { return 0.0 };
        if t <= first.t {
            return first.v;
        }
        let i = match k.iter().position(|x| x.t >= t) {
            Some(i) => i,
            None => return k.last().unwrap().v,
        };
        let (a, b) = (&k[i - 1], &k[i]);
        let span = (b.t - a.t).max(1e-9);
        match a.after {
            _ if t >= b.t => return b.v,
            Segment::Constant => return a.v,
            Segment::Linear => return a.v + (b.v - a.v) * (t - a.t) / span,
            Segment::Bezier => {}
        }
        // A handle longer than its segment is shortened to the segment, keeping its slope.
        let fit = |(dt, dv): (f64, f64)| if dt.abs() > span { (dt.signum() * span, dv * span / dt.abs()) } else { (dt, dv) };
        let (r, l) = (
            fit(a.right.unwrap_or_else(|| (span / 3.0, self.auto_slope(i - 1) * span / 3.0))),
            fit(b.left.unwrap_or_else(|| (-span / 3.0, -self.auto_slope(i) * span / 3.0))),
        );
        let (x0, x1, x2, x3) = (a.t, a.t + r.0, b.t + l.0, b.t);
        let (y0, y1, y2, y3) = (a.v, a.v + r.1, b.v + l.1, b.v);
        let bez = |p0: f64, p1: f64, p2: f64, p3: f64, s: f64| {
            let u = 1.0 - s;
            u * u * u * p0 + 3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s * p3
        };
        // Solve x(s) = t; handles stay within the segment, so x is monotonic.
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if bez(x0, x1.clamp(x0, x3), x2.clamp(x0, x3), x3, mid) < t { lo = mid } else { hi = mid }
        }
        bez(y0, y1, y2, y3, (lo + hi) / 2.0)
    }
}

/// ALMotion's duration rounding: the nearest whole number of cycles, at least one.
pub fn round_to_cycle(t: f64, dt: f64) -> f64 {
    let r = (t / dt + 0.5).floor() * dt;
    if r < 1e-5 { dt } else { r }
}

/// ALMotion's time for a cubic move from `p0` at velocity `v0` to `p1` at `v1` whose peak velocity
/// is `vmax`, rounded to the cycle. From rest it is 1.5·|p1 − p0| / vmax.
pub fn cubic_time(p0: f64, p1: f64, v0: f64, v1: f64, vmax: f64, dt: f64) -> f64 {
    let (a, b, d) = (v0.clamp(-vmax, vmax), v1.clamp(-vmax, vmax), p1 - p0);
    let near_zero = |x: f64| x.abs() < 2e-5;
    // Sign of the peak velocity of the normalised cubic through both ends.
    let (c2, c3) = (3.0 * d - 2.0 * a - b, -2.0 * d + a + b);
    let s = if near_zero(c3) { 1.0 } else { -c2 / (3.0 * c3) };
    let peak = 3.0 * c3 * s * s + 2.0 * c2 * s + a;
    let v = if peak < 0.0 { -vmax } else { vmax };
    let mut t = 3.0 * d / (2.0 * v);
    if !(near_zero(a) && near_zero(b)) {
        let q = v * v - (a + b) * v + a * b;
        if q > 0.0 {
            let r = if peak > 0.0 { a + b + v - q.sqrt() } else { a + b + v + q.sqrt() };
            t = 3.0 * d * r / (a * a + 3.0 * (a + b) * v + b * b + a * b);
        }
    }
    let t = t.abs();
    round_to_cycle(if t > 1000.0 { 20.0 } else { t }, dt)
}

/// ALMotion's bang-bang acceleration interpolator: the fastest move from `(p0, v0)` to a stop at
/// the target with |acceleration| ≤ `a` and |velocity| ≤ `vmax` (trapezoidal or triangular
/// velocity, overshooting and coming back when the joint cannot stop in time).
#[derive(Clone, Debug)]
pub struct BangBang {
    p0: f64,
    v0: f64,
    target: f64,
    a: f64,
    /// Main direction (±1) and the (duration, acceleration) phases.
    dir: f64,
    phases: [(f64, f64); 3],
}

impl BangBang {
    pub fn new(p0: f64, v0: f64, target: f64, vmax: f64, a: f64) -> BangBang {
        let v0 = v0.clamp(-vmax, vmax);
        let stop = v0 * v0.abs() / (2.0 * a);
        let dir = if target - p0 - stop >= 0.0 { 1.0 } else { -1.0 };
        let (d, v) = ((target - p0) * dir, v0 * dir);
        let peak = (a * d + v * v / 2.0).max(0.0).sqrt().min(vmax);
        let mut b = BangBang { p0, v0, target, a, dir, phases: [(0.0, 0.0); 3] };
        b.set_peak(peak);
        b
    }

    /// Phases reaching `peak` (in the main direction) and stopping at the target.
    fn set_peak(&mut self, peak: f64) {
        let (d, v, a, s) = ((self.target - self.p0) * self.dir, self.v0 * self.dir, self.a, self.dir);
        let t1 = (peak - v).abs() / a;
        let t3 = peak / a;
        let rest = d - (peak + v) / 2.0 * t1 - peak * t3 / 2.0;
        let cruise = if peak > 1e-12 { (rest / peak).max(0.0) } else { 0.0 };
        self.phases = [(t1, s * a * (peak - v).signum()), (cruise, 0.0), (t3, -s * a)];
    }

    pub fn duration(&self) -> f64 {
        self.phases.iter().map(|p| p.0).sum()
    }

    /// Stretches the move to `t` seconds (to end with slower joints). From rest the profile is
    /// scaled in time; when moving, the peak velocity is lowered until the move takes `t`.
    pub fn stretch(&mut self, t: f64) {
        let d = self.duration();
        if t <= d + 1e-12 || d <= 0.0 {
            return;
        }
        if self.v0.abs() < 1e-9 {
            let k = t / d;
            for p in &mut self.phases {
                p.0 *= k;
                p.1 /= k * k;
            }
            return;
        }
        let (mut lo, mut hi) = (0.0, self.phases[2].0 * self.a);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            self.set_peak(mid);
            if self.duration() > t { lo = mid } else { hi = mid }
        }
        self.set_peak(hi);
    }

    /// Position and velocity `t` seconds after the start.
    pub fn sample(&self, t: f64) -> (f64, f64) {
        let (mut p, mut v, mut left) = (self.p0, self.v0, t.max(0.0));
        for &(dur, acc) in &self.phases {
            let h = left.min(dur);
            p += v * h + 0.5 * acc * h * h;
            v += acc * h;
            left -= h;
            if left <= 0.0 {
                return (p, v);
            }
        }
        (self.target, 0.0)
    }
}

/// ALMotion's articular interpolator: a clamped cubic spline through `(time, angle)` keys with
/// given start and end velocities. Through two keys it is the cubic Hermite segment.
#[derive(Clone, Debug)]
pub struct Spline {
    t: Vec<f64>,
    p: Vec<f64>,
    /// Second derivative at each key.
    m: Vec<f64>,
}

impl Spline {
    pub fn new(t: Vec<f64>, p: Vec<f64>, v0: f64, v1: f64) -> Spline {
        let n = t.len() - 1;
        let h: Vec<f64> = (0..n).map(|i| (t[i + 1] - t[i]).max(1e-9)).collect();
        let s: Vec<f64> = (0..n).map(|i| (p[i + 1] - p[i]) / h[i]).collect();
        // Tridiagonal system for the second derivatives with the end slopes imposed.
        let (mut sub, mut diag, mut sup, mut rhs) = (vec![0.0; n + 1], vec![0.0; n + 1], vec![0.0; n + 1], vec![0.0; n + 1]);
        diag[0] = 2.0 * h[0];
        sup[0] = h[0];
        rhs[0] = 6.0 * (s[0] - v0);
        for i in 1..n {
            sub[i] = h[i - 1];
            diag[i] = 2.0 * (h[i - 1] + h[i]);
            sup[i] = h[i];
            rhs[i] = 6.0 * (s[i] - s[i - 1]);
        }
        sub[n] = h[n - 1];
        diag[n] = 2.0 * h[n - 1];
        rhs[n] = 6.0 * (v1 - s[n - 1]);
        for i in 1..=n {
            let w = sub[i] / diag[i - 1];
            diag[i] -= w * sup[i - 1];
            rhs[i] -= w * rhs[i - 1];
        }
        let mut m = vec![0.0; n + 1];
        m[n] = rhs[n] / diag[n];
        for i in (0..n).rev() {
            m[i] = (rhs[i] - sup[i] * m[i + 1]) / diag[i];
        }
        Spline { t, p, m }
    }

    /// A Hermite segment from `(p0, v0)` to a stop at `p1` in `duration` seconds.
    pub fn hermite(p0: f64, v0: f64, p1: f64, duration: f64) -> Spline {
        Spline::new(vec![0.0, duration], vec![p0, p1], v0, 0.0)
    }

    pub fn duration(&self) -> f64 {
        *self.t.last().unwrap()
    }

    /// Position and velocity at time `t`.
    pub fn sample(&self, t: f64) -> (f64, f64) {
        let n = self.t.len() - 1;
        if t >= self.t[n] {
            return (self.p[n], 0.0);
        }
        let i = self.t[1..n].iter().take_while(|&&k| k <= t).count();
        let (h, x) = (self.t[i + 1] - self.t[i], (t - self.t[i]).max(0.0));
        let (m0, m1) = (self.m[i], self.m[i + 1]);
        let b = (self.p[i + 1] - self.p[i]) / h - h * (2.0 * m0 + m1) / 6.0;
        let c3 = (m1 - m0) / (6.0 * h);
        (self.p[i] + b * x + m0 / 2.0 * x * x + c3 * x * x * x, b + m0 * x + 3.0 * c3 * x * x)
    }
}

/// Bezier keys sampled every cycle, each sample moved at most `step` from the previous command and
/// kept within the joint's range (ALMotion's speed-saturated Bezier interpolation).
#[derive(Clone, Debug)]
pub struct Saturated {
    pub curve: Curve,
    prev: f64,
    step: f64,
    range: (f64, f64),
}

impl Saturated {
    fn next(&mut self, t: f64) -> f64 {
        let x = self.curve.sample(t);
        self.prev = x.clamp((self.prev - self.step).max(self.range.0), (self.prev + self.step).min(self.range.1));
        self.prev
    }
}

#[derive(Clone, Debug)]
pub enum Profile {
    Ramp(BangBang),
    Spline(Spline),
    Bezier(Saturated),
}

/// One joint's commands within a task.
#[derive(Clone, Debug)]
pub struct Track {
    pub profile: Profile,
    /// Cycles before the profile starts (stiffness ramps); the joint holds still meanwhile.
    pub delay: u32,
    /// The profile is sampled this many cycles ahead (angleInterpolation runs one cycle ahead).
    pub lead: u32,
    /// Cycle (after the delay) that commands the target.
    pub cycles: u32,
    pub target: f64,
}

impl Track {
    pub fn new(profile: Profile, cycles: u32, target: f64) -> Track {
        Track { profile, delay: 0, lead: 0, cycles: cycles.max(1), target }
    }

    /// Command and velocity at cycle `k` of the task (1 = the first cycle), `None` while waiting.
    fn at(&mut self, k: u32, dt: f64) -> Option<(f64, f64)> {
        let k = k.checked_sub(self.delay).filter(|&k| k > 0)?;
        let t = (k + self.lead) as f64 * dt;
        Some(match &mut self.profile {
            // The last cycle commands the target exactly: a task that succeeds has reached it.
            Profile::Ramp(_) | Profile::Spline(_) if k >= self.cycles => (self.target, 0.0),
            Profile::Ramp(b) => b.sample(t),
            Profile::Spline(s) => s.sample(t),
            Profile::Bezier(s) => {
                let before = s.prev;
                let x = s.next(t.min(s.curve.end()));
                (x, (x - before) / dt)
            }
        })
    }

    fn done(&self, k: u32) -> bool {
        k >= self.delay + self.cycles && match &self.profile {
            // A saturated Bezier keeps moving at full speed until it gets to its last key.
            Profile::Bezier(s) => (s.prev - self.target).abs() < 1e-9,
            _ => true,
        }
    }
}

pub struct Task {
    pub id: u32,
    /// Motion cycle at which the task was started.
    pub start: u64,
    pub joints: Vec<(usize, Track)>,
    pub torso: Option<Track>,
    pub done: Option<oneshot::Sender<bool>>,
}

pub struct MoveCmd {
    /// Velocity in the robot frame (m/s, m/s, rad/s) or a relative target to reach.
    pub velocity: (f64, f64, f64),
    pub target: Option<(f64, f64, f64)>,
    pub done: Option<oneshot::Sender<bool>>,
}

pub struct State {
    pub time: f64,
    pub motion_speed: f64,
    /// Motion cycles run so far, and the time not yet spent on a cycle.
    pub cycle: u64,
    pending: f64,
    pub angles: Vec<f64>,
    /// Commanded joint velocities (rad/s), where the next move starts from.
    pub velocity: Vec<f64>,
    pub stiffness: Vec<f64>,
    pub torso_pitch: f64,
    /// World position of the robot (x, y, theta).
    pub pose: (f64, f64, f64),
    pub moving: Option<MoveCmd>,
    pub tasks: Vec<Task>,
    pub next_task: u32,
    /// Intensity (0–1) of each LED actuator (`Robot::led_actuator_names`).
    pub leds: BTreeMap<String, f32>,
}

pub struct Sim {
    pub robot: Robot,
    pub state: Mutex<State>,
    clock: watch::Sender<f64>,
}

impl Sim {
    /// Starts awake in the pose Choregraphe's virtual robot starts in (Stand), so behaviors run without a wake-up step.
    pub fn new(robot: Robot) -> Sim {
        let init = if robot.initial_angles.len() == robot.joints.len() {
            robot.initial_angles.clone()
        } else {
            robot.posture("StandInit").map(|p| p.angles.clone()).unwrap_or_else(|| vec![0.0; robot.joints.len()])
        };
        let n = robot.joints.len();
        let leds = robot.led_actuator_names().into_iter().map(|a| (a, 1.0)).collect();
        let state = State { time: 0.0, motion_speed: 1.0, cycle: 0, pending: 0.0, angles: init, velocity: vec![0.0; n], stiffness: vec![1.0; n], torso_pitch: 0.0, pose: (0.0, 0.0, 0.0), moving: None, tasks: vec![], next_task: 1, leds };
        Sim { robot, state: Mutex::new(state), clock: watch::channel(0.0).0 }
    }

    pub fn set_motion_speed(&self, speed: f64) -> Result<(), String> {
        if !speed.is_finite() || !(0.25..=2.0).contains(&speed) {
            return Err("Movement speed must be between 0.25 and 2.".into());
        }
        self.state.lock().unwrap().motion_speed = speed;
        Ok(())
    }

    /// Animation frames and joint trajectories share one clock, including live speed changes.
    pub fn motion_clock(&self) -> watch::Receiver<f64> {
        self.clock.subscribe()
    }

    /// The motion cycle (s).
    pub fn dt(&self) -> f64 {
        self.robot.control_period_ms.max(1) as f64 / 1000.0
    }

    /// Starts a task. Older tasks that use any of the same joints are cancelled.
    pub fn start_tracks(&self, joints: Vec<(usize, Track)>, torso: Option<Track>) -> (u32, oneshot::Receiver<bool>) {
        let (tx, rx) = oneshot::channel();
        let mut s = self.state.lock().unwrap();
        let used: Vec<usize> = joints.iter().map(|j| j.0).collect();
        let takes_torso = torso.is_some();
        let (keep, cancelled): (Vec<Task>, Vec<Task>) = std::mem::take(&mut s.tasks).into_iter().partition(|t| !(t.joints.iter().any(|(j, _)| used.contains(j)) || (takes_torso && t.torso.is_some())));
        s.tasks = keep;
        for mut t in cancelled {
            if let Some(d) = t.done.take() {
                let _ = d.send(false);
            }
        }
        let id = s.next_task;
        s.next_task += 1;
        let start = s.cycle;
        s.tasks.push(Task { id, start, joints, torso, done: Some(tx) });
        (id, rx)
    }

    /// Plays Bezier curves (angleInterpolationBezier, timelines, postures): key times fall on
    /// motion cycles, and each joint follows its curve at most at its maximum velocity.
    pub fn start_task(&self, joints: Vec<(usize, Curve)>, torso: Option<Curve>) -> (u32, oneshot::Receiver<bool>) {
        let dt = self.dt();
        let (angles, pitch) = { let s = self.state.lock().unwrap(); (s.angles.clone(), s.torso_pitch) };
        let track = |mut curve: Curve, from: f64, step: f64, range: (f64, f64)| {
            for k in &mut curve.keys {
                k.t = (k.t / dt).round() * dt;
            }
            let cycles = (curve.end() / dt).round() as u32;
            let target = curve.keys.last().map_or(from, |k| k.v).clamp(range.0, range.1);
            Track::new(Profile::Bezier(Saturated { curve, prev: from, step, range }), cycles, target)
        };
        let tracks = joints.into_iter().map(|(j, c)| {
            let jt = &self.robot.joints[j];
            (j, track(c, angles[j], jt.max_velocity * dt, (jt.min, jt.max)))
        });
        let torso = torso.map(|c| track(c, pitch, f64::INFINITY, (f64::MIN, f64::MAX)));
        self.start_tracks(tracks.collect(), torso)
    }

    /// angleInterpolationWithSpeed: bang-bang moves at `fraction` of each joint's maximum velocity,
    /// stretched so that all joints end together. `until_stopped` runs the last partial cycle
    /// (rest) instead of rounding to the nearest cycle.
    pub fn move_with_speed(&self, joints: &[usize], targets: &[f64], fraction: f64, until_stopped: bool) -> (u32, oneshot::Receiver<bool>) {
        let dt = self.dt();
        let (angles, vel) = { let s = self.state.lock().unwrap(); (s.angles.clone(), s.velocity.clone()) };
        let fraction = fraction.clamp(1e-3, 1.0);
        let mut moves: Vec<(usize, BangBang, f64)> = joints.iter().zip(targets).map(|(&j, &v)| {
            let target = self.robot.clamp(j, v);
            (j, BangBang::new(angles[j], vel[j], target, self.robot.joints[j].max_velocity * fraction, JOINT_MAX_ACCELERATION), target)
        }).collect();
        let t = moves.iter().map(|m| m.1.duration()).fold(0.0, f64::max);
        for m in &mut moves {
            m.1.stretch(t);
        }
        let cycles = if until_stopped { (t / dt - 1e-9).ceil() } else { (t / dt).round() } as u32;
        self.start_tracks(moves.into_iter().map(|(j, b, target)| (j, Track::new(Profile::Ramp(b), cycles, target))).collect(), None)
    }

    /// setAngles/changeAngles: each joint independently follows a cubic from its commanded angle
    /// and velocity to a stop at the target, its peak velocity `fraction` of the maximum.
    pub fn set_angles(&self, joints: &[usize], targets: &[f64], fraction: f64) -> (u32, oneshot::Receiver<bool>) {
        let dt = self.dt();
        let (angles, vel) = { let s = self.state.lock().unwrap(); (s.angles.clone(), s.velocity.clone()) };
        let fraction = fraction.clamp(1e-3, 1.0);
        let tracks = joints.iter().zip(targets).map(|(&j, &v)| {
            let target = self.robot.clamp(j, v);
            let t = cubic_time(angles[j], target, vel[j], 0.0, self.robot.joints[j].max_velocity * fraction, dt);
            (j, Track::new(Profile::Spline(Spline::hermite(angles[j], vel[j], target, t)), (t / dt).round() as u32, target))
        });
        self.start_tracks(tracks.collect(), None)
    }

    /// angleInterpolation: per joint, a clamped cubic spline through `(time, angle)` keys from the
    /// commanded angle and velocity, ending at rest. The last time is rounded to the cycle and the
    /// spline is sampled one cycle ahead, as ALMotion does.
    pub fn interpolate(&self, joints: &[usize], keys: &[Vec<(f64, f64)>]) -> Result<(u32, oneshot::Receiver<bool>), String> {
        let dt = self.dt();
        let (angles, vel) = { let s = self.state.lock().unwrap(); (s.angles.clone(), s.velocity.clone()) };
        let mut tracks = vec![];
        for (&j, keys) in joints.iter().zip(keys) {
            let (mut t, mut p) = (vec![0.0], vec![angles[j]]);
            for &(time, v) in keys {
                if time <= *t.last().unwrap() {
                    return Err("ALMotion: angleInterpolation: times must be strictly increasing and positive".into());
                }
                t.push(time);
                p.push(self.robot.clamp(j, v));
            }
            let end = round_to_cycle(*t.last().unwrap(), dt).max(t[t.len() - 2] + dt);
            *t.last_mut().unwrap() = end;
            let target = *p.last().unwrap();
            let mut track = Track::new(Profile::Spline(Spline::new(t, p, vel[j], 0.0)), ((end / dt).round() as u32).saturating_sub(1), target);
            track.lead = 1;
            tracks.push((j, track));
        }
        Ok(self.start_tracks(tracks, None))
    }

    /// goToStateWithMaxSpeed (wakeUp): cubics from rest to `targets`, all lasting as long as the
    /// slowest joint needs at `fraction` of its maximum velocity, after `delay` cycles. The torso
    /// pitch follows to `torso`.
    pub fn go_to_state(&self, joints: &[usize], targets: &[f64], fraction: f64, delay: u32, torso: Option<f64>) -> (u32, oneshot::Receiver<bool>) {
        let dt = self.dt();
        let (angles, pitch) = { let s = self.state.lock().unwrap(); (s.angles.clone(), s.torso_pitch) };
        let targets: Vec<f64> = joints.iter().zip(targets).map(|(&j, &v)| self.robot.clamp(j, v)).collect();
        let t = joints.iter().zip(&targets).map(|(&j, &v)| cubic_time(angles[j], v, 0.0, 0.0, self.robot.joints[j].max_velocity * fraction, dt)).fold(dt, f64::max);
        let cycles = (t / dt).round() as u32;
        let track = |from: f64, to: f64| Track { delay, ..Track::new(Profile::Spline(Spline::hermite(from, 0.0, to, t)), cycles, to) };
        let tracks = joints.iter().zip(&targets).map(|(&j, &v)| (j, track(angles[j], v))).collect();
        self.start_tracks(tracks, torso.map(|to| track(pitch, to)))
    }

    /// openHand/closeHand: a 0.1 s stiffness ramp and two set-up cycles, then a 1.2 s cubic.
    pub fn move_hand(&self, joint: usize, target: f64) -> (u32, oneshot::Receiver<bool>) {
        let dt = self.dt();
        let from = self.state.lock().unwrap().angles[joint];
        let target = self.robot.clamp(joint, target);
        let track = Track { delay: (round_to_cycle(0.1, dt) / dt).round() as u32 + 2, ..Track::new(Profile::Spline(Spline::hermite(from, 0.0, target, 1.2)), (1.2 / dt).round() as u32, target) };
        self.start_tracks(vec![(joint, track)], None)
    }

    /// Cancels one trajectory task (its waiter gets `false`); other tasks keep running.
    pub fn kill_task(&self, id: u32) {
        let mut s = self.state.lock().unwrap();
        if let Some(i) = s.tasks.iter().position(|t| t.id == id) {
            if let Some(d) = s.tasks.remove(i).done.take() {
                let _ = d.send(false);
            }
        }
    }

    pub fn kill_all(&self) {
        let mut s = self.state.lock().unwrap();
        for mut t in std::mem::take(&mut s.tasks) {
            if let Some(d) = t.done.take() {
                let _ = d.send(false);
            }
        }
        if let Some(mut m) = s.moving.take() {
            if let Some(d) = m.done.take() {
                let _ = d.send(false);
            }
        }
    }

    /// Advances the simulation by `dt` seconds: the motion cycles that fall in it, then walking.
    pub fn step(&self, dt: f64) {
        let cycle = self.dt();
        let mut guard = self.state.lock().unwrap();
        let s = &mut *guard;
        let dt = dt * s.motion_speed;
        s.time += dt;
        s.pending += dt;
        while s.pending >= cycle - 1e-9 {
            s.pending -= cycle;
            self.run_cycle(s, cycle);
        }
        if let Some(m) = &mut s.moving {
            let w = &self.robot.walk;
            let (mut vx, mut vy, mut vt) = m.velocity;
            if let Some((tx, ty, tt)) = &mut m.target {
                // Head for the remaining relative target at the walk limits.
                let (dx, dy, dth) = (*tx, *ty, *tt);
                let dist = (dx * dx + dy * dy).sqrt();
                let time = (dist / w.max_x).max(dth.abs() / w.max_theta).max(dt);
                vx = dx / time;
                vy = dy / time;
                vt = dth / time;
                let k = (dt / time).min(1.0);
                *tx -= dx * k;
                *ty -= dy * k;
                *tt -= dth * k;
            } else {
                vx = vx.clamp(-w.max_x, w.max_x);
                vy = vy.clamp(-w.max_y, w.max_y);
                vt = vt.clamp(-w.max_theta, w.max_theta);
            }
            let (x, y, th) = s.pose;
            s.pose = (x + (vx * th.cos() - vy * th.sin()) * dt, y + (vx * th.sin() + vy * th.cos()) * dt, th + vt * dt);
            if m.target.is_some_and(|(a, b, c)| a.abs() + b.abs() + c.abs() < 1e-4) {
                if let Some(d) = s.moving.take().and_then(|mut m| m.done.take()) {
                    let _ = d.send(true);
                }
            }
        }
        self.clock.send_replace(s.time);
    }

    /// One motion cycle: every task commands its joints, and the sensors take the commands.
    fn run_cycle(&self, s: &mut State, dt: f64) {
        s.cycle += 1;
        let n = s.angles.len();
        let mut command: Vec<Option<(f64, f64)>> = vec![None; n];
        let mut finished = vec![];
        for (ti, task) in s.tasks.iter_mut().enumerate() {
            let k = (s.cycle - task.start) as u32;
            for (j, track) in &mut task.joints {
                if let Some(c) = track.at(k, dt) {
                    command[*j] = Some(c);
                }
            }
            if let Some(c) = task.torso.as_mut().and_then(|t| t.at(k, dt)) {
                s.torso_pitch = c.0;
            }
            if task.joints.iter().all(|(_, t)| t.done(k)) && task.torso.as_ref().is_none_or(|t| t.done(k)) {
                finished.push(ti);
            }
        }
        for ti in finished.into_iter().rev() {
            let mut task = s.tasks.remove(ti);
            if let Some(d) = task.done.take() {
                let _ = d.send(true);
            }
        }
        for j in 0..n {
            match command[j] {
                // A relaxed joint does not follow commands.
                Some((p, v)) if s.stiffness[j] > 0.0 => {
                    s.angles[j] = self.robot.clamp(j, p);
                    s.velocity[j] = v;
                }
                _ => s.velocity[j] = 0.0,
            }
        }
        for (j, joint) in self.robot.joints.iter().enumerate() {
            if let Some(i) = joint.coupled_to.as_deref().and_then(|m| self.robot.joint_index(m)) {
                s.angles[j] = s.angles[i];
            }
        }
        for limit in &self.robot.coupled_limits {
            if let (Some(j), Some(by)) = (self.robot.joint_index(&limit.joint), self.robot.joint_index(&limit.by)) {
                let (lo, hi) = Robot::coupled_range(limit, s.angles[by]);
                s.angles[j] = s.angles[j].clamp(lo, hi);
            }
        }
    }

    pub fn angles(&self) -> Vec<f64> {
        self.state.lock().unwrap().angles.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_speed_scales_motion_walking_and_the_animation_clock() {
        let sim = Sim::new(Robot::load("nao6").unwrap());
        // Motion runs in whole cycles (24 ms), so the gesture lasts 40 cycles.
        let c = sim.dt();
        let yaw = sim.robot.joint_index("HeadYaw").unwrap();
        let (_, mut done) = sim.start_task(vec![(yaw, Curve { keys: vec![
            Key { after: Segment::Linear, ..Key::new(0.0, 0.0) }, Key::new(40.0 * c, 1.0),
        ] })], None);
        sim.state.lock().unwrap().moving = Some(MoveCmd { velocity: (0.1, 0.0, 0.0), target: None, done: None });
        let clock = sim.motion_clock();
        sim.set_motion_speed(0.5).unwrap();
        for _ in 0..20 { sim.step(c); }
        assert!((sim.angles()[yaw] - 0.25).abs() < 1e-6, "{}", sim.angles()[yaw]);
        assert!((*clock.borrow() - 10.0 * c).abs() < 1e-6);
        assert!((sim.state.lock().unwrap().pose.0 - 0.1 * 10.0 * c).abs() < 1e-6);
        assert!(done.try_recv().is_err(), "slow motion must not finish on the old clock");
        sim.set_motion_speed(2.0).unwrap();
        assert!((sim.angles()[yaw] - 0.25).abs() < 1e-6, "a speed change must not jump the pose");
        for _ in 0..10 { sim.step(c); }
        assert!((sim.angles()[yaw] - 0.75).abs() < 1e-6);
        assert!(done.try_recv().is_err());
        for _ in 0..5 { sim.step(c); }
        assert!(done.await.unwrap());
        assert!((sim.angles()[yaw] - 1.0).abs() < 1e-6);
        for invalid in [0.0, -1.0, 2.1, f64::NAN, f64::INFINITY] {
            assert!(sim.set_motion_speed(invalid).is_err());
        }
        assert_eq!(sim.state.lock().unwrap().motion_speed, 2.0);
    }

    #[test]
    fn curves_interpolate_without_overshoot() {
        let c = Curve { keys: vec![Key::new(0.0, 0.0), Key::new(1.0, 1.0), Key::new(2.0, 0.5)] };
        assert_eq!(c.sample(-1.0), 0.0);
        assert!((c.sample(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(c.sample(5.0), 0.5);
        for i in 0..=200 {
            let v = c.sample(i as f64 / 100.0);
            assert!((-1e-9..=1.0 + 1e-9).contains(&v), "overshoot {v}");
        }
        let linear_handles = Curve { keys: vec![Key { right: Some((0.5, 0.5)), ..Key::new(0.0, 0.0) }, Key { left: Some((-0.5, -0.5)), ..Key::new(1.5, 1.5) }] };
        assert!((linear_handles.sample(0.75) - 0.75).abs() < 1e-3);
        let seg = |after| Curve { keys: vec![Key { after, ..Key::new(0.0, 0.0) }, Key::new(2.0, 60.0)] };
        assert_eq!(seg(Segment::Constant).sample(1.0), 0.0, "constant holds until the next key");
        assert_eq!(seg(Segment::Constant).sample(2.0), 60.0);
        assert_eq!(seg(Segment::Linear).sample(0.5), 15.0);
        // Automatic slopes stop short of the neighbouring keys: no overshoot past a close key.
        let steep = Curve { keys: vec![Key::new(0.0, 0.0), Key::new(1.0, 1.0), Key::new(1.1, 1.01)] };
        assert!((0..=110).all(|i| steep.sample(i as f64 / 100.0) <= 1.01 + 1e-9));
    }

    #[test]
    fn bang_bang_profiles() {
        // From rest with the velocity limit reached: accelerate, cruise, brake.
        let b = BangBang::new(0.0, 0.0, 1.0, 0.5, 10.0);
        assert!((b.duration() - (1.0 / 0.5 + 0.5 / 10.0)).abs() < 1e-12);
        assert!((b.sample(0.024).0 - 0.5 * 10.0 * 0.024 * 0.024).abs() < 1e-12);
        assert!((b.sample(1.0).1 - 0.5).abs() < 1e-12);
        assert_eq!(b.sample(9.0), (1.0, 0.0));
        // Short move: triangle at sqrt(a·d).
        let t = BangBang::new(0.0, 0.0, -0.1, 8.0, 10.0);
        assert!((t.duration() - 2.0 * (0.1f64 / 10.0).sqrt()).abs() < 1e-12);
        // Stretched from rest: same shape, scaled in time.
        let mut s = BangBang::new(0.0, 0.0, 0.3, 2.0, 10.0);
        s.stretch(1.0);
        assert!((s.duration() - 1.0).abs() < 1e-12 && (s.sample(0.5).0 - 0.15).abs() < 1e-9);
        // Moving towards the target too fast to stop: overshoot, then come back.
        let o = BangBang::new(0.0, 2.0, 0.05, 4.0, 10.0);
        let peak = (0..1000).map(|i| o.sample(i as f64 * o.duration() / 1000.0).0).fold(f64::MIN, f64::max);
        assert!(peak > 0.199 && (o.sample(o.duration()).0 - 0.05).abs() < 1e-9, "stops 0.2 ahead: {peak}");
        // Moving and stretched: still ends at the target at the requested time.
        let mut m = BangBang::new(0.0, 1.0, 1.0, 4.0, 10.0);
        m.stretch(3.0);
        assert!((m.duration() - 3.0).abs() < 1e-6 && (m.sample(3.0).0 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cubic_timing_and_spline() {
        // From rest: 1.5·distance / peak velocity, rounded to whole cycles.
        assert!((cubic_time(0.0, 1.0, 0.0, 0.0, 0.5 * 8.267974, 0.024) - 15.0 * 0.024).abs() < 1e-9);
        assert_eq!(cubic_time(0.3, 0.3, 0.0, 0.0, 1.0, 0.024), 0.024);
        let h = Spline::hermite(0.0, 0.0, 1.0, 1.0);
        assert!((h.sample(0.5).0 - 0.5).abs() < 1e-12 && (h.sample(0.5).1 - 1.5).abs() < 1e-12);
        // Through keys, with continuous velocity at an inner key and the end velocities imposed.
        let s = Spline::new(vec![0.0, 0.5, 1.0, 1.7], vec![0.0, 0.5, 1.0, 0.2], 0.3, 0.0);
        assert!((s.sample(1.0).0 - 1.0).abs() < 1e-12 && (s.sample(0.0).1 - 0.3).abs() < 1e-9 && s.sample(1.7).1 == 0.0);
        assert!((s.sample(0.5 - 1e-7).1 - s.sample(0.5 + 1e-7).1).abs() < 1e-4);
    }

    #[tokio::test]
    async fn tasks_reach_targets_and_cancel() {
        let sim = Sim::new(Robot::load("nao6").unwrap());
        let dt = sim.dt();
        let yaw = sim.robot.joint_index("HeadYaw").unwrap();
        let (_, done) = sim.move_with_speed(&[yaw], &[10.0], 1.0, false); // beyond the limit
        let (_, second) = sim.set_angles(&[yaw], &[1.0], 0.5);
        assert!(!done.await.unwrap(), "first task is cancelled by the overlapping one");
        for _ in 0..30 {
            sim.step(dt);
        }
        assert!(second.await.unwrap());
        assert_eq!(sim.angles()[yaw], 1.0);
        // Commands change once per motion cycle, whatever the step.
        sim.set_angles(&[yaw], &[0.0], 1.0);
        sim.step(dt / 2.0);
        assert_eq!(sim.angles()[yaw], 1.0);
        sim.step(dt / 2.0);
        assert!(sim.angles()[yaw] < 1.0);
        let lhyp = sim.robot.joint_index("LHipYawPitch").unwrap();
        let rhyp = sim.robot.joint_index("RHipYawPitch").unwrap();
        sim.set_angles(&[lhyp], &[-0.3], 1.0);
        for _ in 0..30 {
            sim.step(dt);
        }
        assert_eq!(sim.angles()[rhyp], sim.angles()[lhyp]);
        // The head pitch range shrinks with the yaw (anti-collision limits).
        let pitch = sim.robot.joint_index("HeadPitch").unwrap();
        sim.set_angles(&[yaw, pitch], &[1.527, 0.5], 1.0);
        for _ in 0..60 {
            sim.step(dt);
        }
        assert!((sim.angles()[pitch] - 0.2).abs() < 1e-9);
    }
}
