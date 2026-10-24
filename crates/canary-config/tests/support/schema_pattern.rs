//! The `pattern` subset shared by the configuration schema test suites.
//!
//! `docs/schemas/config-v1.schema.json` constrains the shape of three string
//! settings with the `pattern` keyword. JSON Schema patterns are RFC-regexps,
//! and this crate has no regular-expression dependency (the lockfile digest
//! check in `crate::loader` is written the same way), so the schema is
//! deliberately written in the narrow subset implemented here: anchored
//! literals, character classes with ranges and `^` negation, and the `+ * ?`
//! repetitions. [`collect_patterns`] plus the two tests that use it fail if a
//! schema pattern steps outside that subset, so the published file can never
//! quietly become something these suites no longer interpret — and never needs
//! a syntax a small validator cannot run.

use serde_json::Value;

/// One atom of a pattern: the characters it accepts and how often they
/// repeat. `negated` inverts the set, which is how `[^ ]` is represented.
struct Piece {
    set: Vec<(char, char)>,
    negated: bool,
    min: usize,
    max: Option<usize>,
}

impl Piece {
    fn holds(&self, c: char) -> bool {
        self.set.iter().any(|&(lo, hi)| c >= lo && c <= hi) != self.negated
    }

    fn of(c: char) -> Piece {
        Piece {
            set: vec![(c, c)],
            negated: false,
            min: 1,
            max: None,
        }
    }
}

/// A parsed pattern: its atoms, and whether it is anchored to the start
/// and/or the end of the value it constrains.
pub struct Pattern {
    pieces: Vec<Piece>,
    anchored_start: bool,
    anchored_end: bool,
}

impl Pattern {
    /// Whether the pattern is anchored at both ends, which is what makes it
    /// constrain a whole value rather than a substring of it.
    pub fn is_anchored(&self) -> bool {
        self.anchored_start && self.anchored_end
    }
}

/// Parses a pattern in the documented subset, naming the construct it cannot
/// interpret otherwise.
pub fn parse(text: &str) -> Result<Pattern, String> {
    let input: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut anchored_start = false;
    let mut anchored_end = false;
    let mut pieces = Vec::new();

    if input.first() == Some(&'^') {
        anchored_start = true;
        i += 1;
    }
    while i < input.len() {
        let c = input[i];
        if c == '$' && i + 1 == input.len() {
            anchored_end = true;
            break;
        }
        let mut piece = match c {
            '[' => {
                i += 1;
                parse_class(text, &input, &mut i)?
            }
            '\\' => {
                i += 1;
                let escaped = *input
                    .get(i)
                    .ok_or_else(|| format!("{text}: the pattern ends in an escape"))?;
                i += 1;
                Piece::of(escaped)
            }
            '.' | '(' | ')' | '|' | '{' | '}' | '^' | '$' | '+' | '*' | '?' => {
                return Err(format!(
                    "{text}: `{c}` is outside the pattern subset these suites implement"
                ));
            }
            other => {
                i += 1;
                Piece::of(other)
            }
        };
        match input.get(i) {
            Some('+') => i += 1,
            Some('*') => {
                piece.min = 0;
                i += 1;
            }
            Some('?') => {
                piece.min = 0;
                piece.max = Some(1);
                i += 1;
            }
            _ => {}
        }
        pieces.push(piece);
    }

    Ok(Pattern {
        pieces,
        anchored_start,
        anchored_end,
    })
}

/// Reads one `[...]` class, `i` having already moved past the opening `[`.
fn parse_class(text: &str, input: &[char], i: &mut usize) -> Result<Piece, String> {
    let mut negated = false;
    if input.get(*i) == Some(&'^') {
        negated = true;
        *i += 1;
    }
    let mut set = Vec::new();
    loop {
        let Some(&c) = input.get(*i) else {
            return Err(format!("{text}: an unterminated character class"));
        };
        // A `]` terminates the class once it holds something, so `[]...]` can
        // still name a literal bracket.
        if c == ']' && !set.is_empty() {
            *i += 1;
            break;
        }
        let low = read_class_char(text, input, i)?;
        // `-` between two characters is a range; at either end it is literal.
        if input.get(*i) == Some(&'-')
            && input.get(*i + 1).is_some()
            && input.get(*i + 1) != Some(&']')
        {
            *i += 1;
            let high = read_class_char(text, input, i)?;
            if high < low {
                return Err(format!("{text}: an inverted range in a character class"));
            }
            set.push((low, high));
        } else {
            set.push((low, low));
        }
    }
    if set.is_empty() {
        return Err(format!("{text}: an empty character class"));
    }
    Ok(Piece {
        set,
        negated,
        min: 1,
        max: None,
    })
}

/// One class member, honouring `\` escapes for `]`, `-`, `^`, `\` and friends.
fn read_class_char(text: &str, input: &[char], i: &mut usize) -> Result<char, String> {
    let c = *input
        .get(*i)
        .ok_or_else(|| format!("{text}: an unterminated character class"))?;
    *i += 1;
    if c == '\\' {
        let escaped = *input
            .get(*i)
            .ok_or_else(|| format!("{text}: the pattern ends in an escape"))?;
        *i += 1;
        return Ok(escaped);
    }
    Ok(c)
}

/// Whether `value` matches the pattern `text`, reporting a failure to parse
/// `text` itself rather than pretending it matched nothing.
pub fn matches(text: &str, value: &str) -> Result<bool, String> {
    let pattern = parse(text)?;
    let input: Vec<char> = value.chars().collect();
    let firsts: Vec<usize> = if pattern.anchored_start {
        vec![0]
    } else {
        (0..=input.len()).collect()
    };
    Ok(firsts
        .into_iter()
        .any(|at| match_here(&pattern.pieces, 0, &input, at, pattern.anchored_end)))
}

/// Matches `pieces[pi..]` against `input[ci..]`, trying the longest repetition
/// first so the usual greedy patterns settle quickly.
fn match_here(pieces: &[Piece], pi: usize, input: &[char], ci: usize, to_end: bool) -> bool {
    let Some(piece) = pieces.get(pi) else {
        return !to_end || ci == input.len();
    };

    let mut pos = ci;
    for _ in 0..piece.min {
        if input.get(pos).is_some_and(|c| piece.holds(*c)) {
            pos += 1;
        } else {
            return false;
        }
    }
    let mut stops = vec![pos];
    let mut count = piece.min;
    while piece.max.is_none() || count < piece.max.unwrap_or(0) {
        match input.get(pos) {
            Some(c) if piece.holds(*c) => {
                pos += 1;
                count += 1;
                stops.push(pos);
            }
            _ => break,
        }
    }
    stops
        .iter()
        .rev()
        .any(|&at| match_here(pieces, pi + 1, input, at, to_end))
}

/// Every `pattern` a schema document declares, in document order, walking only
/// the positions that hold subschemas: the values of `properties`, `not`,
/// `items`, each `anyOf` branch, and a schema-valued `additionalProperties`.
/// Key names, enums, required lists and pattern *strings* are data.
pub fn collect_patterns(node: &Value, into: &mut Vec<String>) {
    let Some(map) = node.as_object() else {
        return;
    };
    for (keyword, value) in map {
        match keyword.as_str() {
            "pattern" => {
                if let Some(text) = value.as_str() {
                    into.push(text.to_string());
                }
            }
            "properties" => {
                if let Some(properties) = value.as_object() {
                    for sub in properties.values() {
                        collect_patterns(sub, into);
                    }
                }
            }
            "not" | "items" => collect_patterns(value, into),
            "anyOf" | "oneOf" | "allOf" => {
                if let Some(branches) = value.as_array() {
                    for branch in branches {
                        collect_patterns(branch, into);
                    }
                }
            }
            "additionalProperties" if value.is_object() => collect_patterns(value, into),
            _ => {}
        }
    }
}
