# Refactor Mnx Tab CLI for Idiomatic Rust (Updated)

## Goal Description
Refactor the Rust codebase at `/Users/john.platter/Documents/mnx_tab_cli` to follow idiomatic Rust practices, improve readability, maintainability, and align with the 2024 edition of Cargo. The refactor will include API changes where they simplify the code, update serialization to use `serde-saphyr`, and enhance the test suite.

## User Review Required
[!IMPORTANT] This plan includes API changes (e.g., function signatures) that may affect downstream code. Please confirm that these changes are acceptable.

## Open Questions
- **Prioritization**: Do you prefer us to tackle `render.rs` first (largest file) or start with data model (`mnx.rs`) to propagate changes?
- **CI Integration**: Should we set up a GitHub Actions workflow now, or defer to a later iteration?
- **Documentation**: Any specific style guide (e.g., `rustdoc` conventions) you want us to follow?

## Proposed Changes
---
### Cargo.toml & Project Settings
- Update the edition to `2024` (`edition = "2024"`).
- Ensure `serde-saphyr` is listed as a dependency (replace any `serde_yaml` entries).
- Enable `panic = "abort"` if appropriate for binary size.

---
### src/mnx.rs
- Replace `serde_yaml` imports with `serde_saphyr`.
- Derive common traits (`Clone`, `Debug`, `PartialEq`, `Serialize`, `Deserialize`) for structs (`Global`, `Part`, `TimeSignature`, `Event`, `Duration`).
- Adjust any deserialization code to use `serde_saphyr::from_str` or appropriate helpers.
- Add module‑level documentation describing the MNX data model.
- Refine visibility: keep fields `pub(crate)` where needed, otherwise private.

---
### src/render.rs
- Split the large `draw_tablature` function into smaller private helpers:
  - `draw_header`, `draw_staff`, `draw_time_signature_section`, `process_events`, etc.
- Reduce mutable state; favor iterator chains (`map`, `filter`, `collect`).
- Eliminate unnecessary clones; pass references.
- Replace any `unwrap`/`panic!` with proper `Result` propagation where feasible.
- Add comprehensive doc comments for public helpers (`draw_time_signature`, `smufl_time_sig_digits`).
- Consolidate magic numbers into `const` definitions or a `Style` struct.

---
### src/gui.rs
- Extract UI sections into separate modules (`ui_top.rs`, `shortcuts.rs`, `yaml_utils.rs`).
- Refactor keyboard shortcut handling into a static map of `(&str, Shortcut)`.
- Reduce mutable borrows by using functional patterns (`if let`, `matches!`).
- Add documentation/comments for complex UI flows.
- Update call sites to new helper modules.

---
### src/main.rs
- Create a helper `fn setup_bravura_font() -> egui::FontDefinitions` that encapsulates font registration.
- Use `Arc::new` directly where possible.
- Add a top‑level crate documentation comment.

---
### src/midi.rs
- Split parsing and synthesis into `midi/parser.rs` and `midi/synth.rs`.
- Refactor large `match` blocks into smaller functions.
- Propagate errors via `Result` rather than panics.

---
### src/tuning_table.rs
- Review `HashMap` usage; consider `FxHashMap` for performance if needed.
- Add documentation for the tuning table format.

---
### src/validate.rs
- Decompose validation steps into small functions returning `Result<(), ValidationError>`.
- Expose a public `validate_document` that aggregates checks.
- Add unit tests for edge‑case validation scenarios.

---
### Tests & Linting
- Run `cargo fmt` and commit formatted code.
- Run `cargo clippy -- -D warnings` and fix all warnings.
- Add new unit tests for the new helper functions.
- Ensure existing tests continue to pass.

---
### CI Workflow (Optional)
- Add a GitHub Actions workflow to run `cargo fmt -- --check`, `cargo clippy -- -D warnings`, and `cargo test` on pushes and PRs.

## Verification Plan
### Automated
- After each refactor batch, execute `cargo test` to ensure no regressions.
- Run `cargo fmt -- --check` and `cargo clippy -- -D warnings`.
### Manual
- Build and run the application (`cargo run`) to verify UI rendering, time‑signature display, and keyboard shortcuts remain functional.
- Confirm Bravura font is still applied correctly.

---
*Once you approve this updated plan, we will begin the incremental refactor.*
