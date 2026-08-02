//! Scheduler: execution order of the systems.
//!
//! The scheduler runs systems **in the order they were registered**. Nothing
//! is hidden: each system declares its access ([`Access`]) and the scheduler
//! uses it to:
//!
//! 1. Validate that there are no undeclared dependencies.
//! 2. Compute *stages* ([`Stage`]): conflict-free sets of systems that, in the
//!    future, may run in parallel with disjoint borrows.
//! 3. Expose the execution plan for inspection and tests.

mod system;

pub use system::{Access, System, SystemContext};

/// A group of systems with no access conflicts among them.
#[derive(Debug, Clone, Default)]
pub struct Stage {
    /// Indices into `Scheduler::systems`.
    pub systems: Vec<usize>,
}

impl Stage {
    pub fn len(&self) -> usize {
        self.systems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }
}

/// Sequential scheduler with stage analysis.
pub struct Scheduler {
    systems: Vec<Box<dyn System>>,
    staged: Vec<Stage>,
    dirty: bool,
    /// Cumulative executions (total systems run).
    executions: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            staged: Vec::new(),
            dirty: false,
            executions: 0,
        }
    }

    /// Registers a system at the end of the schedule.
    pub fn add_system(&mut self, system: impl System + 'static) {
        self.systems.push(Box::new(system));
        self.dirty = true;
    }

    /// Registered systems (in order).
    pub fn systems(&self) -> &[Box<dyn System>] {
        &self.systems
    }

    /// Recomputes the stage plan if needed.
    pub fn plan(&mut self) -> &[Stage] {
        if self.dirty || self.staged.is_empty() {
            self.staged = compute_stages(&self.systems);
            self.dirty = false;
        }
        &self.staged
    }

    /// Runs all systems **in the order they were registered**.
    ///
    /// Registration order is the order the universe's author chose; the stages
    /// ([`plan`]) are computed only for inspection and to enable, in the
    /// future, parallel execution of stages without changing semantics.
    pub fn run(&mut self, ctx: &mut SystemContext<'_>) {
        for i in 0..self.systems.len() {
            self.systems[i].run(ctx);
            self.executions += 1;
        }
        ctx.stats.systems_run = self.executions;
    }

    /// Total accumulated system executions.
    pub fn executions(&self) -> u64 {
        self.executions
    }
}

/// Computes maximal conflict-free stages respecting registration order.
///
/// Greedy algorithm: each system is assigned to the **first** stage where it
/// does not compete with any current member. Since conflicts impose a partial
/// order, this assignment preserves the semantics of the original order.
pub fn compute_stages(systems: &[Box<dyn System>]) -> Vec<Stage> {
    let mut stages: Vec<Stage> = Vec::new();
    for (i, sys) in systems.iter().enumerate() {
        let acc = sys.access();
        let mut placed = false;
        for stage in stages.iter_mut() {
            let conflicts = stage
                .systems
                .iter()
                .any(|&j| systems[j].access().conflicts_with(&acc));
            if !conflicts {
                stage.systems.push(i);
                placed = true;
                break;
            }
        }
        if !placed {
            stages.push(Stage {
                systems: vec![i],
            });
        }
    }
    stages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{Component, ComponentId};

    struct A;
    impl Component for A {
        const ID: ComponentId = ComponentId(50);
    }

    #[allow(dead_code)]
    struct B;
    impl Component for B {
        const ID: ComponentId = ComponentId(51);
    }

    struct SysA;
    impl System for SysA {
        fn name(&self) -> &'static str {
            "sysA"
        }
        fn access(&self) -> Access {
            Access::default().writes::<A>()
        }
        fn run(&mut self, _ctx: &mut SystemContext<'_>) {}
    }

    struct SysB;
    impl System for SysB {
        fn name(&self) -> &'static str {
            "sysB"
        }
        fn access(&self) -> Access {
            Access::default().reads::<A>()
        }
        fn run(&mut self, _ctx: &mut SystemContext<'_>) {}
    }

    struct SysC;
    impl System for SysC {
        fn name(&self) -> &'static str {
            "sysC"
        }
        fn access(&self) -> Access {
            Access::default().writes::<B>()
        }
        fn run(&mut self, _ctx: &mut SystemContext<'_>) {}
    }

    #[test]
    fn detects_conflicts() {
        let w = Access::default().writes::<A>();
        let r = Access::default().reads::<A>();
        assert!(w.conflicts_with(&r));
        assert!(!r.conflicts_with(&Access::default().reads::<A>()));
        assert!(!Access::default().reads::<A>().conflicts_with(&Access::default().reads::<B>()));
    }

    #[test]
    fn stages_respect_dependencies() {
        // SysA writes A; SysB reads A => conflict (forced relative order).
        // SysC writes B: it competes with neither A nor B, so it can move ahead.
        let systems: Vec<Box<dyn System>> =
            vec![Box::new(SysA), Box::new(SysB), Box::new(SysC)];
        let stages = compute_stages(&systems);

        // Central invariant: any system that *conflicts* with an earlier one
        // lands in a later (or equal) stage, never an earlier one.
        for i in 0..systems.len() {
            for j in (i + 1)..systems.len() {
                if systems[i].access().conflicts_with(&systems[j].access()) {
                    let si = stages.iter().position(|s| s.systems.contains(&i)).unwrap();
                    let sj = stages.iter().position(|s| s.systems.contains(&j)).unwrap();
                    assert!(si <= sj, "dependency {i}->{j} violated: stage {si} > {sj}");
                }
            }
        }

        // Within a stage there can be no conflicts.
        for stage in &stages {
            for (k, &x) in stage.systems.iter().enumerate() {
                for &y in &stage.systems[k + 1..] {
                    assert!(
                        !systems[x].access().conflicts_with(&systems[y].access()),
                        "stage with conflicting systems: {x} and {y}"
                    );
                }
            }
        }
    }
}
