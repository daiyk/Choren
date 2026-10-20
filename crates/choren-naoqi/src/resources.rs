//! Resource arbitration (ALResourceManager semantics as Choregraphe exposes them on boxes).
//!
//! A box declares `<Resource name type timeout>`; it acquires them when its onStart fires and
//! releases them when it stops or unloads. Two resources conflict when their leaves in the robot's
//! resource tree overlap ("All motors" vs "LArm"). On a conflict, the *holder's* mode decides:
//! - **Lock** / **Wait** / Pause on demand: the holder keeps it; the requester waits.
//! - **Stop on demand:** the holder box is stopped, which releases it.
//! - **Callback on demand:** the holder's `onResource(name)` runs and may call `releaseResource()`.
//!
//! The requester waits `timeout` seconds (a **Wait** requester waits without limit), and at least
//! `DEMAND_GRACE` when it asked a holder to stop or release.
//!
//! A box may use what an enclosing box of the same behavior holds: library boxes such as Stand Up
//! lock "All motors" and contain a Goto Posture that locks it again.
//! shortcut: modes come from Choregraphe's documented behavior, not from oracle traces (plan
//! R-R4); Pause on demand is treated as Lock until timeline pause exists.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::watch;

pub const DEMAND_GRACE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Lock,
    Wait,
    StopOnDemand,
    CallbackOnDemand,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s {
            "Wait" => Mode::Wait,
            "Stop on demand" => Mode::StopOnDemand,
            "Callback on demand" => Mode::CallbackOnDemand,
            _ => Mode::Lock,
        }
    }
}

/// A box of a running behavior.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Owner {
    pub behavior: String,
    pub node: usize,
}

#[derive(Clone, Debug)]
pub struct Want {
    pub name: String,
    pub leaves: BTreeSet<String>,
    pub mode: Mode,
}

#[derive(Clone, Debug)]
struct Hold {
    want: Want,
    owner: Owner,
}

pub struct Resources {
    holds: Mutex<Vec<Hold>>,
    changed: watch::Sender<u64>,
}

impl Default for Resources {
    fn default() -> Self {
        Resources { holds: Default::default(), changed: watch::channel(0).0 }
    }
}

/// A conflicting holder and what the requester must ask of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    pub owner: Owner,
    pub mode: Mode,
    pub resource: String,
}

impl Resources {
    fn bump(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    /// Subscribe before `try_acquire`, then await `changed()` to retry without missing a release.
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// Grants every wanted resource at once, or none and the holders in the way. `ancestors` are
    /// the boxes enclosing `owner`; their holds are shared with it.
    pub fn try_acquire(&self, owner: &Owner, ancestors: &[usize], wants: &[Want]) -> Result<(), Vec<Conflict>> {
        let mut holds = self.holds.lock().unwrap();
        let inherited = |h: &Hold| h.owner.behavior == owner.behavior && ancestors.contains(&h.owner.node);
        let conflicts: Vec<Conflict> = holds
            .iter()
            .filter(|h| &h.owner != owner && !inherited(h) && wants.iter().any(|w| !w.leaves.is_disjoint(&h.want.leaves)))
            .map(|h| Conflict { owner: h.owner.clone(), mode: h.want.mode, resource: h.want.name.clone() })
            .collect();
        if !conflicts.is_empty() {
            return Err(conflicts);
        }
        for w in wants {
            if !holds.iter().any(|h| &h.owner == owner && h.want.name == w.name) {
                holds.push(Hold { want: w.clone(), owner: owner.clone() });
            }
        }
        Ok(())
    }

    pub fn holds_all(&self, owner: &Owner, wants: &[Want]) -> bool {
        let holds = self.holds.lock().unwrap();
        wants.iter().all(|w| holds.iter().any(|h| &h.owner == owner && h.want.name == w.name))
    }

    /// True when nobody but `except` holds anything overlapping `leaves`.
    pub fn is_free(&self, leaves: &BTreeSet<String>, except: Option<&Owner>) -> bool {
        self.holds.lock().unwrap().iter().all(|h| Some(&h.owner) == except || h.want.leaves.is_disjoint(leaves))
    }

    pub fn release(&self, owner: &Owner) {
        let mut holds = self.holds.lock().unwrap();
        let before = holds.len();
        holds.retain(|h| &h.owner != owner);
        if holds.len() != before {
            drop(holds);
            self.bump();
        }
    }

    pub fn release_behavior(&self, behavior: &str) {
        self.holds.lock().unwrap().retain(|h| h.owner.behavior != behavior);
        self.bump();
    }

    /// (resource, behavior, box node) for every hold, for diagnostics and tests.
    pub fn held(&self) -> Vec<(String, String, usize)> {
        self.holds.lock().unwrap().iter().map(|h| (h.want.name.clone(), h.owner.behavior.clone(), h.owner.node)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_leaves_conflict() {
        let r = choren_robot::Robot::load("nao6").unwrap();
        let want = |n: &str, m: Mode| Want { name: n.into(), leaves: r.resource_leaves(n), mode: m };
        let res = Resources::default();
        let (a, b) = (Owner { behavior: "x".into(), node: 1 }, Owner { behavior: "x".into(), node: 2 });
        res.try_acquire(&a, &[], &[want("Left arm", Mode::StopOnDemand)]).unwrap();
        assert!(res.try_acquire(&a, &[], &[want("Left arm", Mode::StopOnDemand)]).is_ok(), "re-acquiring is a no-op");
        res.try_acquire(&b, &[], &[want("Head", Mode::Lock)]).unwrap();
        let c = res.try_acquire(&b, &[], &[want("All motors", Mode::Lock)]).unwrap_err();
        assert_eq!(c, [Conflict { owner: a.clone(), mode: Mode::StopOnDemand, resource: "Left arm".into() }]);
        assert!(!res.is_free(&r.resource_leaves("LHand"), None));
        assert!(res.is_free(&r.resource_leaves("Speech"), None));
        let mut rx = res.watch();
        res.release(&a);
        assert!(rx.has_changed().unwrap());
        rx.mark_unchanged();
        res.try_acquire(&b, &[], &[want("All motors", Mode::Lock)]).unwrap();
        assert_eq!(res.held().len(), 2);
        res.release_behavior("x");
        assert!(res.held().is_empty());
        res.try_acquire(&a, &[], &[want("All motors", Mode::Lock)]).unwrap();
        res.try_acquire(&b, &[a.node], &[want("All motors", Mode::Lock)]).expect("a child box uses its parent's resources");
        let other = Owner { behavior: "y".into(), node: 2 };
        assert!(res.try_acquire(&other, &[a.node], &[want("Head", Mode::Lock)]).is_err(), "only within the same behavior");
        assert_eq!(Mode::parse("Callback on demand"), Mode::CallbackOnDemand);
    }
}
