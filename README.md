# osu-tablet-optimizer

Analyzes your osu! replays (stable and lazer) and suggests a better [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver) tablet area: size, aspect ratio and rotation, based on whether your aim systematically overshoots, undershoots or drifts in one direction.

Suggestions are exported as OTD presets, so your current settings stay untouched.

Status: in development. Setup, the replay list, the analysis, area suggestions and the history work.

## Download

Each release on the [Releases page](https://github.com/Mindus1/osu-tablet-optimizer/releases) has a build for each platform:
- Windows: `osu-tablet-optimizer-windows-x64.exe`
- Linux: `osu-tablet-optimizer-linux-x64.tar.gz`
- macOS on Apple silicon: `osu-tablet-optimizer-macos-arm64.tar.gz`

The builds are unsigned, so Windows SmartScreen or macOS may ask before the first start. On macOS, allow it under *System Settings → Privacy & Security*.

## Usage

1. **Setup** opens on first start.
   - **osu! folders:** stable and lazer are detected. Add other folders with *Add folder…*.
   - **OpenTabletDriver:** found automatically when it's installed. For a portable OpenTabletDriver, *Choose folder…* and pick its program folder; the `userdata` folder inside is used.
   - **Profile and tablet:** pick your tablet's profile. The tablet's size comes from OpenTabletDriver's tablet list, or you can enter it yourself.
   - **Players:** filled in after the first scan with the name that appears most in your replays. Edit it if you share the PC.
2. **Replays** lists your osu!standard replays, newest first.
   - **Filter:** by last session, 7 days, 30 days or all time, by tablet area (see History), or search map titles. Analyze your current area's replays: the suggestion corrects your current area, so replays played with an older one would blur it.
   - **Add replays:** with *Add files…* or by dropping `.osr` files onto the window.
   - **Select:** tick the replays you want analyzed, then press *Analyze*.
   - **Area column:** the tablet area each replay was played with. When it's wrong (see History), select the replays and pick the right one under *Assign selected to area…*; *By date* undoes that.
3. **Analysis** measures how your jumps land, over every jump of at least three circle radii in the selected replays. It needs at least 5 replays. Each number comes with a 90% range; when that range includes "no change", the page says *no clear effect*.
   - **Overshoot:** how much farther (+) or shorter (−) than intended your jumps travel.
   - **Rotation:** how many degrees your moves come out turned clockwise (+) or counter-clockwise (−).
   - **Horizontal ÷ vertical:** below 1 means horizontal moves fall shorter than vertical ones.
   - **Shear:** a skew OpenTabletDriver can't correct; shown for information.
   - **Error scatter:** each dot is one jump's error, along the jump (right = too far) and across it. The circle is the hit radius.
   - **Polar plot:** the outline shows distance and turn per move direction, magnified 5×. The ring means no error, and an outline inside the ring means undershoot. Stopping at the circle's edge is left out, as in the Overshoot number, and directions with fewer than 10 jumps are left open.
   - When you change the selection or setup afterwards, the page says so; press *Analyze* there to update.
4. **Area** draws the suggested tablet area over your current one and lists the numbers. It needs an analysis made with OpenTabletDriver's settings found, and a profile in absolute mode.
   - **Capped step:** changes the size by at most 3% and the rotation by at most 2°. You can change these limits in Setup. Take this step, play a few sessions, then analyze again.
   - **Full fit:** the whole correction the analysis measured.
   - Size, aspect ratio and rotation stay as they are when the analysis shows no clear effect.
   - **Export:** saves the area as an OpenTabletDriver preset named like `oto-2026-10-09-capped` and copies your other settings as they are. The area keeps its center, moved only as far as needed to stay on the tablet. In OpenTabletDriver, apply the preset under *File → Presets*; if OpenTabletDriver is already running, use *Refresh presets* first.
5. **History** shows whether a new area helped.
   - **Areas:** the app logs your profile's area whenever it differs from the last one, at start and before each scan. Each area counts from the time OpenTabletDriver saved its settings, so replays from before the first logged area are *unknown*. Saving any other OpenTabletDriver setting also updates that time; assign replays by hand on the Replays page when that matters. Picking another profile or OpenTabletDriver folder in Setup replaces the last logged area, because that change has no date of its own. Give an area a name by typing it in.
   - **Chart:** each analyzed replay adds a point, colored by its area: overshoot, rotation, horizontal ÷ vertical, or the distance from the circle's center. Each point is measured from that replay alone, so points scatter more than the Analysis page's numbers, and stopping at a circle's edge counts as a little undershoot. The list shows each area's mean.

The first scan of a large library takes a minute or two; later scans only read new or changed files. Settings (`config.json`), the history (`history.json`) and the scan cache (`library.json`) live in your app data folder under `osu-tablet-optimizer` (`%APPDATA%` on Windows, `~/.local/share` on Linux, `~/Library/Application Support` on macOS). OpenTabletDriver's own settings are only read.

## Build from source

Requires [Rust](https://rustup.rs) (stable).

```sh
cargo run --release
```

## Credits

The interface uses [Exo 2](https://github.com/googlefonts/Exo-2.0) (SIL Open Font License 1.1, see `assets/fonts/OFL.txt`).

Tablet dimensions in `src/core/tablets.tsv` come from [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver)'s tablet configurations (LGPL-3.0).
