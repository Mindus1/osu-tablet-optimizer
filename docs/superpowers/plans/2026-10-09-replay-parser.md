# Replay Parser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `core::replay` reads osu! `.osr` files from stable and lazer: a fast header-only read for listing, and a full read with cursor frames and the mod list.

**Architecture:** The crate becomes lib + bin. `src/lib.rs` exposes `core` (pure logic, no iced), and `src/main.rs` stays the iced binary. Without the library target, `core`'s public functions would be dead code to the binary until the UI uses them (feature 7), and `clippy -D warnings` would fail. `core::replay` is one file: little-endian primitive readers, header, LZMA-compressed frame text, and mods. Mods come from the legacy bitmask, or from lazer's JSON score-info block when the file has one.

**Tech Stack:** Rust 2024, `anyhow`, `lzma-rs` 0.3 (pure Rust LZMA, "lzma alone" format as osu! writes it), `serde` + `serde_json`.

**Spec:** `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

## Global Constraints

- `src/core/` is pure logic, no iced. Tests live in the module (`#[cfg(test)]`).
- Errors: `anyhow::Result` with `.context(..)` at I/O boundaries. Never `unwrap()` on user data (tests may unwrap).
- Comments only for a non-obvious *why*. No doc comments restating signatures.
- Real-file tests read the gitignored `tests/fixtures/` and skip when it's missing. They must not depend on specific file names.
- Units: osu!pixels for replay coordinates. Frame `time` is absolute ms (sum of deltas).
- No hardcoded user data.
- Commits: Conventional Commits ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**Format facts (verified against `ppy/osu` `LegacyScoreDecoder` and `SerializationReader`, and against real files):**
- Strings: byte `0x00` means absent. Any other byte means present, followed by a ULEB128 length and UTF-8.
- Header order: mode u8, version i32, beatmap md5, player, replay md5, counts 300/100/50/geki/katu/miss (u16 each), score i32, max combo u16, perfect u8, mods i32, life bar string, timestamp i64 (.NET ticks, UTC).
- Replay data: i32 length + LZMA bytes (≤ 0 means empty). The text is `w|x|y|keys` frames separated by `,`. `w` is the delta in ms (int, or float that gets rounded). The seed frame starts with `-12345`, and segments with fewer than 4 parts are skipped.
- `version >= 30000001` (lazer): after the replay data come the online id i64 and an i32-length LZMA block of JSON `{"mods":[{"acronym":..,"settings":{..}}], ..}`. Stable files stop mattering after the replay data.

## Review Focus

- **A corrupt length prefix (e.g. `i32::MAX`) on a short file:** expected to fail fast without allocating gigabytes. Pinned by `corrupt_length_prefix_is_an_error` (Task 2).
- **Non-UTF-8 bytes in a player name** (old stable replays): expected to read lossily, not fail. Pinned by `non_utf8_player_name_reads_lossily` (Task 1).
- **Negative replay-data length** (osu! writes -1 for "no data"): expected to give no frames and no error. Pinned by `negative_replay_data_length_has_no_frames` (Task 2).
- **Lazer score info without a `mods` key, or with unknown fields:** expected to give empty mods with no error. Pinned by `lazer_score_info_without_mods_gives_no_mods` (Task 3).
- **Version 30000000** (first lazer version, no score-info block yet): expected to use the legacy bitmask and not try to read a block. Pinned by `first_lazer_version_without_score_info_uses_legacy_mods` (Task 3).

---

### Task 1: Library target and header reading

**Files:**
- Modify: `Cargo.toml` (deps `anyhow`, `lzma-rs`), `CLAUDE.md` (Architecture section)
- Create: `src/lib.rs`, `src/core.rs`, `src/core/replay.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `osu_tablet_optimizer::core::replay::{Header, read_header}`.
  `pub struct Header { mode: u8, version: i32, beatmap_md5: String, player: String, count_300: u16, count_100: u16, count_50: u16, count_miss: u16, legacy_mods: u32, played_at_ms: i64 }` (all fields `pub`; `played_at_ms` = Unix epoch ms, UTC).
  `pub fn read_header(r: &mut impl Read) -> anyhow::Result<Header>` reads only the header and leaves the reader positioned at the replay-data length.
  Private helpers that later tasks reuse: `read_exact_vec`, `read_string`, `read_u8`, `read_u16`, `read_i32`, `read_i64`. Test helpers: `Osr` builder, `push_string`, `push_byte_array`, `compress`, `header`.

- [ ] **Step 1: Add dependencies and module skeleton**

Run: `cargo add anyhow@1 lzma-rs@0.3`

Create `src/lib.rs`:

```rust
pub mod core;
```

Create `src/core.rs`:

```rust
pub mod replay;
```

- [ ] **Step 2: Write the failing tests**

Create `src/core/replay.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const STABLE_VERSION: i32 = 20_250_107;

    struct Osr<'a> {
        version: i32,
        player: &'a str,
        legacy_mods: u32,
        ticks: i64,
        frames: &'a str,
        score_info: Option<&'a str>,
    }

    impl Default for Osr<'_> {
        fn default() -> Self {
            Osr {
                version: STABLE_VERSION,
                player: "player",
                legacy_mods: 0,
                ticks: TICKS_AT_UNIX_EPOCH,
                frames: "0|256|192|0,",
                score_info: None,
            }
        }
    }

    impl Osr<'_> {
        fn bytes(&self) -> Vec<u8> {
            let mut b = vec![0];
            b.extend(self.version.to_le_bytes());
            push_string(&mut b, "0123456789abcdef0123456789abcdef");
            push_string(&mut b, self.player);
            push_string(&mut b, "replay-md5");
            for count in [300u16, 20, 3, 7, 5, 1] {
                b.extend(count.to_le_bytes());
            }
            b.extend(123_456i32.to_le_bytes());
            b.extend(500u16.to_le_bytes());
            b.push(0);
            b.extend((self.legacy_mods as i32).to_le_bytes());
            push_string(&mut b, "");
            b.extend(self.ticks.to_le_bytes());
            push_byte_array(&mut b, &compress(self.frames));
            if let Some(info) = self.score_info {
                b.extend(42i64.to_le_bytes());
                push_byte_array(&mut b, &compress(info));
            }
            b
        }
    }

    // An empty string is written the way osu! writes an absent one: a single 0x00.
    fn push_string(b: &mut Vec<u8>, s: &str) {
        if s.is_empty() {
            b.push(0);
            return;
        }
        b.push(0x0b);
        let mut len = s.len();
        while len >= 0x80 {
            b.push((len & 0x7f) as u8 | 0x80);
            len >>= 7;
        }
        b.push(len as u8);
        b.extend(s.as_bytes());
    }

    fn push_byte_array(b: &mut Vec<u8>, data: &[u8]) {
        b.extend((data.len() as i32).to_le_bytes());
        b.extend(data);
    }

    fn compress(text: &str) -> Vec<u8> {
        if text.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        lzma_rs::lzma_compress(&mut text.as_bytes(), &mut out).unwrap();
        out
    }

    fn header(osr: Osr) -> Header {
        read_header(&mut osr.bytes().as_slice()).unwrap()
    }

    #[test]
    fn reads_header_fields() {
        let header = header(Osr::default());

        assert_eq!(header.mode, 0);
        assert_eq!(header.version, STABLE_VERSION);
        assert_eq!(header.beatmap_md5, "0123456789abcdef0123456789abcdef");
        assert_eq!(header.player, "player");
        assert_eq!(
            (
                header.count_300,
                header.count_100,
                header.count_50,
                header.count_miss
            ),
            (300, 20, 3, 1)
        );
    }

    #[test]
    fn converts_windows_ticks_to_unix_ms() {
        let osr = Osr {
            ticks: TICKS_AT_UNIX_EPOCH + 1_700_000_000_000 * TICKS_PER_MS,
            ..Osr::default()
        };

        assert_eq!(header(osr).played_at_ms, 1_700_000_000_000);
    }

    #[test]
    fn absent_player_name_reads_as_empty() {
        let osr = Osr {
            player: "",
            ..Osr::default()
        };

        assert_eq!(header(osr).player, "");
    }

    #[test]
    fn long_strings_use_multibyte_length_prefix() {
        let long_name = "x".repeat(300);
        let osr = Osr {
            player: &long_name,
            ..Osr::default()
        };

        assert_eq!(header(osr).player, long_name);
    }

    #[test]
    fn non_utf8_player_name_reads_lossily() {
        let mut bytes = Osr::default().bytes();
        let name_at = bytes.windows(6).position(|w| w == b"player").unwrap();
        bytes[name_at] = 0xff;

        let header = read_header(&mut bytes.as_slice()).unwrap();

        assert_eq!(header.player, "\u{fffd}layer");
    }

    #[test]
    fn header_read_stops_before_replay_data() {
        let bytes = Osr::default().bytes();
        let header_only = &bytes[..bytes.len() - compress("0|256|192|0,").len() - 4];

        let header = read_header(&mut &header_only[..]).unwrap();

        assert_eq!(header.player, "player");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib core::replay`
Expected: compile errors `cannot find type Header`, `cannot find function read_header`, `cannot find value TICKS_AT_UNIX_EPOCH`.

- [ ] **Step 4: Write the implementation above the test module**

```rust
use std::io::Read;

use anyhow::{Result, bail, ensure};

const TICKS_AT_UNIX_EPOCH: i64 = 621_355_968_000_000_000;
const TICKS_PER_MS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub mode: u8,
    pub version: i32,
    pub beatmap_md5: String,
    pub player: String,
    pub count_300: u16,
    pub count_100: u16,
    pub count_50: u16,
    pub count_miss: u16,
    pub legacy_mods: u32,
    pub played_at_ms: i64,
}

pub fn read_header(r: &mut impl Read) -> Result<Header> {
    let mode = read_u8(r)?;
    let version = read_i32(r)?;
    let beatmap_md5 = read_string(r)?;
    let player = read_string(r)?;
    let _replay_md5 = read_string(r)?;
    let count_300 = read_u16(r)?;
    let count_100 = read_u16(r)?;
    let count_50 = read_u16(r)?;
    let _count_geki = read_u16(r)?;
    let _count_katu = read_u16(r)?;
    let count_miss = read_u16(r)?;
    let _score = read_i32(r)?;
    let _max_combo = read_u16(r)?;
    let _perfect = read_u8(r)?;
    let legacy_mods = read_i32(r)? as u32;
    let _life_bar = read_string(r)?;
    let ticks = read_i64(r)?;

    Ok(Header {
        mode,
        version,
        beatmap_md5,
        player,
        count_300,
        count_100,
        count_50,
        count_miss,
        legacy_mods,
        played_at_ms: (ticks - TICKS_AT_UNIX_EPOCH) / TICKS_PER_MS,
    })
}

fn read_exact_vec(r: &mut impl Read, len: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.take(len as u64).read_to_end(&mut buf)?;
    ensure!(buf.len() == len, "unexpected end of replay file");
    Ok(buf)
}

fn read_string(r: &mut impl Read) -> Result<String> {
    if read_u8(r)? == 0 {
        return Ok(String::new());
    }
    let len = read_uleb128(r)?;
    Ok(String::from_utf8_lossy(&read_exact_vec(r, len)?).into_owned())
}

fn read_uleb128(r: &mut impl Read) -> Result<usize> {
    let mut value = 0;
    for shift in (0..64).step_by(7) {
        let byte = read_u8(r)?;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    bail!("string length prefix too long")
}

fn read_array<const N: usize>(r: &mut impl Read) -> Result<[u8; N]> {
    let mut buf = [0; N];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_u8(r: &mut impl Read) -> Result<u8> {
    Ok(read_array::<1>(r)?[0])
}

fn read_u16(r: &mut impl Read) -> Result<u16> {
    Ok(u16::from_le_bytes(read_array(r)?))
}

fn read_i32(r: &mut impl Read) -> Result<i32> {
    Ok(i32::from_le_bytes(read_array(r)?))
}

fn read_i64(r: &mut impl Read) -> Result<i64> {
    Ok(i64::from_le_bytes(read_array(r)?))
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib core::replay`
Expected: `test result: ok. 6 passed`.

- [ ] **Step 6: Document the library target in CLAUDE.md**

In `CLAUDE.md`, in the `## Architecture` list, insert this as the first bullet:

```markdown
- `src/lib.rs` exposes `core` as a library; `src/main.rs` is the iced binary on top of it.
```

- [ ] **Step 7: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add Cargo.toml Cargo.lock CLAUDE.md src/lib.rs src/core.rs src/core/replay.rs
git commit -m "feat: read osu! replay headers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Replay data and cursor frames

**Files:**
- Modify: `src/core/replay.rs`

**Interfaces:**
- Consumes: Task 1's `Header`, `read_header`, `read_exact_vec`, `read_i32`, test helpers `Osr`, `compress`.
- Produces: `pub struct Frame { time: i64, x: f32, y: f32, keys: u32 }` (all `pub`, `Copy`; `time` = absolute ms; `keys` = osu! bitflags M1=1, M2=2, K1=4, K2=8, smoke=16).
  `pub struct Replay { header: Header, frames: Vec<Frame> }` (Task 3 adds `mods`).
  `pub fn read_replay(r: &mut impl Read) -> anyhow::Result<Replay>`.
  Private: `read_byte_array`, `decompress`, `parse_frames`. Test helper: `parse(osr) -> Result<Replay>`.

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests`, after `fn header`:

```rust
    fn parse(osr: Osr) -> Result<Replay> {
        read_replay(&mut osr.bytes().as_slice())
    }
```

Add these tests at the end of `mod tests`:

```rust
    #[test]
    fn frame_times_accumulate_deltas() {
        let osr = Osr {
            frames: "0|256|-500|0,-1|256|-500|0,16|100.5|200.25|1,17|101|201|5,",
            ..Osr::default()
        };

        let frames = parse(osr).unwrap().frames;

        let times: Vec<i64> = frames.iter().map(|f| f.time).collect();
        assert_eq!(times, [0, -1, 15, 32]);
        assert_eq!(
            frames[2],
            Frame {
                time: 15,
                x: 100.5,
                y: 200.25,
                keys: 1
            }
        );
    }

    #[test]
    fn seed_frame_is_skipped() {
        let osr = Osr {
            frames: "16|1|2|0,-12345|0|0|987654,",
            ..Osr::default()
        };

        assert_eq!(parse(osr).unwrap().frames.len(), 1);
    }

    #[test]
    fn fractional_delta_is_rounded() {
        let osr = Osr {
            frames: "16.6|1|2|0,",
            ..Osr::default()
        };

        assert_eq!(parse(osr).unwrap().frames[0].time, 17);
    }

    #[test]
    fn empty_replay_data_has_no_frames() {
        let osr = Osr {
            frames: "",
            ..Osr::default()
        };

        assert!(parse(osr).unwrap().frames.is_empty());
    }

    #[test]
    fn negative_replay_data_length_has_no_frames() {
        let mut bytes = Osr {
            frames: "",
            ..Osr::default()
        }
        .bytes();
        let len_at = bytes.len() - 4;
        bytes[len_at..].copy_from_slice(&(-1i32).to_le_bytes());

        assert!(read_replay(&mut bytes.as_slice()).unwrap().frames.is_empty());
    }

    #[test]
    fn corrupt_length_prefix_is_an_error() {
        let mut bytes = Osr::default().bytes();
        let len_at = bytes.len() - compress("0|256|192|0,").len() - 4;
        bytes[len_at..len_at + 4].copy_from_slice(&i32::MAX.to_le_bytes());

        assert!(read_replay(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn malformed_frame_is_an_error() {
        let osr = Osr {
            frames: "16|abc|2|0,",
            ..Osr::default()
        };

        let error = parse(osr).unwrap_err();

        assert!(format!("{error:#}").contains("malformed replay frame"));
    }

    #[test]
    fn real_replays_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            eprintln!("skipped: no {}", dir.display());
            return;
        };
        for path in entries.map(|e| e.unwrap().path()) {
            if path.extension().is_none_or(|ext| ext != "osr") {
                continue;
            }
            let replay = read_replay(&mut std::fs::File::open(&path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
            let header = read_header(&mut std::fs::File::open(&path).unwrap()).unwrap();

            assert_eq!(header, replay.header);
            assert_eq!(header.beatmap_md5.len(), 32);
            assert!(replay.frames.len() > 100, "{}", path.display());
            assert!(replay.frames.last().unwrap().time > 0);
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::replay`
Expected: compile errors `cannot find type Replay`, `cannot find function read_replay`, `cannot find struct Frame`.

- [ ] **Step 3: Implement**

Change the `anyhow` import to:

```rust
use anyhow::{Context, Result, bail, ensure};
```

Add after `TICKS_PER_MS`:

```rust
const SEED_FRAME_DELTA: &str = "-12345";
```

Add after `struct Header`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub time: i64,
    pub x: f32,
    pub y: f32,
    pub keys: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub header: Header,
    pub frames: Vec<Frame>,
}
```

Add after `fn read_header`:

```rust
pub fn read_replay(r: &mut impl Read) -> Result<Replay> {
    let header = read_header(r).context("reading replay header")?;
    let data = read_byte_array(r).context("reading replay data")?;
    let frames = parse_frames(&decompress(&data).context("decompressing replay data")?)?;

    Ok(Replay { header, frames })
}

fn parse_frames(text: &str) -> Result<Vec<Frame>> {
    let mut time = 0;
    let mut frames = Vec::new();
    for raw in text.split(',') {
        let parts: Vec<&str> = raw.split('|').collect();
        if parts.len() < 4 || parts[0] == SEED_FRAME_DELTA {
            continue;
        }
        let malformed = || format!("malformed replay frame {raw:?}");
        let delta: f64 = parts[0].parse().with_context(malformed)?;
        time += delta.round() as i64;
        frames.push(Frame {
            time,
            x: parts[1].parse().with_context(malformed)?,
            y: parts[2].parse().with_context(malformed)?,
            keys: parts[3].parse().with_context(malformed)?,
        });
    }
    Ok(frames)
}

fn decompress(data: &[u8]) -> Result<String> {
    if data.is_empty() {
        return Ok(String::new());
    }
    let mut input = data;
    let mut out = Vec::new();
    lzma_rs::lzma_decompress(&mut input, &mut out)?;
    Ok(String::from_utf8(out)?)
}
```

Add after `fn read_exact_vec`:

```rust
fn read_byte_array(r: &mut impl Read) -> Result<Vec<u8>> {
    let len = read_i32(r)?;
    if len <= 0 {
        return Ok(Vec::new());
    }
    read_exact_vec(r, len as usize)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib core::replay -- --nocapture`
Expected: `test result: ok. 14 passed`. `real_replays_parse` either parses every `.osr` in `tests/fixtures/` or prints `skipped: no …`.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add src/core/replay.rs
git commit -m "feat: decode replay cursor frames

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Mods (legacy bitmask and lazer score info)

**Files:**
- Modify: `Cargo.toml` (deps `serde` with `derive`, `serde_json`), `src/core/replay.rs`

**Interfaces:**
- Consumes: Task 2's `Replay`, `read_replay`, `read_byte_array`, `decompress`, test helpers `Osr`, `parse`.
- Produces: `pub struct Mod { acronym: String, settings: serde_json::Map<String, serde_json::Value> }` (both `pub`).
  `Replay` gains `pub mods: Vec<Mod>`. Legacy bits map to osu! acronyms ordered by bit, implied bits are dropped (NC drops DT, PF drops SD), and unknown bits become `"bit<N>"` so a later mod whitelist rejects them. When `version >= 30000001`, the lazer score-info mods replace the legacy list.

- [ ] **Step 1: Add dependencies**

Run: `cargo add serde@1 --features derive && cargo add serde_json@1`

- [ ] **Step 2: Write the failing tests**

Add inside `mod tests`, after `const STABLE_VERSION`:

```rust
    const LAZER_VERSION: i32 = 30_000_017;
```

Add after `fn parse`:

```rust
    fn acronyms(replay: &Replay) -> Vec<&str> {
        replay.mods.iter().map(|m| m.acronym.as_str()).collect()
    }
```

Add these tests at the end of `mod tests`:

```rust
    #[test]
    fn legacy_mods_drop_implied_bits() {
        let osr = Osr {
            legacy_mods: DOUBLE_TIME | NIGHTCORE | (1 << 3) | (1 << 4) | SUDDEN_DEATH | PERFECT,
            ..Osr::default()
        };

        assert_eq!(acronyms(&parse(osr).unwrap()), ["HD", "HR", "NC", "PF"]);
    }

    #[test]
    fn unknown_legacy_bits_are_kept() {
        let osr = Osr {
            legacy_mods: 1 << 15,
            ..Osr::default()
        };

        assert_eq!(acronyms(&parse(osr).unwrap()), ["bit15"]);
    }

    #[test]
    fn lazer_score_info_mods_replace_legacy_mods() {
        let osr = Osr {
            version: LAZER_VERSION,
            legacy_mods: 1 << 4,
            score_info: Some(
                r#"{"online_id":1,"mods":[{"acronym":"HR"},{"acronym":"MR","settings":{"reflection":"1"}}]}"#,
            ),
            ..Osr::default()
        };

        let replay = parse(osr).unwrap();

        assert_eq!(acronyms(&replay), ["HR", "MR"]);
        assert_eq!(replay.mods[1].settings["reflection"], "1");
    }

    #[test]
    fn lazer_score_info_without_mods_gives_no_mods() {
        let osr = Osr {
            version: LAZER_VERSION,
            legacy_mods: 1 << 4,
            score_info: Some(r#"{"online_id":1,"rank":"A"}"#),
            ..Osr::default()
        };

        assert!(parse(osr).unwrap().mods.is_empty());
    }

    #[test]
    fn first_lazer_version_without_score_info_uses_legacy_mods() {
        let osr = Osr {
            version: 30_000_000,
            legacy_mods: 1 << 4,
            ..Osr::default()
        };

        assert_eq!(acronyms(&parse(osr).unwrap()), ["HR"]);
    }

    #[test]
    fn truncated_replay_is_an_error() {
        let bytes = Osr {
            version: LAZER_VERSION,
            score_info: Some(r#"{"mods":[]}"#),
            ..Osr::default()
        }
        .bytes();

        for len in 0..bytes.len() {
            assert!(
                read_replay(&mut &bytes[..len]).is_err(),
                "prefix {len} parsed"
            );
        }
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib core::replay`
Expected: compile errors `no field mods on type Replay`, `cannot find value DOUBLE_TIME` (and NIGHTCORE, SUDDEN_DEATH, PERFECT).

- [ ] **Step 4: Implement**

Add after the `anyhow` import:

```rust
use serde::Deserialize;
use serde_json::{Map, Value};
```

Add before `TICKS_AT_UNIX_EPOCH`:

```rust
// From this version on, lazer appends a JSON score-info block that holds the full mod list.
const FIRST_LAZER_SCORE_INFO_VERSION: i32 = 30_000_001;
```

Add after `SEED_FRAME_DELTA`:

```rust
const DOUBLE_TIME: u32 = 1 << 6;
const NIGHTCORE: u32 = 1 << 9;
const SUDDEN_DEATH: u32 = 1 << 5;
const PERFECT: u32 = 1 << 14;
const LEGACY_MODS: [(u32, &str); 19] = [
    (1 << 0, "NF"),
    (1 << 1, "EZ"),
    (1 << 2, "TD"),
    (1 << 3, "HD"),
    (1 << 4, "HR"),
    (SUDDEN_DEATH, "SD"),
    (DOUBLE_TIME, "DT"),
    (1 << 7, "RX"),
    (1 << 8, "HT"),
    (NIGHTCORE, "NC"),
    (1 << 10, "FL"),
    (1 << 11, "AT"),
    (1 << 12, "SO"),
    (1 << 13, "AP"),
    (PERFECT, "PF"),
    (1 << 22, "CN"),
    (1 << 23, "TP"),
    (1 << 29, "SV2"),
    (1 << 30, "MR"),
];
```

Add after `struct Frame`:

```rust
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Mod {
    pub acronym: String,
    #[serde(default)]
    pub settings: Map<String, Value>,
}
```

Add the field to `Replay`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub header: Header,
    pub frames: Vec<Frame>,
    pub mods: Vec<Mod>,
}
```

Replace `read_replay` with:

```rust
pub fn read_replay(r: &mut impl Read) -> Result<Replay> {
    let header = read_header(r).context("reading replay header")?;
    let data = read_byte_array(r).context("reading replay data")?;
    let frames = parse_frames(&decompress(&data).context("decompressing replay data")?)?;
    let mods = if header.version >= FIRST_LAZER_SCORE_INFO_VERSION {
        let _online_id = read_i64(r)?;
        let info = read_byte_array(r).context("reading lazer score info")?;
        lazer_mods(&decompress(&info).context("decompressing lazer score info")?)?
    } else {
        legacy_mods(header.legacy_mods)
    };

    Ok(Replay {
        header,
        frames,
        mods,
    })
}
```

Add after `fn parse_frames`:

```rust
fn legacy_mods(bits: u32) -> Vec<Mod> {
    let mut bits = bits;
    if bits & NIGHTCORE != 0 {
        bits &= !DOUBLE_TIME;
    }
    if bits & PERFECT != 0 {
        bits &= !SUDDEN_DEATH;
    }
    (0..32)
        .map(|i| 1u32 << i)
        .filter(|bit| bits & bit != 0)
        .map(|bit| {
            let acronym = LEGACY_MODS
                .iter()
                .find(|(known, _)| *known == bit)
                .map_or_else(
                    || format!("bit{}", bit.trailing_zeros()),
                    |(_, acronym)| acronym.to_string(),
                );
            Mod {
                acronym,
                settings: Map::new(),
            }
        })
        .collect()
}

fn lazer_mods(json: &str) -> Result<Vec<Mod>> {
    #[derive(Deserialize)]
    struct ScoreInfo {
        #[serde(default)]
        mods: Vec<Mod>,
    }
    let info: ScoreInfo = serde_json::from_str(json).context("parsing lazer score info")?;
    Ok(info.mods)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib core::replay -- --nocapture`
Expected: `test result: ok. 20 passed`. With fixtures present, `real_replays_parse` now also runs the lazer score-info path on any lazer-exported file.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add Cargo.toml Cargo.lock src/core/replay.rs
git commit -m "feat: read replay mods from legacy bitmask and lazer score info

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Verify on GitHub and merge (CLAUDE.md feature flow)

**Files:** none new.

**Interfaces:**
- Consumes: Tasks 1–3 on `feature/replay-parser`.
- Produces: `core::replay` merged into `dev`.

- [ ] **Step 1: Local gate**

Run: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`
Expected: all pass, 20 tests.

- [ ] **Step 2: Refactor new code only**

Re-read `src/core/replay.rs` for naming, duplication and dead code. If anything changes, re-run Step 1 and commit `refactor: ...`.

- [ ] **Step 3: README check**

Parsing isn't user-visible yet, so the README stays unchanged. Confirm that's still true.

- [ ] **Step 4: Push, open the PR, CI green**

```bash
git push -u origin feature/replay-parser
gh pr create --base dev --title "feat: osu! replay parser" --body "core::replay: header-only and full .osr reads (stable + lazer), frames, mods.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
gh pr checks --watch
```

Expected: `check` passes. In CI the fixture test prints `skipped: no …/tests/fixtures`.

- [ ] **Step 5: Whole-branch review, then squash-merge and clean up**

After the final review's fixes are committed and pushed and CI is green:

```bash
gh pr merge --squash --delete-branch
git switch dev && git pull && git fetch --prune
git branch -vv | grep ': gone]' || true
```

- [ ] **Step 6: Ask the user**

"Merge dev into main?" Wait for an explicit answer.

## Notes for later features

- Feature 4 (hits): when a beatmap's `osu file format` version is below 5, add 24 ms to every frame time. That's lazer's `EARLY_VERSION_TIMING_OFFSET`, which it applies while decoding.
- Feature 4: frames at (256, −500) are placeholders that stable writes before play starts. They never carry key presses, so press matching ignores them naturally.
- The fixture "lazer" replay is a stable play re-exported by lazer (identical frames). A replay actually played in lazer would cover lazer's own frame timing.
