# Research task: making avian3d physics cheap for a headless village simulation

## What I'm building

A village sim in Rust with **Bevy 0.19** and **avian3d 0.7.0**. Characters chop trees, drag the logs home, and the forest regrows. I need it to run *headless* (no window, no GPU) as fast as possible for two reasons: integration tests that simulate 45–120 s of game time, and in-game fast-forward. Determinism matters: the same seed and settings must reproduce the same world, bit for bit, run after run (a test asserts this).

The physics is deliberately simple — it's a 2D problem being solved with a 3D engine:

- Gravity is zero. Every body is locked to the XZ plane (`LockedAxes`: translation Y, rotation X and Z locked). Heights are set by gameplay code, not physics.
- **Characters**: `RigidBody::Dynamic`, capsule collider (r 0.4, h 1.0), `Mass(80)`, rotation fully locked. Velocity is set directly by a steering system every frame; physics only resolves overlap between characters and between characters and trees.
- **Standing trees**: `RigidBody::Static`, capsule collider (trunk radius 0.25). They grow: a gameplay system writes `Transform.scale` every frame on every growing tree (maturity 0→1 over a growth time), so the collider scales with it.
- **Falling trees**: switched to `RigidBody::Kinematic` for ~1 s while a gameplay system animates the fall.
- **Fallen logs**: `Static`, on a `Log` collision layer that collides with nothing (characters climb over them). Only found via spatial queries.
- **Carried logs**: `Dynamic`, on a `Carried` layer that collides with nothing, attached to the character by a `DistanceJoint` (a slack rope, `xpbd_joints` feature).
- Collision layers: `Character` ↔ {`Character`, `Obstacle`}; `Obstacle` ↔ `Character`; `Log`, `Carried` ↔ nothing.
- Spatial queries per character per frame: one `cast_shape` (character capsule, 2.5 m lookahead) for steering, one `shape_intersections` against the `Log` layer to detect standing on a log. Steering and target selection are gameplay code, not physics.

avian3d Cargo features: `3d, f32, parry-f32, debug-plugin, xpbd_joints, collider-from-mesh, bevy_scene, bevy_picking`; `default-features = false`; the `parallel` feature is **off** for run-to-run determinism. `PhysicsPlugins::default()` is used as-is. `Time<Fixed>` is 64 Hz; `SubstepCount` is the default 6; gameplay runs at a fixed 1/60 s per frame in headless (`TimeUpdateStrategy::ManualDuration`), so ~1.07 physics steps per frame.

Headless app: `MinimalPlugins` + `LogPlugin` + `TransformPlugin` + `AssetPlugin`, all schedules switched to Bevy's `SingleThreadedExecutor` (the multithreaded executor's sync overhead roughly halved throughput for these tiny systems).

## What I've measured

Release build, one core effectively. Per-frame main-thread cost by phase, from Bevy `trace_chrome` spans, for four maps at 10× entity steps (trees : characters = 10 : 1, constant density, trees 3 m apart on a grid):

| phase (ms/frame) | 10 trees / 1 char | 100 / 10 | 1 000 / 100 | 10 000 / 1 000 |
|---|---:|---:|---:|---:|
| Gameplay: characters (steering casts, target choice, log detection) | 0.007 | 0.012 | 0.080 | 1.263 |
| Gameplay: trees (growth, seeding) | 0.009 | 0.010 | 0.027 | 0.195 |
| PostUpdate transform propagation | 0.139 | 0.174 | 0.208 | 0.501 |
| Physics: `update_collider_scale` → `update_collider_mass_properties` → `update_mass_properties` | 0.033 | 0.133 | 0.757 | **8.062** |
| Physics: collider tree + broad phase | 0.077 | 0.115 | 0.157 | 0.442 |
| Physics: narrow phase | 0.007 | 0.010 | 0.045 | 0.563 |
| Physics: substep solver (6 substeps × 23 systems) | 0.388 | 0.585 | 0.799 | 1.110 |
| Physics: `transform_to_position` / `position_to_transform` | 0.196 | 0.215 | 0.300 | 1.054 |
| Physics: other (islands, solver bodies, spatial query pipeline, executor) | 0.928 | 0.860 | 0.931 | 1.430 |
| Schedule executor + rest | 0.208 | 0.182 | 0.182 | 0.284 |
| **Total, traced** | 2.0 | 2.3 | 3.5 | 14.9 |
| **Total, untraced (real)** | **0.31** | **0.55** | **1.26** | **9.26** |
| realtime factor at 1/60 s per frame | 53× | 31× | 13× | 1.8× |

Notes on reading it: tracing adds ~1–2 µs per span, and there are ~1 000 spans per frame, so ~1.7 ms of the traced totals is overhead, concentrated in the rows made of many tiny systems (substep solver, "other", executor). The entity-proportional rows are accurate to ~10 %.

System counts in the headless app: 172 total; per physics step ~60 in `PhysicsSchedule` + 23 × 6 in `SubstepSchedule` + ~35 across `FixedFirst/FixedPostUpdate/FixedLast`. Per frame outside physics: ~35.

Two conclusions so far:

1. **Small maps are bounded by a fixed per-step floor of ~0.3 ms**, almost all of it avian's per-step systems doing nothing over ~30 bodies. That caps a small map at ~50× realtime regardless of how little work there is. `SubstepCount(1)` instead of 6 only bought ~20 % on the small map and nothing on the large one, and it changes results.
2. **Large maps are dominated by mass-property recomputation** triggered by the per-frame `Transform.scale` writes on growing trees — for `Static` bodies, whose mass is never used. 54 % of the 10k-tree frame.

## Already established from the avian 0.7.0 source (don't re-research)

- **Mass recompute chain.** `update_collider_scale` (`collision/collider/backend.rs:460`) runs on `Changed<Transform>` and calls `Collider::set_scale` when scale differs → `update_collider_mass_properties` (`backend.rs:498`) recomputes on `Changed<Collider>` (only `Sensor` is exempt) → `queue_mass_recomputation_on_collider_mass_change` (`dynamics/rigid_body/mass_properties/mod.rs:380`) inserts a `RecomputeMassProperties` marker via commands → `update_mass_properties` (`mod.rs:398`) traverses descendants and removes the marker via commands. Two archetype moves + a shape mass calc per growing tree per step. Nothing checks `RigidBody::Static`. `NoAutoMass`/`NoAutoAngularInertia`/`NoAutoCenterOfMass` do **not** skip the traversal (`system_param.rs:60` computes totals first, then picks values) — they are not an opt-out. Real options: write `Transform.scale` less often (quantised growth), or `PhysicsTransformConfig.transform_to_collider_scale = false` (`physics_transform/mod.rs:153`) and resize colliders explicitly at a few stages.
- **Static bodies are not free.** `transform_to_position` (`physics_transform/mod.rs:187`) iterates every `Position` entity each step, calls `GlobalTransform::compute_transform()` and compares with tolerance — no `Changed` filter. `position_to_transform` is `Changed`-filtered. `update_moved_collider_aabbs` is change-filtered and honours `ColliderDisabled`. `PhysicsTransformConfig.propagate_before_physics` (default `true`) runs a full transform propagation before every physics step; only needed for child colliders, which this game has none of.
- **Plugin group** (`lib.rs:757`): schedule, mass properties, forces, collider hierarchy/transform/cache/backend, collider tree, narrow phase, `SolverPlugins` (solver bodies, integrator, solver, `CcdPlugin`, `IslandPlugin`, `IslandSleepingPlugin`, `JointGraphPlugin` × Fixed/Revolute/Prismatic/Distance/Spherical, `XpbdSolverPlugin`), broad phase, joints, spatial query, physics transform, `PhysicsInterpolationPlugin`. It is a `PluginGroup`, so individual plugins can be `.disable::<…>()`d.
- **`parallel` feature** = `["bevy/multi_threaded", "parry3d/parallel"]` (`Cargo.toml.orig:32`). Bevy's `multi_threaded` is already on via the game's own bevy dependency, and avian's integrator / solver-body systems use `par_iter_mut` unconditionally (`dynamics/integrator/mod.rs:278…`). The feature only gates `utils::par_for_each` (`utils.rs:59`): parallel narrow-phase contact computation and the per-graph-colour constraint loops in `dynamics/solver/plugin.rs:388,476,564,662`. Both write per-item slots and colours share no bodies, so it is designed to be deterministic — unverified. Thresholds (`min_len` 64 constraints / 2 colours) mean it can't help the small map.
- **Spatial queries**: `SpatialQueryPlugin` (`spatial_query/mod.rs:206`) only drives `RayCaster`/`ShapeCaster` components; the `SpatialQuery` param reads the collider BVH directly, no per-step pipeline rebuild.
- **Profiling**: `PhysicsDiagnosticsPlugin` (`diagnostics/mod.rs:105`) writes per-phase physics timings to `DiagnosticsStore`; off by default.

## What I want from this research

Concrete, avian 0.7 / Bevy 0.19-accurate answers, with references to the actual API, source or docs where possible. Rank recommendations by expected payoff for (a) the small-map floor and (b) the large-map scaling, and state what each one costs in determinism or behaviour.

1. **Mass recompute (see above).** Given the chain is known: is quantised `Transform.scale` writes or `transform_to_collider_scale = false` + explicit collider resizing the better fix, and is there any avian 0.7 mechanism I missed that skips mass work for `Static` bodies?
2. **Reducing the per-step floor.** Of the plugins listed above, which are safe to disable for this game (CCD, interpolation, sleeping, unused joint graphs?) and how much of the ~0.3 ms floor do they account for? Can `PhysicsSchedule` / `SubstepSchedule` run with fewer systems, or be skipped when no dynamic body moved?
3. **`transform_to_position = false`.** The game writes `Transform` directly for falling trees (kinematic animation), character heading (rotation, with rotation locked in physics), and in tests. If I turn off `transform_to_position` to stop the per-step scan of all 11k bodies, what must be written to `Position`/`Rotation` instead, and how does that interact with `position_to_transform` overwriting `Transform.rotation` for rotation-locked bodies? Is `propagate_before_physics = false` safe with no child colliders? Is there a cheaper static-only path (`ColliderDisabled`, a separate static tree, `RigidBodyDisabled`) that still lets characters collide with and spatially query trees?
4. **Substeps and step rate.** Given the constraints above (only overlap resolution + one distance joint per hauling character), what is the lowest safe `SubstepCount` and physics rate, and how would I validate "safe" (tunneling, joint stretch, jitter metrics)? Is there any way to make substep count *not* change results, or is that inherent?
5. **Determinism vs. parallelism.** Given the analysis above, is avian's `parallel` feature deterministic in practice (fixed thread count, per-slot writes, graph colouring)? Any known counter-examples or issues? Is there a cheap way to *prove* it for this game beyond running the same-seed test repeatedly?
6. **Right-sizing the engine.** This is a planar simulation. Would `avian2d` on the XZ plane be materially cheaper (smaller solver, 2D BVH, 2D narrow phase) and is the migration straightforward for capsules + distance joints? Alternatively, is there a credible case for replacing avian for the *characters vs. trees* interaction with a hand-rolled spatial hash + circle push-out (trees are just circles on a plane) while keeping avian only for the rope/joint, or dropping it entirely? What do comparable Bevy sims (Factorio-like fast-forward, headless RTS tests) do?
7. **Spatial queries.** Per character per frame I do a `cast_shape` and a `shape_intersections`. Are there cheaper avian 0.7 APIs for "is anything within r ahead of me on layers X" and "am I overlapping layer Y" (`aabb_intersections_with_aabb`, point queries, ray casts instead of shape casts)?
8. **Bevy-side overhead.** Ways to cut scheduler cost per tick in Bevy 0.19 for many tiny systems: system run-condition costs, `apply_deferred` points, `SingleThreadedExecutor` vs `SimpleExecutor`, merging schedules, or driving avian's schedules manually from one system. Is `Schedule::set_executor` on every schedule the right knob?
9. **Profiling.** How to read `PhysicsDiagnosticsPlugin` output headless (which diagnostic paths exist in 0.7, how to dump them to stdout without the UI plugin), and whether Tracy (`bevy/trace_tracy`) is materially lower-overhead than `trace_chrome` for ~1 000 spans/frame.

Please be specific about version: avian's API moved a lot between 0.2 and 0.7 (e.g. `PhysicsSet` → `PhysicsSystems`, `SubstepSolverSet` → `SubstepSolverSystems`, the constraint-graph/island solver in 0.4+), and I need answers that match 0.7.0 and Bevy 0.19, not older tutorials.
