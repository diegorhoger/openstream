//! Typed keyboard shortcut configuration, validated fail closed.
//!
//! A shortcut is a bounded sequence of 1..=[`MAX_SHORTCUT_CHORDS`] chords;
//! each chord is 1..=[`MAX_CHORD_TOKENS`] `+`-separated tokens: zero or more
//! distinct modifiers plus exactly one main key from a closed vocabulary.
//!
//! Validation rules (all rejections structural; rejected input never enters
//! an error value):
//!
//! - Parameters are a JSON object carrying exactly one `keys` field whose
//!   value is one chord string or an array of chord strings.
//! - Tokens are ASCII lowercase alphanumeric only — no whitespace, control
//!   bytes, wildcards, or punctuation — bounded by [`MAX_KEY_TOKEN_BYTES`].
//! - Every token must be in the closed vocabulary (modifiers `ctrl`, `alt`,
//!   `shift`, `meta`; letters `a`–`z`; digits `0`–`9`; function keys
//!   `f1`–`f24`; the named keys listed in [`KeyValue`]). Unknown tokens
//!   reject.
//! - Modifiers may not repeat inside a chord and a chord without exactly
//!   one main key rejects.
//!
//! The same validator runs at authoring time ([`ShortcutSpec`] typed
//! constructors), at graph-registration pre-validation
//! ([`parse_shortcut_params`]), and again per dispatch inside the port, so
//! no untyped path can reach synthesis.

use core::fmt;
use serde_json::Value;

/// Maximum number of chords in one shortcut sequence.
pub const MAX_SHORTCUT_CHORDS: usize = 4;

/// Maximum token count of one chord (distinct modifiers + one main key).
pub const MAX_CHORD_TOKENS: usize = 4;

/// Maximum byte length of one key token (`backspace` is the longest).
pub const MAX_KEY_TOKEN_BYTES: usize = 12;

/// Chord modifier. At most one of each kind per chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    /// Control.
    Ctrl,
    /// Alt / Option.
    Alt,
    /// Shift.
    Shift,
    /// Meta / Win / Command.
    Meta,
}

impl Modifier {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
            Self::Meta => "meta",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "ctrl" => Some(Self::Ctrl),
            "alt" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "meta" => Some(Self::Meta),
            _ => None,
        }
    }

    /// Fixed canonical press order: ctrl, alt, shift, meta.
    const CANONICAL: [Self; 4] = [Self::Ctrl, Self::Alt, Self::Shift, Self::Meta];
}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Main (non-modifier) key of a chord. Closed v1 vocabulary; additions are
/// additive minors of this schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyValue {
    /// Letter or digit key (`a`–`z`, `0`–`9`).
    Char(char),
    /// Function key `F1`–`F24`.
    Function(u8),
    /// Space bar.
    Space,
    /// Enter / Return.
    Enter,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
}

impl KeyValue {
    /// Canonical lowercase token.
    #[must_use]
    pub fn token(self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Function(n) => format!("f{n}"),
            Self::Space => "space".to_string(),
            Self::Enter => "enter".to_string(),
            Self::Tab => "tab".to_string(),
            Self::Escape => "escape".to_string(),
            Self::Backspace => "backspace".to_string(),
            Self::Delete => "delete".to_string(),
            Self::Insert => "insert".to_string(),
            Self::Home => "home".to_string(),
            Self::End => "end".to_string(),
            Self::PageUp => "pageup".to_string(),
            Self::PageDown => "pagedown".to_string(),
            Self::Left => "left".to_string(),
            Self::Right => "right".to_string(),
            Self::Up => "up".to_string(),
            Self::Down => "down".to_string(),
        }
    }

    fn parse_named(token: &str) -> Option<Self> {
        match token {
            "space" => Some(Self::Space),
            "enter" | "return" => Some(Self::Enter),
            "tab" => Some(Self::Tab),
            "escape" | "esc" => Some(Self::Escape),
            "backspace" => Some(Self::Backspace),
            "delete" | "del" => Some(Self::Delete),
            "insert" => Some(Self::Insert),
            "home" => Some(Self::Home),
            "end" => Some(Self::End),
            "pageup" => Some(Self::PageUp),
            "pagedown" => Some(Self::PageDown),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }
}

impl fmt::Display for KeyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.token())
    }
}

/// One chord: zero or more distinct modifiers plus exactly one main key.
/// Modifier order is normalized to the fixed canonical order so equal sets
/// compare equal regardless of authored order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chord {
    modifiers: Vec<Modifier>,
    key: KeyValue,
}

impl Chord {
    /// Builds a validated chord from modifiers (deduplicated into canonical
    /// order) and one main key.
    ///
    /// # Errors
    /// [`ShortcutConfigError::DuplicateModifier`] on repeated modifiers;
    /// [`ShortcutConfigError::ChordTokenCountExceeded`] above
    /// [`MAX_CHORD_TOKENS`] tokens total.
    pub fn try_new(modifiers: &[Modifier], key: KeyValue) -> Result<Self, ShortcutConfigError> {
        if modifiers.len() + 1 > MAX_CHORD_TOKENS {
            return Err(ShortcutConfigError::ChordTokenCountExceeded);
        }
        let mut ordered = Vec::with_capacity(modifiers.len());
        for candidate in Modifier::CANONICAL {
            if modifiers.contains(&candidate) {
                ordered.push(candidate);
            }
        }
        if ordered.len() != modifiers.len() {
            return Err(ShortcutConfigError::DuplicateModifier);
        }
        Ok(Self {
            modifiers: ordered,
            key,
        })
    }

    /// Distinct modifiers in canonical press order.
    #[must_use]
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    /// The main key.
    #[must_use]
    pub const fn key(&self) -> KeyValue {
        self.key
    }

    fn parse(raw: &str) -> Result<Self, ShortcutConfigError> {
        let mut raw_tokens = raw.split('+');
        // split always yields at least one element; an empty input yields
        // one empty token, which fails below as TokenEmpty.
        let mut seen_modifiers: Vec<Modifier> = Vec::new();
        let mut main_key: Option<KeyValue> = None;
        let mut token_count = 0usize;
        for token in raw_tokens.by_ref() {
            token_count += 1;
            if token_count > MAX_CHORD_TOKENS {
                return Err(ShortcutConfigError::ChordTokenCountExceeded);
            }
            if token.is_empty() {
                return Err(ShortcutConfigError::TokenEmpty);
            }
            if token.len() > MAX_KEY_TOKEN_BYTES
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            {
                return Err(ShortcutConfigError::TokenInvalidChar);
            }
            if let Some(modifier) = Modifier::parse(token) {
                if seen_modifiers.contains(&modifier) {
                    return Err(ShortcutConfigError::DuplicateModifier);
                }
                seen_modifiers.push(modifier);
                continue;
            }
            if main_key.is_some() {
                return Err(ShortcutConfigError::MultipleMainKeys);
            }
            main_key = Some(parse_main_key(token)?);
        }
        let Some(key) = main_key else {
            return Err(ShortcutConfigError::ModifierOnlyChord);
        };
        Ok(Self {
            modifiers: normalize(&seen_modifiers),
            key,
        })
    }
}

fn normalize(seen: &[Modifier]) -> Vec<Modifier> {
    Modifier::CANONICAL
        .into_iter()
        .filter(|candidate| seen.contains(candidate))
        .collect()
}

fn parse_main_key(token: &str) -> Result<KeyValue, ShortcutConfigError> {
    if let Some(named) = KeyValue::parse_named(token) {
        return Ok(named);
    }
    let body = token.strip_prefix('f').unwrap_or("");
    if !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) {
        let number: u32 = body
            .parse()
            .map_err(|_| ShortcutConfigError::UnknownToken)?;
        if (1..=24).contains(&number) {
            return u8::try_from(number)
                .map(KeyValue::Function)
                .map_err(|_| ShortcutConfigError::UnknownToken);
        }
        return Err(ShortcutConfigError::UnknownToken);
    }
    let mut chars = token.chars();
    if let (Some(first), None) = (chars.next(), chars.next())
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
    {
        return Ok(KeyValue::Char(first));
    }
    Err(ShortcutConfigError::UnknownToken)
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            f.write_str(modifier.as_str())?;
            f.write_str("+")?;
        }
        f.write_str(&self.key.token())
    }
}

/// Validated shortcut configuration: a bounded, non-empty chord sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShortcutSpec {
    chords: Vec<Chord>,
}

impl ShortcutSpec {
    /// Builds a validated spec from already-typed chords.
    ///
    /// # Errors
    /// [`ShortcutConfigError::SequenceEmpty`] for empty input;
    /// [`ShortcutConfigError::ChordCountExceeded`] above
    /// [`MAX_SHORTCUT_CHORDS`].
    pub fn try_new(chords: Vec<Chord>) -> Result<Self, ShortcutConfigError> {
        if chords.is_empty() {
            return Err(ShortcutConfigError::SequenceEmpty);
        }
        if chords.len() > MAX_SHORTCUT_CHORDS {
            return Err(ShortcutConfigError::ChordCountExceeded);
        }
        Ok(Self { chords })
    }

    /// Single-chord convenience constructor.
    ///
    /// # Errors
    /// [`Chord::try_new`] / [`Self::try_new`] failures.
    pub fn single(modifiers: &[Modifier], key: KeyValue) -> Result<Self, ShortcutConfigError> {
        Self::try_new(vec![Chord::try_new(modifiers, key)?])
    }

    /// Chords in authored order.
    #[must_use]
    pub fn chords(&self) -> &[Chord] {
        &self.chords
    }

    /// Parses action-graph parameters into a validated spec. This is the
    /// exact validation the port applies at dispatch; hosts may also call it
    /// earlier when authoring or registering bindings.
    ///
    /// # Errors
    /// Any [`ShortcutConfigError`] variant; see the module docs.
    pub fn from_params(params: &Value) -> Result<Self, ShortcutConfigError> {
        parse_shortcut_params(params)
    }
}

impl fmt::Display for ShortcutSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, chord) in self.chords.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{chord}")?;
        }
        Ok(())
    }
}

/// Validates action parameters against the declared config schema.
///
/// Accepted shapes:
///
/// ```json
/// {"keys": "ctrl+shift+t"}
/// {"keys": ["ctrl+k", "s"]}
/// ```
///
/// Anything else — wrong JSON type, extra fields, off-vocabulary tokens,
/// unbounded sequences — rejects fail closed with a structural reason.
///
/// # Errors
/// [`ShortcutConfigError`] variants; rejected input never enters errors.
pub fn parse_shortcut_params(params: &Value) -> Result<ShortcutSpec, ShortcutConfigError> {
    let Value::Object(fields) = params else {
        return Err(ShortcutConfigError::NotAnObject);
    };
    if fields.len() != 1 || !fields.contains_key("keys") {
        return Err(ShortcutConfigError::UnexpectedField);
    }
    let keys = fields
        .get("keys")
        .unwrap_or_else(|| unreachable!("checked above"));
    let chords_raw: Vec<&str> = match keys {
        Value::String(single) => vec![single.as_str()],
        Value::Array(items) => {
            let mut collected = Vec::with_capacity(items.len());
            for item in items {
                let Value::String(chord) = item else {
                    return Err(ShortcutConfigError::KeysWrongType);
                };
                collected.push(chord.as_str());
            }
            collected
        }
        _ => return Err(ShortcutConfigError::KeysWrongType),
    };
    if chords_raw.is_empty() {
        return Err(ShortcutConfigError::SequenceEmpty);
    }
    if chords_raw.len() > MAX_SHORTCUT_CHORDS {
        return Err(ShortcutConfigError::ChordCountExceeded);
    }
    let mut chords = Vec::with_capacity(chords_raw.len());
    for raw in chords_raw {
        chords.push(Chord::parse(raw)?);
    }
    Ok(ShortcutSpec { chords })
}

/// Typed configuration failures. Structural reasons only: rejected input
/// values never appear in any variant (redaction rules, TM-LOG-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutConfigError {
    /// Parameters were not a JSON object.
    NotAnObject,
    /// The object carried fields other than the single declared `keys`
    /// field, or missed it entirely.
    UnexpectedField,
    /// `keys` was neither a string nor an array of strings.
    KeysWrongType,
    /// An array-valued `keys` carried zero chords.
    SequenceEmpty,
    /// More than [`MAX_SHORTCUT_CHORDS`] chords were requested.
    ChordCountExceeded,
    /// A chord carried more than [`MAX_CHORD_TOKENS`] `+`-separated tokens.
    ChordTokenCountExceeded,
    /// An empty `+`-separated token was found.
    TokenEmpty,
    /// A token contained characters outside `[a-z0-9]` or exceeded
    /// [`MAX_KEY_TOKEN_BYTES`].
    TokenInvalidChar,
    /// A well-formed token is outside the closed vocabulary.
    UnknownToken,
    /// The same modifier appeared twice in one chord.
    DuplicateModifier,
    /// A chord carried more than one main key (e.g. two letters).
    MultipleMainKeys,
    /// A chord carried modifiers but no main key.
    ModifierOnlyChord,
}

impl fmt::Display for ShortcutConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAnObject => "shortcut params must be a JSON object",
            Self::UnexpectedField => "shortcut params must carry exactly the 'keys' field",
            Self::KeysWrongType => "'keys' must be a string or an array of strings",
            Self::SequenceEmpty => "'keys' must carry at least one chord",
            Self::ChordCountExceeded => "too many chords in shortcut sequence",
            Self::ChordTokenCountExceeded => "too many tokens in one chord",
            Self::TokenEmpty => "empty token in chord",
            Self::TokenInvalidChar => "token contains characters outside [a-z0-9]",
            Self::UnknownToken => "token is outside the closed key vocabulary",
            Self::DuplicateModifier => "modifier repeated within one chord",
            Self::MultipleMainKeys => "chord carries more than one main key",
            Self::ModifierOnlyChord => "chord carries no main key",
        })
    }
}

impl std::error::Error for ShortcutConfigError {}

#[cfg(test)]
mod tests {
    use super::{
        Chord, KeyValue, MAX_KEY_TOKEN_BYTES, MAX_SHORTCUT_CHORDS, Modifier, ShortcutConfigError,
        ShortcutSpec, parse_shortcut_params,
    };
    use serde_json::{Value, json};

    fn chord(spec: &str) -> Chord {
        let value = json!({ "keys": spec });
        let parsed = parse_shortcut_params(&value).unwrap_or_else(|e| panic!("{spec}: {e}"));
        parsed.chords()[0].clone()
    }

    #[test]
    fn single_chord_parses_with_canonical_modifier_order() {
        let parsed = chord("shift+ctrl+t");
        assert_eq!(parsed.modifiers(), [Modifier::Ctrl, Modifier::Shift]);
        assert_eq!(parsed.key(), KeyValue::Char('t'));
        assert_eq!(parsed.to_string(), "ctrl+shift+t");
    }

    #[test]
    fn vocabulary_covers_declared_closed_set() {
        assert_eq!(chord("f12").key(), KeyValue::Function(12));
        assert_eq!(chord("f24").key(), KeyValue::Function(24));
        assert_eq!(chord("pagedown").key(), KeyValue::PageDown);
        assert_eq!(chord("return").key(), KeyValue::Enter);
        assert_eq!(chord("esc").key(), KeyValue::Escape);
        assert_eq!(chord("del").key(), KeyValue::Delete);
        assert_eq!(chord("7").key(), KeyValue::Char('7'));
        assert_eq!(chord("meta+home").key(), KeyValue::Home);
    }

    #[test]
    fn multi_chord_sequence_parses_in_order() {
        let spec = ShortcutSpec::from_params(&json!({ "keys": ["ctrl+k", "s"] })).unwrap();
        assert_eq!(
            spec.chords(),
            [
                Chord::try_new(&[Modifier::Ctrl], KeyValue::Char('k')).unwrap(),
                Chord::try_new(&[], KeyValue::Char('s')).unwrap(),
            ]
        );
        assert_eq!(spec.chords().len(), 2);
    }

    #[test]
    fn limits_hold_exactly_at_boundaries() {
        let max_chord = "ctrl+alt+shift+f5";
        let parsed = parse_shortcut_params(&json!({ "keys": max_chord }))
            .expect("a 4-token chord sits exactly at the limit");
        assert_eq!(parsed.chords()[0].modifiers().len(), 3);
        assert_eq!(parsed.chords()[0].key(), KeyValue::Function(5));

        let over_tokens = "ctrl+alt+shift+meta+a";
        assert_eq!(
            parse_shortcut_params(&json!({ "keys": over_tokens })).unwrap_err(),
            ShortcutConfigError::ChordTokenCountExceeded
        );
        assert_eq!(
            parse_shortcut_params(&json!({ "keys": "ctrl+alt+shift+meta" })).unwrap_err(),
            ShortcutConfigError::ModifierOnlyChord
        );

        let mut chords_at_limit = vec!["ctrl+k"; MAX_SHORTCUT_CHORDS - 1];
        chords_at_limit.push("s");
        let ok = parse_shortcut_params(&json!({ "keys": chords_at_limit }))
            .expect("boundary sequence parses");
        assert_eq!(ok.chords().len(), MAX_SHORTCUT_CHORDS);

        let over_chords: Vec<String> =
            std::iter::repeat_n("ctrl+a".to_string(), MAX_SHORTCUT_CHORDS + 1).collect();
        let over = json!({ "keys": over_chords });
        assert_eq!(
            parse_shortcut_params(&over).unwrap_err(),
            ShortcutConfigError::ChordCountExceeded
        );
    }

    #[test]
    fn malformed_inputs_reject_with_typed_reasons() {
        let cases: Vec<(Value, ShortcutConfigError)> = vec![
            (json!("ctrl+a"), ShortcutConfigError::NotAnObject),
            (json!([]), ShortcutConfigError::NotAnObject),
            (json!(null), ShortcutConfigError::NotAnObject),
            (json!({}), ShortcutConfigError::UnexpectedField),
            (
                json!({ "keys": "a", "extra": 1 }),
                ShortcutConfigError::UnexpectedField,
            ),
            (json!({ "keyz": "a" }), ShortcutConfigError::UnexpectedField),
            (json!({ "keys": 5 }), ShortcutConfigError::KeysWrongType),
            (
                json!({ "keys": ["ctrl+a", 5] }),
                ShortcutConfigError::KeysWrongType,
            ),
            (json!({ "keys": [] }), ShortcutConfigError::SequenceEmpty),
            (json!({ "keys": "" }), ShortcutConfigError::TokenEmpty),
            (json!({ "keys": "ctrl+" }), ShortcutConfigError::TokenEmpty),
            (
                json!({ "keys": "+ctrl+a" }),
                ShortcutConfigError::TokenEmpty,
            ),
            (
                json!({ "keys": "Ctrl+A" }),
                ShortcutConfigError::TokenInvalidChar,
            ),
            (
                json!({ "keys": "ctrl alt" }),
                ShortcutConfigError::TokenInvalidChar,
            ),
            (
                json!({ "keys": "ctrl+shift+t;" }),
                ShortcutConfigError::TokenInvalidChar,
            ),
            (
                json!({ "keys": &format!("ctrl+{}", "a".repeat(MAX_KEY_TOKEN_BYTES + 1)) }),
                ShortcutConfigError::TokenInvalidChar,
            ),
            (
                json!({ "keys": &format!("ctrl+{}", "a".repeat(MAX_KEY_TOKEN_BYTES)) }),
                ShortcutConfigError::UnknownToken,
            ),
            (
                json!({ "keys": "ctrl+drop+table" }),
                ShortcutConfigError::UnknownToken,
            ),
            (json!({ "keys": "f25" }), ShortcutConfigError::UnknownToken),
            (json!({ "keys": "f0" }), ShortcutConfigError::UnknownToken),
            (
                json!({ "keys": "ctrl+ctrl+a" }),
                ShortcutConfigError::DuplicateModifier,
            ),
            (
                json!({ "keys": "ctrl+a+b" }),
                ShortcutConfigError::MultipleMainKeys,
            ),
            (
                json!({ "keys": "ctrl+shift" }),
                ShortcutConfigError::ModifierOnlyChord,
            ),
        ];
        for (params, expected) in cases {
            assert_eq!(
                parse_shortcut_params(&params).unwrap_err(),
                expected,
                "case {params}"
            );
        }
    }

    #[test]
    fn regression_sweep_never_panics_and_always_rejects_garbage() {
        let alphabet = [
            "",
            "+",
            "++",
            "a+",
            "+a",
            "CTRL",
            "\u{0}",
            "\n",
            " ",
            "a b",
            "ü",
            "🔥",
            "a+b+c+d+e",
            "f00",
            "f25",
            "ff",
            "0x41",
            "ctrl+alt+del+extra+more",
            "meta+meta+m",
            "..",
            "-a",
        ];
        for raw in alphabet {
            let params = json!({ "keys": raw });
            assert!(
                parse_shortcut_params(&params).is_err(),
                "garbage must reject: {raw:?}"
            );
        }
    }

    #[test]
    fn typed_constructors_validate_like_the_parser() {
        assert_eq!(
            ShortcutSpec::try_new(Vec::new()).unwrap_err(),
            ShortcutConfigError::SequenceEmpty
        );
        assert_eq!(
            Chord::try_new(&[Modifier::Alt, Modifier::Alt], KeyValue::Char('x')).unwrap_err(),
            ShortcutConfigError::DuplicateModifier
        );
        assert_eq!(
            ShortcutSpec::single(&[Modifier::Ctrl], KeyValue::Char('c')).unwrap(),
            parse_shortcut_params(&json!({ "keys": "ctrl+c" })).unwrap()
        );
    }
}
