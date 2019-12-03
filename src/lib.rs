//! `coarse-prof` allows you to hierarchically measure the time that blocks in
//! your program take, enabling you to get an intuition of where most time is
//! spent. This can be useful for game development, where you have a bunch of
//! things that need to run in every frame, such as physics, rendering,
//! networking and so on, and you may wish to identify the hot spots, so that
//! you know whether and what to optimize.
//!
//! `coarse-prof`'s implementation has been inspired by
//! [hprof](https://cmr.github.io/hprof/src/hprof/lib.rs.html).
//! In contrast to `hprof`, which resets measurements after each frame, this
//! library tracks averages over multiple frames. Also, `coarse-prof` provides
//! a macro for profiling a scope, so that users do not have to assign a name
//! to scope guards.
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
//! coarse_prof::print(&mut std::io::stdout());
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
use std::rc::Rc;
use std::time::{Duration, Instant};

use floating_duration::TimeAsFloat;

thread_local!(
    /// Global thread-local instance of the profiler.
    pub static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new())
);

/// Print profiling scope tree.
pub fn print<W: std::io::Write>(out: &mut W) {
    PROFILER.with(|p| p.borrow().print(out));
}

/// Reset profiling information.
pub fn reset() {
    PROFILER.with(|p| p.borrow_mut().reset());
}

/// Use this macro to add the current scope to profiling. In effect, the time
/// taken from entering to leaving the scope will be measured.
///
/// Internally, the scope is added as a `Scope` to the global thread-local
/// `PROFILER`.
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
}

impl Scope {
    fn new(name: &'static str, pred: Option<Rc<RefCell<Scope>>>) -> Scope {
        Scope {
            name,
            pred,
            succs: Vec::new(),
            num_calls: 0,
            duration_sum: Duration::new(0, 0),
        }
    }

    /// Enter this scope. Returns a `Guard` instance that should be dropped
    /// when leaving the scope.
    fn enter(&mut self) -> Guard {
        self.num_calls += 1;
        Guard::enter()
    }

    /// Leave this scope. Called automatically by the `Guard` instance.
    fn leave(&mut self, duration: Duration) {
        let duration_sum = self.duration_sum.checked_add(duration);

        // Even though this is extremely unlikely, let's not panic on overflow.
        self.duration_sum = duration_sum.unwrap_or(Duration::from_millis(0));
    }

    fn print_recursive<W: std::io::Write>(
        &self,
        out: &mut W,
        total_duration: Duration,
        depth: usize,
    ) {
        let total_duration_secs = total_duration.as_fractional_secs();
        let duration_sum_secs = self.duration_sum.as_fractional_secs();

        let percent = self
            .pred
            .clone()
            .map(|pred| duration_sum_secs / pred.borrow().duration_sum.as_fractional_secs())
            .unwrap_or(1.0)
            * 100.0;

        // Write self
        for _ in 0..depth {
            write!(out, "  ").unwrap();
        }
        writeln!(
            out,
            "{}: {:3.2}%, {:>4.2}ms/call @ {:.2}Hz",
            self.name,
            percent,
            duration_sum_secs * 1000.0 / (self.num_calls as f64),
            self.num_calls as f64 / total_duration_secs,
        )
        .unwrap();

        // Write children
        for succ in &self.succs {
            succ.borrow()
                .print_recursive(out, total_duration, depth + 1);
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
/// Note that there is a global instance of `Profiler` in `PROFILER`, so it is
/// not possible to manually create an instance of `Profiler`.
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

    /// Enter a scope. Returns a `Guard` that should be dropped upon leaving
    /// the scope.
    ///
    /// Usually, this method will be called by the `profile!` macro, so it does
    /// not need to be used directly.
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
            current.borrow().pred.as_ref().map(|pred| pred.clone())
        } else {
            // This should not happen with proper usage.
            log::error!("Called coarse_prof::leave() while not in any scope");

            None
        };
    }

    fn print<W: std::io::Write>(&self, out: &mut W) {
        let total_duration = self
            .roots
            .iter()
            .map(|root| root.borrow().duration_sum)
            .sum();

        for root in self.roots.iter() {
            root.borrow().print_recursive(out, total_duration, 0);
        }

        out.flush().unwrap();
    }
}
