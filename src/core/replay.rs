use std::io::Read;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// From this version on, lazer appends a JSON score-info block that holds the full mod list.
const FIRST_LAZER_SCORE_INFO_VERSION: i32 = 30_000_001;
const TICKS_AT_UNIX_EPOCH: i64 = 621_355_968_000_000_000;
const TICKS_PER_MS: i64 = 10_000;
const SEED_FRAME_DELTA: &str = "-12345";

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl Header {
    pub fn is_standard(&self) -> bool {
        self.mode == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub time: i64,
    pub x: f32,
    pub y: f32,
    pub keys: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Mod {
    pub acronym: String,
    #[serde(default)]
    pub settings: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub header: Header,
    pub frames: Vec<Frame>,
    pub mods: Vec<Mod>,
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
    ensure!(ticks >= 0, "invalid replay timestamp {ticks}");

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

fn parse_frames(text: &str) -> Result<Vec<Frame>> {
    let mut time: i64 = 0;
    let mut frames = Vec::new();
    for raw in text.split(',') {
        let parts: Vec<&str> = raw.split('|').collect();
        if parts.len() < 4 || parts[0] == SEED_FRAME_DELTA {
            continue;
        }
        let malformed = || format!("malformed replay frame {raw:?}");
        let delta: f64 = parts[0].parse().with_context(malformed)?;
        ensure!(delta.is_finite(), malformed());
        time = time
            .checked_add(delta.round() as i64)
            .with_context(malformed)?;
        frames.push(Frame {
            time,
            x: parts[1].parse().with_context(malformed)?,
            y: parts[2].parse().with_context(malformed)?,
            keys: parts[3].parse().with_context(malformed)?,
        });
    }
    Ok(frames)
}

pub fn legacy_mods(mut bits: u32) -> Vec<Mod> {
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

fn decompress(data: &[u8]) -> Result<String> {
    if data.is_empty() {
        return Ok(String::new());
    }
    let mut input = data;
    let mut out = Vec::new();
    lzma_rs::lzma_decompress(&mut input, &mut out)?;
    Ok(String::from_utf8(out)?)
}

fn read_exact_vec(r: &mut impl Read, len: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.take(len as u64).read_to_end(&mut buf)?;
    ensure!(buf.len() == len, "unexpected end of replay file");
    Ok(buf)
}

fn read_byte_array(r: &mut impl Read) -> Result<Vec<u8>> {
    let len = read_i32(r)?;
    if len <= 0 {
        return Ok(Vec::new());
    }
    read_exact_vec(r, len as usize)
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const STABLE_VERSION: i32 = 20_250_107;
    const LAZER_VERSION: i32 = 30_000_017;

    pub(crate) struct Osr<'a> {
        pub(crate) version: i32,
        pub(crate) player: &'a str,
        pub(crate) beatmap_md5: &'a str,
        pub(crate) legacy_mods: u32,
        pub(crate) ticks: i64,
        pub(crate) frames: &'a str,
        pub(crate) score_info: Option<&'a str>,
    }

    impl Default for Osr<'_> {
        fn default() -> Self {
            Osr {
                version: STABLE_VERSION,
                player: "player",
                beatmap_md5: "0123456789abcdef0123456789abcdef",
                legacy_mods: 0,
                ticks: TICKS_AT_UNIX_EPOCH,
                frames: "0|256|192|0,",
                score_info: None,
            }
        }
    }

    impl Osr<'_> {
        pub(crate) fn bytes(&self) -> Vec<u8> {
            let mut b = vec![0];
            b.extend(self.version.to_le_bytes());
            push_string(&mut b, self.beatmap_md5);
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

    fn parse(osr: Osr) -> Result<Replay> {
        read_replay(&mut osr.bytes().as_slice())
    }

    fn acronyms(replay: &Replay) -> Vec<&str> {
        replay.mods.iter().map(|m| m.acronym.as_str()).collect()
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
    fn negative_timestamp_is_an_error() {
        let osr = Osr {
            ticks: i64::MIN,
            ..Osr::default()
        };

        assert!(read_header(&mut osr.bytes().as_slice()).is_err());
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

        assert!(
            read_replay(&mut bytes.as_slice())
                .unwrap()
                .frames
                .is_empty()
        );
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
    fn non_finite_frame_delta_is_an_error() {
        for frames in ["inf|1|2|0,", "NaN|1|2|0,"] {
            let osr = Osr {
                frames,
                ..Osr::default()
            };

            assert!(parse(osr).is_err(), "{frames}");
        }
    }

    #[test]
    fn overflowing_frame_time_is_an_error() {
        let osr = Osr {
            frames: "9e18|1|2|0,9e18|1|2|0,",
            ..Osr::default()
        };

        assert!(parse(osr).is_err());
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
}
