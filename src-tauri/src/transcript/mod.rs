use crate::settings::{
    AppProfileBehavior, CodeCasingStyle, CodeModeSettings, DomainPackId, FormatProfile,
};

pub fn sanitize_user_transcript(input: &str) -> String {
    let without_artifacts = strip_bracket_artifacts(input);
    normalize_whitespace(&without_artifacts)
}

pub fn finalize_user_transcript(input: &str) -> String {
    let sanitized = sanitize_user_transcript(input);
    if sanitized.is_empty() {
        return sanitized;
    }

    if let Some(list_text) = format_spoken_numbered_list(&sanitized) {
        return list_text;
    }

    format_sentence_fragment(&sanitized)
}

pub struct ProTranscriptOptions<'a> {
    pub format_profile: FormatProfile,
    pub domain_packs: &'a [DomainPackId],
    pub code_mode: &'a CodeModeSettings,
    pub post_processing_enabled: bool,
    pub spoken_edit_commands: bool,
    pub app_profile_behavior: &'a AppProfileBehavior,
    pub custom_terms: &'a [String],
}

pub fn finalize_pro_transcript(input: &str, options: &ProTranscriptOptions<'_>) -> String {
    // Cleanup must run before sentence finalization: pruning a leading "Um,"
    // after capitalization would leave the sentence starting lowercase.
    let mut cleaned = input.to_string();
    if options.post_processing_enabled {
        let pruned = collapse_repeated_tokens(&prune_fillers(&cleaned));
        // If the utterance was nothing but fillers, keep it verbatim rather
        // than erasing the user's dictation into an error.
        if !pruned.trim().is_empty() {
            cleaned = pruned;
        }
    }

    let mut text = finalize_user_transcript(&cleaned);
    if text.is_empty() {
        return text;
    }

    if options.spoken_edit_commands {
        text = apply_structural_commands(&text);
    }

    text = apply_domain_corrections(&text, options.domain_packs);
    text = stabilize_custom_terms(&text, options.custom_terms);
    text = apply_format_profile(&text, options.format_profile);
    text = apply_app_profile_behavior(&text, options.app_profile_behavior);

    if options.code_mode.enabled {
        text = apply_code_mode(&text, options.code_mode);
    }

    normalize_output_whitespace(&text)
}

/// Literal profile finalization (plan 010): branches from the sanitized ASR
/// baseline BEFORE the deterministic transform stack, because by the end of
/// `finalize_pro_transcript` the original word sequence (fillers, repeated
/// tokens) is already gone.
///
/// Applies ONLY:
/// - explicit spoken punctuation/structural commands (user commands, not AI
///   rewriting) when `spoken_edit_commands` is on,
/// - user-dictionary stabilization (corrects recognition toward what was
///   actually said),
/// - punctuation/capitalization (sentence casing, pronoun "I", terminal
///   punctuation).
///
/// Deliberately skipped: filler pruning, stutter collapse, domain
/// corrections, format profiles, app-profile behavior, code mode, and the
/// heuristic spoken-numbered-list formatter (a formatting transform, not an
/// explicit command).
pub fn finalize_literal_transcript(
    input: &str,
    spoken_edit_commands: bool,
    custom_terms: &[String],
) -> String {
    let sanitized = sanitize_user_transcript(input);
    if sanitized.is_empty() {
        return sanitized;
    }
    let mut text = format_sentence_fragment(&sanitized);
    if spoken_edit_commands {
        text = apply_structural_commands(&text);
    }
    text = stabilize_custom_terms(&text, custom_terms);
    normalize_output_whitespace(&text)
}

pub fn merge_incremental_transcript(
    committed: &str,
    incoming: &str,
    overlap_token_limit: usize,
) -> String {
    let committed_norm = normalize_whitespace(committed);
    let incoming_norm = normalize_whitespace(incoming);
    if committed_norm.is_empty() {
        return incoming_norm;
    }
    if incoming_norm.is_empty() {
        return committed_norm;
    }

    let committed_tokens = committed_norm.split_whitespace().collect::<Vec<_>>();
    let incoming_tokens = incoming_norm.split_whitespace().collect::<Vec<_>>();
    let overlap_cap = overlap_token_limit
        .max(1)
        .min(committed_tokens.len())
        .min(incoming_tokens.len());

    for overlap_len in (1..=overlap_cap).rev() {
        let committed_tail = &committed_tokens[committed_tokens.len() - overlap_len..];
        let incoming_head = &incoming_tokens[..overlap_len];
        if tokens_match_case_insensitive(committed_tail, incoming_head) {
            let mut merged = committed_tokens
                .iter()
                .map(|token| (*token).to_string())
                .collect::<Vec<_>>();
            merged.extend(
                incoming_tokens[overlap_len..]
                    .iter()
                    .map(|token| (*token).to_string()),
            );
            return merged.join(" ");
        }
    }

    format!("{committed_norm} {incoming_norm}")
}

fn strip_bracket_artifacts(input: &str) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(input.len());
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] == '[' {
            let mut end = idx + 1;
            while end < chars.len() && chars[end] != ']' {
                end += 1;
            }

            if end < chars.len() {
                let token = chars[idx + 1..end].iter().collect::<String>();
                if is_artifact_token(&token) {
                    idx = end + 1;
                    continue;
                }
            }
        }

        output.push(chars[idx]);
        idx += 1;
    }

    output
}

fn is_artifact_token(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 48 {
        return false;
    }

    let mut saw_ascii_alpha = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphabetic() {
            if ch.is_ascii_lowercase() {
                return false;
            }
            saw_ascii_alpha = true;
            continue;
        }

        if ch.is_ascii_digit() || ch == '_' || ch == '-' || ch == ' ' {
            continue;
        }

        return false;
    }

    saw_ascii_alpha
}

fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_output_whitespace(input: &str) -> String {
    input
        .split('\n')
        .map(normalize_whitespace)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_spoken_numbered_list(input: &str) -> Option<String> {
    let tokens = input.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 6 {
        return None;
    }

    let mut entries = Vec::<(u32, Vec<String>)>::new();
    let mut current_number = None::<u32>;
    let mut current_words = Vec::<String>::new();
    let mut preamble_words = Vec::<String>::new();
    let mut saw_compound_number_marker = false;
    let mut first_list_marker_idx = None::<usize>;

    let mut idx = 0usize;
    while idx < tokens.len() {
        if let Some((number, marker_len)) = parse_number_marker_at(&tokens, idx) {
            if marker_len > 1 {
                saw_compound_number_marker = true;
            }
            if current_number.is_none() {
                // Treat non-`one` prefacing counts (for example "there are two ways ...")
                // as intro context instead of a list start.
                if number != 1 || is_intro_count_marker(&tokens, idx) {
                    for marker_token in &tokens[idx..idx + marker_len] {
                        preamble_words.push((*marker_token).to_string());
                    }
                    idx += marker_len;
                    continue;
                }
                first_list_marker_idx = Some(idx);
            }
            if let Some(existing) = current_number {
                if !current_words.is_empty() {
                    entries.push((existing, std::mem::take(&mut current_words)));
                }
            }
            current_number = Some(number);
            idx += marker_len;
            continue;
        }

        if current_number.is_some() {
            current_words.push(tokens[idx].to_string());
        } else {
            preamble_words.push(tokens[idx].to_string());
        }
        idx += 1;
    }

    let Some(last_number) = current_number else {
        return None;
    };
    if !current_words.is_empty() {
        entries.push((last_number, current_words));
    }

    if entries.len() < 2 {
        return None;
    }

    if entries[0].0 != 1 {
        return None;
    }
    if !entries
        .windows(2)
        .all(|window| window[1].0 == window[0].0 + 1)
    {
        return None;
    }
    let list_intent_from_intro = if preamble_words.is_empty() {
        false
    } else {
        has_list_introduction_intent(&preamble_words.join(" "), entries.len())
    };
    if !preamble_words.is_empty() && !list_intent_from_intro && !saw_compound_number_marker {
        return None;
    }
    if entries.iter().any(|(_, words)| {
        words.is_empty()
            || (!list_intent_from_intro && !saw_compound_number_marker && words.len() < 2)
    }) {
        return None;
    }

    let lines = entries
        .into_iter()
        .map(|(number, words)| {
            let content = words.join(" ");
            format!("{number}. {}", format_sentence_fragment(&content))
        })
        .collect::<Vec<_>>();
    let list_body = lines.join("\n");

    let preserve_preamble = first_list_marker_idx
        .map(|marker_idx| has_sentence_boundary_before_index(&tokens, marker_idx))
        .unwrap_or(false);

    if preserve_preamble && !preamble_words.is_empty() {
        let preamble = format_sentence_fragment(&preamble_words.join(" "));
        if !preamble.is_empty() {
            return Some(format!("{preamble}\n{list_body}"));
        }
    }

    Some(list_body)
}

fn has_sentence_boundary_before_index(tokens: &[&str], marker_idx: usize) -> bool {
    tokens
        .iter()
        .take(marker_idx)
        .any(|token| token.trim_end().ends_with(['.', '!', '?']))
}

fn is_intro_count_marker(tokens: &[&str], idx: usize) -> bool {
    let Some(next_token) = tokens.get(idx + 1) else {
        return false;
    };
    let next = normalize_marker_token(next_token);
    matches!(
        next.as_str(),
        "process"
            | "processes"
            | "way"
            | "ways"
            | "points"
            | "items"
            | "things"
            | "steps"
            | "reasons"
            | "parts"
            | "topics"
    )
}

fn has_list_introduction_intent(input: &str, entry_count: usize) -> bool {
    let lowered = input.to_ascii_lowercase();
    let intent_markers = [
        "there are",
        "there is",
        "here are",
        "here is",
        "following",
        "list",
        "steps",
        "step",
        "process",
        "processes",
        "points",
        "items",
        "tasks",
        "reasons",
        "things",
        "parts",
        "priority",
        "priorities",
    ];
    if intent_markers.iter().any(|marker| lowered.contains(marker)) {
        return true;
    }

    match entry_count {
        2 => lowered.contains("two") || lowered.contains("2"),
        3 => lowered.contains("three") || lowered.contains("3"),
        4 => lowered.contains("four") || lowered.contains("4"),
        _ => false,
    }
}

fn parse_number_marker(token: &str) -> Option<u32> {
    let lowered = token
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    match lowered.as_str() {
        "1" | "one" | "first" | "firstly" => Some(1),
        "2" | "two" | "second" | "secondly" => Some(2),
        "3" | "three" | "third" | "thirdly" => Some(3),
        "4" | "four" | "fourth" => Some(4),
        "5" | "five" | "fifth" => Some(5),
        "6" | "six" | "sixth" => Some(6),
        "7" | "seven" | "seventh" => Some(7),
        "8" | "eight" | "eighth" => Some(8),
        "9" | "nine" | "ninth" => Some(9),
        "10" | "ten" | "tenth" => Some(10),
        _ => None,
    }
}

fn parse_number_marker_at(tokens: &[&str], idx: usize) -> Option<(u32, usize)> {
    if let Some(number) = parse_number_marker(tokens[idx]) {
        return Some((number, 1));
    }

    let marker = normalize_marker_token(tokens[idx]);
    if marker == "number" {
        let next = tokens.get(idx + 1)?;
        if let Some(number) = parse_number_marker(next) {
            return Some((number, 2));
        }
    }

    None
}

fn format_sentence_fragment(input: &str) -> String {
    let mut text = normalize_whitespace(input);
    text = normalize_punctuation_spacing(&text);
    text = normalize_pronoun_i(&text);
    text = capitalize_first_alpha(&text);
    ensure_terminal_punctuation(&text)
}

fn normalize_punctuation_spacing(input: &str) -> String {
    let mut output = input.to_string();
    for pattern in [" ,", " .", " !", " ?", " ;", " :"] {
        let replacement = &pattern[1..];
        output = output.replace(pattern, replacement);
    }
    output
}

fn normalize_pronoun_i(input: &str) -> String {
    input
        .split_whitespace()
        .map(|token| {
            if token.eq_ignore_ascii_case("i") {
                "I".to_string()
            } else if token.to_ascii_lowercase().starts_with("i'") {
                format!("I{}", &token[1..])
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_first_alpha(input: &str) -> String {
    let mut chars = input.chars().collect::<Vec<_>>();
    for ch in &mut chars {
        if ch.is_ascii_alphabetic() {
            *ch = ch.to_ascii_uppercase();
            break;
        }
    }
    chars.into_iter().collect()
}

fn ensure_terminal_punctuation(input: &str) -> String {
    let trimmed = input.trim();
    let trimmed = trimmed
        .trim_end_matches(|ch| matches!(ch, ',' | ';' | ':'))
        .trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if matches!(trimmed.chars().last(), Some('.' | '!' | '?')) {
        return trimmed.to_string();
    }
    format!("{trimmed}.")
}

fn prune_fillers(input: &str) -> String {
    // Hesitation sounds only — never drop discourse words like "like",
    // "well", or "so", which are real words far more often than fillers.
    let fillers = ["uh", "uhh", "um", "umm", "uhm", "erm", "er", "ahem", "mhm"];
    input
        .split('\n')
        .map(|line| {
            line.split_whitespace()
                .filter(|token| {
                    let cleaned = token
                        .trim_matches(|ch: char| !ch.is_ascii_alphabetic())
                        .to_ascii_lowercase();
                    !fillers.contains(&cleaned.as_str())
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn collapse_repeated_tokens(input: &str) -> String {
    input
        .split('\n')
        .map(collapse_repeated_tokens_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn collapse_repeated_tokens_line(line: &str) -> String {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    let mut output = Vec::<&str>::with_capacity(tokens.len());
    let mut idx = 0usize;
    while idx < tokens.len() {
        // On a match we keep the LATER occurrence: the earlier one usually
        // carries the stutter's pause punctuation ("i, i think" -> "i think").

        // Restart bigram: "I think I think this works" -> "I think this works".
        if output.len() >= 2 && idx + 1 < tokens.len() {
            let prev_a = normalize_marker_token(output[output.len() - 2]);
            let prev_b = normalize_marker_token(output[output.len() - 1]);
            if !prev_a.is_empty()
                && !prev_b.is_empty()
                && prev_a == normalize_marker_token(tokens[idx])
                && prev_b == normalize_marker_token(tokens[idx + 1])
            {
                output.truncate(output.len() - 2);
                output.push(tokens[idx]);
                output.push(tokens[idx + 1]);
                idx += 2;
                continue;
            }
        }
        // Stutter: "the the" / "I, I" -> single token (punctuation-insensitive).
        if let Some(last) = output.last_mut() {
            let normalized = normalize_marker_token(last);
            if !normalized.is_empty() && normalized == normalize_marker_token(tokens[idx]) {
                *last = tokens[idx];
                idx += 1;
                continue;
            }
        }
        output.push(tokens[idx]);
        idx += 1;
    }
    output.join(" ")
}

fn apply_domain_corrections(input: &str, packs: &[DomainPackId]) -> String {
    let mut text = input.to_string();
    for pack in packs {
        let replacements: &[(&str, &str)] = match pack {
            DomainPackId::Coding => &[
                ("github", "GitHub"),
                ("api", "API"),
                ("json", "JSON"),
                ("typescript", "TypeScript"),
                ("javascript", "JavaScript"),
                ("rust", "Rust"),
            ],
            DomainPackId::Student => &[("gpa", "GPA"), ("phd", "PhD"), ("masters", "Master's")],
            DomainPackId::Productivity => &[("to do", "to-do"), ("follow up", "follow-up")],
        };

        for (from, to) in replacements {
            text = replace_boundary_phrase_case_insensitive(&text, from, to);
        }
    }
    text
}

pub(crate) fn stabilize_custom_terms(input: &str, custom_terms: &[String]) -> String {
    let mut text = input.to_string();
    for term in custom_terms {
        let normalized = term.trim();
        if normalized.len() < 2 {
            continue;
        }
        text = replace_boundary_phrase_case_insensitive(&text, normalized, normalized);
    }
    text
}

fn apply_format_profile(input: &str, profile: FormatProfile) -> String {
    match profile {
        FormatProfile::Default => input.to_string(),
        FormatProfile::Academic => apply_writing_profile(input),
        FormatProfile::Technical => {
            let mut text = input.to_string();
            text = replace_boundary_phrase_case_insensitive(&text, " e g ", " e.g. ");
            text = replace_boundary_phrase_case_insensitive(&text, " i e ", " i.e. ");
            text
        }
        FormatProfile::Concise => apply_study_profile(input),
        FormatProfile::CodeDoc => {
            if let Some(list) = format_spoken_numbered_list(input) {
                list
            } else {
                input.to_string()
            }
        }
    }
}

/// Spoken structural commands that apply regardless of format profile.
/// Recognized only as standalone boundary phrases (via
/// `replace_boundary_phrase_case_insensitive`) so embedded usage such as
/// "newline" inside an identifier is not rewritten. These were previously
/// trapped inside the Academic/Concise profiles; this step is now the single
/// source for them.
fn apply_structural_commands(input: &str) -> String {
    let mut text = input.to_string();
    text = replace_boundary_phrase_case_insensitive(&text, "new paragraph", "\n\n");
    text = replace_boundary_phrase_case_insensitive(&text, "new line", "\n");
    text = replace_boundary_phrase_case_insensitive(&text, "next line", "\n");
    text = replace_boundary_phrase_case_insensitive(&text, "bullet point", "\n- ");
    text = replace_boundary_phrase_case_insensitive(&text, "new bullet", "\n- ");
    text
}

fn apply_writing_profile(input: &str) -> String {
    let mut text = input.to_string();
    text = replace_boundary_phrase_case_insensitive(&text, "don't", "do not");
    text = replace_boundary_phrase_case_insensitive(&text, "can't", "cannot");
    text = replace_boundary_phrase_case_insensitive(&text, "won't", "will not");

    if let Some(list) = format_spoken_numbered_list(&text) {
        return list;
    }

    text
}

fn apply_study_profile(input: &str) -> String {
    let mut text = input.to_string();
    text = text
        .replace("Basically ", "")
        .replace("Actually ", "")
        .replace("In order to", "To");

    if let Some(note_sections) = format_study_note_sections(&text) {
        return note_sections;
    }
    if let Some(list) = format_spoken_numbered_list(&text) {
        return list;
    }

    text
}

fn format_study_note_sections(input: &str) -> Option<String> {
    let tokens = input.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 6 {
        return None;
    }

    let mut sections = Vec::<(String, Vec<String>)>::new();
    let mut current_label: Option<&str> = None;
    let mut current_words = Vec::<String>::new();
    let mut idx = 0usize;

    while idx < tokens.len() {
        let current = normalize_marker_token(tokens[idx]);
        let next = tokens
            .get(idx + 1)
            .map(|token| normalize_marker_token(token))
            .unwrap_or_default();

        let marker = if current == "topic" {
            Some(("Topic", 1usize))
        } else if current == "definition" {
            Some(("Definition", 1usize))
        } else if current == "example" {
            Some(("Example", 1usize))
        } else if current == "summary" {
            Some(("Summary", 1usize))
        } else if current == "recap" {
            Some(("Recap", 1usize))
        } else if current == "key" && next == "point" {
            Some(("Key Point", 2usize))
        } else {
            None
        };

        if let Some((label, consumed_tokens)) = marker {
            if let Some(existing) = current_label {
                if !current_words.is_empty() {
                    sections.push((existing.to_string(), std::mem::take(&mut current_words)));
                }
            }
            current_label = Some(label);
            idx += consumed_tokens;
            continue;
        }

        if current_label.is_some() {
            current_words.push(tokens[idx].to_string());
        }
        idx += 1;
    }

    if let Some(existing) = current_label {
        if !current_words.is_empty() {
            sections.push((existing.to_string(), current_words));
        }
    }

    if sections.len() < 2 {
        return None;
    }

    Some(
        sections
            .into_iter()
            .map(|(label, words)| {
                format!("{label}: {}", format_sentence_fragment(&words.join(" ")))
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn normalize_marker_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

fn apply_app_profile_behavior(input: &str, behavior: &AppProfileBehavior) -> String {
    let mut text = input.to_string();

    if behavior.sentence_compactness >= 2 {
        text = text.replace(". ", "; ");
    }

    if behavior.auto_list_formatting && text.contains("; ") {
        let segments = text
            .split(';')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() >= 3 {
            text = segments
                .iter()
                .map(|segment| format!("- {}", capitalize_first_alpha(segment)))
                .collect::<Vec<_>>()
                .join("\n");
        }
    }

    if behavior.punctuation_aggressiveness == 0 {
        text = text
            .trim_end_matches(|ch| matches!(ch, '.' | '!' | '?'))
            .to_string();
    } else if behavior.punctuation_aggressiveness >= 2 {
        text = ensure_terminal_punctuation(&text);
    }

    text
}

fn apply_code_mode(input: &str, settings: &CodeModeSettings) -> String {
    let mut text = input
        .trim()
        .trim_end_matches(|ch| matches!(ch, '.' | '!' | '?'))
        .to_string();

    if settings.spoken_symbols {
        let symbol_map = [
            ("open parenthesis", "("),
            ("close parenthesis", ")"),
            ("open parens", "("),
            ("close parens", ")"),
            ("left paren", "("),
            ("right paren", ")"),
            ("open parent", "("),
            ("close parent", ")"),
            ("open paren", "("),
            ("close paren", ")"),
            ("open square bracket", "["),
            ("close square bracket", "]"),
            ("open bracket", "["),
            ("close bracket", "]"),
            ("open curly brace", "{"),
            ("close curly brace", "}"),
            ("open brace", "{"),
            ("close brace", "}"),
            ("open angle bracket", "<"),
            ("close angle bracket", ">"),
            ("underscore", "_"),
            ("dash", "-"),
            ("arrow", "->"),
            ("double equals", "=="),
            ("equals", "="),
            ("colon", ":"),
            ("comma", ","),
            ("dot", "."),
            ("slash", "/"),
            ("back slash", "\\"),
            ("double quote", "\""),
            ("single quote", "'"),
        ];
        for (spoken, symbol) in symbol_map {
            text = replace_boundary_phrase_case_insensitive(&text, spoken, symbol);
        }
    }
    text = compact_symbol_spacing(&text);

    let words = text
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_'))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    let has_non_word_symbols = text
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch.is_whitespace()));
    if words.len() > 1 && words.len() <= 8 && !has_non_word_symbols {
        text = match settings.preferred_casing {
            CodeCasingStyle::Preserve => text,
            CodeCasingStyle::CamelCase => to_camel_case(&words),
            CodeCasingStyle::SnakeCase => words
                .iter()
                .map(|token| token.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("_"),
            CodeCasingStyle::PascalCase => words
                .iter()
                .map(|token| capitalize_first_alpha(&token.to_ascii_lowercase()))
                .collect::<Vec<_>>()
                .join(""),
        };
    }

    if settings.wrap_in_fenced_block {
        format!("```\n{}\n```", text.trim())
    } else {
        text.trim().to_string()
    }
}

fn compact_symbol_spacing(input: &str) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(input.len());
    let mut idx = 0usize;

    while idx < chars.len() {
        let ch = chars[idx];
        if ch == ' ' {
            let prev = out.chars().last();
            let next = chars.get(idx + 1).copied();
            if prev.is_some_and(is_tight_code_symbol) || next.is_some_and(is_tight_code_symbol) {
                idx += 1;
                continue;
            }
            if !out.ends_with(' ') {
                out.push(' ');
            }
            idx += 1;
            continue;
        }

        out.push(ch);
        idx += 1;
    }

    out.trim().to_string()
}

fn is_tight_code_symbol(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '_'
            | ','
            | '.'
            | ':'
            | ';'
            | '='
            | '+'
            | '-'
            | '*'
            | '/'
            | '<'
            | '>'
    )
}

fn to_camel_case(words: &[&str]) -> String {
    let mut out = String::new();
    for (idx, word) in words.iter().enumerate() {
        let lowered = word.to_ascii_lowercase();
        if idx == 0 {
            out.push_str(&lowered);
        } else {
            out.push_str(&capitalize_first_alpha(&lowered));
        }
    }
    out
}

fn replace_boundary_phrase_case_insensitive(
    input: &str,
    needle: &str,
    replacement: &str,
) -> String {
    let source_chars = input.chars().collect::<Vec<_>>();
    let source_lower = input.to_ascii_lowercase();
    let lower_chars = source_lower.chars().collect::<Vec<_>>();
    let needle_lower = needle.to_ascii_lowercase();
    let needle_chars = needle_lower.chars().collect::<Vec<_>>();

    if needle_chars.is_empty() || needle_chars.len() > lower_chars.len() {
        return input.to_string();
    }

    let mut idx = 0usize;
    let mut out = String::with_capacity(input.len());
    while idx < lower_chars.len() {
        let end = idx + needle_chars.len();
        if end <= lower_chars.len() && lower_chars[idx..end] == needle_chars[..] {
            let boundary_before = idx == 0 || !source_chars[idx - 1].is_ascii_alphanumeric();
            let boundary_after =
                end == source_chars.len() || !source_chars[end].is_ascii_alphanumeric();
            if boundary_before && boundary_after {
                out.push_str(replacement);
                idx = end;
                continue;
            }
        }
        out.push(source_chars[idx]);
        idx += 1;
    }

    out
}

fn tokens_match_case_insensitive(left: &[&str], right: &[&str]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .all(|(lhs, rhs)| lhs.eq_ignore_ascii_case(rhs))
}

#[cfg(test)]
mod tests {
    use super::{
        finalize_pro_transcript, finalize_user_transcript, merge_incremental_transcript,
        sanitize_user_transcript, ProTranscriptOptions,
    };
    use crate::settings::{
        AppProfileBehavior, CodeCasingStyle, CodeModeSettings, DomainPackId, FormatProfile,
    };

    #[test]
    fn strips_blank_audio_marker() {
        assert_eq!(
            sanitize_user_transcript("Have you [BLANK_AUDIO] there"),
            "Have you there"
        );
    }

    #[test]
    fn keeps_normal_bracket_text() {
        assert_eq!(
            sanitize_user_transcript("version [v1] works"),
            "version [v1] works"
        );
    }

    #[test]
    fn collapses_whitespace_after_sanitization() {
        assert_eq!(
            sanitize_user_transcript("  hello   [MUSIC]   world  "),
            "hello world"
        );
    }

    #[test]
    fn merge_uses_overlap_to_replace_tail() {
        let merged = merge_incremental_transcript("how is it going", "it going to work now", 8);
        assert_eq!(merged, "how is it going to work now");
    }

    #[test]
    fn merge_appends_when_no_overlap_exists() {
        let merged = merge_incremental_transcript("hello world", "fresh segment", 8);
        assert_eq!(merged, "hello world fresh segment");
    }

    #[test]
    fn merge_is_case_insensitive_for_overlap_matching() {
        let merged = merge_incremental_transcript("VoiceWave Works", "works great", 8);
        assert_eq!(merged, "VoiceWave Works great");
    }

    #[test]
    fn finalize_formats_sentence_grammar() {
        assert_eq!(
            finalize_user_transcript("i think this is working now"),
            "I think this is working now."
        );
    }

    #[test]
    fn finalize_formats_spoken_numbered_lists() {
        let output = finalize_user_transcript(
            "one fix microphone settings two check model install three run dictation test",
        );
        assert_eq!(
            output,
            "1. Fix microphone settings.\n2. Check model install.\n3. Run dictation test."
        );
    }

    #[test]
    fn finalize_formats_numbered_lists_with_intro_and_single_word_items() {
        let output = finalize_user_transcript("there are two process one hi two real");
        assert_eq!(output, "1. Hi.\n2. Real.");
    }

    #[test]
    fn finalize_formats_numbered_lists_with_intro_counts_and_commas() {
        let output = finalize_user_transcript(
            "there are only two ways that you can do better in your life. one, hard work, two, determination, three, failing",
        );
        assert_eq!(
            output,
            "There are only two ways that you can do better in your life.\n1. Hard work.\n2. Determination.\n3. Failing."
        );
    }

    #[test]
    fn finalize_formats_numbered_lists_with_number_prefix_markers() {
        let output = finalize_user_transcript(
            "here are priorities number one fix auth number two add tests number three ship patch",
        );
        assert_eq!(output, "1. Fix auth.\n2. Add tests.\n3. Ship patch.");
    }

    #[test]
    fn finalize_preserves_intro_sentence_before_list_markers() {
        let output = finalize_user_transcript(
            "there are only two ways that you can make your life better. one hard work two determination",
        );
        assert_eq!(
            output,
            "There are only two ways that you can make your life better.\n1. Hard work.\n2. Determination."
        );
    }

    #[test]
    fn finalize_avoids_false_numbered_list_conversion() {
        assert_eq!(
            finalize_user_transcript("one day two nights in paris"),
            "One day two nights in paris."
        );
    }

    fn cleanup(input: &str) -> String {
        let code_mode = CodeModeSettings::default();
        let behavior = AppProfileBehavior::default();
        finalize_pro_transcript(
            input,
            &ProTranscriptOptions {
                format_profile: FormatProfile::Default,
                domain_packs: &[],
                code_mode: &code_mode,
                post_processing_enabled: true,
                spoken_edit_commands: true,
                app_profile_behavior: &behavior,
                custom_terms: &[],
            },
        )
    }

    #[test]
    fn cleanup_prunes_leading_filler_and_recapitalizes() {
        assert_eq!(cleanup("um, i think this works now"), "I think this works now.");
    }

    #[test]
    fn cleanup_prunes_filler_variants_mid_sentence() {
        assert_eq!(
            cleanup("send the, umm, report by uhh friday"),
            "Send the, report by friday."
        );
    }

    #[test]
    fn cleanup_collapses_stutters_ignoring_punctuation() {
        assert_eq!(
            cleanup("i, i think the the plan is good"),
            "I think the plan is good."
        );
    }

    #[test]
    fn cleanup_collapses_restart_bigrams() {
        assert_eq!(
            cleanup("we should we should ship this today"),
            "We should ship this today."
        );
    }

    #[test]
    fn cleanup_keeps_all_filler_utterance_verbatim() {
        assert_eq!(cleanup("um"), "Um.");
    }

    #[test]
    fn pro_pipeline_applies_domain_pack_and_code_mode() {
        let options = ProTranscriptOptions {
            format_profile: FormatProfile::Technical,
            domain_packs: &[DomainPackId::Coding],
            code_mode: &CodeModeSettings {
                enabled: true,
                spoken_symbols: true,
                preferred_casing: CodeCasingStyle::SnakeCase,
                wrap_in_fenced_block: false,
            },
            post_processing_enabled: true,
            spoken_edit_commands: true,
            app_profile_behavior: &AppProfileBehavior::default(),
            custom_terms: &[],
        };

        let output = finalize_pro_transcript("api client open paren user id close paren", &options);
        assert!(output.to_ascii_lowercase().contains("api"));
        assert!(output.contains('_') || output.contains('('));
    }

    #[test]
    fn code_mode_snake_case_applies_even_after_sentence_finalize() {
        let options = ProTranscriptOptions {
            format_profile: FormatProfile::Default,
            domain_packs: &[],
            code_mode: &CodeModeSettings {
                enabled: true,
                spoken_symbols: false,
                preferred_casing: CodeCasingStyle::SnakeCase,
                wrap_in_fenced_block: false,
            },
            post_processing_enabled: false,
            spoken_edit_commands: true,
            app_profile_behavior: &AppProfileBehavior::default(),
            custom_terms: &[],
        };

        let output = finalize_pro_transcript("user profile id", &options);
        assert_eq!(output, "user_profile_id");
    }

    #[test]
    fn code_mode_compacts_symbol_spacing() {
        let options = ProTranscriptOptions {
            format_profile: FormatProfile::Default,
            domain_packs: &[],
            code_mode: &CodeModeSettings {
                enabled: true,
                spoken_symbols: true,
                preferred_casing: CodeCasingStyle::Preserve,
                wrap_in_fenced_block: false,
            },
            post_processing_enabled: false,
            spoken_edit_commands: true,
            app_profile_behavior: &AppProfileBehavior::default(),
            custom_terms: &[],
        };

        let output =
            finalize_pro_transcript("open paren user id close paren arrow result", &options);
        assert_eq!(output, "(user id)->result");
    }

    #[test]
    fn code_mode_maps_parenthesis_aliases() {
        let options = ProTranscriptOptions {
            format_profile: FormatProfile::Default,
            domain_packs: &[],
            code_mode: &CodeModeSettings {
                enabled: true,
                spoken_symbols: true,
                preferred_casing: CodeCasingStyle::Preserve,
                wrap_in_fenced_block: false,
            },
            post_processing_enabled: false,
            spoken_edit_commands: true,
            app_profile_behavior: &AppProfileBehavior::default(),
            custom_terms: &[],
        };

        let output =
            finalize_pro_transcript("open parenthesis user id close parenthesis", &options);
        assert_eq!(output, "(user id)");
    }

    #[test]
    fn writing_profile_formats_intro_list_intent() {
        let options = ProTranscriptOptions {
            format_profile: FormatProfile::Academic,
            domain_packs: &[],
            code_mode: &CodeModeSettings::default(),
            post_processing_enabled: true,
            spoken_edit_commands: true,
            app_profile_behavior: &AppProfileBehavior::default(),
            custom_terms: &[],
        };

        let output = finalize_pro_transcript("there are two points one hi two real", &options);
        assert_eq!(output, "1. Hi.\n2. Real.");
    }

    #[test]
    fn study_profile_formats_tagged_note_sections() {
        let options = ProTranscriptOptions {
            format_profile: FormatProfile::Concise,
            domain_packs: &[],
            code_mode: &CodeModeSettings::default(),
            post_processing_enabled: true,
            spoken_edit_commands: true,
            app_profile_behavior: &AppProfileBehavior::default(),
            custom_terms: &[],
        };

        let output = finalize_pro_transcript(
            "topic momentum definition rate of change of velocity example pushing a cart summary revise this before exam",
            &options,
        );
        assert_eq!(
            output,
            "Topic: Momentum.\nDefinition: Rate of change of velocity.\nExample: Pushing a cart.\nSummary: Revise this before exam."
        );
    }

    fn structural_options<'a>(
        spoken_edit_commands: bool,
        code_mode: &'a CodeModeSettings,
        behavior: &'a AppProfileBehavior,
    ) -> ProTranscriptOptions<'a> {
        ProTranscriptOptions {
            format_profile: FormatProfile::Default,
            domain_packs: &[],
            code_mode,
            post_processing_enabled: true,
            spoken_edit_commands,
            app_profile_behavior: behavior,
            custom_terms: &[],
        }
    }

    #[test]
    fn structural_commands_apply_in_default_profile() {
        let code_mode = CodeModeSettings::default();
        let behavior = AppProfileBehavior::default();
        let options = structural_options(true, &code_mode, &behavior);

        let output = finalize_pro_transcript("check the logs new line restart the app", &options);
        assert_eq!(output, "Check the logs\nrestart the app.");
    }

    #[test]
    fn new_paragraph_maps_to_double_newline() {
        let code_mode = CodeModeSettings::default();
        let behavior = AppProfileBehavior::default();
        let options = structural_options(true, &code_mode, &behavior);

        let output = finalize_pro_transcript("intro new paragraph details", &options);
        assert_eq!(output, "Intro\n\ndetails.");
    }

    #[test]
    fn bullet_point_starts_new_bulleted_line() {
        let code_mode = CodeModeSettings::default();
        let behavior = AppProfileBehavior::default();
        let options = structural_options(true, &code_mode, &behavior);

        let output = finalize_pro_transcript("buy milk bullet point buy eggs", &options);
        assert_eq!(output, "Buy milk\n- buy eggs.");
    }

    #[test]
    fn structural_commands_disabled_leaves_text_literal() {
        let code_mode = CodeModeSettings::default();
        let behavior = AppProfileBehavior::default();
        let options = structural_options(false, &code_mode, &behavior);

        let output = finalize_pro_transcript("buy milk bullet point buy eggs", &options);
        assert_eq!(output, "Buy milk bullet point buy eggs.");
        assert!(!output.contains('\n'));
    }

    #[test]
    fn literal_keeps_fillers_and_stutters_verbatim() {
        use super::finalize_literal_transcript;
        // No filler pruning, no stutter collapse, no domain corrections —
        // just capitalization + terminal punctuation.
        assert_eq!(
            finalize_literal_transcript("um, i i think the the api works now", true, &[]),
            // (Pronoun "i" capitalization is part of the always-on
            // punctuation/capitalization tier, so both "i"s become "I".)
            "Um, I I think the the api works now."
        );
    }

    #[test]
    fn literal_applies_structural_commands_and_dictionary() {
        use super::finalize_literal_transcript;
        let custom_terms = vec!["VoiceWave".to_string()];
        assert_eq!(
            finalize_literal_transcript(
                "voicewave is ready new line ship it",
                true,
                &custom_terms
            ),
            "VoiceWave is ready\nship it."
        );
        // Structural commands off -> spoken phrase stays literal words.
        assert_eq!(
            finalize_literal_transcript("first line new line second line", false, &[]),
            "First line new line second line."
        );
    }

    #[test]
    fn literal_does_not_format_spoken_numbered_lists() {
        use super::finalize_literal_transcript;
        let output = finalize_literal_transcript(
            "one fix microphone settings two check model install three run dictation test",
            true,
            &[],
        );
        assert!(!output.contains('\n'));
        assert!(output.starts_with("One fix microphone settings"));
    }

    #[test]
    fn profile_mappings_not_double_applied() {
        let options = ProTranscriptOptions {
            format_profile: FormatProfile::Academic,
            domain_packs: &[],
            code_mode: &CodeModeSettings::default(),
            post_processing_enabled: true,
            spoken_edit_commands: true,
            app_profile_behavior: &AppProfileBehavior::default(),
            custom_terms: &[],
        };

        let output = finalize_pro_transcript("intro new paragraph details", &options);
        assert_eq!(output, "Intro\n\ndetails.");
        assert!(!output.contains("\n\n\n"));
    }
}
