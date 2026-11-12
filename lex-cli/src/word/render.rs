//! The report: a row per word, or the step-by-step view of one word on a terminal.

use lex_data::{Language, MAX_WORD_LENGTH, MIN_SUPPORTED_WORDS, MIN_WORD_LENGTH};

use super::{Corpus, Dictionary, Facts, ProfileFact, ProfileStatus};
use crate::corpora::not_cached_hint as corpus_hint;
use crate::dicts::not_cached_hint as dict_hint;
use crate::table::{flag, render_rows, row};

/// The label every Prior carries: it is the one the default Strategy derives.
const STRATEGY_LABEL: &str = "(default Strategy)";

/// A cell for a step that was not reached.
const SKIPPED: &str = "-";

/// A cell for a step that needs a cache that is not there.
const UNKNOWN: &str = "unknown";

/// The report: the step-by-step view of a single word on a terminal, otherwise a row per word.
#[must_use]
pub fn render(facts: &[Facts], lang: Language, terminal: bool) -> String {
    match facts {
        [only] if terminal => render_steps(only, lang, terminal),
        _ => render_table(facts, lang, terminal),
    }
}

/// A Prior as a short scientific number.
fn prior_cell(prior: f64) -> String {
    format!("{prior:.3e}")
}

/// The input column: what was typed, empty when it is already the normalized form.
pub fn input_cell(facts: &Facts) -> String {
    if facts.input == facts.word {
        String::new()
    } else {
        facts.input.clone()
    }
}

/// `facts`' verdict: the first failing step, or that it is a Candidate in Corpus runs.
#[must_use]
pub fn verdict(facts: &Facts, lang: Language) -> String {
    if !facts.shape {
        return "not a word: not a single alphabetic token".to_owned();
    }
    if !facts.in_range {
        return format!(
            "length {} is outside the Supported range {MIN_WORD_LENGTH}..={MAX_WORD_LENGTH}",
            facts.length
        );
    }
    match (facts.corpus, facts.dictionary) {
        (Corpus::Unsupported { count }, _) => format!(
            "length {} is not Supported in {lang}: its Corpus holds {count} words, under {MIN_SUPPORTED_WORDS}",
            facts.length
        ),
        (_, Dictionary::Missing) => format!("not in the {lang} Dictionary"),
        (Corpus::Seen { rank, size, .. }, _) => {
            format!("a Candidate in Corpus runs (rank {rank} of {size})")
        }
        (Corpus::Unseen, Dictionary::Member) => {
            "in the Dictionary, but the ngrams snapshot never saw it".to_owned()
        }
        (Corpus::Unseen, _) => {
            "the ngrams snapshot never saw it (the Dictionary is not cached)".to_owned()
        }
        (_, Dictionary::NotCached) => {
            "unknown: the Dictionary and the Corpus are not cached".to_owned()
        }
        (_, _) => "unknown: the Corpus is not cached".to_owned(),
    }
}

/// The Dictionary column.
const fn dictionary_cell(dictionary: Dictionary) -> &'static str {
    match dictionary {
        Dictionary::Skipped => SKIPPED,
        Dictionary::NotCached => "not cached",
        Dictionary::Member => "yes",
        Dictionary::Missing => "no",
    }
}

/// The rank, frequency and Prior columns.
fn corpus_cells(corpus: Corpus) -> [String; 3] {
    match corpus {
        Corpus::Skipped | Corpus::Unsupported { .. } => [SKIPPED; 3].map(str::to_owned),
        Corpus::NotCached => [UNKNOWN; 3].map(str::to_owned),
        Corpus::Unseen => ["unseen", "0", SKIPPED].map(str::to_owned),
        Corpus::Seen {
            rank,
            size,
            frequency,
            prior,
        } => [
            format!("{rank}/{size}"),
            frequency.to_string(),
            prior_cell(prior),
        ],
    }
}

/// One Profile's status as a short cell.
fn profile_cell(fact: &ProfileFact) -> String {
    let status = match fact.status {
        ProfileStatus::NotFetched => "not fetched".to_owned(),
        ProfileStatus::Answer { prior: None, .. } => "answer".to_owned(),
        ProfileStatus::Answer {
            prior: Some(prior), ..
        } => format!("answer {}", prior_cell(prior)),
        ProfileStatus::GuessOnly => "guess-only".to_owned(),
        ProfileStatus::Outside => "no".to_owned(),
    };
    format!("{}: {status}", fact.profile)
}

/// A row per word, as a table (`terminal`) or tab-separated lines.
fn render_table(facts: &[Facts], lang: Language, terminal: bool) -> String {
    let prior_header = format!("prior {STRATEGY_LABEL}");
    let header = [
        "word",
        "input",
        "length",
        "dictionary",
        "rank",
        "frequency",
        prior_header.as_str(),
        "profiles",
        "verdict",
    ];
    let rows = facts
        .iter()
        .map(|f| {
            let [rank, frequency, prior] = corpus_cells(f.corpus);
            let profiles = if f.profiles.is_empty() {
                SKIPPED.to_owned()
            } else {
                f.profiles
                    .iter()
                    .map(profile_cell)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            vec![
                f.word.clone(),
                input_cell(f),
                f.length.to_string(),
                dictionary_cell(f.dictionary).to_owned(),
                rank,
                frequency,
                prior,
                profiles,
                verdict(f, lang),
            ]
        })
        .collect();
    render_rows(&header, rows, terminal)
}

/// The step-by-step report of one word, as `step`, `result` and `note` rows.
pub fn render_steps(facts: &Facts, lang: Language, terminal: bool) -> String {
    let mut rows = Vec::new();
    if facts.input != facts.word {
        rows.push(row("input", &facts.input, "as typed"));
    }
    rows.extend([
        row("word", &facts.word, ""),
        row(
            "shape",
            yes_no(facts.shape, terminal),
            "one alphabetic token after NFC and lowercasing",
        ),
    ]);
    if facts.shape {
        rows.push(length_row(facts, lang, terminal));
    }
    if facts.reaches_caches() {
        rows.extend(corpus_rows(facts, lang, terminal));
        rows.extend(
            facts
                .profiles
                .iter()
                .flat_map(|p| profile_rows(p, terminal)),
        );
    }
    rows.push(row("verdict", verdict(facts, lang), ""));
    render_rows(&["step", "result", "note"], rows, terminal)
}

/// `yes`, or `no` in the failure style.
fn yes_no(value: bool, terminal: bool) -> String {
    if value {
        "yes".to_owned()
    } else {
        flag("no", terminal)
    }
}

/// The length row: in range, and Supported when the Corpus says so.
fn length_row(facts: &Facts, lang: Language, terminal: bool) -> Vec<String> {
    let note = if facts.in_range {
        match facts.corpus {
            Corpus::Unsupported { count } => flag(
                &format!(
                    "not Supported in {lang}: the Corpus holds {count} words, under {MIN_SUPPORTED_WORDS}"
                ),
                terminal,
            ),
            Corpus::NotCached => format!("Supported unknown: {}", corpus_hint(lang, facts.length)),
            Corpus::Skipped | Corpus::Unseen | Corpus::Seen { .. } => {
                "in range and Supported".to_owned()
            }
        }
    } else {
        flag(
            &format!("outside the Supported range {MIN_WORD_LENGTH}..={MAX_WORD_LENGTH}"),
            terminal,
        )
    };
    row("length", facts.length.to_string(), note)
}

/// The Dictionary, Corpus, frequency and Prior rows.
fn corpus_rows(facts: &Facts, lang: Language, terminal: bool) -> Vec<Vec<String>> {
    let dictionary_note = match facts.dictionary {
        Dictionary::NotCached => dict_hint(lang),
        Dictionary::Skipped | Dictionary::Member | Dictionary::Missing => String::new(),
    };
    let dictionary = match facts.dictionary {
        Dictionary::Missing => flag("no", terminal),
        Dictionary::Skipped | Dictionary::NotCached | Dictionary::Member => {
            dictionary_cell(facts.dictionary).to_owned()
        }
    };
    let mut rows = vec![row("dictionary", dictionary, dictionary_note)];
    let prior_label = format!("prior {STRATEGY_LABEL}");
    match facts.corpus {
        Corpus::Skipped | Corpus::Unsupported { .. } => {}
        Corpus::NotCached => {
            rows.push(row("corpus", "not cached", corpus_hint(lang, facts.length)));
            rows.push(row(&prior_label, UNKNOWN, ""));
        }
        Corpus::Unseen => {
            let note = match facts.dictionary {
                Dictionary::Member => "in the Dictionary, but the ngrams snapshot never saw it",
                Dictionary::Missing | Dictionary::NotCached | Dictionary::Skipped => {
                    "the ngrams snapshot never saw it"
                }
            };
            rows.push(row("corpus", flag("not in the Corpus", terminal), note));
        }
        Corpus::Seen {
            rank,
            size,
            frequency,
            prior,
        } => {
            rows.push(row("corpus", format!("rank {rank} of {size}"), ""));
            rows.push(row(
                "frequency",
                frequency.to_string(),
                "summed ngrams count",
            ));
            rows.push(row(
                &prior_label,
                prior_cell(prior),
                format!("over the {lang} Corpus of {size} words"),
            ));
        }
    }
    rows
}

/// The Answer list and Guess pool rows of one Profile.
fn profile_rows(fact: &ProfileFact, terminal: bool) -> Vec<Vec<String>> {
    let name = fact.profile;
    let answers = format!("{name} answer list");
    let pool = format!("{name} guess pool");
    match fact.status {
        ProfileStatus::NotFetched => vec![row(
            format!("{name} lists"),
            "not fetched",
            format!("fetch it with `lex profile fetch --profile {name}`"),
        )],
        ProfileStatus::Answer { prior, answers: n } => vec![
            row(
                answers,
                "yes",
                prior.map_or_else(
                    || format!("prior unknown: the Corpus is not cached {STRATEGY_LABEL}"),
                    |p| format!("prior {} over {n} answers {STRATEGY_LABEL}", prior_cell(p)),
                ),
            ),
            row(pool, "yes", ""),
        ],
        ProfileStatus::GuessOnly => vec![
            row(answers, yes_no(false, terminal), ""),
            row(pool, "yes", "guess-only"),
        ],
        ProfileStatus::Outside => vec![
            row(answers, yes_no(false, terminal), ""),
            row(pool, yes_no(false, terminal), ""),
        ],
    }
}
