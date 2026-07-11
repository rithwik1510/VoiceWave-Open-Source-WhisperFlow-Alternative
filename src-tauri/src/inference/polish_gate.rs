//! Fail-closed fidelity gate for on-device LLM polish (plans 005 + 010).
//!
//! Decides whether an LLM rewrite is SAFE to insert/offer using ONLY the raw
//! deterministic transcript and the candidate — exactly what the app has at
//! runtime. Extracted from `state.rs` (plan 010 Phase 0) with structured
//! rejection reasons instead of a bool, plus the per-profile checks of plan
//! 010 Phase 3.
//!
//! Design rules, in priority order:
//! - Accepted-output safety is 100%, not 99%: any protected-entity or
//!   identifier-casing alteration rejects, for every profile.
//! - Manual string scanning only (NO regex crate, by design): an over-broad
//!   detector here yields more SAFE false rejects, never false accepts.
//! - Rust is the authority; the Python worker only generates candidates. A
//!   rejection means the caller keeps the deterministic floor text.

use crate::settings::PolishProfile;
use crate::transcript::stabilize_custom_terms;
use std::collections::HashMap;

/// Why a polish candidate was rejected. Codes exist for telemetry and for the
/// (future, async-only) retry-with-correction prompt keyed to the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolishRejection {
    /// Candidate is empty/whitespace.
    EmptyOutput,
    /// Token overlap with the raw text fell below the profile's floor
    /// (wholesale rewrite / topic drift).
    LowOverlap,
    /// A protected raw token (name/number/code/URL/path/email) is missing
    /// from the candidate.
    EntityMissing,
    /// An identifier-like raw token (camelCase / snake_case / PascalCase /
    /// path) does not appear byte-identical in the candidate — catches the
    /// small-model `maxRetries` -> `MaxRetries` class.
    IdentifierChanged,
    /// The candidate contains a digit-bearing token absent from the raw text.
    NumberInvented,
    /// Negation count changed (added or dropped) — the highest-severity
    /// meaning flip, invisible to the overlap floor.
    NegationDropped,
    /// A modality word (maybe/must/probably/might/perhaps/possibly) was added
    /// or dropped — hedges must not be strengthened into directives or vice
    /// versa.
    ModalityDropped,
    /// A rare/technical literal token from the raw text (consonant-run
    /// heuristic: len >= 4 with 4+ consecutive consonants, e.g. a password or
    /// keyboard-mash string) vanished from the candidate. Ported from the
    /// plan-010 harness where it was required to reach 0 false-accepts.
    LiteralTokenDropped,
    /// The candidate looks cut off mid-thought (e.g. the model hit its token
    /// cap).
    OutputTruncated,
    /// Writing profile: a contraction survived in the output.
    ContractionInOutput,
    /// Writing profile: a hesitation filler survived in the output.
    FillerInOutput,
    /// Casual profile: headings / bullets / sign-offs in the output.
    StructureInOutput,
    /// Literal never calls the model; any candidate offered for it is a bug
    /// and is rejected outright.
    LiteralNeverPolished,
}

/// Punctuation trimmed from the ends of a whitespace token before
/// classification / comparison.
const TRIM_PUNCT: &str = ".,;:!?()\"'";

/// Per-profile floor on raw<->candidate token overlap (0..=1). Coding and
/// Casual legitimately drop more words (terse phrasing / chat register), so
/// their floors sit lower; drift below any floor rejects.
fn min_token_overlap(profile: PolishProfile) -> f32 {
    match profile {
        PolishProfile::Standard | PolishProfile::Writing => 0.55,
        PolishProfile::Coding | PolishProfile::Casual => 0.50,
        // Never LLM-gated; validate_polish rejects before reading this.
        PolishProfile::Literal => 1.0,
    }
}

fn trim_token(token: &str) -> &str {
    token.trim_matches(|c: char| TRIM_PUNCT.contains(c))
}

/// True if `lowered` (pre-lowercased) contains a run of 4+ consecutive ASCII
/// consonants (y excluded, matching the harness's validated heuristic — it
/// flags only rare/technical tokens, which legit rewrites keep anyway).
fn has_consonant_run(lowered: &str) -> bool {
    let mut run = 0usize;
    for c in lowered.chars() {
        if c.is_ascii_alphabetic() && !"aeiouy".contains(c) {
            run += 1;
            if run >= 4 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn contains_ascii_digit(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
}

/// True if the token has an internal capital (camelCase): some index i>0
/// where char[i] is uppercase and char[i-1] is lowercase.
fn has_internal_capital(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    for i in 1..chars.len() {
        if chars[i].is_uppercase() && chars[i - 1].is_lowercase() {
            return true;
        }
    }
    false
}

/// True if the token contains an internal `.` with alphanumerics on both
/// sides (URLs, filenames, `voicewave.dev`, `diagnostics.json`, `2.0`).
fn has_internal_dot(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    for i in 1..chars.len().saturating_sub(1) {
        if chars[i] == '.' && chars[i - 1].is_alphanumeric() && chars[i + 1].is_alphanumeric() {
            return true;
        }
    }
    false
}

/// A raw token is "protected" (must survive, case-insensitively at minimum)
/// when, after trimming surrounding punctuation, it looks like a
/// name/number/code/URL/path/email rather than an ordinary word.
fn is_protected(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    contains_ascii_digit(token)
        || token.contains('_')
        || token.contains('/')
        || token.contains('\\')
        || token.contains('@')
        || token.contains("::")
        || token.contains("()")
        || has_internal_capital(token)
        || has_internal_dot(token)
}

/// Identifier-like tokens must additionally survive BYTE-IDENTICAL
/// (case-sensitive): camelCase (`maxRetries`), mixed-case Pascal/acronym
/// forms (`MaxRetries`, `HTTPServer`), snake_case, paths, `::`, calls, and
/// dotted names with letters. Plain numbers ("2.0") carry no case and are
/// covered by the protected-token + digit checks instead.
fn is_identifier_like(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let has_lower = token.chars().any(|c| c.is_lowercase());
    let upper_count = token.chars().filter(|c| c.is_uppercase()).count();
    let has_alpha = token.chars().any(|c| c.is_alphabetic());
    has_internal_capital(token)
        || (upper_count >= 2 && has_lower)
        || (token.contains('_') && has_alpha)
        || token.contains('/')
        || token.contains('\\')
        || token.contains("::")
        || token.contains("()")
        || (has_internal_dot(token) && has_alpha)
}

/// Core negation markers (after contraction expansion). Every `n't`
/// contraction becomes " not", so it collapses to `not` here.
const NEGATION_WORDS: &[&str] = &[
    "not", "no", "never", "none", "nobody", "nothing", "nowhere", "neither", "nor", "without",
    "cannot",
];

/// Apostrophe-less negated contractions ASR frequently emits ("doesnt",
/// "dont"). Counted as negations so "doesnt" -> "doesn't" repairs are not
/// false-rejected as polarity changes.
const BARE_NEGATION_WORDS: &[&str] = &[
    "dont", "doesnt", "didnt", "isnt", "arent", "wasnt", "werent", "cant", "wont", "couldnt",
    "shouldnt", "wouldnt", "hasnt", "havent", "hadnt", "aint", "mustnt", "neednt",
];

/// Modality words whose presence count must be preserved (beyond the negation
/// set): a hedge must not be strengthened into a directive, nor a directive
/// weakened into a hedge. "not"/"never" already live in NEGATION_WORDS.
/// "probably"/"might"/"perhaps"/"possibly" were added after the plan-010
/// holdout run caught the 1.5B model turning "we should probably pin numpy"
/// into the directive "Pin numpy." — a hedge drop the maybe/must pair missed.
/// Deliberately excludes "should"/"could": legit restructures ("I think we
/// should X" -> "I recommend X") drop them without changing commitment.
const MODALITY_WORDS: &[&str] = &["maybe", "must", "probably", "might", "perhaps", "possibly"];

/// Hesitation fillers (the deterministic pruner's list). Used by the Writing
/// profile's no-fillers-in-output check.
const FILLER_WORDS: &[&str] = &[
    "um", "umm", "uhm", "uh", "uhh", "erm", "er", "ahem", "mhm",
];

/// Classic e-mail sign-off lines the Casual profile must never produce.
const SIGNOFF_LINES: &[&str] = &[
    "best",
    "regards",
    "best regards",
    "kind regards",
    "warm regards",
    "sincerely",
    "cheers",
    "yours truly",
];

/// Lowercase the text and expand `n't` contractions so `don't` == `do not`
/// for word counting.
fn expand_contractions_lower(text: &str) -> String {
    text.to_lowercase()
        .replace('\u{2019}', "'") // curly apostrophe -> straight
        .replace("n't", " not")
}

fn count_word(expanded_lower: &str, word: &str) -> usize {
    expanded_lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|candidate| *candidate == word)
        .count()
}

/// Count negation markers, robust to contraction form ("don't" == "do not"
/// == "dont", all count 1).
fn negation_count(text: &str) -> usize {
    let expanded = expand_contractions_lower(text);
    expanded
        .split(|c: char| !c.is_alphabetic())
        .filter(|word| NEGATION_WORDS.contains(word) || BARE_NEGATION_WORDS.contains(word))
        .count()
}

/// Lowercased word tokens (punctuation-trimmed), for the overlap metric.
fn overlap_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(trim_token)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

/// Multiset token-overlap F1 between two texts (0..=1). Self-contained port
/// of the controller's `asr_integrity_metrics` so this module has no
/// dependency on the desktop-only controller.
fn token_overlap(raw: &str, candidate: &str) -> f32 {
    let raw_tokens = overlap_tokens(raw);
    let candidate_tokens = overlap_tokens(candidate);
    if raw_tokens.is_empty() && candidate_tokens.is_empty() {
        return 1.0;
    }
    if raw_tokens.is_empty() || candidate_tokens.is_empty() {
        return 0.0;
    }
    let mut raw_counts = HashMap::<&str, u32>::new();
    for token in &raw_tokens {
        *raw_counts.entry(token.as_str()).or_default() += 1;
    }
    let mut overlap = 0u32;
    for token in &candidate_tokens {
        if let Some(remaining) = raw_counts.get_mut(token.as_str()) {
            if *remaining > 0 {
                *remaining -= 1;
                overlap += 1;
            }
        }
    }
    (2.0 * overlap as f32) / (raw_tokens.len() + candidate_tokens.len()) as f32
}

/// Heuristic for a generation cut off mid-thought: the candidate ends on a
/// connective punctuation mark or an opening bracket. finish_reason=length is
/// not visible through the worker protocol (old workers), so this is the
/// Rust-side floor; the new worker additionally reports truncation as an
/// error before we ever see the text.
fn looks_truncated(candidate: &str) -> bool {
    matches!(
        candidate.trim_end().chars().last(),
        Some(',' | ';' | ':' | '-' | '(' | '[' | '{')
    )
}

/// Common apostrophe contractions (Writing profile rejects these in output).
/// `'s` is only treated as a contraction for a fixed pronoun/wh-word list, so
/// possessives ("the team's plan") never false-reject.
fn is_contraction(token_lower: &str) -> bool {
    let token = token_lower.replace('\u{2019}', "'");
    let trimmed = token.trim_matches(|c: char| TRIM_PUNCT.contains(c) && c != '\'');
    let trimmed = trimmed.trim_matches(|c: char| c == '"');
    if trimmed.is_empty() || !trimmed.contains('\'') {
        return false;
    }
    if trimmed.ends_with("n't") {
        return true;
    }
    for suffix in ["'ll", "'re", "'ve", "'m", "'d"] {
        if trimmed.ends_with(suffix) {
            return true;
        }
    }
    matches!(
        trimmed,
        "it's"
            | "that's"
            | "there's"
            | "here's"
            | "what's"
            | "who's"
            | "where's"
            | "how's"
            | "he's"
            | "she's"
            | "let's"
    )
}

/// Detect headings/bullets/numbered items and sign-off lines (Casual must
/// stay plain chat prose).
fn has_structured_formatting(candidate: &str) -> bool {
    let lines: Vec<&str> = candidate
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    for (idx, line) in lines.iter().enumerate() {
        if line.starts_with('#')
            || line.starts_with("- ")
            || line.starts_with("* ")
            || line.starts_with("\u{2022}") // bullet char
        {
            return true;
        }
        // Numbered list item: leading digits then "." or ")" then space.
        let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let rest = &line[digits.len()..];
            if rest.starts_with(". ") || rest.starts_with(") ") {
                return true;
            }
        }
        // Short heading line ("Summary:") followed by more content.
        if idx + 1 < lines.len()
            && line.ends_with(':')
            && line.split_whitespace().count() <= 4
        {
            return true;
        }
        // Sign-off line ("Best," / "Regards").
        let bare = line.trim_end_matches([',', '.', '!']).to_lowercase();
        if SIGNOFF_LINES.contains(&bare.as_str()) {
            return true;
        }
    }
    false
}

/// Validate an LLM rewrite against the raw deterministic transcript for the
/// given profile. `Ok(())` only when EVERY check passes; otherwise all
/// collected rejection reasons (fail closed: caller keeps the deterministic
/// floor).
pub fn validate_polish(
    raw: &str,
    polished: &str,
    profile: PolishProfile,
) -> Result<(), Vec<PolishRejection>> {
    let mut rejections = Vec::new();

    if profile == PolishProfile::Literal {
        // Literal never calls the model; reject any candidate outright.
        return Err(vec![PolishRejection::LiteralNeverPolished]);
    }

    let polished_trimmed = polished.trim();
    if polished_trimmed.is_empty() {
        return Err(vec![PolishRejection::EmptyOutput]);
    }

    // 1. Token overlap floor (guards wholesale rewrite / topic drift), with a
    // per-profile floor.
    if token_overlap(raw, polished_trimmed) < min_token_overlap(profile) {
        rejections.push(PolishRejection::LowOverlap);
    }

    let polished_lower = polished_trimmed.to_lowercase();

    // 2. Entity preservation: every protected raw token must survive
    // (case-insensitive substring) in the rewrite; identifier-like tokens
    // must additionally survive byte-identical (case-sensitive). The casing
    // rule is absolute for every profile — it is what catches the 1.5B
    // model's `maxRetries` -> `MaxRetries` class.
    let mut entity_missing = false;
    let mut identifier_changed = false;
    for token in raw.split_whitespace() {
        let trimmed = trim_token(token);
        if trimmed.is_empty() {
            continue;
        }
        if is_identifier_like(trimmed) && !polished_trimmed.contains(trimmed) {
            identifier_changed = true;
        }
        if is_protected(trimmed) && !polished_lower.contains(&trimmed.to_lowercase()) {
            entity_missing = true;
        }
    }
    if identifier_changed {
        rejections.push(PolishRejection::IdentifierChanged);
    }
    if entity_missing {
        rejections.push(PolishRejection::EntityMissing);
    }

    // 3. No invented numbers: any digit-bearing token in the rewrite must
    // appear (case-insensitive substring) in the raw transcript.
    let raw_lower = raw.to_lowercase();
    for token in polished_trimmed.split_whitespace() {
        let trimmed = trim_token(token);
        if contains_ascii_digit(trimmed) && !raw_lower.contains(&trimmed.to_lowercase()) {
            rejections.push(PolishRejection::NumberInvented);
            break;
        }
    }

    // 4. Polarity preservation: the rewrite must not add or drop a negation.
    // One-word flips keep overlap high, so only this check catches them.
    if negation_count(raw) != negation_count(polished_trimmed) {
        rejections.push(PolishRejection::NegationDropped);
    }

    // 5. Modality preservation: hedges/directives must survive with the same
    // count — "maybe we should" must never become "we must", and "we should
    // probably pin numpy" must never become the directive "Pin numpy."
    let raw_expanded = expand_contractions_lower(raw);
    let polished_expanded = expand_contractions_lower(polished_trimmed);
    for word in MODALITY_WORDS {
        if count_word(&raw_expanded, word) != count_word(&polished_expanded, word) {
            rejections.push(PolishRejection::ModalityDropped);
            break;
        }
    }

    // 5b. Rare-literal preservation: raw tokens with a 4+ consonant run
    // (passwords, keyboard-mash, unusual technical strings) must survive
    // somewhere in the candidate. The small model tends to silently drop
    // them; the overlap floor is too coarse to notice one missing token.
    let polished_lower = polished_trimmed.to_lowercase();
    'outer: for token in raw.split(|c: char| !c.is_ascii_alphabetic()) {
        let lowered = token.to_lowercase();
        if lowered.len() >= 4
            && has_consonant_run(&lowered)
            && !polished_lower.contains(&lowered)
        {
            rejections.push(PolishRejection::LiteralTokenDropped);
            break 'outer;
        }
    }

    // 6. Truncation floor: a candidate cut off mid-thought must never land.
    if looks_truncated(polished_trimmed) {
        rejections.push(PolishRejection::OutputTruncated);
    }

    // 7. Per-profile register contracts.
    match profile {
        PolishProfile::Writing => {
            let mut contraction = false;
            let mut filler = false;
            for token in polished_trimmed.split_whitespace() {
                let lowered = token.to_lowercase();
                if is_contraction(&lowered) {
                    contraction = true;
                }
                let bare = lowered
                    .trim_matches(|c: char| !c.is_ascii_alphabetic())
                    .to_string();
                if FILLER_WORDS.contains(&bare.as_str()) {
                    filler = true;
                }
            }
            if contraction {
                rejections.push(PolishRejection::ContractionInOutput);
            }
            if filler {
                rejections.push(PolishRejection::FillerInOutput);
            }
        }
        PolishProfile::Casual => {
            if has_structured_formatting(polished_trimmed) {
                rejections.push(PolishRejection::StructureInOutput);
            }
        }
        // Coding's "reject any altered identifier or path" is enforced by the
        // absolute byte-identical identifier check above (check 2).
        PolishProfile::Standard | PolishProfile::Coding | PolishProfile::Literal => {}
    }

    if rejections.is_empty() {
        Ok(())
    } else {
        Err(rejections)
    }
}

/// Full gate for a worker candidate: re-apply the dictionary-term stabilizer
/// (exactly as the deterministic pipeline does), then validate. On accept,
/// returns the stabilized text that is safe to insert/offer.
pub fn gate_polish_candidate(
    raw: &str,
    candidate: &str,
    custom_terms: &[String],
    profile: PolishProfile,
) -> Result<String, Vec<PolishRejection>> {
    let stabilized = stabilize_custom_terms(candidate, custom_terms);
    validate_polish(raw, &stabilized, profile).map(|()| stabilized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepts(raw: &str, polished: &str, profile: PolishProfile) -> bool {
        validate_polish(raw, polished, profile).is_ok()
    }

    fn rejected_with(raw: &str, polished: &str, profile: PolishProfile) -> Vec<PolishRejection> {
        validate_polish(raw, polished, profile).expect_err("expected rejection")
    }

    // ---- Ports of the original plan-005 validator tests (semantics kept) ---

    #[test]
    fn rejects_dropped_or_changed_number() {
        let reasons = rejected_with(
            "set the limit to 5",
            "set the limit to five",
            PolishProfile::Standard,
        );
        assert!(reasons.contains(&PolishRejection::EntityMissing));
    }

    #[test]
    fn accepts_clean_filler_removal() {
        assert!(accepts(
            "um so we should ship on monday",
            "We should ship on Monday.",
            PolishProfile::Standard,
        ));
    }

    #[test]
    fn rejects_negation_flip() {
        let reasons = rejected_with(
            "i don't think we should ship this on friday",
            "I think we should ship this on Friday.",
            PolishProfile::Standard,
        );
        assert!(reasons.contains(&PolishRejection::NegationDropped));
    }

    #[test]
    fn accepts_contraction_expansion() {
        // "don't" -> "do not" is the SAME polarity; must NOT be a false reject.
        assert!(accepts(
            "we don't need to refactor everything right now",
            "We do not need to refactor everything right now.",
            PolishProfile::Standard,
        ));
    }

    // ---- New plan-010 checks, table-driven ---------------------------------

    #[test]
    fn identifier_casing_alterations_reject_for_every_profile() {
        let cases: &[(&str, &str, &str)] = &[
            // (raw, polished, note)
            (
                "set maxRetries to a higher value",
                "Set MaxRetries to a higher value.",
                "camelCase -> PascalCase",
            ),
            (
                "rename user_id in the payload",
                "Rename User_Id in the payload.",
                "snake_case recased",
            ),
            (
                "check the HTTPServer config",
                "Check the httpserver config.",
                "acronym-mixed identifier lowercased",
            ),
            (
                "open src/lib.rs and fix it",
                "Open SRC/LIB.RS and fix it.",
                "path recased",
            ),
        ];
        for profile in [
            PolishProfile::Standard,
            PolishProfile::Coding,
            PolishProfile::Writing,
            PolishProfile::Casual,
        ] {
            for (raw, polished, note) in cases {
                let reasons = rejected_with(raw, polished, profile);
                assert!(
                    reasons.contains(&PolishRejection::IdentifierChanged),
                    "{note} must reject as IdentifierChanged for {profile:?}, got {reasons:?}"
                );
            }
        }
    }

    #[test]
    fn identifiers_preserved_byte_identical_accept() {
        let cases: &[(&str, &str)] = &[
            (
                "refactor getUserById to return null when the user doesn't exist",
                "Refactor getUserById to return null when the user doesn't exist.",
            ),
            (
                "rename user_id to account_id in the payload struct",
                "Rename user_id to account_id in the payload struct.",
            ),
            (
                "open src/lib.rs and look at the insert function",
                "Open src/lib.rs and look at the insert function.",
            ),
        ];
        for (raw, polished) in cases {
            assert!(
                accepts(raw, polished, PolishProfile::Coding),
                "should accept: {polished}"
            );
        }
    }

    #[test]
    fn identifier_dropped_entirely_rejects() {
        let reasons = rejected_with(
            "call fetchUserProfile before rendering the page",
            "Call the fetch function before rendering the page.",
            PolishProfile::Coding,
        );
        assert!(reasons.contains(&PolishRejection::IdentifierChanged));
        assert!(reasons.contains(&PolishRejection::EntityMissing));
    }

    #[test]
    fn modality_words_must_survive() {
        let cases: &[(&str, &str)] = &[
            (
                "maybe we should refactor the parser",
                "We should refactor the parser.",
            ),
            (
                "we must ship the fix before friday",
                "We should ship the fix before friday.",
            ),
            (
                "we should ship this",
                "We must ship this.",
            ),
            // The plan-010 holdout false-accept: "probably" dropped, hedge
            // became a directive. Must reject under the extended word list.
            (
                "we should probably pin numpy to the old version",
                "Pin numpy to the old version.",
            ),
            (
                "it might just be the cold start",
                "It is just the cold start.",
            ),
        ];
        for (raw, polished) in cases {
            let reasons = rejected_with(raw, polished, PolishProfile::Standard);
            assert!(
                reasons.contains(&PolishRejection::ModalityDropped),
                "expected ModalityDropped for '{polished}', got {reasons:?}"
            );
        }
        // Preserved modality must not false-reject.
        assert!(accepts(
            "maybe we should refactor the parser",
            "Maybe we should refactor the parser.",
            PolishProfile::Standard,
        ));
        assert!(accepts(
            "it might just be the cold start",
            "It might just be the cold start.",
            PolishProfile::Standard,
        ));
    }

    #[test]
    fn rare_literal_tokens_must_survive() {
        // Consonant-run tokens (passwords, technical strings) silently dropped
        // by the model must reject; ported from the plan-010 harness.
        let reasons = rejected_with(
            "the temp password is asdfgh please rotate it after",
            "The temp password is — please rotate it after.",
            PolishProfile::Standard,
        );
        assert!(
            reasons.contains(&PolishRejection::LiteralTokenDropped),
            "expected LiteralTokenDropped, got {reasons:?}"
        );
        // Kept literal (case-folded) must not false-reject.
        assert!(accepts(
            "the temp password is asdfgh please rotate it after",
            "The temp password is asdfgh; please rotate it after.",
            PolishProfile::Standard,
        ));
        // Ordinary words with short consonant runs must not trip the check.
        assert!(accepts(
            "we should strengthen the tests first",
            "We should strengthen the tests first.",
            PolishProfile::Standard,
        ));
    }

    #[test]
    fn paraphrase_drift_rejects_on_low_overlap() {
        let reasons = rejected_with(
            "we should meet tomorrow to talk about the release plan and the website",
            "The quarterly financials show strong growth across all segments.",
            PolishProfile::Standard,
        );
        assert!(reasons.contains(&PolishRejection::LowOverlap));
    }

    #[test]
    fn truncated_output_rejects() {
        let cases = ["We should refactor the parser and,", "The plan is:", "First we fix the (",];
        for polished in cases {
            let reasons = rejected_with(
                "we should refactor the parser and fix the tests",
                polished,
                PolishProfile::Standard,
            );
            assert!(
                reasons.contains(&PolishRejection::OutputTruncated)
                    || reasons.contains(&PolishRejection::LowOverlap),
                "expected truncation-class rejection for '{polished}', got {reasons:?}"
            );
        }
    }

    #[test]
    fn writing_rejects_contractions_and_fillers_in_output() {
        let contraction = rejected_with(
            "we do not need to change the api surface for this",
            "We don't need to change the api surface for this.",
            PolishProfile::Writing,
        );
        assert!(contraction.contains(&PolishRejection::ContractionInOutput));

        let filler = rejected_with(
            "we should schedule the meeting for monday morning",
            "Um, we should schedule the meeting for Monday morning.",
            PolishProfile::Writing,
        );
        assert!(filler.contains(&PolishRejection::FillerInOutput));

        // Possessives are NOT contractions — must not false-reject.
        assert!(accepts(
            "the team's plan needs a review before monday",
            "The team's plan needs a review before Monday.",
            PolishProfile::Writing,
        ));
    }

    #[test]
    fn standard_and_casual_allow_contractions() {
        for profile in [PolishProfile::Standard, PolishProfile::Casual] {
            assert!(accepts(
                "we do not need to change this right now",
                "We don't need to change this right now.",
                profile,
            ));
        }
    }

    #[test]
    fn casual_rejects_headings_bullets_and_signoffs() {
        let raw = "let them know the release slipped to tuesday and we will send the build then";
        let cases: &[(&str, &str)] = &[
            (
                "Update:\nThe release slipped to Tuesday. We will send the build then.",
                "heading line",
            ),
            (
                "- The release slipped to Tuesday\n- We will send the build then",
                "bullets",
            ),
            (
                "1. The release slipped to Tuesday. 2. We will send the build then.\n2) done",
                "numbered list",
            ),
            (
                "The release slipped to Tuesday, we will send the build then.\nBest regards,",
                "sign-off",
            ),
        ];
        for (polished, note) in cases {
            let reasons = rejected_with(raw, polished, PolishProfile::Casual);
            assert!(
                reasons.contains(&PolishRejection::StructureInOutput),
                "{note} must reject for Casual, got {reasons:?}"
            );
        }
        // Plain chat register passes.
        assert!(accepts(
            raw,
            "The release slipped to Tuesday, we'll send the build then.",
            PolishProfile::Casual,
        ));
    }

    #[test]
    fn coding_tolerates_terse_rewrites_within_floor() {
        assert!(accepts(
            "so um i think we should refactor getUserById to not throw when the user doesnt exist and instead return null",
            "Refactor getUserById to not throw when the user doesn't exist and instead return null.",
            PolishProfile::Coding,
        ));
    }

    #[test]
    fn bare_negation_repair_is_not_a_polarity_change() {
        // ASR often drops apostrophes; repairing "doesnt" -> "doesn't" keeps
        // polarity and must not false-reject.
        assert!(accepts(
            "it doesnt work when the cache is cold",
            "It doesn't work when the cache is cold.",
            PolishProfile::Standard,
        ));
    }

    #[test]
    fn literal_rejects_any_candidate() {
        let reasons = rejected_with(
            "keep my words as they are",
            "Keep my words as they are.",
            PolishProfile::Literal,
        );
        assert_eq!(reasons, vec![PolishRejection::LiteralNeverPolished]);
    }

    #[test]
    fn empty_output_rejects() {
        let reasons = rejected_with("say something", "   ", PolishProfile::Standard);
        assert_eq!(reasons, vec![PolishRejection::EmptyOutput]);
    }

    #[test]
    fn invented_number_rejects() {
        let reasons = rejected_with(
            "increase the timeout a little",
            "Increase the timeout to 5000 ms.",
            PolishProfile::Standard,
        );
        assert!(reasons.contains(&PolishRejection::NumberInvented));
    }

    #[test]
    fn gate_reapplies_custom_terms_before_validation() {
        // The raw deterministic text contains the stabilized dictionary
        // casing "VoiceWave"; the model output drifted to "voicewave". The
        // stabilizer restores it, so the identifier check passes and the
        // returned text carries the canonical casing.
        let custom_terms = vec!["VoiceWave".to_string()];
        let gated = gate_polish_candidate(
            "tell them VoiceWave shipped the new profiles today",
            "voicewave shipped the new profiles today.",
            &custom_terms,
            PolishProfile::Standard,
        )
        .expect("gate should accept after stabilization");
        assert!(gated.contains("VoiceWave"));
    }
}
