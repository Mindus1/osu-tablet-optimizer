# osu-tablet-optimizer

Analyzes your osu! replays (stable and lazer) and suggests a better [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver) tablet area: size, aspect ratio and rotation, based on whether your aim systematically overshoots, undershoots or drifts in one direction.

Suggestions are exported as OTD presets, so your current settings stay untouched.

Status: in development. Setup and the replay list work; analysis and area suggestions are next.

## Usage

1. **Setup** opens on first start.
   - **osu! folders:** stable and lazer are detected. Add other folders with *Add folder…*.
   - **OpenTabletDriver:** found automatically when it's installed. For a portable OpenTabletDriver, *Choose folder…* and pick its program folder; the `userdata` folder inside is used.
   - **Profile and tablet:** pick your tablet's profile. The tablet's size comes from OpenTabletDriver's tablet list, or you can enter it yourself.
   - **Players:** filled in after the first scan with the name that appears most in your replays. Edit it if you share the PC.
2. **Replays** lists your osu!standard replays, newest first.
   - **Filter:** by last session, 7 days, 30 days or all time, or search map titles.
   - **Add replays:** with *Add files…* or by dropping `.osr` files onto the window.
   - **Select:** tick the replays you want analysed.

The first scan of a large library takes a minute or two; later scans only read new or changed files. Settings (`config.json`) and the scan cache (`library.json`) live in your app data folder under `osu-tablet-optimizer` (`%APPDATA%` on Windows, `~/.local/share` on Linux, `~/Library/Application Support` on macOS). OpenTabletDriver's own settings are only read.

## Build from source

Requires [Rust](https://rustup.rs) (stable).

```sh
cargo run --release
```

## Credits

Tablet dimensions in `src/core/tablets.tsv` come from [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver)'s tablet configurations (LGPL-3.0).
