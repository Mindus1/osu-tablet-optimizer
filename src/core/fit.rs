use crate::core::hits::Sample;

// ponytail: fixed statistics knobs; expose them in Setup if users ask.
const MIN_SAMPLES: usize = 50;
// The bootstrap resamples whole replays (aim varies between sessions), which needs a few of them.
const MIN_REPLAYS: usize = 5;
const BOOTSTRAP_ROUNDS: usize = 200;
// Two-sided 90% interval.
const TAIL: f64 = 0.05;

type Mat2 = [[f64; 2]; 2];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub min_jump_radii: f64,
    pub max_error_radii: f64,
    pub max_size_step: f64,
    pub max_rotation_step_deg: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            min_jump_radii: 3.0,
            max_error_radii: 3.0,
            max_size_step: 0.03,
            max_rotation_step_deg: 2.0,
        }
    }
}

// OpenTabletDriver's absolute mapping: tablet area (mm, degrees) onto a display area (px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapping {
    pub area_width: f64,
    pub area_height: f64,
    pub rotation: f64,
    pub display_width: f64,
    pub display_height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub value: f64,
    pub low: f64,
    pub high: f64,
}

impl Estimate {
    fn new(value: f64, rounds: impl Iterator<Item = f64>) -> Estimate {
        let mut rounds: Vec<f64> = rounds.collect();
        if rounds.is_empty() {
            return Estimate {
                value,
                low: value,
                high: value,
            };
        }
        rounds.sort_by(f64::total_cmp);
        let at = |q: f64| rounds[((rounds.len() - 1) as f64 * q).round() as usize];
        Estimate {
            value,
            low: at(TAIL),
            high: at(1.0 - TAIL),
        }
    }

    pub fn contains(&self, x: f64) -> bool {
        self.low <= x && x <= self.high
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion {
    pub replays: usize,
    pub samples: usize,
    // How far the cursor travels per intended distance, minus one: +0.05 = 5% overshoot.
    pub overshoot: Estimate,
    // Angle (degrees) by which actual moves are turned against intended ones, x towards y.
    pub rotation_bias: Estimate,
    // Horizontal gain over vertical gain.
    pub axis_ratio: Estimate,
    // Area scale (geometric mean of the width and height factors): 0.98 = 2% smaller.
    pub size: Estimate,
    // Change of the width/height ratio: 1.02 = 2% wider relative to its height.
    pub aspect: Estimate,
    pub rotation: Estimate,
    // Skew OTD cannot express; left over after the area fit (degrees).
    pub shear: f64,
    // Size, aspect and rotation each stay unchanged when their interval contains "no change";
    // deciding width and height separately could invent an aspect change the data does not show.
    pub full: Mapping,
    pub capped: Mapping,
}

// Least-squares sums for actual = gain · intended + edge_stop · direction(intended). The
// edge-stop term soaks up stopping at the circle's edge, which does not grow with jump distance;
// a wrong area size does, so the two separate when jump distances vary.
#[derive(Debug, Clone, Copy, Default)]
struct Normal {
    a: [[f64; 5]; 5],
    b: [f64; 5],
}

impl Normal {
    fn of(samples: &[Sample]) -> Normal {
        let mut normal = Normal::default();
        for sample in samples {
            let (ux, uy) = sample.intended;
            let (vx, vy) = sample.actual;
            let length = ux.hypot(uy);
            let rows = [
                ([ux, uy, 0.0, 0.0, ux / length], vx),
                ([0.0, 0.0, ux, uy, uy / length], vy),
            ];
            for (row, value) in rows {
                for i in 0..5 {
                    for j in 0..5 {
                        normal.a[i][j] += row[i] * row[j];
                    }
                    normal.b[i] += row[i] * value;
                }
            }
        }
        normal
    }

    fn add(&mut self, other: &Normal) {
        for i in 0..5 {
            for j in 0..5 {
                self.a[i][j] += other.a[i][j];
            }
            self.b[i] += other.b[i];
        }
    }

    fn gain(&self) -> Option<Mat2> {
        let [b00, b01, b10, b11, _edge_stop] = solve(self.a, self.b)?;
        Some([[b00, b01], [b10, b11]])
    }
}

// One sample list per replay.
pub fn suggest(
    replays: &[Vec<Sample>],
    current: Mapping,
    settings: &Settings,
) -> Option<Suggestion> {
    if !is_valid(&current) {
        return None;
    }
    let groups: Vec<(usize, Normal)> = replays
        .iter()
        .map(|samples| {
            let usable: Vec<Sample> = samples
                .iter()
                .copied()
                .filter(|sample| is_usable(sample, settings))
                .collect();
            (usable.len(), Normal::of(&usable))
        })
        .filter(|(count, _)| *count > 0)
        .collect();
    let samples: usize = groups.iter().map(|(count, _)| count).sum();
    if groups.len() < MIN_REPLAYS || samples < MIN_SAMPLES {
        return None;
    }
    let mut total = Normal::default();
    for (_, normal) in &groups {
        total.add(normal);
    }
    let gain = total.gain()?;
    let (mapping, shear) = corrected(current, &gain)?;

    let mut rng = Rng(samples as u64);
    let rounds: Vec<(Mat2, Mapping)> = (0..BOOTSTRAP_ROUNDS)
        .filter_map(|_| {
            let mut resample = Normal::default();
            for _ in 0..groups.len() {
                resample.add(&groups[rng.below(groups.len())].1);
            }
            let matrix = resample.gain()?;
            Some((matrix, corrected(current, &matrix)?.0))
        })
        .collect();
    let estimate = |value: f64, round: &dyn Fn(&(Mat2, Mapping)) -> f64| {
        Estimate::new(value, rounds.iter().map(round))
    };
    let width_factor = |m: &Mapping| m.area_width / current.area_width;
    let height_factor = |m: &Mapping| m.area_height / current.area_height;
    let size_of = |m: &Mapping| (width_factor(m) * height_factor(m)).sqrt();
    let aspect_of = |m: &Mapping| width_factor(m) / height_factor(m);

    let size = estimate(size_of(&mapping), &|(_, m)| size_of(m));
    let aspect = estimate(aspect_of(&mapping), &|(_, m)| aspect_of(m));
    let rotation = estimate(mapping.rotation, &|(_, m)| m.rotation);
    let (kept_size, kept_aspect) = (keep(size, 1.0), keep(aspect, 1.0));
    let full = Mapping {
        area_width: current.area_width * kept_size * kept_aspect.sqrt(),
        area_height: current.area_height * kept_size / kept_aspect.sqrt(),
        rotation: keep(rotation, current.rotation),
        ..current
    };
    let size_step = |full: f64, now: f64| {
        now * (full / now).clamp(1.0 - settings.max_size_step, 1.0 + settings.max_size_step)
    };
    let capped = Mapping {
        area_width: size_step(full.area_width, current.area_width),
        area_height: size_step(full.area_height, current.area_height),
        rotation: current.rotation
            + (full.rotation - current.rotation).clamp(
                -settings.max_rotation_step_deg,
                settings.max_rotation_step_deg,
            ),
        ..current
    };

    Some(Suggestion {
        replays: groups.len(),
        samples,
        overshoot: estimate(overshoot(&gain), &|(m, _)| overshoot(m)),
        rotation_bias: estimate(rotation_bias(&gain), &|(m, _)| rotation_bias(m)),
        axis_ratio: estimate(axis_ratio(&gain), &|(m, _)| axis_ratio(m)),
        size,
        aspect,
        rotation,
        shear,
        full,
        capped,
    })
}

fn is_valid(mapping: &Mapping) -> bool {
    let sizes = [
        mapping.area_width,
        mapping.area_height,
        mapping.display_width,
        mapping.display_height,
    ];
    sizes.iter().all(|size| size.is_finite() && *size > 0.0) && mapping.rotation.is_finite()
}

fn is_usable(sample: &Sample, settings: &Settings) -> bool {
    let (ux, uy) = sample.intended;
    let (vx, vy) = sample.actual;
    sample.radius > 0.0
        && ux.hypot(uy) >= settings.min_jump_radii * sample.radius
        && (vx - ux).hypot(vy - uy) <= settings.max_error_radii * sample.radius
}

fn keep(estimate: Estimate, current: f64) -> f64 {
    if estimate.contains(current) {
        current
    } else {
        estimate.value
    }
}

// OTD maps tablet mm to display px as diag(Dw/w, Dh/h) · R(−θ) (AbsoluteOutputMode). The mapping
// that cancels the player's gain is gain⁻¹ · that; its rows give back width, height and angle.
fn corrected(current: Mapping, gain: &Mat2) -> Option<(Mapping, f64)> {
    let theta = current.rotation.to_radians();
    let (sin, cos) = theta.sin_cos();
    let otd = [
        [
            current.display_width / current.area_width * cos,
            current.display_width / current.area_width * sin,
        ],
        [
            -current.display_height / current.area_height * sin,
            current.display_height / current.area_height * cos,
        ],
    ];
    let [row_x, row_y] = mul(&inverse(gain)?, &otd);
    let angle_x = row_x[1].atan2(row_x[0]).to_degrees();
    let angle_y = (-row_y[0]).atan2(row_y[1]).to_degrees();
    let shear = wrap_degrees(angle_y - angle_x);
    let angle = angle_x + shear / 2.0;
    Some((
        Mapping {
            area_width: current.display_width / row_x[0].hypot(row_x[1]),
            area_height: current.display_height / row_y[0].hypot(row_y[1]),
            rotation: current.rotation + wrap_degrees(angle - current.rotation),
            ..current
        },
        shear,
    ))
}

fn overshoot(gain: &Mat2) -> f64 {
    determinant(gain).abs().sqrt() - 1.0
}

fn rotation_bias(gain: &Mat2) -> f64 {
    (gain[1][0] - gain[0][1])
        .atan2(gain[0][0] + gain[1][1])
        .to_degrees()
}

fn axis_ratio(gain: &Mat2) -> f64 {
    gain[0][0].hypot(gain[1][0]) / gain[0][1].hypot(gain[1][1])
}

fn determinant(m: &Mat2) -> f64 {
    m[0][0] * m[1][1] - m[0][1] * m[1][0]
}

fn inverse(m: &Mat2) -> Option<Mat2> {
    let det = determinant(m);
    if det.abs() < 1e-12 {
        return None;
    }
    Some([
        [m[1][1] / det, -m[0][1] / det],
        [-m[1][0] / det, m[0][0] / det],
    ])
}

fn mul(a: &Mat2, b: &Mat2) -> Mat2 {
    let cell = |i: usize, j: usize| a[i][0] * b[0][j] + a[i][1] * b[1][j];
    [[cell(0, 0), cell(0, 1)], [cell(1, 0), cell(1, 1)]]
}

fn wrap_degrees(angle: f64) -> f64 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

// Gaussian elimination with partial pivoting; None when the system is (near) singular.
fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    let scale = a.iter().flatten().fold(0.0_f64, |max, x| max.max(x.abs()));
    for col in 0..N {
        let pivot = (col..N).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() <= scale * 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..N {
            let factor = a[row][col] / a[col][col];
            let pivot_row = a[col];
            for (x, p) in a[row][col..].iter_mut().zip(&pivot_row[col..]) {
                *x -= factor * p;
            }
            b[row] -= factor * b[col];
        }
    }
    let mut x = [0.0; N];
    for row in (0..N).rev() {
        let rest: f64 = (row + 1..N).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - rest) / a[row][row];
    }
    Some(x)
}

// splitmix64, seeded from the data: the same samples always give the same intervals.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RADIUS: f64 = 30.0;
    const MAPPING: Mapping = Mapping {
        area_width: 100.0,
        area_height: 56.25,
        rotation: 5.0,
        display_width: 1920.0,
        display_height: 1080.0,
    };

    fn rotation(degrees: f64) -> Mat2 {
        let (sin, cos) = degrees.to_radians().sin_cos();
        [[cos, -sin], [sin, cos]]
    }

    // Jumps in 16 directions and 6 distances (480 in total), moved by `gain` plus `edge_stop`
    // along the jump, with deterministic noise of up to `noise` px per axis.
    fn synthetic(gain: Mat2, edge_stop: f64, noise: f64) -> Vec<Sample> {
        let mut rng = Rng(7);
        let mut jitter = || (rng.below(2001) as f64 / 1000.0 - 1.0) * noise;
        let mut samples = Vec::new();
        for _ in 0..5 {
            for direction in 0..16 {
                let angle = (f64::from(direction) * 22.5_f64).to_radians();
                for distance in [150.0, 200.0, 250.0, 300.0, 350.0, 400.0] {
                    let u = (distance * angle.cos(), distance * angle.sin());
                    let v = (
                        gain[0][0] * u.0 + gain[0][1] * u.1 + edge_stop * angle.cos() + jitter(),
                        gain[1][0] * u.0 + gain[1][1] * u.1 + edge_stop * angle.sin() + jitter(),
                    );
                    samples.push(Sample {
                        intended: u,
                        actual: v,
                        radius: RADIUS,
                    });
                }
            }
        }
        samples
    }

    fn replays(samples: &[Sample]) -> Vec<Vec<Sample>> {
        samples
            .chunks(samples.len().div_ceil(10))
            .map(<[Sample]>::to_vec)
            .collect()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn fit_recovers_injected_gain_and_edge_stop() {
        let truth = [[1.05, 0.02], [-0.03, 0.97]];

        let normal = Normal::of(&synthetic(truth, -8.0, 0.0));

        let solved = solve(normal.a, normal.b).unwrap();
        let expected = [truth[0][0], truth[0][1], truth[1][0], truth[1][1], -8.0];
        for (actual, expected) in solved.into_iter().zip(expected) {
            assert_close(actual, expected);
        }
    }

    #[test]
    fn unchanged_gain_round_trips_the_mapping() {
        let mapping = Mapping {
            area_width: 80.0,
            area_height: 45.0,
            rotation: 10.0,
            display_width: 1920.0,
            display_height: 1200.0,
        };

        let (corrected, shear) = corrected(mapping, &rotation(0.0)).unwrap();

        assert_close(corrected.area_width, 80.0);
        assert_close(corrected.area_height, 45.0);
        assert_close(corrected.rotation, 10.0);
        assert_close(shear, 0.0);
    }

    #[test]
    fn undershoot_shrinks_the_area() {
        let (corrected, _) = corrected(MAPPING, &[[0.95, 0.0], [0.0, 0.95]]).unwrap();

        assert_close(corrected.area_width, 95.0);
        assert_close(corrected.area_height, 56.25 * 0.95);
        assert_close(corrected.rotation, 5.0);
    }

    #[test]
    fn horizontal_overshoot_widens_the_area() {
        let (corrected, _) = corrected(MAPPING, &[[1.03, 0.0], [0.0, 1.0]]).unwrap();

        assert_close(corrected.area_width, 103.0);
        assert_close(corrected.area_height, 56.25);
    }

    #[test]
    fn rotated_aim_rotates_the_area_the_same_way() {
        let (corrected, _) = corrected(MAPPING, &rotation(2.0)).unwrap();

        assert_close(corrected.rotation, 7.0);
        assert_close(corrected.area_width, 100.0);
        assert_close(corrected.area_height, 56.25);
    }

    #[test]
    fn metrics_describe_the_gain() {
        let gain = mul(&rotation(3.0), &[[1.1, 0.0], [0.0, 1.0]]);

        assert_close(rotation_bias(&rotation(3.0)), 3.0);
        assert_close(axis_ratio(&gain), 1.1);
        assert_close(overshoot(&[[1.05, 0.0], [0.0, 1.05]]), 0.05);
    }

    #[test]
    fn clear_overshoot_is_suggested_and_capped() {
        let samples = synthetic([[1.1, 0.0], [0.0, 1.1]], -5.0, 6.0);

        let suggestion = suggest(&replays(&samples), MAPPING, &Settings::default()).unwrap();

        assert!((suggestion.size.value - 1.1).abs() < 0.01);
        assert!(!suggestion.size.contains(1.0));
        assert!((suggestion.full.area_width - 110.0).abs() < 1.0);
        assert_close(suggestion.capped.area_width, 103.0);
        assert_close(suggestion.capped.area_height, 56.25 * 1.03);
    }

    #[test]
    fn rotation_step_is_capped() {
        let samples = synthetic(rotation(5.0), 0.0, 3.0);

        let suggestion = suggest(&replays(&samples), MAPPING, &Settings::default()).unwrap();

        assert!((suggestion.full.rotation - 10.0).abs() < 0.5);
        assert_close(suggestion.capped.rotation, 7.0);
    }

    #[test]
    fn unbiased_noisy_aim_keeps_the_current_area() {
        let samples = synthetic(rotation(0.0), -6.0, 20.0);

        let suggestion = suggest(&replays(&samples), MAPPING, &Settings::default()).unwrap();

        assert!(suggestion.size.contains(1.0));
        assert!(suggestion.aspect.contains(1.0));
        assert_eq!(suggestion.full, MAPPING);
        assert_eq!(suggestion.capped, MAPPING);
    }

    #[test]
    fn replay_to_replay_variation_widens_the_interval() {
        let sessions: Vec<Vec<Sample>> = (0..10)
            .map(|i| {
                let gain = if i % 2 == 0 { 0.97 } else { 1.03 };
                synthetic([[gain, 0.0], [0.0, gain]], 0.0, 0.0)
            })
            .collect();

        let suggestion = suggest(&sessions, MAPPING, &Settings::default()).unwrap();

        assert!(suggestion.size.high - suggestion.size.low > 0.02);
        assert!(suggestion.size.contains(1.0));
        assert_eq!(suggestion.full.area_width, MAPPING.area_width);
    }

    #[test]
    fn insignificant_aspect_keeps_the_area_ratio() {
        let sessions: Vec<Vec<Sample>> = (0..10)
            .map(|i| {
                let skew = if i % 2 == 0 { 0.05 } else { -0.05 };
                synthetic(
                    [[0.96 * (1.0 + skew), 0.0], [0.0, 0.985 * (1.0 - skew)]],
                    0.0,
                    0.0,
                )
            })
            .collect();

        let suggestion = suggest(&sessions, MAPPING, &Settings::default()).unwrap();

        assert!(suggestion.full.area_width < MAPPING.area_width);
        assert_close(
            suggestion.full.area_width / suggestion.full.area_height,
            MAPPING.area_width / MAPPING.area_height,
        );
    }

    #[test]
    fn short_jumps_and_outliers_are_left_out() {
        let samples = synthetic(rotation(0.0), 0.0, 0.0);
        let mut sessions = replays(&samples);
        sessions[0].extend([
            Sample {
                intended: (60.0, 0.0),
                actual: (60.0, 0.0),
                radius: RADIUS,
            },
            Sample {
                intended: (300.0, 0.0),
                actual: (300.0, 120.0),
                radius: RADIUS,
            },
            Sample {
                intended: (300.0, 0.0),
                actual: (300.0, 0.0),
                radius: 0.0,
            },
        ]);

        let suggestion = suggest(&sessions, MAPPING, &Settings::default()).unwrap();

        assert_eq!(suggestion.samples, samples.len());
        assert_eq!(suggestion.replays, 10);
    }

    #[test]
    fn non_finite_samples_are_left_out() {
        let samples = synthetic(rotation(0.0), 0.0, 0.0);
        let mut sessions = replays(&samples);
        sessions[0].push(Sample {
            intended: (f64::NAN, 300.0),
            actual: (300.0, 0.0),
            radius: RADIUS,
        });
        sessions[1].push(Sample {
            intended: (300.0, 0.0),
            actual: (f64::INFINITY, 0.0),
            radius: RADIUS,
        });

        let suggestion = suggest(&sessions, MAPPING, &Settings::default()).unwrap();

        assert_eq!(suggestion.samples, samples.len());
    }

    #[test]
    fn rotation_wraps_around_180_degrees() {
        let current = Mapping {
            rotation: 179.0,
            ..MAPPING
        };

        let (corrected, _) = corrected(current, &rotation(2.0)).unwrap();

        assert_close(corrected.rotation, 181.0);
    }

    #[test]
    fn invalid_mapping_gives_no_suggestion() {
        let samples = synthetic(rotation(0.0), 0.0, 0.0);
        let broken = Mapping {
            area_height: 0.0,
            ..MAPPING
        };

        assert!(suggest(&replays(&samples), broken, &Settings::default()).is_none());
    }

    #[test]
    fn too_few_samples_give_no_suggestion() {
        let samples = synthetic(rotation(0.0), 0.0, 0.0);

        let few = replays(&samples[..MIN_SAMPLES - 1]);

        assert!(suggest(&few, MAPPING, &Settings::default()).is_none());
    }

    #[test]
    fn one_replay_is_not_enough() {
        let samples = synthetic([[1.1, 0.0], [0.0, 1.1]], 0.0, 0.0);

        assert!(suggest(&[samples], MAPPING, &Settings::default()).is_none());
    }

    #[test]
    fn jumps_along_one_line_give_no_suggestion() {
        let samples: Vec<Sample> = (0..100)
            .map(|i| Sample {
                intended: (150.0 + f64::from(i), 0.0),
                actual: (150.0 + f64::from(i), 0.0),
                radius: RADIUS,
            })
            .collect();

        assert!(suggest(&replays(&samples), MAPPING, &Settings::default()).is_none());
    }

    #[test]
    #[ignore = "scans the osu! installs on this machine"]
    fn real_replays_give_a_bounded_suggestion() {
        use crate::core::{hits, library};

        let mut library = library::Library::default();
        library.scan(&library::default_roots());
        let beatmaps = library.beatmaps_by_md5();
        let mut replays: Vec<_> = library
            .replays()
            .filter(|(_, header)| header.mode == 0)
            .collect();
        replays.sort_by_key(|(_, header)| std::cmp::Reverse(header.played_at_ms));
        let sessions: Vec<Vec<Sample>> = replays
            .iter()
            .take(300)
            .filter_map(|(path, _)| hits::replay_samples(path, &beatmaps).ok())
            .collect();
        let current = Mapping {
            rotation: 0.0,
            ..MAPPING
        };

        let suggestion = suggest(&sessions, current, &Settings::default()).unwrap();

        eprintln!("{suggestion:#?}");
        assert!((suggestion.full.area_width / current.area_width - 1.0).abs() < 0.3);
        assert!((suggestion.full.area_height / current.area_height - 1.0).abs() < 0.3);
        assert!(suggestion.full.rotation.abs() < 20.0);
    }
}
