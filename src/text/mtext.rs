/// One run of MTEXT characters sharing a font and size.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    /// `\f` font override; empty means "use the style's font".
    pub font: String,
    /// Height multiplier from `\H<n>x;`, relative to the entity's height.
    pub height: f64,
    /// Width multiplier from `\W<n>;`.
    pub width: f64,
    /// A hard line break (`\P`) follows this span.
    pub break_after: bool,
}

impl Default for Span {
    fn default() -> Self {
        Self {
            text: String::new(),
            font: String::new(),
            height: 1.0,
            width: 1.0,
            break_after: false,
        }
    }
}

/// The formatting state braces push and pop.
#[derive(Clone)]
struct State {
    font: String,
    height: f64,
    width: f64,
}

/// Deepest `{}` nesting honoured. Real MTEXT nests one or two levels; the
/// cap stops a file full of `{` from growing the stack without bound.
const MAX_GROUPS: usize = 32;

/// Split MTEXT's payload into spans, applying the codes we understand and
/// **deleting** the ones we do not.
///
/// R-TXT-3.3: an unrecognised control code must never reach the page.
/// `\A1;` printed as body text is a worse defect than the text missing —
/// it looks like drawing data.
pub fn parse(raw: &str) -> Vec<Span> {
    let chars: Vec<char> = raw.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut stack: Vec<State> = Vec::new();
    let mut state = State { font: String::new(), height: 1.0, width: 1.0 };
    let mut current = Span::default();
    let mut i = 0usize;

    // Close the current run whenever formatting changes, so each span is
    // uniform.
    macro_rules! flush {
        ($break_after:expr) => {{
            if !current.text.is_empty() || $break_after {
                current.break_after = $break_after;
                spans.push(std::mem::take(&mut current));
            }
            current.font = state.font.clone();
            current.height = state.height;
            current.width = state.width;
        }};
    }

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '{' => {
                if stack.len() < MAX_GROUPS {
                    flush!(false);
                    stack.push(state.clone());
                }
                i += 1;
            }
            '}' => {
                if let Some(previous) = stack.pop() {
                    flush!(false);
                    state = previous;
                    current.font = state.font.clone();
                    current.height = state.height;
                    current.width = state.width;
                }
                i += 1;
            }
            '%' if i + 2 < chars.len() && chars[i + 1] == '%' => {
                let replacement = match chars[i + 2].to_ascii_lowercase() {
                    'd' => Some('\u{00B0}'),
                    'c' => Some('\u{2205}'),
                    'p' => Some('\u{00B1}'),
                    '%' => Some('%'),
                    _ => None,
                };
                match replacement {
                    Some(c) => {
                        current.text.push(c);
                        i += 3;
                    }
                    None => {
                        current.text.push(ch);
                        i += 1;
                    }
                }
            }
            '\\' => {
                let Some(code) = chars.get(i + 1).copied() else {
                    // A trailing backslash is not a code; drop it.
                    break;
                };
                i += 2;
                match code {
                    // Escapes for the literal characters.
                    '\\' | '{' | '}' => current.text.push(code),
                    'P' => flush!(true),
                    // A non-breaking space.
                    '~' => current.text.push('\u{00A0}'),
                    'f' | 'F' => {
                        let arg = take_until_semicolon(&chars, &mut i);
                        // `SimSun|b0|i0|c134|p2` — only the family matters
                        // here; weight, italic, codepage and pitch are
                        // carried by the font file we resolve to.
                        state.font =
                            arg.split('|').next().unwrap_or_default().trim().to_owned();
                        flush!(false);
                    }
                    'H' => {
                        let arg = take_until_semicolon(&chars, &mut i);
                        // `2x` is a multiplier; a bare number is an
                        // absolute height in drawing units, which the
                        // layout stage cannot honour from here, so leave
                        // the multiplier at 1 rather than scaling by it.
                        if let Some(value) = arg.strip_suffix(['x', 'X'])
                            && let Ok(parsed) = value.parse::<f64>()
                            && parsed > 0.0
                        {
                            state.height = parsed;
                        }
                        flush!(false);
                    }
                    'W' => {
                        let arg = take_until_semicolon(&chars, &mut i);
                        if let Ok(parsed) = arg.trim_end_matches(['x', 'X']).parse::<f64>()
                            && parsed > 0.0
                        {
                            state.width = parsed;
                        }
                        flush!(false);
                    }
                    'S' => {
                        // Stacked fraction. PRD 5.6.7 makes proper
                        // typesetting a non-goal; keep the operands
                        // readable and drop the separator.
                        let arg = take_until_semicolon(&chars, &mut i);
                        current.text.push_str(&arg.replace(['^'], "/"));
                    }
                    // Every remaining code takes a `;`-terminated argument
                    // that must be swallowed whole: \A alignment, \C and
                    // \c colour, \Q oblique, \p paragraph, \T tracking,
                    // \L \l \O \o \K \k over/underline toggles.
                    _ => {
                        let _ = take_until_semicolon(&chars, &mut i);
                    }
                }
            }
            _ => {
                current.text.push(ch);
                i += 1;
            }
        }
    }
    if !current.text.is_empty() {
        spans.push(current);
    }
    spans
}

/// Consume up to and including the next `;`, returning what came before.
///
/// Bounded by the input length: an unterminated code swallows the rest of
/// the string rather than reading past it.
fn take_until_semicolon(chars: &[char], i: &mut usize) -> String {
    let mut out = String::new();
    while *i < chars.len() {
        let c = chars[*i];
        *i += 1;
        if c == ';' {
            break;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(raw: &str) -> String {
        parse(raw).iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn text_without_codes_survives_unchanged() {
        assert_eq!(plain("2000"), "2000");
        assert_eq!(plain("图纸说明"), "图纸说明");
    }

    /// Measured in the reference drawing: `\A1;2000` appears 226 times.
    /// Printing the code itself is the failure R-TXT-3.3 names.
    #[test]
    fn the_alignment_code_is_consumed_not_printed() {
        assert_eq!(plain("\\A1;2000"), "2000");
        assert_eq!(plain("\\A1;100"), "100");
        assert!(!plain("\\A1;2000").contains('A'), "the control code leaked into the body");
    }

    /// Also measured verbatim: an inline font override that switches
    /// codepage mid-string.
    #[test]
    fn an_inline_font_override_is_applied_and_removed() {
        let spans = parse("{\\fSimSun|b0|i0|c0|p2;DKM\\fSimSun|b0|i0|c134|p2;1021}");
        let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "DKM1021");
        assert!(spans.iter().any(|s| s.font.eq_ignore_ascii_case("SimSun")), "{spans:?}");
    }

    #[test]
    fn braces_group_without_appearing_in_the_output() {
        assert_eq!(plain("{abc}def"), "abcdef");
    }

    /// A `\H` inside a group must not leak out of it.
    #[test]
    fn formatting_is_restored_when_a_group_closes() {
        let spans = parse("{\\H2x;big}small");
        let small = spans.last().expect("a trailing span");
        assert_eq!(small.text, "small");
        assert!((small.height - 1.0).abs() < 1e-9, "height leaked: {}", small.height);
    }

    #[test]
    fn relative_and_absolute_height_codes_are_read() {
        assert!((parse("\\H2x;a")[0].height - 2.0).abs() < 1e-9);
        // An absolute height has no trailing 'x'; it is relative to the
        // entity height, which the layout stage supplies, so record it as
        // a multiplier of 1 rather than mis-scaling by the raw number.
        assert!((parse("\\H350;a")[0].height - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_width_code_is_read() {
        assert!((parse("\\W0.8;a")[0].width - 0.8).abs() < 1e-9);
    }

    #[test]
    fn a_hard_break_splits_spans() {
        let spans = parse("one\\Ptwo");
        assert_eq!(spans.len(), 2);
        assert!(spans[0].break_after);
        assert_eq!(spans[1].text, "two");
    }

    /// R-TXT-3.3: anything unrecognised must vanish, never print.
    #[test]
    fn unknown_control_codes_are_stripped() {
        assert_eq!(plain("\\Q15;slanted"), "slanted");
        assert_eq!(plain("\\pxi-2,l2;indented"), "indented");
        assert_eq!(plain("\\C1;red"), "red");
    }

    /// `\\` and `\{` are escapes for the literal characters.
    #[test]
    fn escaped_backslashes_and_braces_become_literals() {
        assert_eq!(plain("a\\\\b"), "a\\b");
        assert_eq!(plain("a\\{b"), "a{b");
    }

    /// A stacked fraction is drawn as readable text rather than properly
    /// typeset (PRD 5.6.7 non-goal), but the `\S` and its terminator must
    /// not print.
    #[test]
    fn stacked_fractions_degrade_to_readable_text() {
        let out = plain("\\S1/2;");
        assert!(out.contains('1') && out.contains('2'), "got {out:?}");
        assert!(!out.contains('S'), "the control code leaked: {out:?}");
    }

    /// `%%d` and friends are the older TEXT-era substitutions and appear
    /// in MTEXT too.
    #[test]
    fn percent_substitutions_are_expanded() {
        assert_eq!(plain("45%%d"), "45\u{00B0}");
        assert_eq!(plain("%%c100"), "\u{2205}100");
        assert_eq!(plain("%%p0.5"), "\u{00B1}0.5");
    }

    /// Untrusted input: a code with no terminator must not run off the
    /// end or loop.
    #[test]
    fn an_unterminated_code_is_survivable() {
        let _ = parse("\\f");
        let _ = parse("\\H");
        let _ = parse("{{{{{{");
        let _ = parse("\\");
    }
}
