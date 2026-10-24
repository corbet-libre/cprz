# Agent instructions

Write all code comments and documentation in English.

## Product boundary

- `cprz` shrinks payloads as far as possible, then pads them. The facade chooses; sub-crates execute.
- Wrap established, maintained libraries. Never invent a compression algorithm or a container format that an established one already covers.
- **Search before you write.** Before implementing anything, search crates.io, GitHub and the literature for existing implementations (also of the specific scheme, e.g. Padmé). If one exists and is maintained, write a thin facade or shim over it instead of reimplementing it. Record the candidates you evaluated, the one you chose and why in the crate's README.
- Every crate must build for native targets and for `wasm32-unknown-unknown`. Prefer pure-Rust dependencies; a C dependency is acceptable only if it builds for wasm32 in CI.
- No network code and no dependency on Veilid, Arti, Tor or any transport. Network limits exist only as data in target profiles.
- Sub-crates never depend on the facade crate `cprz`; the facade depends on them.
- Encoding favours the smallest output (highest levels); decoding must stay fast.
- Every packed payload is self-describing; `expand(shrink(x)) == x` for lossless kinds.
- Never train dictionaries on user data.
- This workspace is LGPL-3.0-only WITH LGPL-3.0-linking-exception. Do not add dependencies available only under the full GPL or AGPL.

## Quality boundary

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` and `cargo check --workspace --target wasm32-unknown-unknown`
  must pass before every merge.
- Do not build locally. Push and let GitHub Actions run the checks; read the
  results with `gh run list` / `gh run view --log-failed`. Crow is the fallback only when
  Actions is unavailable.
- Tests use real data round trips and size assertions, not mocks.
- No publishing to any registry. No paid services.
