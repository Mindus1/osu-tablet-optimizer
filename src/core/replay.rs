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
