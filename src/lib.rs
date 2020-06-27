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
//! frame: 100.00%, 10.40ms/call @ 96.17Hz
//!   physics: 3.04%, 3.16ms/call @ 9.62Hz
//!     collisions: 33.85%, 1.07ms/call @ 9.62Hz
//!   render: 96.84%, 10.07ms/call @ 96.17Hz
//! ```

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use instant::Instant;

thread_local!(
    /// Global thread-local instance of the profiler.
    pub static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new())
);

/// Print profiling scope tree.
///
/// Example output:
/// ```text
/// frame: 100.00%, 10.40ms/call @ 96.17Hz
///   physics: 3.04%, 3.16ms/call @ 9.62Hz
///     collisions: 33.85%, 1.07ms/call @ 9.62Hz
///   render: 96.84%, 10.07ms/call @ 96.17Hz
/// ```
///
/// Percentages represent the amount of time taken relative to the parent node.
///
/// Frequencies are computed with respect to the total amount of time spent in
/// root nodes. Thus, if you have multiple root nodes and they do not cover
/// all code that runs in your program, the printed frequencies will be
/// overestimated.
pub fn write<W: io::Write>(out: &mut W) -> io::Result<()> {
    PROFILER.with(|p| p.borrow().write(out))
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
    PROFILER.with(|p| p.borrow_mut().enter(name))
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
        let _guard = $crate::PROFILER.with(|p| p.borrow_mut().enter($name));
    };
}

/// Internal representation of scopes as a tree.
struct Scope {
    /// Name of the scope.
    name: &'static str,

    /// Parent scope in the tree. Root scopes have no parent.
    pred: Option<Rc<RefCell<Scope>>>,

    /// Child scopes in the tree.
    succs: Vec<Rc<RefCell<Scope>>>,

    /// How often has this scope been visited?
    num_calls: usize,

    /// In total, how much time has been spent in this scope?
    duration_sum: Duration,

    /// Minimal duration spent in this scope.
    duration_min: Duration,

    /// Maximal duration spent in this scope.
    duration_max: Duration,
}

impl Scope {
    fn new(name: &'static str, pred: Option<Rc<RefCell<Scope>>>) -> Scope {
        Scope {
            name,
            pred,
            succs: Vec::new(),
            num_calls: 0,
            duration_sum: Duration::new(0, 0),
            duration_min: Duration::new(u64::MAX, u32::MIN),
            duration_max: Duration::new(0, 0),
        }
    }

    /// Enter this scope. Returns a `Guard` instance that should be dropped
    /// when leaving the scope.
    fn enter(&mut self) -> Guard {
        Guard::enter()
    }

    /// Leave this scope. Called automatically by the `Guard` instance.
    fn leave(&mut self, duration: Duration) {
        self.num_calls += 1;

        // Even though this is extremely unlikely, let's not panic on overflow.
        let duration_sum = self.duration_sum.checked_add(duration);
        self.duration_sum = duration_sum.unwrap_or(Duration::from_millis(0));

        self.duration_min = self.duration_min.min(duration);
        self.duration_max = self.duration_max.max(duration);
    }

    fn write_recursive<W: io::Write>(
        &self,
        out: &mut W,
        total_duration: Duration,
        depth: usize,
    ) -> io::Result<()> {
        let total_duration_secs = total_duration.as_secs_f64();
        let duration_sum_secs = self.duration_sum.as_secs_f64();
        let pred_sum_secs = self.pred.clone().map_or(total_duration_secs, |pred| {
            pred.borrow().duration_sum.as_secs_f64()
        });

        let percent = duration_sum_secs / pred_sum_secs * 100.0;

        // Write self
        for _ in 0..depth {
            write!(out, "  ")?;
        }
        writeln!(
            out,
            "{}: {:3.2}%, {:>4.2}ms avg, {:>4.2}ms min, {:>4.2}ms max @ {:.2}Hz",
            self.name,
            percent,
            duration_sum_secs * 1000.0 / (self.num_calls as f64),
            self.duration_min.as_secs_f64() * 1000.0,
            self.duration_max.as_secs_f64() * 1000.0,
            self.num_calls as f64 / total_duration_secs,
        )?;

        // Write children
        for succ in &self.succs {
            succ.borrow()
                .write_recursive(out, total_duration, depth + 1)?;
        }

        Ok(())
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
}

impl Profiler {
    fn new() -> Profiler {
        Profiler {
            roots: Vec::new(),
            current: None,
        }
    }

    /// Enter a scope. Returns a [`Guard`](struct.Guard.html) that should be
    /// dropped upon leaving the scope.
    ///
    /// Usually, this method will be called by the
    /// [`profile`](macro.profile.html) macro, so it does not need to be used
    /// directly.
    pub fn enter(&mut self, name: &'static str) -> Guard {
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

        // Note that we could now still be anywhere in the previous profiling
        // tree, so we can not simply reset `self.current`. However, as the
        // frame comes to an end we will eventually leave a root node, at which
        // point `self.current` will be set to `None`.
    }

    /// Leave the current scope.
    fn leave(&mut self, duration: Duration) {
        self.current = if let Some(current) = self.current.as_ref() {
            current.borrow_mut().leave(duration);

            // Set current scope back to the parent node (if any).
            current.borrow().pred.as_ref().cloned()
        } else {
            // This should not happen with proper usage.
            log::error!("Called coarse_prof::leave() while not in any scope");

            None
        };
    }

    fn write<W: io::Write>(&self, out: &mut W) -> io::Result<()> {
        let total_duration = self
            .roots
            .iter()
            .map(|root| root.borrow().duration_sum)
            .sum();

        for root in self.roots.iter() {
            root.borrow().write_recursive(out, total_duration, 0)?;
        }

        out.flush()
    }
}

#[cfg(test)]
mod tests {
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

            assert_eq!(p.roots[0].borrow().name, "b");
            assert_eq!(p.roots[1].borrow().name, "a");

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
            assert_eq!(root.name, "a");
            assert!(root.pred.is_none());
            assert_eq!(root.succs.len(), 1);
            assert_eq!(root.num_calls, 6);

            let succ = root.succs[0].borrow();
            assert_eq!(succ.name, "b");
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
