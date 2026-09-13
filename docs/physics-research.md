# Physics performance research — findings

> **Update — gameplay moved to the fixed timestep.** See the last section; it supersedes the
> "Q3" and "Q4" notes below where they talk about `transform_to_position` and per-mode rates.

Answers to the questions in `physics-research-prompt.md`, from the avian3d 0.7.0 source
(`~/.cargo/registry/src/*/avian3d-0.7.0/src/…`, cited as `file:line`), the Bevy 0.19.1 source,
the avian release posts, and experiments on this repo (release build, headless, one map per column;
`empty` = no bodies, `m10` = 10 trees / 1 character, `m1000` = 1 000 / 100, `m10000` = 10 000 / 1 000).

## TL;DR — ranked by payoff

| # | Change | Small map (floor) | 10k map | Changes results? | Effort |
|---|---|---|---|---|---|
| 1 | Quantise tree growth so `Transform.scale` is written ~100× per tree instead of 5 400× | — | **−8 ms/frame (≈ −55 %)** | negligible (visual steps of ~1 %) | small |
| 2 | Disable `CcdPlugin`, `PhysicsInterpolationPlugin`, `IslandSleepingPlugin`; `propagate_before_physics = false` | −0.05 ms (−20 %) | ~0 | no (nothing uses them) | trivial |
| 3 | `SubstepCount(1)` (or 2) in headless | −0.07 ms (−30 %) | −1.4 ms | **yes** — a different trajectory | trivial |
| 4 | Hand-rolled circle push-out on a spatial hash instead of avian for characters/trees | floor → ~0.05 ms (≈ 300×) | est. ~1–2 ms | yes — new solver | large |
| 5 | `transform_to_position = false` | — | −1 ms | needs gameplay to write `Position`/`Rotation` | medium; **breaks as-is** |
| — | avian's `parallel` feature | none (thresholds never met) | maybe narrow phase | maintainers say "disable for strict determinism" | — |
| — | `avian2d` | none (same system count) | maybe −30 % per-body | requires XY-plane world or custom sync | large |

Measured stack (ms/frame, ±10 % run-to-run noise):

| variant | empty | m10 | m1000 | m10000 |
|---|---:|---:|---:|---:|
| baseline (today) | 0.132 | 0.305 | 1.261 | 10.43 |
| − CCD, interpolation, sleeping | 0.113 | 0.282 | 1.179 | 10.77 |
| + `propagate_before_physics = false` | 0.086 | 0.243 | 1.236 | 10.54 |
| + `SubstepCount(1)` | 0.075 | 0.169 | 0.821 | 9.17 |
| + `transform_to_position = false` | 0.072 | 0.188 | 14.1 ✗ | 1 842 ✗ |

The last row is broken, not faster (3 tests fail; bodies never get their spawn position, everything
overlaps at the origin). Growth quantisation (#1) wasn't in this stack; it's independent of the rest.

The floor with zero bodies is **43 µs per frame + 91 µs per physics step** ≈ 0.38 µs per system
over the ~240 systems a physics step runs. That is the whole reason a small map tops out near 50×.

---

## Q1 — Mass recomputation from tree growth

**Chain** (every physics step, per tree whose `Transform` changed):

1. `update_collider_scale` — `collision/collider/backend.rs:460`. Query filter `Or<(Changed<Transform>, Changed<C>)>`; if `transform.scale != collider.scale()` → `collider.set_scale(scale, 10)`. Gated by `PhysicsTransformConfig.transform_to_collider_scale` (`physics_transform/mod.rs:153`).
2. `update_collider_mass_properties` — `backend.rs:498`. `Changed<Collider>` → `ColliderMassProperties::from(collider.mass_properties(density))`. Only `Sensor` is exempt.
3. `queue_mass_recomputation_on_collider_mass_change` — `dynamics/rigid_body/mass_properties/mod.rs:380`. `Changed<ColliderMassProperties>` → `commands.insert(RecomputeMassProperties)` (archetype move).
4. `update_mass_properties` — `mod.rs:398`. Runs `MassPropertyHelper::total_mass_properties` (descendant traversal, `system_param.rs:60`), then `commands.remove::<RecomputeMassProperties>()` (second archetype move).

Nothing in the chain checks `RigidBody::Static`. `NoAutoMass`/`NoAutoAngularInertia`/`NoAutoCenterOfMass` do **not** short-circuit it: `system_param.rs:60–125` computes the totals first and the markers only decide which values are stored. Uniform scale keeps a capsule a capsule; non-uniform scale would turn it into a convex approximation (`set_scale(_, 10)` subdivisions) — keep scale uniform.

**Options**

- **Quantise the writes** (recommended). `GROW_TIME` is 90 s → 5 400 `Transform.scale` writes per tree. Writing only when `maturity` crosses a 1/100 step cuts the chain 54×; at 1 % scale steps the growth is visually continuous. No physics config change, nothing else affected.
- `transform_to_collider_scale = false` and resize the collider explicitly at a few stages (`Collider::capsule(r·s, l·s)` replacement, or `collider.set_scale`). Also removes the per-step `Changed<Transform>` scan of the collider-scale system. Requires `tree_base()` to stop reading `transform.scale.y` if the visual keeps growing on a child.
- Sensor trees would skip step 2 but sensors don't collide — not an option.

## Q2 — The per-step floor

`PhysicsPlugins::default()` (`lib.rs:757`): `PhysicsSchedulePlugin`, `MassPropertyPlugin`, `ForcePlugin`, `ColliderHierarchyPlugin`, `ColliderTransformPlugin`, `ColliderCachePlugin`, `ColliderBackendPlugin`, `ColliderTreePlugin`, `NarrowPhasePlugin`, `SolverPlugins` = { `SolverBodyPlugin`, `SolverSchedulePlugin`, `IntegratorPlugin`, `SolverPlugin`, `CcdPlugin`, `IslandPlugin`, `IslandSleepingPlugin`, `JointGraphPlugin<Fixed|Revolute|Prismatic|Distance|Spherical>`, `XpbdSolverPlugin` } (`dynamics/solver/mod.rs:61`), `BroadPhaseCorePlugin`, `BvhBroadPhasePlugin`, `JointPlugin`, `SpatialQueryPlugin`, `PhysicsTransformPlugin`, `PhysicsInterpolationPlugin`.

It's a `PluginGroup`, so:

```rust
PhysicsPlugins::default()
    .build()
    .disable::<CcdPlugin>()
    .disable::<PhysicsInterpolationPlugin>()
    .disable::<IslandSleepingPlugin>()
```

compiles and passes the suite. CCD adds `solve_swept_ccd` per step (`dynamics/ccd/mod.rs:264`) — nothing here moves fast enough to need it. Interpolation is only meaningful with a renderer. Sleeping never triggers (characters never rest) and adds observers plus per-step island checks (`islands/sleeping.rs:46–81`). Together: ~15 % of the zero-body floor.

`propagate_before_physics` (`physics_transform/mod.rs:95–106`) runs `mark_dirty_trees` + `propagate_parent_transforms` + `sync_simple_transforms` inside every physics step, on top of Bevy's own `PostUpdate` propagation. It exists so child colliders see up-to-date `GlobalTransform`; this game has no child colliders. Off: another ~25 % of the floor, suite passes.

Skipping steps entirely: `run_physics_schedule` (`schedule/mod.rs:235`) doesn't run the schedule when `Time<Physics>` is paused or the delta is zero, so `Time<Physics>::pause()` is the only "skip" switch. There is no "nothing moved" short-circuit, and this game always has moving characters, so it doesn't apply.

The joint-graph plugins for unused joint types register systems that iterate empty queries; they're on the order of a µs each and not worth removing individually.

## Q3 — Static bodies and transform sync

- `transform_to_position` (`physics_transform/mod.rs:187`) iterates **every** `(GlobalTransform, Position, Rotation)` each step, calls `GlobalTransform::compute_transform()` (affine decomposition) and compares against tolerances. No `Changed` filter, statics included. On m10000 that is the ~1 ms "transform ↔ position sync" row.
- `position_to_transform` (`mod.rs:271`) is filtered on `Or<(Changed<Position>, Changed<Rotation>)>` — cheap.
- `writeback_solver_bodies` (`dynamics/solver/solver_body/plugin.rs:263`) unconditionally writes `Position` and `Rotation` for every solver body (dynamic + awake kinematic) every step, so those always count as changed. This is why the game's `walker.transform.rotation = heading` works today: `transform_to_position` copies it into `Rotation` at the start of the next step, and locked rotation keeps it there.
- Turning `transform_to_position` off therefore requires: writing `Rotation` (not `Transform.rotation`) for character heading, writing `Position`/`Rotation` for the falling-tree animation, and setting `Position` at spawn (with it off, nothing initialises `Position` from `Transform` — the experiment above put every body at the origin). Tests that poke `Transform` directly would need the same treatment. `PhysicsTransformHelper::update_physics_transform` (`physics_transform/helper.rs:38`) exists for one-off syncs after a teleport.
- `update_moved_collider_aabbs` (`collider_tree/update.rs:839`) is change-tick filtered and skips `ColliderDisabled`; static trees that don't move cost nothing there. The 0.6 release post says the remaining static-collider overhead is "inefficient change detection for AABB updates".
- `ColliderDisabled` / `RigidBodyDisabled` remove the body from collision and spatial queries entirely — not usable for trees that characters must collide with.

Net: `transform_to_position = false` is worth ~1 ms on the 10k map and nothing on small maps, at the cost of a medium refactor of every `Transform` write in gameplay. Low priority.

## Q4 — Substeps and step rate

`SubstepCount` (`dynamics/solver/schedule.rs:187`) defaults to 6; each substep runs the 23-system `SubstepSchedule` (integrate velocities → warm start → solve with bias → integrate positions → relax → damping; `schedule.rs:123–133`). Collision detection is once per step; substeps only re-solve the same contacts at `dt/N`.

For this game: no gravity, no stacking, no resting contacts, capsule push-out plus one `DistanceJoint` per hauling character. Measured `SubstepCount(1)`: −30 % on the small map, −1.4 ms on the 10k map, suite passes, delivered-log count changes by ±1 over 60 s. What degrades at 1 substep is joint stiffness (rope gets springier) and how many frames overlap takes to resolve — neither affects gameplay outcomes.

Validation, if you want numbers rather than "tests pass": log per frame the max character–tree penetration (`Collisions` contact `penetration`), the max rope stretch (`DistanceJoint` anchor distance − limit), and count `LostLog` events; compare 6 vs 2 vs 1 on the same seed. Substep count is inherently part of the trajectory — the solver integrates at `dt/N` — so it must be part of "the config" for determinism, exactly like `physics_hz`.

## Q5 — `parallel` and determinism

- `parallel = ["bevy/multi_threaded", "parry3d/parallel"]` (`Cargo.toml.orig:32`). Bevy's `multi_threaded` is already on through the game's Bevy dependency, so avian's `par_iter_mut` in the integrator, solver-body prepare/writeback and collider-tree updates (`dynamics/integrator/mod.rs:278,322,356,512`, `solver_body/plugin.rs:196,276,299`, `collider_tree/update.rs:713,890`) run on the `ComputeTaskPool` regardless of the feature. Those are per-entity independent writes — deterministic.
- The feature gates only `utils::par_for_each` (`utils.rs:59`): the narrow-phase contact loop (`collision/narrow_phase/system_param.rs:457`, writes per-pair slots, merges status bits with OR) and the per-graph-colour contact loops in the solver (`dynamics/solver/plugin.rs:388,476,564,662`). Graph colouring guarantees constraints in one colour share no bodies, so parallel solving within a colour is order-independent. Structurally it looks deterministic.
- The project's own docs (DeepWiki summary of avian's determinism page) still say: "disable [`parallel`] if strict determinism is required". Take that as the maintainers' position; nothing in the source contradicts it, but nothing tests it either.
- It doesn't matter here: `par_for_each` falls back to serial below `min_len` = 64 constraints / 2 colours, which the small map never reaches, and on the 10k map the narrow phase is ~0.5 ms. Leave it off.
- `enhanced-determinism` (`Cargo.toml.orig:33`, libm everywhere) is for *cross-platform* bit-equality; same-machine run-to-run determinism doesn't need it. It costs 10–30 %.

## Q6 — Right-sizing

**avian2d.** Same crate, `2d` feature: 2-component `Position`, scalar angular inertia, 2D capsule–capsule narrow phase. Per-body work is smaller, but the system count and therefore the floor are the same, and 2D maps `Position` to `Transform.x/y` — this game's ground is XZ. You would have to either build the world in XY and tilt the camera, or disable `position_to_transform` and write your own sync. Not worth it for a per-body saving that only shows on the 10k map.

**Hand-rolled.** Everything physics does for this game is: (a) keep circles (character r 0.4, trunk r 0.25·scale) from overlapping on a plane, (b) a shape cast 2.5 m ahead for steering, (c) an overlap test against logs, (d) a slack rope. A uniform grid (cell ≈ 3 m) keyed by XZ gives all four in O(neighbours): push-out by iterating the 3×3 cells around each character and moving it along the overlap normal (a couple of Jacobi iterations), a swept circle against the same cells for steering, a point-in-capsule test for logs, and the rope as "if the log is farther than L, move it to distance L along the line to the character". That's a few hundred lines, trivially deterministic (iterate entities in a stable order), and its cost is proportional to actual interactions — likely ~50 µs for 1 000 characters and nothing for 10 000 idle trees. It removes ~240 systems per step and the 91 µs step floor, which is the only route to a small map running at hundreds of × realtime. Costs: you own the collision code, fallen-log climbing becomes an explicit rule instead of a layer trick, and the windowed build loses the avian debug gizmos.

This is what fast-forward-heavy sims do: keep a tiny fixed-function sim core and only use a general physics engine where they actually need rigid-body dynamics.

## Q7 — Spatial queries

`SpatialQuery` reads the broad-phase `ColliderTree` BVHs directly (0.6 change, "spatial queries now reuse the same BVH trees used by the broad phase"); `SpatialQueryPlugin` only drives the `RayCaster`/`ShapeCaster` components (`spatial_query/mod.rs:206`). So per-query cost is a BVH descent plus a parry shape test; there's no pipeline to cache or rebuild.

Cheaper alternatives for the two per-character calls:
- Steering `cast_shape` (capsule, 2.5 m): a `cast_ray` from the centre is roughly 3–5× cheaper but misses grazing contacts; two rays (edges of the capsule) is a common compromise.
- `climb_logs` `shape_intersections` against `Layer::Log`: `point_intersections` at the character's centre, or `aabb_intersections_with_aabb` with a small AABB, avoids the capsule–capsule test. Or drop the query entirely and set `Climbing` from the contact list (`Collisions`), except logs collide with nothing by design.

On m10000 both calls together are ~0.8 ms for 1 000 characters (0.8 µs per call) — already cheap; only worth touching if characters go to 10 000.

## Q8 — Bevy-side overhead

`SingleThreadedExecutor::run` (`bevy_ecs-0.19.1/src/schedule/executor/single_threaded.rs:71–160`) per system: evaluate set conditions (cached per set), evaluate system conditions, `catch_unwind`, `run_without_applying_deferred` (param validation, archetype access update, change-tick bookkeeping), and command flushes at each `apply_deferred` point. Measured: 91 µs / ~240 systems ≈ 0.38 µs per system, which is about the practical minimum for this executor. `SimpleExecutor` no longer exists in 0.19. `Schedule::set_executor(SingleThreadedExecutor::new())` on every schedule (already done) is the right knob; there's no cheaper executor to switch to.

Reducing the *count* is the only remaining lever: fewer avian plugins (Q2), fewer substeps (Q4), or no avian (Q6). Running avian's schedules by hand from one exclusive system doesn't remove per-system cost — the schedule still runs the same systems.

## Q9 — Profiling

- `SolverDiagnostics` (`dynamics/solver/diagnostics.rs:13`: `prepare_constraints`, `integrate_velocities`, `warm_start`, `solve_constraints`, `integrate_positions`, `relax_velocities`, `apply_restitution`, `finalize`, `swept_ccd`, `contact_constraint_count`) and `CollisionDiagnostics` (`collision/diagnostics.rs:13`: `broad_phase`, `narrow_phase`, `contact_count`) are plain resources that avian's systems fill with `Instant` timings **unconditionally** — no feature needed. A `Res<SolverDiagnostics>` in the existing `report_stats` system can print them every 10 sim-seconds; they reset each step, so accumulate them yourself. The `bevy_diagnostic` feature + `PhysicsDiagnosticsPlugin` (`diagnostics/mod.rs:105`) only forwards them to `DiagnosticsStore`.
- `bevy/trace_tracy` records the same spans as `trace_chrome` through Tracy's binary client instead of a JSON writer; per-span overhead is much lower (tens of ns vs the ~1–2 µs seen here). It needs the Tracy profiler UI to capture. For A/B comparisons of variants, the untraced wall-clock harness used above (`--duration N` minus `--duration 0`) is the least distorted.

---

## Sources

- avian3d 0.7.0 source in the cargo registry (`file:line` references above)
- bevy_ecs 0.19.1 source, `schedule/executor/single_threaded.rs`
- [Avian Physics 0.6 — Joona Aalto](https://joonaa.dev/blog/12/avian-0-6) (BVH broad phase, four collider trees, spatial queries reusing the BVH, 40 000-static-collider benchmark, "inefficient change detection for AABB updates")
- [Avian Physics 0.7 — Joona Aalto](https://joonaa.dev/blog/13/avian-0-7) (no perf-relevant changes for this game; 0.8 roadmap: CCD overhaul, contact recycling, multithreading improvements)
- [Determinism — avian (DeepWiki)](https://deepwiki.com/avianphysics/avian/10.3-determinism) (recommendation to disable `parallel` for strict determinism; `enhanced-determinism` = libm, 10–30 % cost)
- [Determinism — Rapier](https://rapier.rs/docs/user_guides/rust/determinism/) (same feature-flag trade-off in the sibling engine)
- [avian releases](https://github.com/avianphysics/avian/releases)

---

## Follow-up: gameplay in `FixedUpdate` (best-practice check)

**Question.** After interpolation broke gameplay (characters measured arrival on an eased
`Transform`), was "refactor gameplay onto `Position`" the right fix?

**Answer from the sources: the fix is the schedule, not the component.** Body-moving gameplay
belongs in `FixedUpdate`; once it is there, `Transform` reads are exact and interpolation works.

- avian `src/lib.rs:403-418`: "`Transform` can be used for the vast majority of things …
  using `Position` and `Rotation` is only necessary when you need to manage positions within
  `PhysicsSystems::StepSimulation`."
- avian `examples/kinematic_character_3d/plugin.rs`: input in `PreUpdate`, "Run movement logic in
  `FixedUpdate` to ensure consistent behavior regardless of frame rate", `TransformInterpolation`
  on the player.
- avian `src/interpolation.rs:135-136`: writing `Transform` "in any schedule that *doesn't* use a
  fixed timestep … is equivalent to teleporting, and disables interpolation for the entity for
  the remainder of that fixed timestep." — exactly what the per-frame heading writes in `Update`
  were doing.
- `bevy_transform_interpolation-0.5.0/src/interpolation.rs:249-260`: `complete_*_easing` in
  `FixedFirst` restores the exact end-of-step `Transform` before any fixed system runs, so inside
  fixed schedules `Transform` is never the eased value.
- Bevy `bevy_app/src/main_schedule.rs:162`: "`Update` … For most gameplay logic, consider using
  `FixedUpdate` instead."

**What was done.** Gameplay (`gather`, `haul`, `climb_logs`, `face_heading`, `grow_trees`,
`disperse_seeds`, `animate_falling_trees`, `sync_tree_bodies`) runs in `FixedUpdate`. It reads and
writes `Position`/`Rotation` rather than `Transform` — avian allows either; the pose was chosen so
that avian's transform→position sync and its pre-physics propagation can stay off and the custom
change-filtered sync could be deleted. `Transform` is now render-only: written by
`position_to_transform` after each step, smoothed by `TransformInterpolation` (characters, and
trees once felled) when windowed. Visual-only motion (mesh lift on logs, chop swing) is a
per-frame `Update` system on the mesh child — the swing used to be written into the body's
rotation and tilted the capsule collider. Spawns set `Position`/`Rotation` explicitly.

Consequences: one physics rate (20 Hz) in every mode, so a seed reproduces the same run windowed
and headless; a headless frame is one physics step (`step = 1/physics_hz`) since nothing else
happens per frame; `walk_toward` has one `dt`; the outcome no longer depends on the render frame
rate. Verified: same seed at ~100 fps (vsync), ~500 fps (uncapped), 16× fast-forward and headless
give identical stats at 10/20/30 s.

| map | before (60 fps frames, 20 Hz) | after (1 step / frame) |
|---|---:|---:|
| 10 trees / 1 char | 0.099 ms/frame, 169× | 0.174 ms/step, **288×** |
| default map, 60 s | 121× | **256×** |
| 1 000 / 100 | 0.331 ms/frame, 50× | 0.539 ms/step, **93×** |
| 10 000 / 1 000 | 2.08 ms/frame, 8× | 4.66 ms/step, **11×** |
| heavy test (30 s) | 4.0 s, 4 467 delivered | **2.75 s**, 4 395 delivered, 0 lost |
| `cargo test --release` (integration) | 2.0 s | **1.2 s** |

Per simulated second the work is now 20 gameplay+physics steps instead of 60 gameplay frames +
20 physics steps; per-step cost rose (steering casts and tree logic happen in the step) but there
are three times fewer of them.
