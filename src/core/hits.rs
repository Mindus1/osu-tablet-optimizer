use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use rosu_map::Beatmap;
use rosu_map::section::hit_objects::HitObjectKind;
use serde_json::Value;

use crate::core::library;
use crate::core::replay::{self, Frame, Mod, Replay};

const PLAYFIELD_WIDTH: f64 = 512.0;
const PLAYFIELD_HEIGHT: f64 = 384.0;
const KEY_A: u32 = 1 | 4;
const KEY_B: u32 = 2 | 8;
// ponytail: fixed structural limits; make them settings if users need to tune them.
const MAX_GAP_MS: f64 = 1000.0;
const STACK_DISTANCE: f64 = 3.0;
// Longer than any osu! stack window (approach time ≤ 1800 ms × stack leniency ≤ 1).
const STACK_WINDOW_MS: f64 = 2000.0;
// Mods that leave object positions alone, plus the ones applied below (HR, EZ, MR, DA).
const SUPPORTED_MODS: [&str; 31] = [
    "NF", "EZ", "HD", "HR", "SD", "PF", "DT", "NC", "HT", "DC", "FL", "SO", "SV2", "CL", "MU",
    "BL", "NS", "TC", "AC", "ST", "SG", "AL", "GR", "DF", "AD", "SY", "BU", "SI", "FR", "MR", "DA",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    // Object centre minus the cursor at the previous hit: where the player meant to move.
    pub intended: (f64, f64),
    // Cursor at this press minus the cursor at the previous hit: where they actually moved.
    pub actual: (f64, f64),
    pub radius: f64,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    time: f64,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct Target {
    time: f64,
    x: f64,
    y: f64,
    is_circle: bool,
    stacked: bool,
}

struct Difficulty {
    circle_size: f64,
    overall_difficulty: f64,
    flip_x: bool,
    flip_y: bool,
}

pub fn replay_samples(
    replay_path: &Path,
    beatmaps: &HashMap<&str, library::Beatmap<'_>>,
) -> Result<Vec<Sample>> {
    let file =
        File::open(replay_path).with_context(|| format!("opening {}", replay_path.display()))?;
    let replay = replay::read_replay(&mut BufReader::new(file))?;
    let beatmap_path = beatmaps
        .get(replay.header.beatmap_md5.as_str())
        .map(|beatmap| beatmap.path)
        .context("beatmap not found in the osu! library")?;
    let beatmap = Beatmap::from_path(beatmap_path)
        .with_context(|| format!("reading {}", beatmap_path.display()))?;
    samples(&replay, &beatmap)
}

pub fn samples(replay: &Replay, beatmap: &Beatmap) -> Result<Vec<Sample>> {
    ensure!(replay.header.mode == 0, "not an osu!standard replay");
    let difficulty = difficulty(beatmap, &replay.mods)?;
    let radius = 54.4 - 4.48 * difficulty.circle_size;
    let window = (200.0 - 10.0 * difficulty.overall_difficulty).floor() - 0.5;
    let presses = presses(&replay.frames);

    let mut next_press = 0;
    let mut previous: Option<(Target, Press)> = None;
    let mut samples = Vec::new();
    for target in targets(beatmap, &difficulty) {
        let Some(target) = target else {
            previous = None;
            continue;
        };
        let hit = take_press(&presses, &mut next_press, target.time, window);
        if let (Some(press), Some((from, start))) = (hit, previous)
            && from.is_circle
            && !target.stacked
            && target.time - from.time <= MAX_GAP_MS
        {
            samples.push(Sample {
                intended: (target.x - start.x, target.y - start.y),
                actual: (press.x - start.x, press.y - start.y),
                radius,
            });
        }
        previous = hit.map(|press| (target, press));
    }
    Ok(samples)
}

fn difficulty(beatmap: &Beatmap, mods: &[Mod]) -> Result<Difficulty> {
    let unsupported: Vec<&str> = mods
        .iter()
        .map(|m| m.acronym.as_str())
        .filter(|acronym| !SUPPORTED_MODS.contains(acronym))
        .collect();
    ensure!(
        unsupported.is_empty(),
        "unsupported mods: {}",
        unsupported.join(", ")
    );

    let mut difficulty = Difficulty {
        circle_size: f64::from(beatmap.circle_size),
        overall_difficulty: f64::from(beatmap.overall_difficulty),
        flip_x: false,
        flip_y: false,
    };
    for m in mods {
        match m.acronym.as_str() {
            "HR" => {
                difficulty.circle_size = (difficulty.circle_size * 1.3).min(10.0);
                difficulty.overall_difficulty = (difficulty.overall_difficulty * 1.4).min(10.0);
                difficulty.flip_y = !difficulty.flip_y;
            }
            "EZ" => {
                difficulty.circle_size *= 0.5;
                difficulty.overall_difficulty *= 0.5;
            }
            "DA" => {
                let setting = |key| m.settings.get(key).and_then(Value::as_f64);
                if let Some(cs) = setting("circle_size") {
                    difficulty.circle_size = cs;
                }
                if let Some(od) = setting("overall_difficulty") {
                    difficulty.overall_difficulty = od;
                }
            }
            "MR" => {
                let (x, y) = mirror_axes(m);
                difficulty.flip_x ^= x;
                difficulty.flip_y ^= y;
            }
            _ => {}
        }
    }
    Ok(difficulty)
}

// lazer stores Mirror's axis as MirrorType: 0 = horizontal (the default), 1 = vertical, 2 = both.
fn mirror_axes(m: &Mod) -> (bool, bool) {
    let reflection = match m.settings.get("reflection") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => match s.as_str() {
            "Vertical" => Some(1),
            "Both" => Some(2),
            other => other.parse().ok(),
        },
        _ => None,
    }
    .unwrap_or(0);
    (reflection != 1, reflection != 0)
}

fn presses(frames: &[Frame]) -> Vec<Press> {
    let mut held = 0;
    let mut presses = Vec::new();
    for frame in frames {
        for key in [KEY_A, KEY_B] {
            if frame.keys & key != 0 && held & key == 0 {
                presses.push(Press {
                    time: frame.time as f64,
                    x: f64::from(frame.x),
                    y: f64::from(frame.y),
                });
            }
        }
        held = frame.keys;
    }
    presses.sort_by(|a, b| a.time.total_cmp(&b.time));
    presses
}

// Spinners become None: they are not aim targets and break the chain of hits.
fn targets(beatmap: &Beatmap, difficulty: &Difficulty) -> Vec<Option<Target>> {
    let mut targets: Vec<Option<Target>> = beatmap
        .hit_objects
        .iter()
        .map(|object| {
            let (pos, is_circle) = match &object.kind {
                HitObjectKind::Circle(circle) => (circle.pos, true),
                HitObjectKind::Slider(slider) => (slider.pos, false),
                _ => return None,
            };
            let x = f64::from(pos.x);
            let y = f64::from(pos.y);
            Some(Target {
                time: object.start_time,
                x: if difficulty.flip_x {
                    PLAYFIELD_WIDTH - x
                } else {
                    x
                },
                y: if difficulty.flip_y {
                    PLAYFIELD_HEIGHT - y
                } else {
                    y
                },
                is_circle,
                stacked: false,
            })
        })
        .collect();

    // osu! nudges an object apart from any earlier one within 3 px and its stack window, so its
    // file position is not where it was drawn; such objects are left out instead of re-stacked.
    // ponytail: objects stacked on slider ends are not detected (needs slider end positions).
    for i in 0..targets.len() {
        let Some(a) = targets[i] else { continue };
        for j in i + 1..targets.len() {
            let Some(b) = targets[j] else { continue };
            if b.time - a.time > STACK_WINDOW_MS {
                break;
            }
            if (a.x - b.x).hypot(a.y - b.y) < STACK_DISTANCE {
                for k in [i, j] {
                    if let Some(target) = &mut targets[k] {
                        target.stacked = true;
                    }
                }
            }
        }
    }
    targets
}

fn take_press(presses: &[Press], next: &mut usize, time: f64, window: f64) -> Option<Press> {
    while presses
        .get(*next)
        .is_some_and(|press| press.time < time - window)
    {
        *next += 1;
    }
    let press = presses
        .get(*next)
        .filter(|press| press.time <= time + window)?;
    *next += 1;
    Some(*press)
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;
    use crate::core::replay::Header;

    fn beatmap(
        format: u32,
        circle_size: f32,
        overall_difficulty: f32,
        objects: &[&str],
    ) -> Beatmap {
        let text = format!(
            "osu file format v{format}\n\n[Difficulty]\nCircleSize:{circle_size}\nOverallDifficulty:{overall_difficulty}\nSliderMultiplier:1.4\n\n[TimingPoints]\n0,500,4,2,0,100,1,0\n\n[HitObjects]\n{}\n",
            objects.join("\n")
        );
        Beatmap::from_bytes(text.as_bytes()).unwrap()
    }

    fn with_mods(mods: &[&str]) -> Vec<Mod> {
        mods.iter()
            .map(|acronym| Mod {
                acronym: acronym.to_string(),
                settings: Map::new(),
            })
            .collect()
    }

    // Each tap presses key A at (time, x, y) and releases it 30 ms later.
    fn replay(mods: Vec<Mod>, taps: &[(i64, f32, f32)]) -> Replay {
        let frames = taps
            .iter()
            .flat_map(|&(time, x, y)| {
                [
                    Frame {
                        time,
                        x,
                        y,
                        keys: 4 | 1,
                    },
                    Frame {
                        time: time + 30,
                        x,
                        y,
                        keys: 0,
                    },
                ]
            })
            .collect();
        Replay {
            header: Header {
                mode: 0,
                version: 30_000_017,
                beatmap_md5: String::new(),
                player: String::new(),
                count_300: 0,
                count_100: 0,
                count_50: 0,
                count_miss: 0,
                legacy_mods: 0,
                played_at_ms: 0,
            },
            frames,
            mods,
        }
    }

    const JUMP: [&str; 2] = ["100,100,1000,1,0", "400,100,1500,1,0"];

    #[test]
    fn jump_gives_one_sample_relative_to_the_previous_hit() {
        let map = beatmap(14, 4.0, 8.0, &JUMP);
        let replay = replay(vec![], &[(1000, 102.0, 98.0), (1500, 395.0, 104.0)]);

        let samples = samples(&replay, &map).unwrap();

        assert_eq!(
            samples,
            [Sample {
                intended: (298.0, 2.0),
                actual: (293.0, 6.0),
                radius: 54.4 - 4.48 * 4.0,
            }]
        );
    }

    #[test]
    fn held_key_counts_as_one_press() {
        let frames = [(0, 4), (500, 4), (1000, 0), (1500, 4)].map(|(time, keys)| Frame {
            time,
            x: 0.0,
            y: 0.0,
            keys,
        });

        let times: Vec<f64> = presses(&frames).iter().map(|p| p.time).collect();

        assert_eq!(times, [0.0, 1500.0]);
    }

    #[test]
    fn press_outside_the_hit_window_misses() {
        let map = beatmap(14, 4.0, 10.0, &JUMP);
        let replay = replay(vec![], &[(1000, 100.0, 100.0), (1600, 400.0, 100.0)]);

        assert!(samples(&replay, &map).unwrap().is_empty());
    }

    #[test]
    fn slider_head_is_a_target_but_not_a_start() {
        let map = beatmap(
            14,
            4.0,
            8.0,
            &[
                "100,100,1000,1,0",
                "300,100,1300,2,0,L|300:300,1,100",
                "100,300,1900,1,0",
            ],
        );
        let replay = replay(
            vec![],
            &[
                (1000, 100.0, 100.0),
                (1300, 300.0, 100.0),
                (1900, 100.0, 300.0),
            ],
        );

        let samples = samples(&replay, &map).unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].intended, (200.0, 0.0));
    }

    #[test]
    fn spinner_breaks_the_chain() {
        let map = beatmap(
            14,
            4.0,
            8.0,
            &[
                "100,100,1000,1,0",
                "256,192,1100,8,0,1400",
                "400,100,1500,1,0",
            ],
        );
        let replay = replay(vec![], &[(1000, 100.0, 100.0), (1500, 400.0, 100.0)]);

        assert!(samples(&replay, &map).unwrap().is_empty());
    }

    #[test]
    fn long_gap_gives_no_sample() {
        let map = beatmap(14, 4.0, 8.0, &["100,100,1000,1,0", "400,100,2500,1,0"]);
        let replay = replay(vec![], &[(1000, 100.0, 100.0), (2500, 400.0, 100.0)]);

        assert!(samples(&replay, &map).unwrap().is_empty());
    }

    #[test]
    fn stacked_objects_give_no_sample() {
        let map = beatmap(
            14,
            4.0,
            8.0,
            &["100,100,1000,1,0", "300,100,1200,1,0", "301,101,1400,1,0"],
        );
        let replay = replay(
            vec![],
            &[
                (1000, 100.0, 100.0),
                (1200, 300.0, 100.0),
                (1400, 300.0, 100.0),
            ],
        );

        assert!(samples(&replay, &map).unwrap().is_empty());
    }

    #[test]
    fn simultaneous_objects_take_separate_presses() {
        let map = beatmap(
            14,
            4.0,
            8.0,
            &["100,100,1000,1,0", "300,100,1300,1,0", "400,300,1300,1,0"],
        );
        let replay = replay(
            vec![],
            &[
                (1000, 100.0, 100.0),
                (1290, 300.0, 100.0),
                (1310, 400.0, 300.0),
            ],
        );

        assert_eq!(samples(&replay, &map).unwrap().len(), 2);
    }

    #[test]
    fn out_of_order_frames_still_match() {
        let map = beatmap(14, 4.0, 8.0, &JUMP);
        let mut replay = replay(vec![], &[(1000, 100.0, 100.0), (1500, 400.0, 100.0)]);
        replay.frames.reverse();

        assert_eq!(samples(&replay, &map).unwrap().len(), 1);
    }

    #[test]
    fn non_standard_mode_is_rejected() {
        let map = beatmap(14, 4.0, 8.0, &JUMP);
        let mut replay = replay(vec![], &[]);
        replay.header.mode = 3;

        assert!(samples(&replay, &map).is_err());
    }

    #[test]
    fn old_beatmap_format_is_not_shifted() {
        let replay = replay(vec![], &[(1090, 100.0, 100.0), (1590, 400.0, 100.0)]);

        assert_eq!(
            samples(&replay, &beatmap(4, 4.0, 10.0, &JUMP))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn objects_revisited_within_the_stack_window_give_no_sample() {
        let map = beatmap(
            14,
            4.0,
            8.0,
            &[
                "100,100,1000,1,0",
                "400,100,1200,1,0",
                "100,100,1400,1,0",
                "400,300,1600,1,0",
            ],
        );
        let replay = replay(
            vec![],
            &[
                (1000, 100.0, 100.0),
                (1200, 400.0, 100.0),
                (1400, 100.0, 100.0),
                (1600, 400.0, 300.0),
            ],
        );

        assert_eq!(samples(&replay, &map).unwrap().len(), 2);
    }

    #[test]
    fn empty_beatmap_gives_no_samples() {
        let map = beatmap(14, 4.0, 8.0, &[]);
        let replay = replay(vec![], &[(1000, 100.0, 100.0)]);

        assert!(samples(&replay, &map).unwrap().is_empty());
    }

    #[test]
    fn hard_rock_flips_objects_vertically() {
        let map = beatmap(14, 4.0, 8.0, &JUMP);
        let replay = replay(
            with_mods(&["HR"]),
            &[(1000, 100.0, 284.0), (1500, 400.0, 284.0)],
        );

        assert_eq!(samples(&replay, &map).unwrap()[0].intended, (300.0, 0.0));
    }

    #[test]
    fn mirror_reflection_accepts_numbers_and_names() {
        let mirror = |reflection: Value| Mod {
            acronym: "MR".to_string(),
            settings: Map::from_iter([("reflection".to_string(), reflection)]),
        };

        assert_eq!(mirror_axes(&with_mods(&["MR"])[0]), (true, false));
        assert_eq!(mirror_axes(&mirror(Value::from(1))), (false, true));
        assert_eq!(mirror_axes(&mirror(Value::from("2"))), (true, true));
        assert_eq!(mirror_axes(&mirror(Value::from("Vertical"))), (false, true));
    }

    #[test]
    fn mods_change_circle_size() {
        let map = beatmap(14, 4.0, 8.0, &JUMP);
        let difficulty_adjust = Mod {
            acronym: "DA".to_string(),
            settings: Map::from_iter([("circle_size".to_string(), Value::from(7.5))]),
        };
        let circle_size = |mods: Vec<Mod>| difficulty(&map, &mods).unwrap().circle_size;

        assert_eq!(circle_size(with_mods(&["HR"])), 5.2);
        assert_eq!(circle_size(with_mods(&["EZ"])), 2.0);
        assert_eq!(circle_size(vec![difficulty_adjust]), 7.5);
    }

    #[test]
    fn unsupported_mods_are_rejected() {
        let map = beatmap(14, 4.0, 8.0, &JUMP);
        let replay = replay(with_mods(&["HD", "AP", "bit15"]), &[]);

        let error = samples(&replay, &map).unwrap_err().to_string();

        assert_eq!(error, "unsupported mods: AP, bit15");
    }

    #[test]
    fn replay_without_installed_beatmap_is_reported() {
        let path = std::env::temp_dir().join(format!("oto-hits-{}.osr", std::process::id()));
        std::fs::write(&path, crate::core::replay::tests::Osr::default().bytes()).unwrap();

        let error = replay_samples(&path, &HashMap::new()).unwrap_err();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(error.to_string(), "beatmap not found in the osu! library");
    }

    #[test]
    #[ignore = "scans the osu! installs on this machine"]
    fn real_replays_land_near_their_objects() {
        let mut library = library::Library::default();
        library.scan(&library::default_roots());
        let beatmaps = library.beatmaps_by_md5();
        let mut replays: Vec<_> = library
            .replays()
            .filter(|(_, header)| header.mode == 0)
            .collect();
        replays.sort_by_key(|(_, header)| std::cmp::Reverse(header.played_at_ms));

        let mut errors: HashMap<bool, Vec<f64>> = HashMap::new();
        let mut skipped: HashMap<String, usize> = HashMap::new();
        let mut taken: HashMap<bool, usize> = HashMap::new();
        for (path, header) in replays {
            let hard_rock = header.legacy_mods & (1 << 4) != 0;
            let count = taken.entry(hard_rock).or_default();
            if *count == 150 {
                continue;
            }
            *count += 1;
            match replay_samples(path, &beatmaps) {
                Ok(samples) => {
                    errors
                        .entry(hard_rock)
                        .or_default()
                        .extend(samples.iter().map(|s| {
                            (s.actual.0 - s.intended.0).hypot(s.actual.1 - s.intended.1) / s.radius
                        }));
                }
                Err(error) => *skipped.entry(error.to_string()).or_default() += 1,
            }
        }
        eprintln!("skipped: {skipped:?}");
        for (hard_rock, mut errors) in errors {
            errors.sort_by(f64::total_cmp);
            let median = errors[errors.len() / 2];
            eprintln!(
                "HR={hard_rock}: {} samples, median error {median:.2} radii",
                errors.len()
            );
            assert!(
                median < 1.0,
                "HR={hard_rock}: median error {median:.2} radii"
            );
        }
    }
}
