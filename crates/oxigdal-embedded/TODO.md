# TODO: oxigdal-embedded

> **Purpose:** `no_std`-compatible geospatial primitives for ARM Cortex-M, RISC-V, ESP32 — static memory pools, embedded-hal abstractions, fixed-point math, low-power & realtime helpers.
> **Status (2026-05-17):** 5,214 LoC · 68 #[test] attributes · 2 real-code soft stubs.
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Real power-mode transition policy (replace "Allow any transition for now")
  - **Verified gap:** `src/power.rs:99-105` — `/// Check if a power mode transition is allowed` / `fn is_transition_allowed(&self, from: PowerMode, to: PowerMode) -> bool {` / `// Allow any transition for now` / `// In real implementation, this would check hardware constraints` / `let _ = from; let _ = to; true`
  - **Goal:** Encode the legal state-transition graph for power modes so callers cannot drive the MCU from `Active → DeepSleep` without first quiescing peripherals (which on ARM Cortex-M would corrupt in-flight DMA). Today every transition silently succeeds, hiding misuse bugs that only manifest as random hangs on real silicon.
  - **Design:** Define a constexpr 2D table indexed by `(from, to)` mapping to `TransitionPolicy::{Allowed, AllowedWithFlush(PeripheralMask), Forbidden}`. Reference: ARM Cortex-M Low Power application note AN945, Section 3 — `Run → Sleep` (WFI) is unconstrained; `Sleep → Stop` requires DMA quiesced; `Stop → Standby` requires SRAM retention bit consideration. Add `PowerController::register_peripheral(handle: PeripheralHandle)` so transitions check the registered list for `is_quiesced()`. Return structured `PowerError::TransitionForbidden { from, to }` / `PowerError::PeripheralBusy { name }` rather than silently succeeding.
  - **Files:** `crates/oxigdal-embedded/src/power.rs` (`is_transition_allowed` + state table); (new) `crates/oxigdal-embedded/src/power/transitions.rs` (table).
  - **Tests:** (proposed) `test_active_to_sleep_always_allowed`, `test_active_to_deep_sleep_requires_dma_quiesced`, `test_deep_sleep_to_active_resets_to_active`, `test_forbidden_transition_returns_error`, `test_peripheral_quiesce_unblocks_transition`.
  - **Risk:** State table grows with new modes; encode in a single const so additions are statically checked.
  - **Prerequisites:** None.

- [ ] Realtime scheduler task execution (currently "Task would be executed here")
  - **Verified gap:** `src/realtime.rs:330-332` — `// Task would be executed here` / `// For now, just mark as executed` / `let end_us = self.scheduler.elapsed_us();`. The scheduler `schedule()` loop iterates ready tasks and records timing stats but never dispatches the closure — every task reports zero exec_us.
  - **Goal:** Real cooperative dispatch of task closures with deadline tracking, missed-deadline statistics, and bounded execution-time enforcement.
  - **Design:** Tasks carry a `task_fn: fn(&mut Context<'_>) -> Result<()>` (fn pointer to keep no_std, no Box<dyn Fn>). The `schedule()` loop, after `task.is_ready(current_us)`, invokes `task.task_fn(&mut ctx)` between the `start_us`/`end_us` timestamps. Bound execution to `task.budget_us` via cooperative yielding (caller checks `ctx.deadline_reached()`). Missed deadlines → record into `TaskStats { missed_deadlines, max_exec_us, last_exec_us }`. Provide an `IsrEntry` variant for tasks dispatched from an ISR (no-allocation, no-blocking).
  - **Files:** `crates/oxigdal-embedded/src/realtime.rs` (dispatch logic); (new) `crates/oxigdal-embedded/src/realtime/context.rs` (Context type).
  - **Tests:** (proposed) `test_realtime_task_fn_called_when_ready`, `test_realtime_missed_deadline_recorded`, `test_realtime_budget_overrun_does_not_block_other_tasks`, `test_realtime_isr_entry_no_alloc`.
  - **Risk:** fn-pointer-only tasks restrict captured state; document the `Context` pattern that scratch state lives in a `&'static AtomicX` or in the task struct.
  - **Prerequisites:** None.

- [ ] Verify compilation across embedded target triples in CI (thumbv7em-none-eabihf, riscv32imc-unknown-none-elf, xtensa-esp32-none-elf)
  - **Verified gap:** Existing TODO line — `[ ] Verify compilation on actual embedded targets (thumbv7em, riscv32imc, xtensa)`. The crate's `default = ["alloc"]` features compile on host, but `no_std + no_alloc + arm + low-power + realtime` likely hasn't been recently exercised. cfg conditions are declared in `build.rs` already.
  - **Goal:** Each release runs `cargo build --target thumbv7em-none-eabihf --no-default-features --features=arm,low-power,realtime`, plus the analogous matrix for RISC-V (`riscv32imc`, `riscv32imac`, `riscv64imac`) and ESP32 (`xtensa-esp32-none-elf` via `+nightly`). All targets must build with zero warnings.
  - **Design:** Use `cargo check` (not `build`) for the host CI to avoid linker setup; reserve full `build` for self-hosted ESP32/Cortex-M runners or a `qemu-system-*` simulator path. Track build size (`.text` section) via `cargo-binutils` `cargo size`, gate at ≤32 KB for the `minimal` feature.
  - **Files:** Build matrix lives outside repo per CLAUDE.md (only `pypi-publish.yml`/`npm-publish.yml` allowed) — instead, encode the matrix as a script `crates/oxigdal-embedded/scripts/check-targets.sh` to be run locally and document in README; (new) `crates/oxigdal-embedded/Cargo.toml` ESP32 target-specific deps.
  - **Tests:** (proposed) `test_no_std_no_alloc_compiles_for_thumbv7em` (build-test integration), `test_minimal_feature_text_under_32kb`, `test_static_pool_size_compile_time_constant`.
  - **Risk:** xtensa needs `esp-rs/rust` fork — document. Run only on opt-in.
  - **Prerequisites:** None.

- [ ] Fixed-point arithmetic for coordinate transforms on no-FPU targets
  - **Verified gap:** Existing TODO line — `[ ] Implement fixed-point arithmetic for coordinate transforms (no FPU targets)`. The crate uses `libm` workspace dep for `f32::sin/cos/atan2`, which is software-emulated on Cortex-M0/M0+ and RISC-V `imc` (no F extension). Real-world embedded GPS code therefore needs a Q15/Q31 fixed-point alternative.
  - **Goal:** A `fixed_point` module exposing `Fixed<I, F>` (I+F=32, e.g., Q16.16) with the standard ops + a `transform::affine_q16_16(point, matrix)` routine. Document accuracy: Q16.16 affine gives 1.5e-5 absolute error on coordinates in [-180, 180].
  - **Design:** Use `fixed = "1.28"` crate (Pure Rust, no_std, no_alloc). Provide `MercatorTransformQ16_16` and `AffineTransformQ16_16`. Benchmark against `libm` softfloat baseline on Cortex-M0 (target: ≥3x speedup). Provide `cfg(target_feature = "+fpv4-sp-d16")` automatic fallback to `f32` on Cortex-M4F.
  - **Files:** (new) `crates/oxigdal-embedded/src/fixed_point/mod.rs`, `crates/oxigdal-embedded/src/fixed_point/affine.rs`, `crates/oxigdal-embedded/src/fixed_point/mercator.rs`; modify `crates/oxigdal-embedded/Cargo.toml` to add `fixed` dep.
  - **Tests:** (proposed) `test_fixed_q16_16_roundtrip_wgs84_to_mercator`, `test_fixed_q16_16_max_error_under_1e_minus_5`, `test_fixed_affine_identity_returns_input`, `test_fixed_affine_vs_f32_within_tolerance`, `bench_fixed_vs_softfloat_mercator_1000_points`.
  - **Risk:** `fixed` crate brings ~5 deps but all are no_std Pure Rust. Verify against COOLJAPAN policy — accepted (Pure Rust).
  - **Prerequisites:** None.

- [ ] Stack usage annotations + analysis on all public APIs
  - **Verified gap:** Existing TODO line — `[ ] Add stack usage analysis annotations for all public functions`. No `#[inline(never)]` / `#[link_section]` annotations or `cargo-call-stack` integration today.
  - **Goal:** Every public function in the crate carries a documented worst-case stack depth (in bytes), enabling embedded users to compute total stack budget. Verified via `cargo call-stack` (LLVM `.stack_sizes` section).
  - **Design:** Add `#![deny(missing_docs)]`-style guard `// stack: ≤<N>B (worst case)` as the first doc line for every public function. Run `cargo +nightly call-stack --target thumbv7em-none-eabihf --features=...` in CI script. For functions exceeding their documented budget, fail the build via a clippy-style helper. Provide a `Stack` measurement macro for embedded users.
  - **Files:** Every public-fn site under `crates/oxigdal-embedded/src/`; (new) `crates/oxigdal-embedded/scripts/check-stack-sizes.sh`.
  - **Tests:** (proposed) `test_static_pool_alloc_stack_under_64b`, `test_realtime_schedule_stack_under_256b`, `test_minimal_meta_construct_stack_under_32b`.
  - **Risk:** `cargo call-stack` requires nightly; document.
  - **Prerequisites:** Item 3 (target compilation in CI).

## Medium Priority
- [ ] DMA-aware buffer alignment + cache-line guards (`#[repr(align(32))]`)
  - **Goal:** Buffers passed to DMA peripherals must be aligned to the device's cache line (typically 32 B on Cortex-M7).
  - **Files:** `crates/oxigdal-embedded/src/buffer.rs` (add `DmaBuffer<N>` wrapper).
  - **Why deferred:** Per-MCU; needs Item 3 (multi-target verification) first.

- [ ] embedded-hal trait implementations for platform abstraction
  - **Goal:** Implement `embedded_hal::spi::SpiBus`, `embedded_hal::i2c::I2c` traits for our SPI/I²C adapter types so users can plug in any HAL.
  - **Files:** (new) `crates/oxigdal-embedded/src/hal/spi.rs`, `crates/oxigdal-embedded/src/hal/i2c.rs`.
  - **Why deferred:** Pending Item 7 (NMEA parser) which needs SPI/I²C first.

- [ ] NMEA-0183 sentence parser in no_std for embedded GPS receivers
  - **Goal:** Parse $GPGGA, $GPRMC, $GPGSV, $GPGSA on a serial byte stream without heap allocation.
  - **Files:** (new) `crates/oxigdal-embedded/src/nmea/mod.rs`.
  - **Why deferred:** Coordinated with embedded-hal SPI/UART abstraction.

- [ ] Lightweight tile renderer for embedded SPI LCD / e-ink displays
  - **Goal:** Render raster tiles into a framebuffer using the `embedded-graphics` crate.
  - **Files:** (new) `crates/oxigdal-embedded/src/display/mod.rs`.
  - **Why deferred:** Pending Item 4 (fixed-point math) for sub-pixel sampling.

- [ ] Flash storage driver for tile cache persistence on NOR/NAND flash
  - **Goal:** A wear-leveling tile cache backend using `embedded-storage` traits.
  - **Files:** (new) `crates/oxigdal-embedded/src/flash_cache.rs`.
  - **Why deferred:** Niche; opportunistic.

- [ ] Watchdog integration to prevent processing hangs
  - **Goal:** Auto-pet the watchdog when a long algorithm enters a loop; refuse to pet if algorithm exceeds budget.
  - **Files:** (new) `crates/oxigdal-embedded/src/watchdog.rs`.
  - **Why deferred:** Coordinated with Item 2 (realtime scheduler).

- [ ] `defmt` logging integration for efficient embedded debugging
  - **Goal:** Replace `log` with `defmt` macros gated under feature `defmt`.
  - **Files:** Most modules — `defmt::info!`, `defmt::warn!` everywhere.
  - **Why deferred:** Easy mechanical change; bundle with v0.2.0 release.

- [ ] Compile-time memory layout verification for `StaticPool<N>`
  - **Goal:** A const-generic assertion that `N` is a power of two ≥64.
  - **Files:** `crates/oxigdal-embedded/src/memory_pool.rs`.
  - **Why deferred:** Quick win once Item 5 (stack sizing) lands.

## Low Priority / Future (one-liners)
- [ ] LoRa/LoRaWAN packet encoder for geospatial telemetry.
- [ ] BLE beacon support for indoor positioning augmentation.
- [ ] Sensor fusion (accelerometer + GPS) for dead reckoning.
- [ ] OTA firmware update support with rollback.
- [ ] RTOS integration (FreeRTOS, Zephyr) task wrappers.
- [ ] Minimal vector renderer for embedded map display.
- [ ] Hardware CRC acceleration for data integrity.
- [ ] Redox OS compatibility (Pure Rust target, no_std-by-default).

## Cross-crate dependencies
- **Blocks:** None directly.
- **Blocked by:** None (self-contained); `oxigdal-core` for shared no_std primitives.

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see README.md for the no_std architecture and `examples/` for ARM Cortex-M usage)

---
*Last audited: 2026-05-17*
