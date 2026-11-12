//! The wording of `assist`'s grading line: Luck (what the played guess left against what it
//! promised) and Skill (the played guess against the solver's pick at the same position).

use std::fmt::Write as _;

/// What a guess is worth at a position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Worth {
    /// Expected remaining: the expected Candidate set size after the feedback.
    pub remaining: f64,
    /// Expected information gain, in bits.
    pub bits: f64,
    /// The Guess cost, where the Endgame or a Rollout computed one.
    pub cost: Option<f64>,
}

/// Everything the grading line is worded from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grading {
    /// Whether the played word is the solver's pick.
    pub matched: bool,
    /// The played guess.
    pub played: Worth,
    /// The solver's pick.
    pub pick: Worth,
    /// Candidates left by the feedback, `None` on the winning turn (no Luck is graded).
    pub left: Option<usize>,
    /// Bits the feedback actually gained, for the details view.
    pub realised_bits: Option<f64>,
}

/// `n` words, singular for one.
fn words(n: usize) -> String {
    format!("{n} word{}", if n == 1 { "" } else { "s" })
}

/// `n` words as an expected count, singular only when it rounds to one.
fn expected_words(n: f64) -> String {
    format!(
        "{n:.0} word{}",
        if (n - 1.0).abs() < 0.5 { "" } else { "s" }
    )
}

/// The Luck clause, `None` on the winning turn.
fn luck(g: &Grading, details: bool) -> Option<String> {
    let left = g.left?;
    let mut line = format!(
        "Luck: {} left (expected {:.0})",
        words(left),
        g.played.remaining
    );
    if let (true, Some(realised)) = (details, g.realised_bits) {
        let _ = write!(
            line,
            ", {realised:.2} bits gained (expected {:.2})",
            g.played.bits
        );
    }
    Some(line)
}

/// The Skill clause, by the strongest measure both guesses have.
fn skill(g: &Grading, details: bool) -> String {
    if g.matched {
        return "Skill: matched lex".to_owned();
    }
    if let (Some(played), Some(pick)) = (g.played.cost, g.pick.cost) {
        let behind = played - pick;
        let rounded = format!("{:.2}", behind.abs());
        let costs = if details {
            format!(" ({played:.2} guesses to finish against {pick:.2})")
        } else {
            String::new()
        };
        return if rounded == "0.00" {
            format!("Skill: as good as lex's pick{costs}")
        } else if behind > 0.0 {
            format!("Skill: {rounded} guesses behind lex's pick{costs}")
        } else {
            format!("Skill: {rounded} guesses ahead of lex's pick{costs}")
        };
    }
    let bits = |w: &Worth| {
        if details {
            format!(" ({:.2} bits)", w.bits)
        } else {
            String::new()
        }
    };
    format!(
        "Skill: leaves {} expected{}, lex's leaves {:.0}{}",
        expected_words(g.played.remaining),
        bits(&g.played),
        g.pick.remaining,
        bits(&g.pick)
    )
}

/// The grading line for one turn: Luck then Skill, Skill alone on the winning turn.
#[must_use]
pub fn grading_line(g: &Grading, details: bool) -> String {
    luck(g, details).map_or_else(
        || skill(g, details),
        |luck| format!("{luck}    {}", skill(g, details)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn worth(remaining: f64, bits: f64, cost: Option<f64>) -> Worth {
        Worth {
            remaining,
            bits,
            cost,
        }
    }

    const fn grading(played: Worth, pick: Worth) -> Grading {
        Grading {
            matched: false,
            played,
            pick,
            left: Some(12),
            realised_bits: Some(4.25),
        }
    }

    #[test]
    fn a_pick_played_matched_lex() {
        let w = worth(38.0, 3.0, Some(3.5));
        let g = Grading {
            matched: true,
            ..grading(w, w)
        };
        assert_eq!(
            grading_line(&g, false),
            "Luck: 12 words left (expected 38)    Skill: matched lex"
        );
    }

    #[test]
    fn a_costlier_guess_is_behind() {
        let g = grading(worth(38.0, 3.0, Some(3.71)), worth(30.0, 3.4, Some(3.50)));
        assert_eq!(
            grading_line(&g, false),
            "Luck: 12 words left (expected 38)    Skill: 0.21 guesses behind lex's pick"
        );
        assert_eq!(
            grading_line(&g, true),
            "Luck: 12 words left (expected 38), 4.25 bits gained (expected 3.00)    \
             Skill: 0.21 guesses behind lex's pick (3.71 guesses to finish against 3.50)"
        );
    }

    #[test]
    fn a_tie_is_as_good() {
        let g = grading(worth(38.0, 3.0, Some(3.501)), worth(30.0, 3.4, Some(3.50)));
        assert_eq!(
            grading_line(&g, false),
            "Luck: 12 words left (expected 38)    Skill: as good as lex's pick"
        );
    }

    #[test]
    fn a_cheaper_guess_is_ahead() {
        let g = grading(worth(38.0, 3.0, Some(3.30)), worth(30.0, 3.4, Some(3.50)));
        assert!(grading_line(&g, false).ends_with("Skill: 0.20 guesses ahead of lex's pick"));
    }

    #[test]
    fn without_two_costs_the_expected_remaining_speaks_with_no_verdict() {
        let g = grading(worth(38.0, 3.0, None), worth(12.0, 4.0, Some(3.5)));
        assert_eq!(
            grading_line(&g, false),
            "Luck: 12 words left (expected 38)    Skill: leaves 38 words expected, lex's leaves 12"
        );
        assert!(
            grading_line(&g, true).ends_with(
                "Skill: leaves 38 words expected (3.00 bits), lex's leaves 12 (4.00 bits)"
            )
        );
    }

    #[test]
    fn the_winning_turn_grades_skill_only() {
        let g = Grading {
            left: None,
            ..grading(worth(38.0, 3.0, None), worth(12.0, 4.0, None))
        };
        assert_eq!(
            grading_line(&g, false),
            "Skill: leaves 38 words expected, lex's leaves 12"
        );
    }

    #[test]
    fn one_word_left_is_singular() {
        let g = Grading {
            left: Some(1),
            matched: true,
            ..grading(worth(1.0, 0.0, None), worth(1.0, 0.0, None))
        };
        assert!(grading_line(&g, false).starts_with("Luck: 1 word left (expected 1)"));
    }
}
