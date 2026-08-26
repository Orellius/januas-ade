<p align="center">
  <img src="./assets/januas-ade-logo.png" alt="Januas ADE logo" width="180" />
</p>

<h1 align="center">Januas ADE</h1>

<p align="center">
  <em>A native Rust Agentic Development Environment. GPU-rendered terminal multiplexer with wizard-driven setup for AI-coding sessions.</em>
</p>

<p align="center">
  Pure Rust. No Tauri, no Electron, no webview, no UI framework. Homegrown ecosystem.
</p>

Part of the [Januas umbrella](../../CLAUDE.md).

## Status

Alpha. Slice **S3** (PTY + single shell) green. The full slice plan (S0 through S18) lives in [`NORTH_STAR.md`](./NORTH_STAR.md). Append-only milestone log in [`docs/CHECKPOINTS.md`](./docs/CHECKPOINTS.md).

## Build

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run --bin januas-ade
```

Launches a 1280×800 window with a real shell running inside.

## License

AGPL-3.0. See [LICENSE](LICENSE).

If you run a modified version as a network service, the AGPL requires you to offer that
modified source to its users.
