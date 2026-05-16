<p align="center">
  <img src="./assets/januas-ade-logo.png" alt="Januas ADE logo" width="180" />
</p>

# Januas ADE

Native Rust Agentic Development Environment. GPU-rendered terminal multiplexer with wizard-driven setup for AI-coding sessions. Pure Rust, no framework reliance, homegrown ecosystem. 1000 fps render target.

Part of the [Januas umbrella](../../CLAUDE.md).

## Status

**v0.0.0** — slice **S0 (scaffold)**. The full slice list (S0 through S18) lives in [`NORTH_STAR.md`](./NORTH_STAR.md).

## Build

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run --bin januas-ade
```

## License

Dual-licensed under either of:

- MIT License ([`LICENSE-MIT`](./LICENSE-MIT))
- Apache License, Version 2.0 ([`LICENSE-APACHE`](./LICENSE-APACHE))

at your option. Contributions are accepted under the same dual-license terms.
