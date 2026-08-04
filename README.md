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
     For large populations the CLI's final g(r) uses a deterministic
     even-spaced subsample, so the O(n²) worst case (few cells when
     `r_max ≈ L/2`) is bounded.
- ✅ **Berendsen thermostat** (velocity rescaling, opt-in via
     `[systems].enable_thermostat`): drives the equipartition temperature to
     `physics.thermostat_temperature` for NVT runs. It is an instrument, not a
     law — NVE (energy conservation) remains the default.
- ✅ **Temperature sweep** (`examples/temperature_sweep.rs`): equilibrates at
     fixed T and measures structure. Observed: the mixture condenses gradually
     over ~20–120 K (deep wells C/O/N bind first, shallow H/He evaporate
     first) — an emergent consequence of the heterogeneous ε wells.
- ✅ **Parallel forces** (rayon): the LJ force pass accumulates per-particle
     forces in parallel (~3× on 8 cores) with deterministic collection order.
     Each pair is evaluated **once** and contributes ±F to its two ends
     (Newton's 3rd law, momentum conserved exactly; the potential is counted
     once, no halving). The result is bit-identical across thread counts.
- ✅ **Bond observation** (`[systems].enable_bond_observation`, emergent):
     `BondObservationSystem` tracks bound pairs (r < 1.5·σ) with a debounce,
     marks a pair **persistent** after `bond_min_periods` of its own
     vibrational period and writes the `Bonds` component. Statistics per tick:
     bonded pairs/entities, mean coordination, a per-species **bond matrix**
     (`bond_species` in the CSV) and closed **bond lifetimes** (formed bonds +
     mean lifetime, in `status_line` and CSV).
- ✅ **Observed bond as an interaction** (`[systems].enable_bond_interaction`,
     opt-in): persistent pairs act as a **harmonic spring** toward the LJ well
     minimum `σ·2^(1/6)` with the well curvature as k, smooth-switched off at
     the binding radius `bond_k_bind·σ` so the force stays continuous. Its
     energy is the exact derivative of its switched potential (verified), so
     NVE stays conserved with bonds on. Bonds are still never programmed: only
     pairs that observation already declared persistent feel the spring.
- ✅ **10 elements** (H, He, C, N, O, Na, Si, P, S, Fe) with LJ parameters in
     `src/physics/forces.rs`, mass and symbols in `src/components/atom_type.rs`.
     Charges follow ionization trends (O −1, Na +1, Si +0.5, metals +1): a
     `config/demo-nacl.toml` composes Na⁺/O⁻ to observe ionic aggregates.
- ✅ **Observability exports** (`src/export/`): metrics CSV (`[stats].csv_path`,
     appended every `csv_interval` ticks) and position frames in XYZ
     (`[stats].xyz_prefix`, one `frame_{tick:08}.xyz` per `xyz_interval` ticks)
     to plot outside the engine. `tools/plot_stats.py data/stats.csv` plots the
     trajectory; `examples/observe_quench.rs` is a reproducible NVT quench that
     writes both.
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
├── export/              # CSV + XYZ exports (observability, I/O only)
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
