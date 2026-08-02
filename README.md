# GENESIS

> Simulation engine of a universe. **Not** a chemistry simulator. **Not** a
> biology simulator. An engine whose goal is to program only the fundamental
> rules and observe whether complexity emerges spontaneously.

The only question it tries to answer:

> What is the minimal set of laws needed for complexity to appear without
> being programmed?

If someday something life-like appears, it must be a consequence of the rules,
**never** a feature implemented explicitly. This README documents the
architecture decisions of stage 0 (the foundation).

---

## Current state

- ✅ Complete bespoke ECS (archetypes, generations, parallel queries).
- ✅ Universe (time, serializable RNG, resources, statistics).
- ✅ Explicit scheduler with stage/conflict analysis.
- ✅ 100% TOML configuration, outside the code.
- ✅ Binary persistence: save and resume identical universes.
- ✅ Base components (position, velocity, mass, charge, atomic type).
- ✅ **Elastic collision** between particles (impulse; conserves momentum and
     energy per pair), with a uniform **spatial grid** as broad-phase.
- ✅ **Lennard-Jones forces** between particles (atom of each element with
     condensed-phase σ and ε): r⁻¹² repulsion + r⁻⁶ attraction, with
     Lorentz–Berthelot mixing, quintic *switch* at 0.9·r_c and hardened core.
- ✅ **Velocity Verlet integrator** (symplectic): conserves total energy
     (kinetic + potential) at the level of the numerical scheme.
- ✅ **Derived energy, temperature and potential** (not stored as components):
     `E = K + V`, temperature by equipartition.
- ✅ **Cubic lattice seeding** + thermal jitter when there are forces (molecular
     dynamics standard): a random seeding overlaps nuclei and the r⁻¹²
     repulsion explodes numerically.
- ✅ Observed: at low temperature the gas **condenses** spontaneously (V turns
     very negative, the system self-heats with latent heat).
- ✅ **Emergent structure analysis** (observation, not laws): g(r) with ideal-gas
     normalization and aggregate detection with friends-of-friends
     (`src/analysis/`). `StructureSystem` samples every `stats.structure_interval`
     ticks and the snapshot reports aggregates/monomers/largest/bound pairs.
- ✅ **Berendsen thermostat** (velocity rescaling, opt-in via
     `[systems].enable_thermostat`): drives the equipartition temperature to
     `physics.thermostat_temperature` for NVT runs. It is an instrument, not a
     law — NVE (energy conservation) remains the default.
- ✅ **Temperature sweep** (`examples/temperature_sweep.rs`): equilibrates at
     fixed T and measures structure. Observed: the mixture condenses gradually
     over ~20–120 K (deep wells C/O/N bind first, shallow H/He evaporate
     first) — an emergent consequence of the heterogeneous ε wells.
- ⏳ Visualization: **outside the engine** (console only today).

```
cargo run --release [config.toml] [ticks] [report_every]
cargo test
```

---

## Architecture decisions (and why)

### 1. Bespoke ECS, not a library

`bevy_ecs`/`specs` bring decades of design, but couple the project to a foreign
type system, with hard compile errors and a costly migration. A bespoke ECS
(~600 lines) gives full control, zero fragile dependencies and the freedom to
evolve for years. All unsafe code is **absent**: downcasts are validated
against a global type registry.

### 2. Archetypes (not loose sparse-sets)

All atoms share the same component set → they live in contiguous SoA arrays
(`Vec<T>` per component). This guarantees:

- **Cache locality**: iterating 10M positions walks contiguous memory.
- **Natural alignment**: row `i` is the same entity in every column, which
  allows parallelizing by chunks without misalignment risk.
- **Heterogeneous growth**: when photons, molecules or organisms with distinct
  sets appear, each type will live in its own archetype. There is no cost for
  "empty" entities.

### 3. Generational `EntityId`

Ids are never reused without incrementing the generation. A "zombie" handle
(saved weeks ago, or from a `Bonds` pointing to a destroyed entity) returns
`None` instead of reading corrupt data. It is the difference between a subtle
crash and a simulation that degrades gracefully.

### 4. Components are pure data

`Component` only requires `Send + Sync + 'static` and a stable `ComponentId`.
No mandatory `Clone`, no inheritance, no logic. Entities **do not** have
behavior methods: behavior lives in the `System`s.

### 5. Systems declare their access

Each `System` declares which components/resources it reads and writes. The
scheduler:

- runs in **explicit registration order** (nothing hidden),
- computes **conflict-free stages** (which today run sequentially and in the
  future will be parallelized with disjoint borrows),
- exposes the plan for inspection and tests.

The real parallelism of today lives **inside** each system: `par_for_each*_mut`
queries with rayon, exactly where the bottleneck occurs in particle
simulation.

### 6. The `Universe` is the only facade

It holds everything: time, RNG, `World`, resources, scheduler, statistics and
configuration. The public API is tiny (`new`, `tick`, `run_ticks`, `save`,
`load`). Systems **never** see the full `Universe`: only a bounded
`SystemContext`, which prevents accidental coupling.

### 7. Persistence is a total snapshot

It saves configuration + clock + **RNG state** + statistics + all entities
with their ids byte by byte. A saved universe resumes with the same random
sequence and the same handles. Binary format (`bincode`).

### 8. The configuration is the "big bang"

The only moments the code creates matter are: (a) the initial seeding
according to `config/universe.toml`, and (b) the restoration of snapshots.
From there on, **everything** must emerge from the laws.

### 9. Physics/chemistry as delimited spaces

`physics/` already contains real laws (elastic collision, spatial grid);
`chemistry/` documents the golden rule: no physical law can depend on
chemistry; no "reaction" is programmed as such. They are the boundary that
protects the project's question.

---

## Structure

```
src/
├── main.rs              # CLI: config + ticks + persistence demo
├── lib.rs
├── universe/            # Universe (facade), Time, initial seeding
├── ecs/                 # entity, component, archetype, world, resource
├── components/          # single catalog + registry (for_each_component! macro)
├── systems/             # laws: movement, boundaries, collisions, forces, integrate
├── scheduler/           # System trait, Access, Scheduler, stages
├── config/              # typed Config (TOML)
├── serialization/       # total snapshot (bincode)
├── stats/               # metrics + history
├── analysis/            # lenses: g(r) and aggregates (observation, not laws)
├── math/                # Vec3
├── rng/                 # serializable RNG (xoshiro256++)
├── physics/             # laws: elastic collision, LJ (tables and switch), grid
├── chemistry/           # reserved: documents the prohibition
```

---

## Adding a new component (1 minute)

1. Create `src/components/<new>.rs` with `impl Component for New { const ID }`.
2. Add it to the `for_each_component!` macro in `src/components/mod.rs`.
   - Serialization, registry and snapshots are generated automatically.

> ⚠️ The `ComponentId`s are permanent: never reassign or reuse them.

## Adding a new law

1. Implement `System` (name + `access()` + `run`).
2. Register it in `build_schedule` (`src/universe/mod.rs`), enable-able from
   `[systems]` in TOML.

---

## Honest limitations

- The scheduler computes parallel stages but today runs sequentially; global
  parallelization requires a *disjoint borrows* abstraction that will be added
  when there is more than one heavy compute system.
- The internal forces are **Lennard-Jones spheres** without identity
  conservation: a high-speed impact between two atoms repels them, but there
  is no "breaking a bond" because bonds are not represented (yet).
- The force broad-phase uses the same uniform grid as collisions; at extreme
  densities the cutoff factor could be readjusted.
- Elastic collisions and forces coexist independently (enable-able separately
  in `[systems]`); there is not yet a unified formalism for both.
- The parallel iteration of two components assumes homogeneous sets per
  archetype (guaranteed by design: within an archetype all rows are aligned).
- The memory metrics are approximations of the ECS itself (capacities).
