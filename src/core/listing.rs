use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::core::library::{self, Library};
use crate::core::replay::{self, Header};

const MS_PER_DAY: i64 = 86_400_000;
// A pause this long between two plays starts a new session.
const SESSION_GAP_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    LastSession,
    Week,
    #[default]
    Month,
    All,
}

impl Period {
    pub const ALL: [Period; 4] = [
        Period::LastSession,
        Period::Week,
        Period::Month,
        Period::All,
    ];
}

impl std::fmt::Display for Period {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Period::LastSession => "Last session",
            Period::Week => "Last 7 days",
            Period::Month => "Last 30 days",
            Period::All => "All time",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub path: PathBuf,
    pub played_at_ms: i64,
    pub player: String,
    pub title: String,
    pub mods: String,
    pub accuracy: f64,
}

// osu!standard replays of the given players (all players when empty), newest first.
pub fn rows(
    library: &Library,
    players: &[String],
    period: Period,
    search: &str,
    now_ms: i64,
) -> Vec<Row> {
    let beatmaps = library.beatmaps_by_md5();
    let mut rows: Vec<Row> = library
        .replays()
        .filter(|(_, header)| header.mode == 0 && is_played_by(header, players))
        .map(|(path, header)| row(path, header, &beatmaps))
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.played_at_ms));

    let since = match period {
        Period::LastSession => session_start(&rows),
        Period::Week => now_ms - 7 * MS_PER_DAY,
        Period::Month => now_ms - 30 * MS_PER_DAY,
        Period::All => i64::MIN,
    };
    let search = search.to_lowercase();
    rows.retain(|row| row.played_at_ms >= since && row.title.to_lowercase().contains(&search));
    rows
}

// For replays the user picked or dropped in; those are never filtered by player.
pub fn row_for_file(path: &Path, beatmaps: &HashMap<&str, library::Beatmap<'_>>) -> Result<Row> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let header = replay::read_header(&mut BufReader::new(file))
        .with_context(|| format!("reading {}", path.display()))?;
    ensure!(
        header.mode == 0,
        "{} is not an osu!standard replay",
        path.display()
    );
    Ok(row(path, &header, beatmaps))
}

fn is_played_by(header: &Header, players: &[String]) -> bool {
    players.is_empty()
        || players
            .iter()
            .any(|player| player.eq_ignore_ascii_case(&header.player))
}

fn session_start(newest_first: &[Row]) -> i64 {
    let mut start = newest_first
        .first()
        .map_or(i64::MIN, |row| row.played_at_ms);
    for row in newest_first {
        if start - row.played_at_ms > SESSION_GAP_MS {
            break;
        }
        start = row.played_at_ms;
    }
    start
}

fn row(path: &Path, header: &Header, beatmaps: &HashMap<&str, library::Beatmap<'_>>) -> Row {
    Row {
        path: path.to_path_buf(),
        played_at_ms: header.played_at_ms,
        player: header.player.clone(),
        title: beatmaps.get(header.beatmap_md5.as_str()).map_or_else(
            || "(beatmap not installed)".to_string(),
            |b| b.title.to_string(),
        ),
        mods: mods(header.legacy_mods),
        accuracy: accuracy(header),
    }
}

fn mods(bits: u32) -> String {
    let acronyms: Vec<String> = replay::legacy_mods(bits)
        .into_iter()
        .map(|m| m.acronym)
        .collect();
    if acronyms.is_empty() {
        "NM".to_string()
    } else {
        acronyms.concat()
    }
}

fn accuracy(header: &Header) -> f64 {
    let [great, ok, meh, miss] = [
        header.count_300,
        header.count_100,
        header.count_50,
        header.count_miss,
    ]
    .map(f64::from);
    let total = great + ok + meh + miss;
    if total == 0.0 {
        return 0.0;
    }
    (300.0 * great + 100.0 * ok + 50.0 * meh) / (300.0 * total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::library::tests::TempDir;
    use crate::core::replay::tests::Osr;

    const HOUR_MS: i64 = 60 * 60 * 1000;
    const NOW_MS: i64 = 1_800_000_000_000;
    const BEATMAP: &str =
        "osu file format v14\n\n[Metadata]\nTitle:Song\nArtist:Band\nVersion:Hard\n";

    fn ticks(played_at_ms: i64) -> i64 {
        621_355_968_000_000_000 + played_at_ms * 10_000
    }

    fn library_with(name: &str, replays: &[(&str, &str, i64)]) -> (TempDir, Library) {
        let dir = TempDir::new(name);
        let md5 = format!("{:x}", md5::compute(BEATMAP));
        dir.write("Songs/set/map.osu", BEATMAP);
        for (i, (name, player, played_at_ms)) in replays.iter().enumerate() {
            let bytes = Osr {
                player,
                beatmap_md5: &md5,
                ticks: ticks(*played_at_ms),
                ..Osr::default()
            }
            .bytes();
            dir.write(&format!("Data/r/{i}-{name}.osr"), bytes);
        }
        let mut library = Library::default();
        library.scan(dir.roots(), &|_, _| {});
        (dir, library)
    }

    fn players(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.player.as_str()).collect()
    }

    #[test]
    fn rows_are_newest_first_for_the_chosen_players() {
        let (_dir, library) = library_with(
            "listing-players",
            &[
                ("a", "me", NOW_MS - 3 * HOUR_MS),
                ("b", "someone else", NOW_MS - 2 * HOUR_MS),
                ("c", "ME", NOW_MS - HOUR_MS),
            ],
        );

        let rows = rows(&library, &["me".to_string()], Period::All, "", NOW_MS);

        assert_eq!(players(&rows), ["ME", "me"]);
        assert_eq!(rows[0].title, "Band - Song [Hard]");
    }

    #[test]
    fn no_players_means_everyone() {
        let (_dir, library) = library_with(
            "listing-everyone",
            &[("a", "me", NOW_MS), ("b", "you", NOW_MS)],
        );

        assert_eq!(rows(&library, &[], Period::All, "", NOW_MS).len(), 2);
    }

    #[test]
    fn last_session_stops_at_an_hour_long_pause() {
        let (_dir, library) = library_with(
            "listing-session",
            &[
                ("old", "a", NOW_MS - 10 * HOUR_MS),
                ("start", "b", NOW_MS - 2 * HOUR_MS),
                ("middle", "c", NOW_MS - HOUR_MS - 30 * 60 * 1000),
                ("end", "d", NOW_MS - HOUR_MS),
            ],
        );

        let rows = rows(&library, &[], Period::LastSession, "", NOW_MS);

        assert_eq!(players(&rows), ["d", "c", "b"]);
    }

    #[test]
    fn week_and_month_count_back_from_now() {
        let (_dir, library) = library_with(
            "listing-period",
            &[
                ("recent", "a", NOW_MS - 2 * MS_PER_DAY),
                ("weeks", "b", NOW_MS - 20 * MS_PER_DAY),
                ("old", "c", NOW_MS - 40 * MS_PER_DAY),
            ],
        );

        assert_eq!(
            players(&rows(&library, &[], Period::Week, "", NOW_MS)),
            ["a"]
        );
        assert_eq!(
            players(&rows(&library, &[], Period::Month, "", NOW_MS)),
            ["a", "b"]
        );
        assert_eq!(rows(&library, &[], Period::All, "", NOW_MS).len(), 3);
    }

    #[test]
    fn search_matches_titles_ignoring_case() {
        let (_dir, library) = library_with("listing-search", &[("a", "me", NOW_MS)]);

        assert_eq!(
            rows(&library, &[], Period::All, "band - SONG", NOW_MS).len(),
            1
        );
        assert!(rows(&library, &[], Period::All, "other", NOW_MS).is_empty());
    }

    #[test]
    fn rows_show_mods_and_accuracy() {
        let (dir, library) = library_with("listing-mods", &[]);
        let path = dir.write(
            "Replays/hddt.osr",
            Osr {
                legacy_mods: (1 << 3) | (1 << 6),
                ..Osr::default()
            }
            .bytes(),
        );

        let row = row_for_file(&path, &library.beatmaps_by_md5()).unwrap();

        assert_eq!(row.mods, "HDDT");
        assert!((row.accuracy - 92_150.0 / 97_200.0).abs() < 1e-12);
        assert_eq!(row.title, "(beatmap not installed)");
        assert_eq!(mods(0), "NM");
    }

    #[test]
    fn other_modes_are_not_listed() {
        let (dir, mut library) = library_with("listing-modes", &[]);
        let mut bytes = Osr::default().bytes();
        bytes[0] = 3;
        let path = dir.write("Data/r/mania.osr", bytes);
        library.scan(dir.roots(), &|_, _| {});

        assert!(rows(&library, &[], Period::All, "", NOW_MS).is_empty());
        assert!(row_for_file(&path, &library.beatmaps_by_md5()).is_err());
    }
}
