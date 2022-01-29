//! `coarse-prof` allows you to hierarchically measure the time that blocks in
//! your program take, enabling you to get an intuition of where most time is
//! spent. This can be useful for game development, where you have a bunch of
//! things that need to run in every frame, such as physics, rendering,
//! networking and so on, and you may wish to identify the hot spots, so that
//! you know whether and what to optimize.
//!
//! `coarse-prof`'s implementation has been inspired by
//! [hprof](https://github.com/cmr/hprof).
//! In contrast to `hprof`, which resets measurements after each frame, this
//! library tracks averages over multiple frames. Also, `coarse-prof` provides
//! the macro [`profile`](macro.profile.html) for profiling a scope, so that
//! users do not have to assign a name to scope guards.
//!
//! # Example
//!
//! ```
//! use std::thread::sleep;
//! use std::time::Duration;
//!
//! use coarse_prof::profile;
//!
//! fn render() {
//!     profile!("render");
//!
//!     // So slow!
//!     sleep(Duration::from_millis(10));
//! }
//!
//! // Our game's main loop
//! let num_frames = 100;
//! for i in 0..num_frames {
//!     profile!("frame");
//!
//!     // Physics don't run every frame
//!     if i % 10 == 0 {
//!         profile!("physics");
//!         sleep(Duration::from_millis(2));
//!         
//!         {
//!             profile!("collisions");
//!             sleep(Duration::from_millis(1));
//!         }
//!     }
//!     
//!     render();
//! }
//!
//! // Print the profiling results.
//! coarse_prof::write(&mut std::io::stdout()).unwrap();
//! ```
//!
//! Example output:
//! ```text
//!                | global[%] local[%] self[%] | calls f[Hz] | mean[ms] min[ms] max[ms] std[ms]
//! frame          |     99.98    99.98    0.02 |   1e2 96.37 |    10.37   10.06   13.19    0.94
//!   physics      |      3.00     3.00   66.13 |   1e1  9.64 |     3.11    3.11    3.12    0.00
//!     collisions |      1.02    33.87  100.00 |   1e1  9.64 |     1.05    1.05    1.06    0.00
//!   render       |     96.96    96.98  100.00 |   1e2 96.37 |    10.06   10.05   10.07    0.00
//! ```

use std::{cell::RefCell, io, rc::Rc, time::Duration};

use instant::Instant;
use tabular::{row, Table};

thread_local!(
    /// Global thread-local instance of the profiler.
    pub static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new())
);

const INDENT_STR: &str = "  ";

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeName {
    Borrowed(&'static str),
    Owned(String),
}

/// Print profiling scope tree.
///
/// See [`to_string`](fn.to_string.html) for example output.
pub fn write<W: io::Write>(out: &mut W) -> Result<(), io::Error> {
    write!(out, "{}", to_string())
}

/// Get profiling scope tree as a string.
///
/// Example result:
/// ```text
///                | global[%] local[%] self[%] | calls f[Hz] | mean[ms] min[ms] max[ms] std[ms]
/// frame          |     99.98    99.98    0.02 |   1e2 96.37 |    10.37   10.06   13.19    0.94
///   physics      |      3.00     3.00   66.13 |   1e1  9.64 |     3.11    3.11    3.12    0.00
///     collisions |      1.02    33.87  100.00 |   1e1  9.64 |     1.05    1.05    1.06    0.00
///   render       |     96.96    96.98  100.00 |   1e2 96.37 |    10.06   10.05   10.07    0.00
/// ```
pub fn to_string() -> String {
    PROFILER.with(|p| p.borrow().to_string())
}

/// Reset profiling information.
pub fn reset() {
    PROFILER.with(|p| p.borrow_mut().reset());
}

/// Manually enter a scope.
///
/// The returned instance of [`Guard`](struct.Guard.html) should be dropped
/// when leaving the scope.
///
/// Usually, it is more convenient to use the macro
/// [`profile`](macro.profile.html) for including a scope in profiling, but in
/// some special cases explicit entering/leaving can make sense.
pub fn enter(name: &'static str) -> Guard {
    PROFILER.with(|p| p.borrow_mut().enter(ScopeName::Borrowed(name)))
}

/// Use this macro to add the current scope to profiling. In effect, the time
/// taken from entering to leaving the scope will be measured.
///
/// Internally, the scope is inserted in the scope tree of the global
/// thread-local [`PROFILER`](constant.PROFILER.html).
///
/// # Example
///
/// The following example will profile the scope `"foo"`, which has the scope
/// `"bar"` as a child.
///
/// ```
/// use coarse_prof::profile;
///
/// {
///     profile!("foo");
///
///     {
///         profile!("bar");
///         // ... do something ...
///     }
///
///     // ... do some more ...
/// }
/// ```
#[macro_export]
macro_rules! profile {
    ($name:expr) => {
        let _guard =
            $crate::PROFILER.with(|p| p.borrow_mut().enter($crate::ScopeName::Borrowed($name)));
    };
}

#[macro_export]
macro_rules! profile_string_name {
    ($name:expr) => {
        let _guard =
            $crate::PROFILER.with(|p| p.borrow_mut().enter($crate::ScopeName::Owned($name)));
    };
}

/// Internal representation of scopes as a tree.
struct Scope {
    /// Name of the scope.
    name: ScopeName,

    /// Parent scope in the tree. Root scopes have no parent.
    pred: Option<Rc<RefCell<Scope>>>,

    /// Child scopes in the tree.
    succs: Vec<Rc<RefCell<Scope>>>,

    /// Is this scope currently being visited?
    is_active: bool,

    /// How often has this scope been visited?
    num_calls: usize,

    /// Total time spent in this scope.
    dur_sum: Duration,

    /// Minimal duration spent in this scope.
    dur_min: Duration,

    /// Maximal duration spent in this scope.
    dur_max: Duration,

    /// Running mean.
    dur_mean_secs: f64,

    /// Running M2 for variance estimation (Welford's online algorithm).
    dur_m2_secs2: f64,
}

impl Scope {
    fn new(name: ScopeName, pred: Option<Rc<RefCell<Scope>>>) -> Scope {
        Scope {
            name,
            pred,
            succs: Vec::new(),
            is_active: false,
            num_calls: 0,
            dur_sum: Duration::new(0, 0),
            dur_min: Duration::new(u64::MAX, u32::MIN),
            dur_max: Duration::new(0, 0),
            dur_mean_secs: 0.0,
            dur_m2_secs2: 0.0,
        }
    }

    /// Enter this scope. Returns a `Guard` instance that should be dropped
    /// when leaving the scope.
    fn enter(&mut self) -> Guard {
        assert!(!self.is_active, "Scope was not left properly");

        self.is_active = true;

        Guard::enter()
    }

    /// Leave this scope. Called automatically by the `Guard` instance.
    fn leave(&mut self, dur_last: Duration) {
        assert!(self.is_active, "Scope was not entered properly");

        self.is_active = false;
        self.num_calls += 1;

        self.dur_sum = self
            .dur_sum
            .checked_add(dur_last)
            .unwrap_or_else(|| Duration::new(0, 0));
        self.dur_min = self.dur_min.min(dur_last);
        self.dur_max = self.dur_max.max(dur_last);

        // Use Welford's online algorithm for variance estimation.
        let prev_dur_mean_secs = self.dur_mean_secs;
        self.dur_mean_secs += (dur_last.as_secs_f64() - self.dur_mean_secs) / self.num_calls as f64;
        self.dur_m2_secs2 += (dur_last.as_secs_f64() - prev_dur_mean_secs)
            * (dur_last.as_secs_f64() - self.dur_mean_secs);
    }

    fn write_recursive(&self, total_dur: Duration, depth: usize, table: &mut Table) {
        // num_calls == 0 happens only if this is a new scope that has not been
        // left yet.
        if self.num_calls > 0 {
            let name = match &self.name {
                ScopeName::Borrowed(s) => s.to_owned(),
                ScopeName::Owned(s) => s,
            };
            let pred_dur_sum_secs = self.pred.as_ref().map_or(total_dur.as_secs_f64(), |pred| {
                pred.borrow().dur_sum.as_secs_f64()
            });
            let succs_dur_sum_secs = self
                .succs
                .iter()
                .map(|succ| succ.borrow().dur_sum.as_secs_f64())
                .sum::<f64>();
            let local_percent = self.dur_sum.as_secs_f64() / pred_dur_sum_secs * 100.0;
            let global_percent = self.dur_sum.as_secs_f64() / total_dur.as_secs_f64() * 100.0;
            let self_percent = (self.dur_sum.as_secs_f64() - succs_dur_sum_secs).max(0.0)
                / self.dur_sum.as_secs_f64()
                * 100.0;
            let freq_hz =
                (self.num_calls + self.is_active as usize) as f64 / total_dur.as_secs_f64();
            let mean_secs = self.dur_sum.as_secs_f64() / self.num_calls as f64;
            let std_secs = (self.dur_m2_secs2 / self.num_calls as f64).sqrt();

            // Write self
            table.add_row(row!(
                INDENT_STR.repeat(depth) + name,
                format!("{:.2}", global_percent),
                format!("{:.2}", local_percent),
                format!(
                    "{:.2}",
                    if self.dur_sum.as_secs_f64() > 0.00000001 {
                        self_percent
                    } else {
                        100.0
                    }
                ),
                format!("{:.2}", mean_secs * 1000.0),
                format!("{:.2}", self.dur_min.as_secs_f64() * 1000.0),
                format!("{:.2}", self.dur_max.as_secs_f64() * 1000.0),
                format!("{:.2}", std_secs * 1000.0),
                format!("{:.2}", freq_hz),
                format!("{:>6.2e}", self.num_calls),
            ));
        }

        // Write children
        for succ in &self.succs {
            succ.borrow().write_recursive(total_dur, depth + 1, table);
        }
    }
}

/// A guard that is created when entering a scope and dropped when leaving it.
pub struct Guard {
    enter_time: Instant,
}

impl Guard {
    fn enter() -> Self {
        Self {
            enter_time: Instant::now(),
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let duration = self.enter_time.elapsed();
        PROFILER.with(|p| p.borrow_mut().leave(duration));
    }
}

/// A `Profiler` stores the scope tree and keeps track of the currently active
/// scope.
///
/// Note that there is a global thread-local instance of `Profiler` in
/// [`PROFILER`](constant.PROFILER.html), so it is not possible to manually
/// create an instance of `Profiler`.
pub struct Profiler {
    roots: Vec<Rc<RefCell<Scope>>>,
    current: Option<Rc<RefCell<Scope>>>,
    start_time: Instant,
}

impl Profiler {
    fn new() -> Profiler {
        Profiler {
            roots: Vec::new(),
            current: None,
            start_time: Instant::now(),
        }
    }

    /// Enter a scope. Returns a [`Guard`](struct.Guard.html) that should be
    /// dropped upon leaving the scope.
    ///
    /// Usually, this method will be called by the
    /// [`profile`](macro.profile.html) macro, so it does not need to be used
    /// directly.
    pub fn enter(&mut self, name: ScopeName) -> Guard {
        // Check if we have already registered `name` at the current point in
        // the tree.
        let succ = if let Some(current) = self.current.as_ref() {
            // We are currently in some scope.
            let existing_succ = current
                .borrow()
                .succs
                .iter()
                .find(|succ| succ.borrow().name == name)
                .cloned();

            existing_succ.unwrap_or_else(|| {
                // Add new successor node to the current node.
                let new_scope = Scope::new(name, Some(current.clone()));
                let succ = Rc::new(RefCell::new(new_scope));

                current.borrow_mut().succs.push(succ.clone());

                succ
            })
        } else {
            // We are currently not within any scope. Check if `name` already
            // is a root.
            let existing_root = self
                .roots
                .iter()
                .find(|root| root.borrow().name == name)
                .cloned();

            existing_root.unwrap_or_else(|| {
                // Add a new root node.
                let new_scope = Scope::new(name, None);
                let succ = Rc::new(RefCell::new(new_scope));

                self.roots.push(succ.clone());

                succ
            })
        };

        let guard = succ.borrow_mut().enter();

        self.current = Some(succ);

        guard
    }

    /// Completely reset profiling data.
    fn reset(&mut self) {
        self.roots.clear();
        self.start_time = Instant::now();

        // Note that we could now still be anywhere in the previous profiling
        // tree, so we can not simply reset `self.current`. However, as the
        // frame comes to an end we will eventually leave a root node, at which
        // point `self.current` will be set to `None`.
    }

    /// Leave the current scope.
    fn leave(&mut self, duration: Duration) {
        let current = self
            .current
            .clone()
            .expect("Called coarse_prof::leave() while not in any scope");

        current.borrow_mut().leave(duration);

        // Set current scope back to the parent node (if any).
        self.current = current.borrow().pred.as_ref().cloned();
    }

    fn to_string(&self) -> String {
        let total_dur = Instant::now().duration_since(self.start_time);

        let mut table = Table::new("{:<} | {:>} {:>} {:>} | {:>} {:>} {:>} {:>} | {:>} {:>}");
        table.add_row(row!(
            "",
            "global[%]",
            "local[%]",
            "self[%]",
            "mean[ms]",
            "min[ms]",
            "max[ms]",
            "std[ms]",
            "f[Hz]",
            "calls",
        ));

        for root in self.roots.iter() {
            root.borrow().write_recursive(total_dur, 0, &mut table);
        }

        format!("{}", table)
    }
}

#[cfg(test)]
mod tests {
    use super::ScopeName;

    #[test]
    fn test_multiple_roots() {
        super::reset();

        for i in 0..=5 {
            if i == 5 {
                profile!("a");
            }
            {
                profile!("b");
            }
        }

        super::PROFILER.with(|p| {
            let p = p.borrow();

            assert_eq!(p.roots.len(), 2);

            for root in p.roots.iter() {
                assert!(root.borrow().pred.is_none());
                assert!(root.borrow().succs.is_empty());
            }

            assert_eq!(p.roots[0].borrow().name, ScopeName::Borrowed("b"));
            assert_eq!(p.roots[1].borrow().name, ScopeName::Borrowed("a"));

            assert_eq!(p.roots[0].borrow().num_calls, 6);
            assert_eq!(p.roots[1].borrow().num_calls, 1);
        });
    }

    #[test]
    fn test_succ_reuse() {
        use std::ptr;

        super::reset();

        for i in 0..=5 {
            profile!("a");
            if i > 2 {
                profile!("b");
            }
        }

        assert_eq!(super::PROFILER.with(|p| p.borrow().roots.len()), 1);

        super::PROFILER.with(|p| {
            let p = p.borrow();

            assert_eq!(p.roots.len(), 1);

            let root = p.roots[0].borrow();
            assert_eq!(root.name, ScopeName::Borrowed("a"));
            assert!(root.pred.is_none());
            assert_eq!(root.succs.len(), 1);
            assert_eq!(root.num_calls, 6);

            let succ = root.succs[0].borrow();
            assert_eq!(succ.name, ScopeName::Borrowed("b"));
            assert!(ptr::eq(
                succ.pred.as_ref().unwrap().as_ref(),
                p.roots[0].as_ref()
            ));
            assert!(succ.succs.is_empty());
            assert_eq!(succ.num_calls, 3);
        });
    }

    #[test]
    fn test_reset_during_frame() {
        super::reset();

        for i in 0..=5 {
            profile!("a");
            profile!("b");
            {
                profile!("c");
                if i == 5 {
                    super::reset();
                }

                assert!(super::PROFILER.with(|p| p.borrow().current.is_some()));

                profile!("d");
            }
        }

        super::PROFILER.with(|p| {
            let p = p.borrow();

            assert!(p.roots.is_empty());
            assert!(p.current.is_none());
        });
    }
}
