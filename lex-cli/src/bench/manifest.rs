//! The Suite manifest: every Track, its Tier and its calibration, as `benchmarks/suite.json`.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use clap::ValueEnum;
use lex_data::{Language, MAX_WORD_LENGTH, MIN_WORD_LENGTH};
use serde::{Deserialize, Serialize};

/// Longest word length listed in the manifest; lengths above `MAX_WORD_LENGTH` are pending.
pub const PENDING_MAX_LENGTH: usize = 25;
/// The length whose Tracks are headline (ADR 0007).
const HEADLINE_LENGTH: usize = 5;
/// The Fibonacci lengths whose Tracks are breadth (ADR 0007).
const BREADTH_LENGTHS: [usize; 4] = [3, 8, 13, 21];
/// Sampled targets on a breadth Track that does not fit the budget in full.
pub const BREADTH_SAMPLE: usize = 500;
/// Sampled targets on a smoke Track.
pub const SMOKE_SAMPLE: usize = 100;

const LANGUAGES: [Language; 6] = [
    Language::English,
    Language::French,
    Language::German,
    Language::Spanish,
    Language::Italian,
    Language::Russian,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Headline,
    Breadth,
    Smoke,
}

impl Tier {
    /// The Tier ADR 0007 gives a Corpus Track of this length.
    pub fn of_length(length: usize) -> Tier {
        if length == HEADLINE_LENGTH {
            Tier::Headline
        } else if BREADTH_LENGTHS.contains(&length) {
            Tier::Breadth
        } else {
            Tier::Smoke
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tier::Headline => "headline",
            Tier::Breadth => "breadth",
            Tier::Smoke => "smoke",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Unconstrained,
    Hard,
    Ultrahard,
}

/// How many targets a Track plays: the whole dictionary or a seeded sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Targets {
    Full,
    Sample(usize),
}

impl From<Targets> for String {
    fn from(t: Targets) -> String {
        match t {
            Targets::Full => "full".into(),
            Targets::Sample(n) => n.to_string(),
        }
    }
}

impl TryFrom<String> for Targets {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        if s == "full" {
            return Ok(Targets::Full);
        }
        s.parse()
            .map(Targets::Sample)
            .map_err(|_| format!("targets must be \"full\" or a number, got {s:?}"))
    }
}

impl Targets {
    /// Whether ADR 0007 allows this many targets on a Track of this Tier.
    fn allowed_in(self, tier: Tier) -> bool {
        match tier {
            Tier::Headline => self == Targets::Full,
            Tier::Breadth => matches!(self, Targets::Full | Targets::Sample(BREADTH_SAMPLE)),
            Tier::Smoke => self == Targets::Sample(SMOKE_SAMPLE),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub cpu_s: f64,
    pub wall_s: f64,
    pub over_budget: bool,
    pub contended: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub language: String,
    pub length: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub mode: Mode,
    pub tier: Tier,
    pub targets: Targets,
    pub reported: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<Calibration>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Track {
    /// The `simulate` arguments that select this Track, without its targets.
    pub fn track_args(&self) -> Vec<String> {
        let mut args: Vec<String> = match &self.profile {
            Some(p) => vec!["--profile".into(), p.clone()],
            None => vec![
                "-l".into(),
                self.length.to_string(),
                "--lang".into(),
                self.language.clone(),
            ],
        };
        match self.mode {
            Mode::Unconstrained => {}
            Mode::Hard => args.push("--hard".into()),
            Mode::Ultrahard => args.push("--ultrahard".into()),
        }
        args
    }

    /// The `simulate` arguments that play this Track on `targets`.
    pub fn simulate_args(&self, targets: Targets) -> Vec<String> {
        let mut args = self.track_args();
        if let Targets::Sample(n) = targets {
            args.extend(["-n".into(), n.to_string(), "--seed".into(), "0".into()]);
        }
        args
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// The commit the calibration ran at, `-dirty` when the tree had changes.
    pub commit: Option<String>,
    pub cpu_model: Option<String>,
    pub tracks: Vec<Track>,
}

fn track(
    id: String,
    language: &str,
    length: usize,
    profile: Option<&str>,
    mode: Mode,
    tier: Tier,
    reported: bool,
) -> Track {
    let targets = match tier {
        Tier::Headline => Targets::Full,
        Tier::Breadth => Targets::Sample(BREADTH_SAMPLE),
        Tier::Smoke => Targets::Sample(SMOKE_SAMPLE),
    };
    Track {
        id,
        language: language.into(),
        length,
        profile: profile.map(Into::into),
        mode,
        tier,
        targets,
        reported,
        pending: false,
        calibration: None,
    }
}

impl Manifest {
    /// The matrix ADR 0007 prescribes, uncalibrated: every language at every length from
    /// `MIN_WORD_LENGTH` to `MAX_WORD_LENGTH`, the Profile Tracks, and the pending lengths above.
    pub fn generated() -> Manifest {
        let corpus = |lang: Language, length: usize, mode: Mode, suffix: &str, reported: bool| {
            track(
                format!("{lang}-{length}{suffix}"),
                &lang.to_string(),
                length,
                None,
                mode,
                Tier::of_length(length),
                reported,
            )
        };
        let wordle = |id: &str, profile: &str, mode: Mode, reported: bool| {
            track(
                id.into(),
                "english",
                5,
                Some(profile),
                mode,
                Tier::Headline,
                reported,
            )
        };

        let mut tracks = Vec::new();
        for lang in LANGUAGES {
            tracks.push(corpus(
                lang,
                HEADLINE_LENGTH,
                Mode::Unconstrained,
                "",
                false,
            ));
        }
        tracks.push(wordle("wordle", "wordle", Mode::Unconstrained, false));
        tracks.push(wordle(
            "wordle-all",
            "wordle-all",
            Mode::Unconstrained,
            true,
        ));
        tracks.push(wordle("wordle-hard", "wordle", Mode::Hard, true));
        tracks.push(corpus(
            Language::English,
            HEADLINE_LENGTH,
            Mode::Ultrahard,
            "-ultrahard",
            true,
        ));
        for length in MIN_WORD_LENGTH..=MAX_WORD_LENGTH {
            if length != HEADLINE_LENGTH {
                for lang in LANGUAGES {
                    tracks.push(corpus(lang, length, Mode::Unconstrained, "", false));
                }
            }
        }
        for length in MAX_WORD_LENGTH + 1..=PENDING_MAX_LENGTH {
            for lang in LANGUAGES {
                let mut t = corpus(lang, length, Mode::Unconstrained, "", false);
                t.pending = true;
                tracks.push(t);
            }
        }
        Manifest {
            commit: None,
            cpu_model: None,
            tracks,
        }
    }

    pub fn load(path: &Path) -> anyhow::Result<Manifest> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing manifest {}", path.display()))
    }

    /// Writes the manifest through a temporary file so an interrupted write never truncates it.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
    }

    /// Every way this manifest departs from the generated matrix or from ADR 0007's Tiers;
    /// empty when it is consistent. Targets and calibrations are data, so only their validity
    /// is checked, not their values.
    pub fn drift(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let expected = Manifest::generated();

        let mut seen = HashSet::new();
        for t in &self.tracks {
            if !seen.insert(t.id.as_str()) {
                problems.push(format!("duplicate track {}", t.id));
            }
        }
        let want: HashSet<&str> = expected.tracks.iter().map(|t| t.id.as_str()).collect();
        for want_track in &expected.tracks {
            let Some(got) = self.tracks.iter().find(|t| t.id == want_track.id) else {
                problems.push(format!("missing track {}", want_track.id));
                continue;
            };
            let id = &got.id;
            for (field, same) in [
                ("language", got.language == want_track.language),
                ("length", got.length == want_track.length),
                ("profile", got.profile == want_track.profile),
                ("mode", got.mode == want_track.mode),
                ("reported", got.reported == want_track.reported),
                ("pending", got.pending == want_track.pending),
            ] {
                if !same {
                    problems.push(format!("track {id}: {field} differs from the matrix"));
                }
            }
            if got.tier != want_track.tier {
                problems.push(format!(
                    "track {id}: tier is {} but ADR 0007 gives length {} the {} tier",
                    got.tier.name(),
                    got.length,
                    want_track.tier.name()
                ));
            }
            if !got.targets.allowed_in(want_track.tier) {
                problems.push(format!(
                    "track {id}: targets {} are not allowed in the {} tier",
                    String::from(got.targets),
                    want_track.tier.name()
                ));
            }
            if got.pending && got.calibration.is_some() {
                problems.push(format!("track {id}: a pending track has a calibration"));
            }
        }
        for t in &self.tracks {
            if !want.contains(t.id.as_str()) {
                problems.push(format!("unexpected track {}", t.id));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::VariantNames;

    fn tier_of(m: &Manifest, id: &str) -> Tier {
        m.tracks.iter().find(|t| t.id == id).unwrap().tier
    }

    fn edit<'a>(m: &'a mut Manifest, id: &str) -> &'a mut Track {
        m.tracks.iter_mut().find(|t| t.id == id).unwrap()
    }

    #[test]
    fn the_language_list_covers_every_language() {
        assert_eq!(LANGUAGES.len(), Language::VARIANTS.len());
    }

    #[test]
    fn generated_matrix_has_the_tracks_of_the_record() {
        let m = Manifest::generated();
        let count = |tier| {
            m.tracks
                .iter()
                .filter(|t| !t.pending && t.tier == tier && t.profile.is_none())
                .count()
        };
        // Six languages at length 5 plus the english ultrahard track, 12 breadth, 30 smoke.
        assert_eq!(count(Tier::Headline), 7);
        assert_eq!(count(Tier::Breadth), 12);
        assert_eq!(count(Tier::Smoke), 30);
        // The 49 corpus tracks plus wordle, wordle-all and wordle-hard.
        assert_eq!(m.tracks.iter().filter(|t| !t.pending).count(), 49 + 3);
        assert_eq!(m.tracks.iter().filter(|t| t.pending).count(), 15 * 6);
        let reported: Vec<&str> = m
            .tracks
            .iter()
            .filter(|t| t.reported)
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(
            reported,
            ["wordle-all", "wordle-hard", "english-5-ultrahard"]
        );
    }

    #[test]
    fn tiers_follow_adr_0007_by_length() {
        let m = Manifest::generated();
        assert_eq!(tier_of(&m, "english-5"), Tier::Headline);
        assert_eq!(tier_of(&m, "wordle"), Tier::Headline);
        assert_eq!(tier_of(&m, "german-3"), Tier::Breadth);
        assert_eq!(tier_of(&m, "german-8"), Tier::Breadth);
        assert_eq!(tier_of(&m, "german-13"), Tier::Breadth);
        assert_eq!(tier_of(&m, "german-21"), Tier::Breadth);
        assert_eq!(tier_of(&m, "german-4"), Tier::Smoke);
        assert_eq!(tier_of(&m, "german-10"), Tier::Smoke);
        assert_eq!(tier_of(&m, "german-11"), Tier::Smoke);
    }

    #[test]
    fn the_generated_matrix_has_no_drift_and_round_trips() {
        let m = Manifest::generated();
        assert_eq!(m.drift(), Vec::<String>::new());
        let text = serde_json::to_string_pretty(&m).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&text).unwrap(), m);
    }

    #[test]
    fn serialization_keeps_one_stable_key_order() {
        let text = serde_json::to_string(&Manifest::generated().tracks[0]).unwrap();
        assert_eq!(
            text,
            r#"{"id":"english-5","language":"english","length":5,"mode":"unconstrained","tier":"headline","targets":"full","reported":false}"#
        );
    }

    #[test]
    fn a_missing_track_is_drift() {
        let mut m = Manifest::generated();
        m.tracks.retain(|t| t.id != "french-4");
        assert_eq!(m.drift(), ["missing track french-4"]);
    }

    #[test]
    fn an_extra_or_duplicate_track_is_drift() {
        let mut m = Manifest::generated();
        let mut extra = m.tracks[0].clone();
        extra.id = "klingon-5".into();
        m.tracks.push(extra);
        m.tracks.push(m.tracks[1].clone());
        let problems = m.drift();
        assert!(problems.contains(&"unexpected track klingon-5".to_string()));
        assert!(problems.contains(&format!("duplicate track {}", m.tracks[1].id)));
    }

    #[test]
    fn a_tier_that_disagrees_with_the_length_is_drift() {
        let mut m = Manifest::generated();
        edit(&mut m, "french-6").tier = Tier::Breadth;
        let problems = m.drift();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("french-6") && p.contains("tier")),
            "{problems:?}"
        );
    }

    #[test]
    fn targets_must_fit_the_tier() {
        let mut m = Manifest::generated();
        edit(&mut m, "german-8").targets = Targets::Full;
        edit(&mut m, "german-3").targets = Targets::Sample(500);
        assert!(m.drift().is_empty());
        edit(&mut m, "english-5").targets = Targets::Sample(500);
        edit(&mut m, "german-8").targets = Targets::Sample(200);
        edit(&mut m, "french-6").targets = Targets::Full;
        assert_eq!(m.drift().len(), 3, "{:?}", m.drift());
    }

    #[test]
    fn pending_lengths_must_be_listed_and_uncalibrated() {
        let mut m = Manifest::generated();
        m.tracks.retain(|t| t.id != "english-25");
        assert_eq!(m.drift(), ["missing track english-25"]);

        let mut m = Manifest::generated();
        edit(&mut m, "english-11").calibration = Some(Calibration {
            cpu_s: 1.0,
            wall_s: 1.0,
            over_budget: false,
            contended: false,
        });
        assert_eq!(
            m.drift(),
            ["track english-11: a pending track has a calibration"]
        );
    }

    #[test]
    fn a_profile_changed_in_place_is_drift() {
        let mut m = Manifest::generated();
        edit(&mut m, "wordle-all").profile = Some("wordle".into());
        assert_eq!(
            m.drift(),
            ["track wordle-all: profile differs from the matrix"]
        );
    }

    #[test]
    fn simulate_args_select_the_track_and_its_targets() {
        let m = Manifest::generated();
        let find = |id: &str| m.tracks.iter().find(|t| t.id == id).unwrap();
        assert_eq!(
            find("french-8").simulate_args(Targets::Sample(500)),
            ["-l", "8", "--lang", "french", "-n", "500", "--seed", "0"]
        );
        assert_eq!(
            find("wordle-hard").simulate_args(Targets::Full),
            ["--profile", "wordle", "--hard"]
        );
        assert_eq!(
            find("english-5-ultrahard").track_args(),
            ["-l", "5", "--lang", "english", "--ultrahard"]
        );
    }
}
