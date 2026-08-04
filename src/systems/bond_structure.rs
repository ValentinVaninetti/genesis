//! `BondStructureSystem` — observation of the "chemistry" that emerges.
//!
//! Same philosophy as `StructureSystem`, but instead of spatial proximity
//! (friends-of-friends) it takes the **persistent-bond graph** written by
//! `BondObservationSystem` and measures its connected components. Each
//! component is a multi-body structure with a definite stoichiometry — an
//! aggregate of the *observed* chemistry. Nothing is programmed per species.
//!
//! Between samples it tracks **lifecycle events**: how many aggregates
//! appeared, disappeared, fused or scissioned.  For each component it also
//! computes the **observed binding energy** (raw unswitched pair potentials
//! summed over its edges) — the energy the physics itself spends to hold it
//! together.  Both are observation lenses, never laws.

use crate::analysis::bond_components;
use crate::components::{AtomType, Bonds, Position};
use crate::config::Config;
use crate::math::Vec3;
use crate::physics::forces::{pair_potential_raw, ElementTable};
use crate::physics::grid::min_image;
use crate::scheduler::{Access, System, SystemContext};
use crate::stats::{ChemicalStructure, CompositionEntry, Reaction};
use std::collections::{HashMap, HashSet};

pub struct BondStructureSystem {
    interval: u64,
    /// (member set, stoichiometry formula) of each aggregate from the previous
    /// sample, for lifecycle tracking.
    prev: Vec<(Vec<u32>, String)>,
}

impl BondStructureSystem {
    pub fn new(interval: u64) -> Self {
        Self {
            interval: interval.max(1),
            prev: Vec::new(),
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
            .reads::<Position>()
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

        // 1. Gather bonded entities, their positions/types and edges.
        let mut entities: Vec<(u32, AtomType)> = Vec::new();
        let mut edges: Vec<(u32, u32)> = Vec::new();
        let mut positions: HashMap<u32, Vec3> = HashMap::new();
        let mut types: HashMap<u32, AtomType> = HashMap::new();
        ctx.world.for_each3::<Bonds, AtomType, Position>(|e, bonds, at, pos| {
            if bonds.neighbors.is_empty() {
                return;
            }
            let idx = e.index();
            entities.push((idx, *at));
            types.insert(idx, *at);
            positions.insert(idx, pos.0);
            for &n in &bonds.neighbors {
                edges.push((idx.min(n.index()), idx.max(n.index())));
            }
        });

        let components = bond_components(&entities, &edges);
        let bound_entities = entities.len();
        let size = cfg.universe.size;
        let tc = cfg.physics.thermal_constant;
        let k_e = cfg.physics.coulomb_constant;
        let with_coulomb = cfg.systems.enable_electrostatics;
        let with_bond = cfg.systems.enable_bond_interaction;
        let mut elements = ElementTable::default_table();
        if let Err(sym) = elements.apply_overrides(&cfg.physics.elements) {
            eprintln!("[genesis] invalid affinity table: {sym}");
            return;
        }

        // 2. Per-component binding energy and per-formula aggregation.
        let mut formula_binding_sum: HashMap<String, (f64, u64)> = HashMap::new();
        let mut largest = 0usize;
        for c in &components {
            largest = largest.max(c.size);
            let binding: f64 = c
                .edges
                .iter()
                .map(|&(a, b)| {
                    let (Some(&pa), Some(&pb), Some(&ta), Some(&tb)) = (
                        positions.get(&a),
                        positions.get(&b),
                        types.get(&a),
                        types.get(&b),
                    ) else {
                        return 0.0;
                    };
                    let r = min_image(pa - pb, size).length().max(1e-9);
                    pair_potential_raw(&elements, tc, ta, tb, r, k_e, with_coulomb, with_bond)
                })
                .sum();
            let entry = formula_binding_sum
                .entry(c.formula.clone())
                .or_insert((0.0, 0));
            entry.0 += binding;
            entry.1 += 1;
        }

        let mut compositions: Vec<CompositionEntry> = formula_binding_sum
            .iter()
            .map(|(formula, &(sum, count))| CompositionEntry {
                formula: formula.clone(),
                count,
                mean_binding: if count > 0 { sum / count as f64 } else { 0.0 },
            })
            .collect();
        compositions.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.formula.cmp(&b.formula))
        });

        // 3. Lifecycle events between this sample and the previous one, with
        // the stoichiometry of every transition (observed reactions).
        let current: Vec<(Vec<u32>, String)> = components
            .iter()
            .map(|c| (c.members.clone(), c.formula.clone()))
            .collect();
        let current_keys: HashSet<Vec<u32>> = current.iter().map(|(m, _)| m.clone()).collect();
        let prev_keys: HashSet<Vec<u32>> = self.prev.iter().map(|(m, _)| m.clone()).collect();
        let appeared = current_keys.difference(&prev_keys).count() as u64;
        let disappeared = prev_keys.difference(&current_keys).count() as u64;

        let formula_of = |key: &Vec<u32>, by_members: &HashMap<Vec<u32>, &str>| -> String {
            by_members.get(key).copied().unwrap_or("?").to_string()
        };
        let current_by_members: HashMap<Vec<u32>, &str> =
            current.iter().map(|(m, f)| (m.clone(), f.as_str())).collect();
        let prev_by_members: HashMap<Vec<u32>, &str> =
            self.prev.iter().map(|(m, f)| (m.clone(), f.as_str())).collect();

        // Fusions: a *new* current component whose member set is the union of
        // two or more components from the previous sample → reaction
        // `reactants -> product`.
        let mut reactions: HashMap<(Vec<String>, Vec<String>), u64> = HashMap::new();
        let mut fusions = 0u64;
        for new_key in current_keys.difference(&prev_keys) {
            let new_set: HashSet<u32> = new_key.iter().copied().collect();
            let contained: Vec<&Vec<u32>> = prev_keys
                .iter()
                .filter(|pk| {
                    let s: HashSet<u32> = pk.iter().copied().collect();
                    s.is_subset(&new_set)
                })
                .collect();
            if contained.len() >= 2 {
                let union: HashSet<u32> =
                    contained.iter().flat_map(|s| s.iter().copied()).collect();
                if union == new_set {
                    fusions += 1;
                    let mut reactants: Vec<String> = contained
                        .iter()
                        .map(|k| formula_of(k, &prev_by_members))
                        .collect();
                    reactants.sort();
                    let product = formula_of(new_key, &current_by_members);
                    *reactions
                        .entry((reactants, vec![product]))
                        .or_insert(0) += 1;
                }
            }
        }

        // Scissions: a *disappeared* previous component whose member set is
        // the union of two or more current components → reaction
        // `reactant -> products`.
        let mut scissions = 0u64;
        for old_key in prev_keys.difference(&current_keys) {
            let old_set: HashSet<u32> = old_key.iter().copied().collect();
            let contained: Vec<&Vec<u32>> = current_keys
                .iter()
                .filter(|ck| {
                    let s: HashSet<u32> = ck.iter().copied().collect();
                    s.is_subset(&old_set)
                })
                .collect();
            if contained.len() >= 2 {
                let union: HashSet<u32> =
                    contained.iter().flat_map(|s| s.iter().copied()).collect();
                if union == old_set {
                    scissions += 1;
                    let reactant = formula_of(old_key, &prev_by_members);
                    let mut products: Vec<String> = contained
                        .iter()
                        .map(|k| formula_of(k, &current_by_members))
                        .collect();
                    products.sort();
                    *reactions
                        .entry((vec![reactant], products))
                        .or_insert(0) += 1;
                }
            }
        }

        let mut reactions_vec: Vec<Reaction> = reactions
            .into_iter()
            .map(|((reactants, products), count)| Reaction {
                reactants,
                products,
                count,
            })
            .collect();
        reactions_vec.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.reactants.join("+").cmp(&b.reactants.join("+")))
        });

        // 4. Write resource and store current state for next sample.
        self.prev = current;
        if let Some(cs) = ctx.resources.get_mut::<ChemicalStructure>() {
            cs.tick = ctx.time.tick;
            cs.aggregates = components.len();
            cs.bound_entities = bound_entities;
            cs.monomers = ctx.world.len().saturating_sub(bound_entities);
            cs.largest = largest;
            cs.compositions = compositions;
            cs.appeared = appeared;
            cs.disappeared = disappeared;
            cs.fusions = fusions;
            cs.scissions = scissions;
            cs.reactions = reactions_vec;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{register_all, Mass, Velocity};
    use crate::ecs::{Resources, World};
    use crate::physics::forces::ElementOverride;
    use crate::rng::Rng;
    use crate::scheduler::SystemContext;
    use crate::stats::StatsCollector;
    use crate::universe::Time;
    use std::collections::HashMap;

    fn two_dimers_and_monomer() -> World {
        register_all();
        let mut w = World::new();
        // Na–O spacing 2.0: near the LJ minimum (σ_mix·2^(1/6) ≈ 2.05), so the
        // pair potential is negative (attractive).
        let data = [
            (AtomType::Sodium, Vec3::new(0.0, 0.0, 0.0)),
            (AtomType::Oxygen, Vec3::new(2.0, 0.0, 0.0)),
            (AtomType::Sodium, Vec3::new(5.0, 0.0, 0.0)),
            (AtomType::Oxygen, Vec3::new(7.0, 0.0, 0.0)),
            (AtomType::Sodium, Vec3::new(10.0, 0.0, 0.0)), // monomer
        ];
        for (at, pos) in &data {
            let e = w.spawn();
            w.insert::<Position>(e, Position(*pos));
            w.insert::<AtomType>(e, *at);
            w.insert::<Mass>(e, Mass(at.mass()));
            w.insert::<Velocity>(e, Velocity(Vec3::ZERO));
            w.insert::<Bonds>(e, Bonds::default());
        }
        let ids: Vec<_> = {
            let mut v = Vec::new();
            w.for_each1::<Bonds>(|e, _| v.push(e));
            v
        };
        // Two dimers: (0,1) and (2,3).
        w.get_mut::<Bonds>(ids[0]).unwrap().neighbors = vec![ids[1]];
        w.get_mut::<Bonds>(ids[1]).unwrap().neighbors = vec![ids[0]];
        w.get_mut::<Bonds>(ids[2]).unwrap().neighbors = vec![ids[3]];
        w.get_mut::<Bonds>(ids[3]).unwrap().neighbors = vec![ids[2]];
        w
    }

    fn fused_ring() -> World {
        register_all();
        let mut w = World::new();
        let data = [
            (AtomType::Sodium, Vec3::new(0.0, 0.0, 0.0)),
            (AtomType::Oxygen, Vec3::new(1.2, 0.0, 0.0)),
            (AtomType::Sodium, Vec3::new(5.0, 0.0, 0.0)),
            (AtomType::Oxygen, Vec3::new(6.2, 0.0, 0.0)),
            (AtomType::Sodium, Vec3::new(10.0, 0.0, 0.0)), // monomer
        ];
        for (at, pos) in &data {
            let e = w.spawn();
            w.insert::<Position>(e, Position(*pos));
            w.insert::<AtomType>(e, *at);
            w.insert::<Mass>(e, Mass(at.mass()));
            w.insert::<Velocity>(e, Velocity(Vec3::ZERO));
            w.insert::<Bonds>(e, Bonds::default());
        }
        let ids: Vec<_> = {
            let mut v = Vec::new();
            w.for_each1::<Bonds>(|e, _| v.push(e));
            v
        };
        // Ring: 0–1–2–3–0.
        w.get_mut::<Bonds>(ids[0]).unwrap().neighbors = vec![ids[1], ids[3]];
        w.get_mut::<Bonds>(ids[1]).unwrap().neighbors = vec![ids[0], ids[2]];
        w.get_mut::<Bonds>(ids[2]).unwrap().neighbors = vec![ids[1], ids[3]];
        w.get_mut::<Bonds>(ids[3]).unwrap().neighbors = vec![ids[2], ids[0]];
        w
    }

    fn drive(
        sys: &mut BondStructureSystem,
        world: &mut World,
        resources: &mut Resources,
        rng: &mut Rng,
        time: &mut Time,
        stats: &mut StatsCollector,
        tick: u64,
    ) {
        while time.tick < tick {
            time.advance();
        }
        let mut ctx = SystemContext {
            world,
            resources,
            rng,
            time,
            stats,
            dt: time.dt,
        };
        sys.run(&mut ctx);
    }

    #[test]
    fn first_sample_records_new_aggregates_with_negative_binding() {
        let mut world = two_dimers_and_monomer();
        let mut cfg = Config::default_config();
        cfg.systems.enable_bond_observation = true;
        cfg.systems.enable_electrostatics = true;
        let mut resources = Resources::new();
        resources.insert(cfg);
        resources.insert(ChemicalStructure::default());
        let mut rng = Rng::new(7);
        let mut time = Time::new(0.0167);
        let mut stats = StatsCollector::new(16);
        let mut sys = BondStructureSystem::new(1);

        drive(&mut sys, &mut world, &mut resources, &mut rng, &mut time, &mut stats, 1);
        let cs = resources.get::<ChemicalStructure>().unwrap();
        assert_eq!(cs.aggregates, 2);
        assert_eq!(cs.monomers, 1);
        assert_eq!(cs.largest, 2);
        assert_eq!(cs.appeared, 2, "both dimers are new");
        assert_eq!(cs.disappeared, 0);
        assert_eq!(cs.fusions, 0);
        // Na-O dimer: LJ min + Coulomb (attractive) → binding < 0.
        let na_o = cs.compositions.iter().find(|c| c.formula == "Na-O").unwrap();
        assert_eq!(na_o.count, 2);
        assert!(
            na_o.mean_binding < 0.0,
            "Na-O binding must be negative (attractive): {}",
            na_o.mean_binding
        );
    }

    #[test]
    fn affinity_table_tunes_binding_energy() {
        // Na–O at 2.0 with electrostatics: LJ + Coulomb attract. Neutralizing
        // both species through the affinity table removes the Coulomb term, so
        // the pair becomes less bound — the table tunes "materials".
        let (tc, k_e) = (0.01, 1.0);
        let (with_coulomb, with_bond) = (true, false);
        let r = 2.0;

        let default = ElementTable::default_table();
        let v_with_charge = pair_potential_raw(
            &default,
            tc,
            AtomType::Sodium,
            AtomType::Oxygen,
            r,
            k_e,
            with_coulomb,
            with_bond,
        );

        let mut neutral = ElementTable::default_table();
        let mut overrides = HashMap::new();
        for (sym, chg) in [("Na", 0.0), ("O", 0.0)] {
            overrides.insert(
                sym.to_string(),
                ElementOverride {
                    sigma: None,
                    epsilon_k: None,
                    charge: Some(chg),
                },
            );
        }
        neutral.apply_overrides(&overrides).unwrap();
        let v_neutral = pair_potential_raw(
            &neutral,
            tc,
            AtomType::Sodium,
            AtomType::Oxygen,
            r,
            k_e,
            with_coulomb,
            with_bond,
        );

        assert!(
            v_with_charge < v_neutral,
            "Coulomb attraction deepens the well: charged {v_with_charge} vs neutral {v_neutral}"
        );
        // The neutral LJ term alone is still attractive at r=2.0.
        assert!(v_neutral < 0.0);
    }

    #[test]
    fn fusion_from_dimers_to_ring() {
        let mut world = two_dimers_and_monomer();
        let mut cfg = Config::default_config();
        cfg.systems.enable_bond_observation = true;
        let mut resources = Resources::new();
        resources.insert(cfg);
        resources.insert(ChemicalStructure::default());
        let mut rng = Rng::new(7);
        let mut time = Time::new(0.0167);
        let mut stats = StatsCollector::new(16);
        let mut sys = BondStructureSystem::new(1);

        // Sample 1: two dimers.
        drive(&mut sys, &mut world, &mut resources, &mut rng, &mut time, &mut stats, 1);
        let cs = resources.get::<ChemicalStructure>().unwrap();
        assert_eq!(cs.appeared, 2);

        // Replace world with a fused ring and sample again.
        world = fused_ring();
        drive(&mut sys, &mut world, &mut resources, &mut rng, &mut time, &mut stats, 2);
        let cs = resources.get::<ChemicalStructure>().unwrap();
        assert_eq!(cs.aggregates, 1);
        assert_eq!(cs.disappeared, 2, "both dimers are gone");
        assert_eq!(cs.appeared, 1, "the ring is new");
        assert_eq!(cs.fusions, 1, "dimers fused into the ring");
        assert_eq!(
            cs.reactions,
            vec![Reaction {
                reactants: vec!["Na-O".into(), "Na-O".into()],
                products: vec!["Na2-O2".into()],
                count: 1,
            }],
            "the fusion is reported as an observed reaction"
        );
    }

    #[test]
    fn scission_from_ring_to_dimers() {
        let mut world = fused_ring();
        let mut cfg = Config::default_config();
        cfg.systems.enable_bond_observation = true;
        let mut resources = Resources::new();
        resources.insert(cfg);
        resources.insert(ChemicalStructure::default());
        let mut rng = Rng::new(7);
        let mut time = Time::new(0.0167);
        let mut stats = StatsCollector::new(16);
        let mut sys = BondStructureSystem::new(1);

        // Sample 1: the fused ring.
        drive(&mut sys, &mut world, &mut resources, &mut rng, &mut time, &mut stats, 1);
        let cs = resources.get::<ChemicalStructure>().unwrap();
        assert_eq!(cs.aggregates, 1);

        // Replace world with two dimers and sample again.
        world = two_dimers_and_monomer();
        drive(&mut sys, &mut world, &mut resources, &mut rng, &mut time, &mut stats, 2);
        let cs = resources.get::<ChemicalStructure>().unwrap();
        assert_eq!(cs.aggregates, 2);
        assert_eq!(cs.disappeared, 1, "the ring is gone");
        assert_eq!(cs.appeared, 2, "both dimers are new");
        assert_eq!(cs.scissions, 1, "ring split into dimers");
        assert_eq!(
            cs.reactions,
            vec![Reaction {
                reactants: vec!["Na2-O2".into()],
                products: vec!["Na-O".into(), "Na-O".into()],
                count: 1,
            }],
            "the scission is reported as an observed reaction"
        );
    }
}
