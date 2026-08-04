//! `BondStructureSystem` — observation of the "chemistry" that emerges.
//!
//! Same philosophy as `StructureSystem`, but instead of spatial proximity
//! (friends-of-friends) it takes the **persistent-bond graph** written by
//! `BondObservationSystem` and measures its connected components. Each
//! component is a multi-body structure with a definite stoichiometry — an
//! aggregate of the *observed* chemistry. Nothing is programmed per species:
//! the graph only exists because the observation measured persistent pairs.
//! It modifies nothing; it only writes the `ChemicalStructure` resource.

use crate::analysis::bond_components;
use crate::components::{AtomType, Bonds};
use crate::config::Config;
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::{ChemicalStructure, CompositionEntry};
use std::collections::HashMap;

/// Samples the persistent-bond graph every `interval` ticks.
pub struct BondStructureSystem {
    interval: u64,
}

impl BondStructureSystem {
    pub fn new(interval: u64) -> Self {
        Self {
            interval: interval.max(1),
        }
    }
}

impl System for BondStructureSystem {
    fn name(&self) -> &'static str {
        "bond-structure"
    }

    fn access(&self) -> Access {
        Access::default()
            .reads::<Bonds>()
            .reads::<AtomType>()
            .resource_read::<Config>()
            .resource_write::<ChemicalStructure>()
    }

    fn run(&mut self, ctx: &mut SystemContext<'_>) {
        if !ctx.time.tick.is_multiple_of(self.interval) {
            return;
        }
        let Some(cfg) = ctx.resources.get::<Config>() else {
            return;
        };
        if !cfg.systems.enable_bond_observation {
            return;
        }

        let mut entities: Vec<(u32, AtomType)> = Vec::new();
        let mut edges: Vec<(u32, u32)> = Vec::new();
        ctx.world.for_each2::<Bonds, AtomType>(|e, bonds, at| {
            if bonds.neighbors.is_empty() {
                return;
            }
            entities.push((e.index(), *at));
            for &n in &bonds.neighbors {
                edges.push((e.index().min(n.index()), e.index().max(n.index())));
            }
        });

        let components = bond_components(&entities, &edges);
        let bound_entities = entities.len();

        let mut counts: HashMap<String, u64> = HashMap::new();
        let mut largest = 0usize;
        for c in &components {
            largest = largest.max(c.size);
            *counts.entry(c.formula.clone()).or_default() += 1;
        }
        let mut compositions: Vec<CompositionEntry> = counts
            .iter()
            .map(|(formula, count)| CompositionEntry {
                formula: formula.clone(),
                count: *count,
            })
            .collect();
        compositions.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.formula.cmp(&b.formula))
        });

        if let Some(cs) = ctx.resources.get_mut::<ChemicalStructure>() {
            cs.tick = ctx.time.tick;
            cs.aggregates = components.len();
            cs.bound_entities = bound_entities;
            cs.monomers = ctx.world.len().saturating_sub(bound_entities);
            cs.largest = largest;
            cs.compositions = compositions;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{register_all, Mass, Position, Velocity};
    use crate::ecs::{Resources, World};
    use crate::math::Vec3;
    use crate::rng::Rng;
    use crate::scheduler::SystemContext;
    use crate::stats::StatsCollector;
    use crate::universe::Time;

    fn world_with_bond_graph() -> World {
        register_all();
        let mut w = World::new();
        // Na–O–Na chain: a single component of size 3 (formula Na2-O).
        let types = [
            (AtomType::Sodium, 0.0),
            (AtomType::Oxygen, 1.0),
            (AtomType::Sodium, 2.0),
            (AtomType::Sodium, 4.0), // isolated monomer
        ];
        for (at, x) in types.iter() {
            let e = w.spawn();
            w.insert::<Position>(e, Position(Vec3::new(*x, 0.0, 0.0)));
            w.insert::<AtomType>(e, *at);
            w.insert::<Mass>(e, Mass(at.mass()));
            w.insert::<Velocity>(e, Velocity(Vec3::ZERO));
            w.insert::<Bonds>(e, Bonds::default());
        }
        let mut chain = Vec::new();
        w.for_each1::<Bonds>(|e, _| chain.push(e));
        let (a, b, c) = (chain[0], chain[1], chain[2]);
        w.get_mut::<Bonds>(a).unwrap().neighbors = vec![b];
        w.get_mut::<Bonds>(b).unwrap().neighbors = vec![a, c];
        w.get_mut::<Bonds>(c).unwrap().neighbors = vec![b];
        w
    }

    #[test]
    fn measures_components_and_stoichiometries() {
        let mut world = world_with_bond_graph();
        let mut cfg = Config::default_config();
        cfg.systems.enable_bond_observation = true;
        let mut resources = Resources::new();
        resources.insert(cfg);
        resources.insert(ChemicalStructure::default());
        let mut rng = Rng::new(7);
        let mut time = Time::new(0.0167);
        let mut stats = StatsCollector::new(16);
        let mut sys = BondStructureSystem::new(1);

        time.advance();
        let mut ctx = SystemContext {
            world: &mut world,
            resources: &mut resources,
            rng: &mut rng,
            time: &time,
            stats: &mut stats,
            dt: time.dt,
        };
        sys.run(&mut ctx);

        let cs = resources.get::<ChemicalStructure>().unwrap();
        assert_eq!(cs.tick, 1);
        assert_eq!(cs.aggregates, 1, "the Na–O–Na chain is one component");
        assert_eq!(cs.bound_entities, 3);
        assert_eq!(cs.monomers, 1, "the isolated Na is a monomer");
        assert_eq!(cs.largest, 3);
        assert_eq!(cs.compositions.len(), 1);
        assert_eq!(cs.compositions[0].formula, "Na2-O");
        assert_eq!(cs.compositions[0].count, 1);
    }
}
