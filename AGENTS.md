# cprz agent notes

Work in a lane (`~/agents/scratch/lane-new <lane>`); never edit `~/agents/scratch/ws`.
cprz is its own workspace root: run every command below from `<lane>/ws/cprz`.

Build and test (quote the verdict line, never infer from a pipe):
- `cargo fmt --all --check` — clean at HEAD.
- `cargo test --locked --workspace` — green, exit 0 (all suites `ok`).
- `cargo clippy --workspace --all-targets -- -D warnings` — green, exit 0.
- `cargo check -p cprz --no-default-features --target wasm32-unknown-unknown`
  — green, exit 0 (portable facade builds for wasm).
- Full gate (locked deps, wasm-bindgen portable execution, coverage) runs in
  CI only: `.ci/verify.sh`. Lane `--locked` needs CI's resolve refresh.

Land: `cargo fmt` in touched repos, rebase on main (never force-push),
`~/agents/scratch/cbuild <lane> xtask land --dry`, then `xtask land`.

Invariants (rule -> enforcing test):
- Portable facade preserves exact bytes under exact limits
  -> `crates/cprz/tests/portable.rs::facade_preserves_exact_bytes`,
  `crates/cprz-blob/tests/portable.rs::exact_bytes_survive_with_exact_limits`
- Oversized expansion never returns partial plaintext; truncated, unknown,
  concatenated and trailing packets fail closed
  -> `oversized_expansion_never_returns_partial_plaintext`,
  `truncated_unknown_and_concatenated_packets_fail_closed`
- Declared sizes are never pre-allocated; diagnostics carry no payload
  -> `crates/cprz-pad/tests/portable.rs::exact_framing_and_oversized_prefixes_refuse_without_allocating_declared_sizes`,
  `diagnostics_expose_no_payload`
- Opus pre-skip discards the whole prefix and rejects impossible lengths
  -> `crates/cprz-audio/src/tests/lib.rs::real_opus_preskip_discards_the_entire_prefix_and_rejects_impossible_length`
- No own compression algorithms; dictionaries use public text only
  (`README.md:62-71`); `forbid(unsafe_code)` (workspace `Cargo.toml`).

Pointers: `crates/`, `docs/CONTRACT.md`, `docs/COVERAGE.md`, `.ci/verify.sh`.
Commits: explicit paths, imperative, author per `~/agents/AGENT.md`; no AI trailers.
`publish=false`; LGPL-3.0-only WITH linking exception; no registry publication.
