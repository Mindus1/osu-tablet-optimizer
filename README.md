# osu-tablet-optimizer

Analyzes your osu! replays (stable and lazer) and suggests a better [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver) tablet area: size, aspect ratio and rotation, based on whether your aim systematically overshoots, undershoots or drifts in one direction.

Suggestions are exported as OTD presets, so your current settings stay untouched.

Status: in development.

## Build from source

Requires [Rust](https://rustup.rs) (stable).

```sh
cargo run --release
```
