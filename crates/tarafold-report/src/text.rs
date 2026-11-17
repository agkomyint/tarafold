//! All user-facing wording (R9.2, design §5 Stage 8). Keep claims honest: observational text
//! never uses causal language; interventional text always carries its qualifiers.

/// Note attached to every report built from observational data.
pub const OBSERVATIONAL_FIT_NOTE: &str = "These equations describe how the variables move together in this \
data. On observational data they do not show what happens if you change an input.";

/// Disclaimer for optimization results on observational data.
pub const OBSERVATIONAL_OPTIMIZE_DISCLAIMER: &str = "Candidates are inputs for which the fitted equation \
predicts a value near the goal. Fits on observational data describe association, not the effect of \
intervening; test a candidate before relying on it.";

/// Note for designed (experimental or simulation) data.
pub const INTERVENTIONAL_FIT_NOTE: &str = "The data source was declared by the user as designed \
(experimental or simulation). Under that user-declared design, the equations describe how the \
outcome responded to the designed inputs, inside the data domain only.";

/// Disclaimer for optimization results on designed data.
pub const INTERVENTIONAL_OPTIMIZE_DISCLAIMER: &str = "Under the user-declared design, candidates are \
settings for which the fitted equation predicts a value near the goal, inside the data domain only. \
Candidates outside the data domain are extrapolations.";

/// Grade cap note while stability is not computed.
pub const STABILITY_PENDING: &str = "Structural stability was not computed (--stability 0), so the \
reliability grade is at most `medium`.";

/// Note attached to suggestions (R19.8).
pub const SUGGEST_NOTE: &str = "Suggestions are proposals of where to measure next, chosen to reduce \
uncertainty or to test settings that look promising. They are not predictions that a goal will be \
reached. Run them, fill in the results, and append the rows to the data for the next run.";

/// Phrases that must never appear in observational wording.
pub const BANNED_CAUSAL_PHRASES: [&str; 6] = [
    "causes",
    "caused by",
    "drives",
    "will increase",
    "will decrease",
    "leads to",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observational_text_has_no_causal_claims() {
        for t in [
            OBSERVATIONAL_FIT_NOTE,
            OBSERVATIONAL_OPTIMIZE_DISCLAIMER,
            STABILITY_PENDING,
        ] {
            let lower = t.to_lowercase();
            for p in BANNED_CAUSAL_PHRASES {
                assert!(!lower.contains(p), "banned phrase `{p}` in: {t}");
            }
        }
    }

    #[test]
    fn interventional_text_keeps_qualifiers() {
        for t in [INTERVENTIONAL_FIT_NOTE, INTERVENTIONAL_OPTIMIZE_DISCLAIMER] {
            assert!(t.contains("user-declared"), "{t}");
            assert!(t.contains("inside the data domain"), "{t}");
        }
    }
}
