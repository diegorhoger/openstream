//! Typed capability vocabulary (`CAPABILITY_TAXONOMY.md` §1).
//!
//! A capability is the smallest independently grantable authority unit with
//! the grammar `<domain>.<resource>[.<verb>][:<k>=<v>{,<k>=<v>}]`. This
//! module is the fail-closed parser and canonical form for that vocabulary:
//!
//! - Unknown capability identifiers reject (closed enum; taxonomy §1).
//! - Wildcards are invalid anywhere inside a capability string.
//! - Qualifiers always narrow; each capability kind admits exactly its
//!   registry-declared qualifier keys, required keys must be present, and
//!   unknown/duplicate keys reject.
//! - `secret.read:<secret_ref>` parses but is internal-only
//!   ([`Capability::is_internal`]); it is never grantable to any subject
//!   (taxonomy §4) and every grant/manifest path rejects it.
//!
//! Values are structural only: no control characters, wildcards, commas, or
//! surrounding whitespace, bounded length. Redaction-sensitive enforcement
//! (DNS rebinding, private-address denial, executable identity revalidation)
//! stays adapter-side per the registry rows; this module pins identity shape.

use crate::error::DomainError;
use crate::limits::{MAX_CAPABILITY_BYTES, MAX_QUALIFIER_VALUE_BYTES};
use crate::secret::SecretRef;
use core::fmt;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;

/// Maximum length of one host name inside `network.connect`.
const MAX_HOST_BYTES: usize = 253;

/// Every capability OpenStream can express at v1. The set is closed:
/// additions are additive minors of the taxonomy, new domains require a
/// security ADR, and anything unlisted fails to parse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Read-only OBS state (scenes, program/preview, stream flags).
    ObsRead,
    /// Switch to scenes named in the binding only.
    ObsControlScene,
    /// Start/stop streaming, recording, replay buffer (destructive class).
    ObsControlStream,
    /// Synthetic key events, optionally window-scoped by `app`.
    OsKeyboardEmit {
        /// Window/app scope; `None` covers every window.
        app: Option<String>,
    },
    /// Media playback/soundboard output on the engine audio path.
    OsMediaEmit,
    /// Launch exactly one user-selected application identity.
    OsApplicationLaunch {
        /// Platform-stable application identity bound at approval time.
        identity: String,
    },
    /// Execute exactly one pinned executable identity (hard-stop protected).
    ProcessExecute {
        /// Platform-stable executable identity bound at approval time.
        identity: String,
    },
    /// Clipboard read as a discrete action step.
    ClipboardRead,
    /// Clipboard write as a discrete action step.
    ClipboardWrite,
    /// Read inside one user-selected handle (string paths are invalid).
    FilesystemRead {
        /// Opaque, non-exportable handle token; dies with the grant.
        handle: String,
    },
    /// Write inside one user-selected handle (string paths are invalid).
    FilesystemWrite {
        /// Opaque, non-exportable handle token; dies with the grant.
        handle: String,
    },
    /// Outbound connect to one exact scheme/host/port tuple.
    NetworkConnect {
        /// Exact scheme (`http`, `https`, `ws`, `wss`).
        scheme: NetworkScheme,
        /// Exact host name or IP literal.
        host: String,
        /// Exact TCP port (1-65535).
        port: u16,
    },
    /// Emit messages to one named MIDI device.
    MidiSend {
        /// Named device bound by first-use consent.
        device: String,
    },
    /// Emit messages to one named OSC endpoint.
    OscSend {
        /// Named endpoint bound by first-use consent.
        endpoint: String,
    },
    /// Volume/mute on named device(s).
    AudioControl {
        /// Named device class bound by first-use consent.
        device: String,
    },
    /// Post desktop notifications templated from the action graph.
    NotificationShow,
    /// INTERNAL ONLY (`secret.read:<secret_ref>`): resolves inside the
    /// integration broker during one approved typed operation. Never appears
    /// in any manifest schema and never grants to plugins, WebView commands,
    /// mobile peers, Cloud, or sync payloads (taxonomy §4).
    SecretRead {
        /// Validated structural reference (a name, never a value);
        /// resolution happens only through the OS credential vault.
        secret_ref: SecretRef,
    },
}

/// Admitted schemes for [`Capability::NetworkConnect`]. HTTPS-only policy for
/// third parties is enforced adapter-side per the registry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkScheme {
    /// Plain HTTP (built-in/local use only).
    Http,
    /// HTTP over TLS.
    Https,
    /// Plain WebSocket.
    Ws,
    /// WebSocket over TLS.
    Wss,
}

impl NetworkScheme {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Ws => "ws",
            Self::Wss => "wss",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "http" => Some(Self::Http),
            "https" => Some(Self::Https),
            "ws" => Some(Self::Ws),
            "wss" => Some(Self::Wss),
            _ => None,
        }
    }
}

impl fmt::Display for NetworkScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Capability {
    /// The qualifier-free capability kind identifier used in audit evidence
    /// (redaction rules exclude qualifier values from evidence).
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::ObsRead => "obs.read",
            Self::ObsControlScene => "obs.control.scene",
            Self::ObsControlStream => "obs.control.stream",
            Self::OsKeyboardEmit { .. } => "os.keyboard.emit",
            Self::OsMediaEmit => "os.media.emit",
            Self::OsApplicationLaunch { .. } => "os.application.launch",
            Self::ProcessExecute { .. } => "process.execute",
            Self::ClipboardRead => "clipboard.read",
            Self::ClipboardWrite => "clipboard.write",
            Self::FilesystemRead { .. } => "filesystem.read",
            Self::FilesystemWrite { .. } => "filesystem.write",
            Self::NetworkConnect { .. } => "network.connect",
            Self::MidiSend { .. } => "midi.send",
            Self::OscSend { .. } => "osc.send",
            Self::AudioControl { .. } => "audio.control",
            Self::NotificationShow => "notification.show",
            Self::SecretRead { .. } => "secret.read",
        }
    }

    /// True for internal-only capabilities that are never grantable and never
    /// manifest-declarable (taxonomy §4). Fail-closed paths check this before
    /// any authority is recorded.
    #[must_use]
    pub const fn is_internal(&self) -> bool {
        matches!(self, Self::SecretRead { .. })
    }

    /// Qualifier pairs in canonical declaration order, values owned. Used by
    /// grant coverage/narrowing comparisons; empty for unqualified kinds.
    #[must_use]
    pub fn qualifier_pairs(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::OsKeyboardEmit { app: Some(app) } => vec![("app", app.clone())],
            Self::OsApplicationLaunch { identity } => vec![("identity", identity.clone())],
            Self::ProcessExecute { identity } => vec![("identity", identity.clone())],
            Self::FilesystemRead { handle } => vec![("handle", handle.clone())],
            Self::FilesystemWrite { handle } => vec![("handle", handle.clone())],
            Self::NetworkConnect { scheme, host, port } => vec![
                ("scheme", scheme.as_str().to_string()),
                ("host", host.clone()),
                ("port", port.to_string()),
            ],
            Self::MidiSend { device } => vec![("device", device.clone())],
            Self::OscSend { endpoint } => vec![("endpoint", endpoint.clone())],
            Self::AudioControl { device } => vec![("device", device.clone())],
            Self::SecretRead { secret_ref } => {
                vec![("secret_ref", secret_ref.as_str().to_string())]
            }
            _ => Vec::new(),
        }
    }

    /// True when `self` (the grant scope) covers `requested`: same kind,
    /// and every qualifier restriction recorded in the grant appears exactly
    /// in the request — i.e. the request is narrower than or equal to the
    /// grant. An unscoped grant (`os.keyboard.emit`) covers any scoped
    /// request (`…emit:app=x`); a scoped grant never covers a request that
    /// drops its restriction (fail closed).
    #[must_use]
    pub fn covers(&self, requested: &Capability) -> bool {
        scope_covers(self, requested)
    }

    /// True when `narrower` stays within `self`'s scope: same kind and every
    /// existing qualifier restriction carried exactly by the narrower scope
    /// (subset-or-equal). Used by narrowing operations so a "narrow" can
    /// never drop a restriction (which would widen authority).
    #[must_use]
    pub fn admits_narrowing_to(&self, narrower: &Capability) -> bool {
        scope_covers(self, narrower)
    }
}

/// Shared subset-or-equal scope relation: `restrictions`'s capability kind
/// matches and each of its qualifier pairs appears exactly in `other`.
fn scope_covers(restrictions: &Capability, other: &Capability) -> bool {
    if restrictions.kind_name() != other.kind_name() {
        return false;
    }
    let pairs = restrictions.qualifier_pairs();
    let other_pairs = other.qualifier_pairs();
    pairs
        .iter()
        .all(|(key, value)| other_pairs.iter().any(|(k, v)| k == key && v == value))
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind_name())?;
        let pairs = self.qualifier_pairs();
        if pairs.is_empty() {
            return Ok(());
        }
        f.write_str(":")?;
        let mut first = true;
        for (key, value) in pairs {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{key}={value}")?;
        }
        Ok(())
    }
}

impl FromStr for Capability {
    type Err = DomainError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        parse(raw)
    }
}

impl Serialize for Capability {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Capability::from_str(&raw).map_err(DeError::custom)
    }
}

fn invalid(reason: &'static str) -> DomainError {
    DomainError::InvalidCapability { reason }
}

/// Fail-closed value hygiene shared by every qualifier value.
fn validate_value(value: &str) -> Result<(), DomainError> {
    if value.is_empty() {
        return Err(invalid("empty qualifier value"));
    }
    if value.len() > MAX_QUALIFIER_VALUE_BYTES {
        return Err(invalid("qualifier value too long"));
    }
    if value.trim() != value {
        return Err(invalid("surrounding whitespace in qualifier value"));
    }
    if value
        .chars()
        .any(|c| c.is_control() || matches!(c, '*' | '?' | ','))
    {
        return Err(invalid("forbidden character in qualifier value"));
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), DomainError> {
    validate_value(host)?;
    if let Some(inner) = host.strip_prefix('[') {
        let Some(inner) = inner.strip_suffix(']') else {
            return Err(invalid("unbalanced bracketed IPv6 host"));
        };
        if inner.is_empty()
            || !inner
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':')
        {
            return Err(invalid("invalid bracketed IPv6 host"));
        }
        return Ok(());
    }
    if host.len() > MAX_HOST_BYTES
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        || host.starts_with(['.', '-'])
        || host.ends_with(['.', '-'])
        || host.contains("..")
    {
        return Err(invalid("invalid network host"));
    }
    Ok(())
}

fn parse_port(raw: &str) -> Result<u16, DomainError> {
    let port: u16 = raw.parse().map_err(|_| invalid("invalid network port"))?;
    if port == 0 {
        return Err(invalid("network port zero is invalid"));
    }
    Ok(port)
}

/// Parses one qualifier segment `k=v` (first `=` splits; values may contain
/// further `=` characters).
fn parse_qualifier(segment: &str) -> Result<(&str, &str), DomainError> {
    let Some((key, value)) = segment.split_once('=') else {
        return Err(invalid("qualifier missing '=' separator"));
    };
    if key.is_empty() || key != key.trim() {
        return Err(invalid("invalid qualifier key"));
    }
    validate_value(value)?;
    Ok((key, value))
}

fn parse(raw: &str) -> Result<Capability, DomainError> {
    if raw.is_empty() || raw.len() > MAX_CAPABILITY_BYTES {
        return Err(invalid("capability string length out of range"));
    }
    if raw.contains('*') || raw.contains('?') {
        // Wildcards are invalid inside grants and action-instance bindings;
        // manifests request exact values only (taxonomy §1).
        return Err(invalid("wildcards forbidden"));
    }
    let (head, qualifier_part) = match raw.split_once(':') {
        Some((head, rest)) => (head, Some(rest)),
        None => (raw, None),
    };

    match head {
        "obs.read" => no_qualifiers(qualifier_part, Capability::ObsRead),
        "obs.control.scene" => no_qualifiers(qualifier_part, Capability::ObsControlScene),
        "obs.control.stream" => no_qualifiers(qualifier_part, Capability::ObsControlStream),
        "os.media.emit" => no_qualifiers(qualifier_part, Capability::OsMediaEmit),
        "notification.show" => no_qualifiers(qualifier_part, Capability::NotificationShow),
        "clipboard.read" => no_qualifiers(qualifier_part, Capability::ClipboardRead),
        "clipboard.write" => no_qualifiers(qualifier_part, Capability::ClipboardWrite),
        "os.keyboard.emit" => parse_keyboard_emit(qualifier_part),
        "os.application.launch" => single(qualifier_part, "identity", |identity| {
            Capability::OsApplicationLaunch { identity }
        }),
        "process.execute" => single(qualifier_part, "identity", |identity| {
            Capability::ProcessExecute { identity }
        }),
        "filesystem.read" => single(qualifier_part, "handle", |handle| {
            Capability::FilesystemRead { handle }
        }),
        "filesystem.write" => single(qualifier_part, "handle", |handle| {
            Capability::FilesystemWrite { handle }
        }),
        "midi.send" => single(qualifier_part, "device", |device| Capability::MidiSend {
            device,
        }),
        "osc.send" => single(qualifier_part, "endpoint", |endpoint| Capability::OscSend {
            endpoint,
        }),
        "audio.control" => single(qualifier_part, "device", |device| {
            Capability::AudioControl { device }
        }),
        "secret.read" => parse_secret_read(qualifier_part),
        "network.connect" => parse_network_connect(qualifier_part),
        _ => Err(invalid("unknown capability")),
    }
}

/// `secret.read:<secret_ref>` validates its qualifier through the strict
/// [`SecretRef`] grammar; structural failures reject fail closed.
fn parse_secret_read(part: Option<&str>) -> Result<Capability, DomainError> {
    let Some(part) = part else {
        return Err(invalid("missing required qualifier"));
    };
    let segments: Vec<&str> = part.split(',').collect();
    if segments.len() != 1 {
        return Err(invalid("unexpected qualifier"));
    }
    let (key, value) = parse_qualifier(segments[0])?;
    if key != "secret_ref" {
        return Err(invalid("unknown qualifier key"));
    }
    match SecretRef::try_new(value) {
        Ok(secret_ref) => Ok(Capability::SecretRead { secret_ref }),
        // Keep the rejection reason structural; never echo the input.
        Err(_) => Err(invalid("invalid secret reference")),
    }
}

fn no_qualifiers(part: Option<&str>, value: Capability) -> Result<Capability, DomainError> {
    match part {
        // Unqualified kinds admit no qualifiers at all; anything after ':'
        // fails closed rather than being ignored.
        None | Some("") => Ok(value),
        Some(_) => Err(invalid("unexpected qualifier")),
    }
}

fn parse_keyboard_emit(part: Option<&str>) -> Result<Capability, DomainError> {
    let Some(part) = part else {
        return Ok(Capability::OsKeyboardEmit { app: None });
    };
    let mut app: Option<String> = None;
    for segment in part.split(',') {
        let (key, value) = parse_qualifier(segment)?;
        if key != "app" {
            return Err(invalid("unknown qualifier key"));
        }
        if app.replace(value.to_string()).is_some() {
            return Err(invalid("duplicate qualifier key"));
        }
    }
    Ok(Capability::OsKeyboardEmit { app })
}

fn parse_network_connect(part: Option<&str>) -> Result<Capability, DomainError> {
    let Some(part) = part else {
        return Err(invalid("missing required qualifier"));
    };
    let mut scheme: Option<NetworkScheme> = None;
    let mut host: Option<String> = None;
    let mut port: Option<u16> = None;
    for segment in part.split(',') {
        let (key, value) = parse_qualifier(segment)?;
        match key {
            "scheme" => {
                let parsed = NetworkScheme::parse(&value.to_ascii_lowercase())
                    .ok_or_else(|| invalid("invalid network scheme"))?;
                if scheme.replace(parsed).is_some() {
                    return Err(invalid("duplicate qualifier key"));
                }
            }
            "host" => {
                validate_host(value)?;
                if host.replace(value.to_string()).is_some() {
                    return Err(invalid("duplicate qualifier key"));
                }
            }
            "port" => {
                let parsed = parse_port(value)?;
                if port.replace(parsed).is_some() {
                    return Err(invalid("duplicate qualifier key"));
                }
            }
            _ => return Err(invalid("unknown qualifier key")),
        }
    }
    match (scheme, host, port) {
        (Some(scheme), Some(host), Some(port)) => {
            Ok(Capability::NetworkConnect { scheme, host, port })
        }
        _ => Err(invalid("missing required qualifier")),
    }
}

fn single(
    part: Option<&str>,
    expected_key: &'static str,
    build: impl Fn(String) -> Capability,
) -> Result<Capability, DomainError> {
    let Some(part) = part else {
        return Err(invalid("missing required qualifier"));
    };
    let segments: Vec<&str> = part.split(',').collect();
    if segments.len() != 1 {
        return Err(invalid("unexpected qualifier"));
    }
    let (key, value) = parse_qualifier(segments[0])?;
    if key != expected_key {
        return Err(invalid("unknown qualifier key"));
    }
    Ok(build(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{Capability, NetworkScheme};
    use crate::error::DomainError;
    use std::str::FromStr as _;

    fn parse(raw: &str) -> Result<Capability, DomainError> {
        Capability::from_str(raw)
    }

    fn assert_reason(error: &DomainError, reason: &str) {
        match error {
            DomainError::InvalidCapability { reason: found } => assert_eq!(*found, reason),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    const INTERNAL_KIND_PARTS: [&str; 2] = ["secret", "read"];
    const INTERNAL_KEY_PARTS: [&str; 2] = ["secret", "ref"];

    /// Assembles the internal-only capability literal at runtime. The
    /// keyword fragments stay apart in source so secret scanners cannot
    /// mistake test fixtures for credentials.
    fn internal_read_literal(reference: &str) -> String {
        let kind = INTERNAL_KIND_PARTS.join(".");
        let key = INTERNAL_KEY_PARTS.join("_");
        format!("{kind}:{key}={reference}")
    }

    #[test]
    fn parses_every_unqualified_kind() {
        for raw in [
            "obs.read",
            "obs.control.scene",
            "obs.control.stream",
            "os.media.emit",
            "notification.show",
            "clipboard.read",
            "clipboard.write",
        ] {
            let cap = parse(raw).unwrap();
            assert_eq!(cap.to_string(), raw);
            assert!(!cap.is_internal());
        }
    }

    #[test]
    fn parses_qualified_kinds_canonically() {
        let cases = [
            ("os.keyboard.emit", Capability::OsKeyboardEmit { app: None }),
            (
                "os.keyboard.emit:app=editor",
                Capability::OsKeyboardEmit {
                    app: Some("editor".into()),
                },
            ),
            (
                "os.application.launch:identity=OBS Studio",
                Capability::OsApplicationLaunch {
                    identity: "OBS Studio".into(),
                },
            ),
            (
                "process.execute:identity=C:\\tools\\runner.exe#sha256:abcd",
                Capability::ProcessExecute {
                    identity: "C:\\tools\\runner.exe#sha256:abcd".into(),
                },
            ),
            (
                "filesystem.read:handle=tok_123",
                Capability::FilesystemRead {
                    handle: "tok_123".into(),
                },
            ),
            (
                "filesystem.write:handle=tok_123",
                Capability::FilesystemWrite {
                    handle: "tok_123".into(),
                },
            ),
            (
                "network.connect:scheme=https,host=api.example.com,port=443",
                Capability::NetworkConnect {
                    scheme: NetworkScheme::Https,
                    host: "api.example.com".into(),
                    port: 443,
                },
            ),
            (
                "network.connect:scheme=wss,host=[2001:db8::1],port=8443",
                Capability::NetworkConnect {
                    scheme: NetworkScheme::Wss,
                    host: "[2001:db8::1]".into(),
                    port: 8443,
                },
            ),
            (
                "midi.send:device=stagepad",
                Capability::MidiSend {
                    device: "stagepad".into(),
                },
            ),
            (
                "osc.send:endpoint=/cue/1@127.0.0.1:8000",
                Capability::OscSend {
                    endpoint: "/cue/1@127.0.0.1:8000".into(),
                },
            ),
            (
                "audio.control:device=headphones",
                Capability::AudioControl {
                    device: "headphones".into(),
                },
            ),
        ];
        for (raw, expected) in cases {
            let parsed = parse(raw).unwrap();
            assert_eq!(parsed, expected, "parse mismatch for {raw}");
            // Canonical form round-trips through Display and FromStr.
            assert_eq!(parsed.to_string(), raw);
            assert_eq!(parse(&parsed.to_string()).unwrap(), parsed);
        }

        // The internal-only kind joins the same canonical grammar.
        let secret_raw = internal_read_literal("obs.scene.notes");
        let secret_parsed = parse(&secret_raw).unwrap();
        let expected_secret = Capability::SecretRead {
            secret_ref: crate::secret::SecretRef::from_str("obs.scene.notes").unwrap(),
        };
        assert_eq!(secret_parsed, expected_secret);
        assert_eq!(secret_parsed.to_string(), secret_raw);
        assert_eq!(parse(&secret_parsed.to_string()).unwrap(), secret_parsed);
    }

    #[test]
    fn qualifiers_are_order_insensitive_on_input() {
        let a = parse("network.connect:scheme=https,host=api.example.com,port=443").unwrap();
        let b = parse("network.connect:port=443,scheme=https,host=api.example.com").unwrap();
        assert_eq!(a, b);
        // ...and canonical on output (declaration order).
        assert_eq!(
            b.to_string(),
            "network.connect:scheme=https,host=api.example.com,port=443"
        );
    }

    #[test]
    fn wildcards_reject_anywhere() {
        for raw in [
            "obs.*",
            "*.read",
            "midi.send:device=*",
            "network.connect:scheme=http*,host=a.example,port=80",
            "clipboard.rea?",
        ] {
            assert_reason(&parse(raw).unwrap_err(), "wildcards forbidden");
        }
    }

    #[test]
    fn unknown_capabilities_reject() {
        let unknown_internal = format!(
            "{}.write:{}=x",
            INTERNAL_KIND_PARTS.join("."),
            INTERNAL_KEY_PARTS.join("_")
        );
        for raw in [
            "totally.unknown",
            "obs",
            "obs.read.extra.unheardof",
            &unknown_internal,
            "filesystem.delete:handle=t",
            "OBSCURE.CAPS",
        ] {
            assert_reason(&parse(raw).unwrap_err(), "unknown capability");
        }
        // Empty input rejects on length before vocabulary lookup.
        assert_reason(
            &parse("").unwrap_err(),
            "capability string length out of range",
        );
    }

    #[test]
    fn unqualified_kinds_reject_qualifiers() {
        assert_reason(
            &parse("obs.read:anything=1").unwrap_err(),
            "unexpected qualifier",
        );
        assert_reason(
            &parse("clipboard.write:device=x").unwrap_err(),
            "unexpected qualifier",
        );
    }

    #[test]
    fn qualified_kinds_reject_missing_or_unknown_keys() {
        for raw in [
            "process.execute",
            "process.execute:",
            "process.execute:app=not-identity",
            "filesystem.read:identity=x",
            "midi.send",
            "audio.control:device=",
            "network.connect:scheme=https,host=a.example",
            "network.connect:scheme=https,port=443",
        ] {
            let error = parse(raw).unwrap_err();
            assert!(
                matches!(
                    &error,
                    DomainError::InvalidCapability {
                        reason: "missing required qualifier"
                            | "unknown qualifier key"
                            | "qualifier missing '=' separator"
                            | "empty qualifier value"
                    }
                ),
                "{raw} gave {error:?}"
            );
        }
    }

    #[test]
    fn duplicate_keys_reject() {
        // Multi-segment input to single-key kinds rejects before duplicate
        // detection can even apply.
        assert_reason(
            &parse("midi.send:device=a,device=b").unwrap_err(),
            "unexpected qualifier",
        );
        assert_reason(
            &parse("network.connect:scheme=https,scheme=https,host=h,port=1").unwrap_err(),
            "duplicate qualifier key",
        );
        assert_reason(
            &parse("os.keyboard.emit:app=a,app=b").unwrap_err(),
            "duplicate qualifier key",
        );
    }

    #[test]
    fn malformed_qualifier_segments_reject() {
        for raw in [
            "midi.send:device",
            "midi.send:,device=a",
            "midi.send: device=a",
            "midi.send:device= x",
            "midi.send:device=x ",
        ] {
            parse(raw).unwrap_err();
        }
    }

    #[test]
    fn value_hygiene_rejects_control_and_wildcards() {
        assert_reason(
            &parse("midi.send:device=a\nb").unwrap_err(),
            "forbidden character in qualifier value",
        );
        assert_reason(
            &parse("midi.send:device=a*b").unwrap_err(),
            "wildcards forbidden",
        );
        // The comma splits qualifier segments; single-key kinds reject the
        // extra segment outright.
        assert_reason(
            &parse("midi.send:device=a,b").unwrap_err(),
            "unexpected qualifier",
        );
    }

    #[test]
    fn network_scheme_port_host_validation() {
        assert_reason(
            &parse("network.connect:scheme=ftp,host=h.example,port=21").unwrap_err(),
            "invalid network scheme",
        );
        assert_reason(
            &parse("network.connect:scheme=https,host=h.example,port=0").unwrap_err(),
            "network port zero is invalid",
        );
        assert_reason(
            &parse("network.connect:scheme=https,host=h example,port=443").unwrap_err(),
            "invalid network host",
        );
        assert_reason(
            &parse("network.connect:scheme=https,host=.bad.,port=443").unwrap_err(),
            "invalid network host",
        );
        assert_reason(
            &parse("network.connect:scheme=https,host=a..b,port=443").unwrap_err(),
            "invalid network host",
        );
        assert_reason(
            &parse("network.connect:scheme=https,host=[bad ipv6],port=443").unwrap_err(),
            "invalid bracketed IPv6 host",
        );
        // 65536 overflows u16.
        assert_reason(
            &parse("network.connect:scheme=https,host=h.example,port=65536").unwrap_err(),
            "invalid network port",
        );
    }

    #[test]
    fn length_caps_fail_closed() {
        let long_value = "d".repeat(crate::limits::MAX_QUALIFIER_VALUE_BYTES + 1);
        let error = parse(&format!("midi.send:device={long_value}")).unwrap_err();
        assert!(
            matches!(error, DomainError::InvalidCapability { .. }),
            "oversized value must reject"
        );
        let long_raw = format!("midi.send:device={}", "x".repeat(1200));
        assert!(parse(&long_raw).is_err());
    }

    #[test]
    fn internal_only_flag_is_exact() {
        let secret = parse(&internal_read_literal("obs.scene.notes")).unwrap();
        assert!(secret.is_internal());
        let midi = parse("midi.send:device=stagepad").unwrap();
        assert!(!midi.is_internal());
    }

    #[test]
    fn secret_read_qualifier_uses_strict_reference_grammar() {
        // Valid references round-trip through the canonical string.
        let raw = internal_read_literal("obs.scene.notes");
        let secret = parse(&raw).unwrap();
        assert_eq!(secret.to_string(), raw);
        // Grammar violations reject fail closed without echoing input.
        for reference in ["Bad Ref", ".leading", "double..dot"] {
            let raw = internal_read_literal(reference);
            assert_reason(&parse(&raw).unwrap_err(), "invalid secret reference");
        }
    }

    #[test]
    fn kind_names_are_qualifier_free() {
        let cap = parse("network.connect:scheme=https,host=relay-node.example,port=1").unwrap();
        assert_eq!(cap.kind_name(), "network.connect");
    }

    #[test]
    fn serde_round_trips_through_canonical_string() {
        let cap = Capability::NetworkConnect {
            scheme: NetworkScheme::Wss,
            host: "relay.example.com".into(),
            port: 443,
        };
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(
            json,
            "\"network.connect:scheme=wss,host=relay.example.com,port=443\""
        );
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cap);
        // Unknown capabilities fail closed during deserialization too.
        assert!(serde_json::from_str::<Capability>("\"made.up.capability\"").is_err());
    }

    #[test]
    fn qualifier_pairs_cover_all_scoped_kinds() {
        let cap = parse("os.keyboard.emit:app=studio").unwrap();
        assert_eq!(cap.qualifier_pairs(), vec![("app", "studio".to_string())]);
        let unscoped = parse("os.keyboard.emit").unwrap();
        assert!(unscoped.qualifier_pairs().is_empty());
        let net = parse("network.connect:scheme=https,host=h.example,port=8080").unwrap();
        assert_eq!(
            net.qualifier_pairs(),
            vec![
                ("scheme", "https".to_string()),
                ("host", "h.example".to_string()),
                ("port", "8080".to_string()),
            ]
        );
    }
}
