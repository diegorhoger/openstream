//! Typed launch-target configuration for issue #11, validated fail closed.
//!
//! Three bounded action kinds ship behind the existing taxonomy row
//! `os.application.launch:<identity>`
//! ([`CAPABILITY_TAXONOMY.md`](https://github.com/diegorhoger/openstream/blob/main/docs/security/CAPABILITY_TAXONOMY.md)
//! §5): opening a file or URL delegates to the OS default-handler
//! resolution, so every kind ultimately *launches* an OS-selected handler
//! for one user-approved target. No new capability vocabulary is added; the
//! `<identity>` qualifier carries the exact approved target token and is
//! revalidated before every launch (taxonomy §6).
//!
//! Kinds and parameter shapes (exactly one field per object):
//!
//! | Kind | Params | Target token |
//! |---|---|---|
//! | [`ACTION_TYPE_LAUNCH_APPLICATION`] | `{"identity": "..."}` | lowercase identity token |
//! | [`ACTION_TYPE_LAUNCH_FILE`] | `{"path": "..."}` | absolute path, byte-exact |
//! | [`ACTION_TYPE_LAUNCH_URL`] | `{"url": "..."}` | absolute `scheme://…`, byte-exact |
//!
//! Validation rules (all rejections structural; rejected input never enters
//! an error value):
//!
//! - Tokens are bounded by [`MAX_TARGET_BYTES`], which mirrors the domain
//!   qualifier-value cap so every token fits its capability string.
//! - Application identities match `[a-z0-9][a-z0-9._-]{0,63}` — no path
//!   separators, colons, whitespace, or uppercase.
//! - Paths must be absolute in a platform-independent sense (POSIX root,
//!   drive-letter, or UNC form), reject `.`/`..` components, empty
//!   segments, trailing separators, device namespaces (`\\.\`, `\\?\`),
//!   control characters, and oversized input.
//! - URLs must be absolute `scheme://authority…` form with a non-empty
//!   host, no userinfo component, no whitespace or control characters, and
//!   a scheme matched case-insensitively against the policy allowlist (a
//!   closed [`UrlScheme`] vocabulary — arbitrary custom schemes are never
//!   admissible).
//! - File targets whose extension looks executable/scriptable refuse as a
//!   policy violation: default-handler launching of such targets would
//!   collapse into direct process execution, which belongs exclusively to
//!   the separately gated `process.execute` registry row.
//!
//! The same validators run at registration time ([`LaunchBinding`]), and
//! again per dispatch inside the port ([`crate::launch::port`]), so no
//! untyped path can reach a launch.

use core::fmt;
use openstream_domain::capability::Capability;
use serde_json::Value;

/// Maximum UTF-8 byte length of any launch-target token. Mirrors the
/// domain qualifier-value cap so tokens always fit their capability string.
pub const MAX_TARGET_BYTES: usize = openstream_domain::limits::MAX_QUALIFIER_VALUE_BYTES;

/// Maximum UTF-8 byte length of an application identity token.
pub const MAX_IDENTITY_TOKEN_BYTES: usize = 64;

/// Registered action type name for the application-launch action.
pub const ACTION_TYPE_LAUNCH_APPLICATION: &str = "os.launch.application";
/// Registered action type name for the file-open action.
pub const ACTION_TYPE_LAUNCH_FILE: &str = "os.launch.file";
/// Registered action type name for the URL-open action.
pub const ACTION_TYPE_LAUNCH_URL: &str = "os.launch.url";

/// Admissible URL schemes. The set is closed: arbitrary custom schemes are
/// never expressible, so a policy can only ever widen between HTTP and
/// HTTPS, never beyond (no arbitrary scheme execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UrlScheme {
    /// Plain HTTP.
    Http,
    /// HTTP over TLS.
    Https,
}

impl UrlScheme {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }

    /// True when `raw` names this scheme (RFC 3986 §3.1: schemes compare
    /// case-insensitively).
    #[must_use]
    pub fn matches(self, raw: &str) -> bool {
        raw.eq_ignore_ascii_case(self.as_str())
    }
}

impl fmt::Display for UrlScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-registration launch policy: the typed bounds a host declares for the
/// launch actions it composes. Defaults deny more than the taxonomy
/// requires; widening happens only through explicit typed choices here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPolicy {
    url_schemes: Vec<UrlScheme>,
}

impl LaunchPolicy {
    /// Builds a policy admitting exactly `url_schemes` for URL-open
    /// bindings. Duplicates collapse preserving order; the sequence may be
    /// empty (no URL target is ever launchable).
    #[must_use]
    pub fn new(url_schemes: &[UrlScheme]) -> Self {
        let mut unique = Vec::new();
        for scheme in url_schemes {
            if !unique.contains(scheme) {
                unique.push(*scheme);
            }
        }
        Self {
            url_schemes: unique,
        }
    }

    /// Conservative default: HTTPS only.
    #[must_use]
    pub fn standard() -> Self {
        Self::new(&[UrlScheme::Https])
    }

    /// Admitted schemes in declaration order.
    #[must_use]
    pub fn url_schemes(&self) -> &[UrlScheme] {
        &self.url_schemes
    }

    /// Checks a URL target against the scheme allowlist.
    ///
    /// # Errors
    /// [`LaunchConfigError::PolicySchemeNotAllowed`] when the target's
    /// scheme is outside the allowlist.
    pub fn check_url(&self, target: &UrlTarget) -> Result<(), LaunchConfigError> {
        let raw_scheme = target.scheme();
        if self.url_schemes.iter().any(|s| s.matches(raw_scheme)) {
            return Ok(());
        }
        Err(LaunchConfigError::PolicySchemeNotAllowed)
    }

    /// Checks a file target against the executable/script extension denial
    /// list (defense in depth; see module docs).
    ///
    /// # Errors
    /// [`LaunchConfigError::PolicyExecutableTarget`] when the final
    /// extension is in the denied set.
    pub fn check_file(&self, target: &FileTarget) -> Result<(), LaunchConfigError> {
        let lowered = target.as_str().to_ascii_lowercase();
        let last_separator = lowered.rfind(['/', '\\']);
        let file_name = match last_separator {
            Some(index) => &lowered[index + 1..],
            None => lowered.as_str(),
        };
        let Some((_, extension)) = file_name.rsplit_once('.') else {
            return Ok(());
        };
        if BLOCKED_FILE_EXTENSIONS.contains(&extension) {
            return Err(LaunchConfigError::PolicyExecutableTarget);
        }
        Ok(())
    }
}

/// Extensions whose default-handler launch would execute program logic
/// rather than open a document. Defense in depth only: the primary control
/// is exact-target explicit selection plus per-dispatch revalidation.
const BLOCKED_FILE_EXTENSIONS: [&str; 24] = [
    "exe",
    "bat",
    "cmd",
    "com",
    "scr",
    "ps1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "msi",
    "msp",
    "mst",
    "hta",
    "jar",
    "lnk",
    "pif",
    "cpl",
    "msc",
    "url",
    "application",
    "gadget",
];

/// Validated application-launch target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApplicationTarget {
    identity: String,
}

impl ApplicationTarget {
    /// Validates and builds a target from an identity token.
    ///
    /// # Errors
    /// [`LaunchConfigError::IdentityEmpty`], [`IdentityInvalidChar`], or
    /// [`IdentityTooLong`] on grammar violations.
    pub fn try_new(identity: &str) -> Result<Self, LaunchConfigError> {
        validate_identity(identity)?;
        Ok(Self {
            identity: identity.to_string(),
        })
    }

    /// The validated identity token (byte-exact capability qualifier).
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// The capability qualifier value this target binds.
    #[must_use]
    pub fn capability_identity(&self) -> &str {
        &self.identity
    }
}

/// Validated file-open target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileTarget {
    path: String,
}

impl FileTarget {
    /// Validates and builds a target from an absolute path string.
    ///
    /// # Errors
    /// Any [`LaunchConfigError`] path variant; see the module docs.
    pub fn try_new(path: &str) -> Result<Self, LaunchConfigError> {
        validate_path(path)?;
        Ok(Self {
            path: path.to_string(),
        })
    }

    /// The validated path (byte-exact capability qualifier).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.path
    }

    /// The capability qualifier value this target binds.
    #[must_use]
    pub fn capability_identity(&self) -> &str {
        &self.path
    }
}

/// Validated URL-open target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UrlTarget {
    url: String,
    scheme_end: usize,
    host_start: usize,
    host_end: usize,
}

impl UrlTarget {
    /// Validates and builds a target from an absolute URL string.
    ///
    /// Structural validation only; the scheme allowlist is a policy
    /// decision applied by [`LaunchPolicy::check_url`].
    ///
    /// # Errors
    /// Any [`LaunchConfigError`] URL variant; see the module docs.
    pub fn try_new(url: &str) -> Result<Self, LaunchConfigError> {
        validate_url(url)
    }

    /// The validated URL (byte-exact capability qualifier).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.url
    }

    /// The scheme substring as authored (case preserved).
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.url[..self.scheme_end]
    }

    /// The authority host substring as authored.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.url[self.host_start..self.host_end]
    }

    /// The capability qualifier value this target binds.
    #[must_use]
    pub fn capability_identity(&self) -> &str {
        &self.url
    }
}

fn validate_identity(identity: &str) -> Result<(), LaunchConfigError> {
    if identity.is_empty() {
        return Err(LaunchConfigError::IdentityEmpty);
    }
    if identity.len() > MAX_IDENTITY_TOKEN_BYTES {
        return Err(LaunchConfigError::IdentityTooLong);
    }
    let mut bytes = identity.bytes();
    let first = bytes.next().unwrap_or_else(|| unreachable!("non-empty"));
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(LaunchConfigError::IdentityInvalidChar);
    }
    if !bytes.all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-'
    }) {
        return Err(LaunchConfigError::IdentityInvalidChar);
    }
    Ok(())
}

/// True when the raw string starts an accepted absolute form: POSIX root,
/// drive letter (`X:\` or `X:/`), or UNC (`\\` / `//` prefix beyond the
/// device namespaces).
fn is_absolute_form(raw: &str) -> bool {
    if raw.starts_with('/') {
        return true;
    }
    let bytes = raw.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    // UNC server/share form; device namespaces are excluded by callers.
    (raw.starts_with("\\\\") || raw.starts_with("//")) && raw.len() > 2
}

fn validate_path(raw: &str) -> Result<(), LaunchConfigError> {
    if raw.is_empty() {
        return Err(LaunchConfigError::PathNotAbsolute);
    }
    if raw.len() > MAX_TARGET_BYTES {
        return Err(LaunchConfigError::PathTooLong);
    }
    if raw.starts_with("\\\\.\\") || raw.starts_with("\\\\?\\") {
        return Err(LaunchConfigError::PathDeviceNamespace);
    }
    if raw
        .chars()
        .any(|c| c.is_control() || matches!(c, '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(LaunchConfigError::PathInvalidChar);
    }
    if !is_absolute_form(raw) {
        return Err(LaunchConfigError::PathNotAbsolute);
    }
    if raw.ends_with('\\') || raw.ends_with('/') {
        return Err(LaunchConfigError::PathTrailingSeparator);
    }
    // Absolute forms carry leading separators (POSIX root, UNC prefix);
    // strip them so only interior emptiness rejects.
    let body = raw.trim_start_matches(['\\', '/']);
    for component in body.split(['\\', '/']) {
        match component {
            ".." => return Err(LaunchConfigError::PathTraversal),
            "." | "" => return Err(LaunchConfigError::PathEmptySegment),
            _ => {}
        }
    }
    Ok(())
}

fn validate_url(raw: &str) -> Result<UrlTarget, LaunchConfigError> {
    if raw.is_empty() {
        return Err(LaunchConfigError::UrlNotAbsoluteForm);
    }
    if raw.len() > MAX_TARGET_BYTES {
        return Err(LaunchConfigError::UrlTooLong);
    }
    if raw.chars().any(|c| c.is_control() || c == ' ') {
        return Err(LaunchConfigError::UrlForbiddenChar);
    }
    let Some(scheme_end) = raw.find(':') else {
        return Err(LaunchConfigError::UrlNotAbsoluteForm);
    };
    let scheme = &raw[..scheme_end];
    if scheme.is_empty()
        || !scheme.bytes().enumerate().all(|(index, b)| {
            if index == 0 {
                b.is_ascii_lowercase() || b.is_ascii_uppercase()
            } else {
                b.is_ascii_lowercase()
                    || b.is_ascii_uppercase()
                    || b.is_ascii_digit()
                    || b == b'+'
                    || b == b'-'
                    || b == b'.'
            }
        })
    {
        return Err(LaunchConfigError::UrlNotAbsoluteForm);
    }
    let rest = &raw[scheme_end + 1..];
    let Some(authority_part) = rest.strip_prefix("//") else {
        return Err(LaunchConfigError::UrlNotAbsoluteForm);
    };
    let authority_end = authority_part
        .find(['/', '?', '#'])
        .unwrap_or(authority_part.len());
    let authority = &authority_part[..authority_end];
    if authority.contains('@') {
        return Err(LaunchConfigError::UrlUserinfo);
    }
    let host = authority.split(':').next().unwrap_or("");
    if host.is_empty() {
        return Err(LaunchConfigError::UrlMissingHost);
    }
    Ok(UrlTarget {
        url: raw.to_string(),
        scheme_end,
        host_start: scheme_end + 3,
        host_end: scheme_end + 3 + host.len(),
    })
}

/// One registration-time binding: an already-validated target together
/// with the action kind it belongs to. Bindings are the manifest layer for
/// launch actions: each becomes exactly one capability scope on its kind's
/// registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchBinding {
    /// Launch one approved application identity.
    Application(ApplicationTarget),
    /// Open one approved file with its default handler.
    File(FileTarget),
    /// Open one approved URL with its default handler.
    Url(UrlTarget),
}

impl LaunchBinding {
    /// The registered action type this binding scopes.
    #[must_use]
    pub const fn action_type(&self) -> &'static str {
        match self {
            Self::Application(_) => ACTION_TYPE_LAUNCH_APPLICATION,
            Self::File(_) => ACTION_TYPE_LAUNCH_FILE,
            Self::Url(_) => ACTION_TYPE_LAUNCH_URL,
        }
    }

    /// The exact capability scope this binding contributes.
    #[must_use]
    pub fn capability(&self) -> Capability {
        let identity = match self {
            Self::Application(target) => target.capability_identity(),
            Self::File(target) => target.capability_identity(),
            Self::Url(target) => target.capability_identity(),
        };
        Capability::OsApplicationLaunch {
            identity: identity.to_string(),
        }
    }
}

/// Parses action-graph parameters into a validated application target.
///
/// # Errors
/// [`LaunchConfigError`] variants; rejected input never enters errors.
pub fn parse_application_params(params: &Value) -> Result<ApplicationTarget, LaunchConfigError> {
    let identity = single_string_field(params, "identity", LaunchConfigError::IdentityWrongType)?;
    ApplicationTarget::try_new(identity)
}

/// Parses action-graph parameters into a validated file target.
///
/// # Errors
/// [`LaunchConfigError`] variants; rejected input never enters errors.
pub fn parse_file_params(params: &Value) -> Result<FileTarget, LaunchConfigError> {
    let path = single_string_field(params, "path", LaunchConfigError::PathWrongType)?;
    FileTarget::try_new(path)
}

/// Parses action-graph parameters into a validated URL target.
///
/// Structural validation only; apply [`LaunchPolicy::check_url`] for the
/// scheme allowlist.
///
/// # Errors
/// [`LaunchConfigError`] variants; rejected input never enters errors.
pub fn parse_url_params(params: &Value) -> Result<UrlTarget, LaunchConfigError> {
    let url = single_string_field(params, "url", LaunchConfigError::UrlWrongType)?;
    UrlTarget::try_new(url)
}

fn single_string_field<'a>(
    params: &'a Value,
    key: &str,
    wrong_type: LaunchConfigError,
) -> Result<&'a str, LaunchConfigError> {
    let Value::Object(fields) = params else {
        return Err(LaunchConfigError::NotAnObject);
    };
    if fields.len() != 1 || !fields.contains_key(key) {
        return Err(LaunchConfigError::UnexpectedField);
    }
    let value = fields
        .get(key)
        .unwrap_or_else(|| unreachable!("checked above"));
    let Some(text) = value.as_str() else {
        return Err(wrong_type);
    };
    Ok(text)
}

/// Typed configuration and policy failures for launch actions. Structural
/// reasons only: rejected input values never appear in any variant
/// (redaction rules, TM-LOG-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchConfigError {
    /// Parameters were not a JSON object.
    NotAnObject,
    /// The object carried fields other than the single declared field, or
    /// missed it entirely.
    UnexpectedField,
    /// `identity` was not a JSON string.
    IdentityWrongType,
    /// The identity token was empty.
    IdentityEmpty,
    /// The identity token contained characters outside
    /// `[a-z0-9._-]` or did not start with `[a-z0-9]`.
    IdentityInvalidChar,
    /// The identity token exceeded [`MAX_IDENTITY_TOKEN_BYTES`].
    IdentityTooLong,
    /// `path` was not a JSON string.
    PathWrongType,
    /// The path was relative or otherwise not an accepted absolute form.
    PathNotAbsolute,
    /// The path carried a `..` component.
    PathTraversal,
    /// The path carried an empty or `.` component, e.g. doubled separators.
    PathEmptySegment,
    /// The path ended in a separator.
    PathTrailingSeparator,
    /// The path used the Win32 device namespace (`\\.\`, `\\?\`).
    PathDeviceNamespace,
    /// The path exceeded [`MAX_TARGET_BYTES`].
    PathTooLong,
    /// The path contained forbidden characters.
    PathInvalidChar,
    /// `url` was not a JSON string.
    UrlWrongType,
    /// The URL was not absolute `scheme://…` form.
    UrlNotAbsoluteForm,
    /// The URL contained whitespace, control characters, or exceeded
    /// [`MAX_TARGET_BYTES`].
    UrlForbiddenChar,
    /// The URL carried a userinfo (`user@host`) component.
    UrlUserinfo,
    /// The URL carried an empty host.
    UrlMissingHost,
    /// The URL exceeded [`MAX_TARGET_BYTES`].
    UrlTooLong,
    /// The URL scheme is outside the registration policy allowlist.
    PolicySchemeNotAllowed,
    /// The file target looks executable or scriptable and refuses as a
    /// policy violation (defense in depth against direct process
    /// execution through the default-handler path).
    PolicyExecutableTarget,
}

impl fmt::Display for LaunchConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAnObject => "launch params must be a JSON object",
            Self::UnexpectedField => "launch params must carry exactly the single declared field",
            Self::IdentityWrongType => "'identity' must be a string",
            Self::IdentityEmpty => "application identity must not be empty",
            Self::IdentityInvalidChar => "application identity must match [a-z0-9][a-z0-9._-]*",
            Self::IdentityTooLong => "application identity exceeds the token limit",
            Self::PathWrongType => "'path' must be a string",
            Self::PathNotAbsolute => "file target must be an absolute path",
            Self::PathTraversal => "file target must not contain '..' components",
            Self::PathEmptySegment => "file target must not contain empty or '.' components",
            Self::PathTrailingSeparator => "file target must not end in a separator",
            Self::PathDeviceNamespace => "file target must not use the device namespace",
            Self::PathTooLong => "file target exceeds the token limit",
            Self::PathInvalidChar => "file target contains forbidden characters",
            Self::UrlWrongType => "'url' must be a string",
            Self::UrlNotAbsoluteForm => "URL target must be absolute 'scheme://...' form",
            Self::UrlForbiddenChar => "URL target contains whitespace or forbidden characters",
            Self::UrlUserinfo => "URL target must not carry a userinfo component",
            Self::UrlMissingHost => "URL target must carry a non-empty host",
            Self::UrlTooLong => "URL target exceeds the token limit",
            Self::PolicySchemeNotAllowed => "URL scheme is outside the policy allowlist",
            Self::PolicyExecutableTarget => {
                "file target looks executable and refuses under launch policy"
            }
        })
    }
}

impl std::error::Error for LaunchConfigError {}

#[cfg(test)]
mod tests {
    use super::{
        ACTION_TYPE_LAUNCH_APPLICATION, ACTION_TYPE_LAUNCH_FILE, ACTION_TYPE_LAUNCH_URL,
        ApplicationTarget, FileTarget, LaunchBinding, LaunchConfigError, LaunchPolicy,
        MAX_IDENTITY_TOKEN_BYTES, MAX_TARGET_BYTES, UrlScheme, UrlTarget, parse_application_params,
        parse_file_params, parse_url_params,
    };
    use serde_json::{Value, json};

    #[test]
    fn application_targets_accept_the_declared_grammar() {
        for raw in ["obs-studio", "a", "0", "app.v2_x-beta", "7zip"] {
            let target = ApplicationTarget::try_new(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(target.capability_identity(), raw);
            let parsed =
                parse_application_params(&json!({ "identity": raw })).expect("parser parity");
            assert_eq!(parsed, target);
        }
    }

    #[test]
    fn application_targets_reject_structural_violations() {
        let cases = [
            (json!({ "identity": "" }), LaunchConfigError::IdentityEmpty),
            (
                json!({ "identity": "-lead" }),
                LaunchConfigError::IdentityInvalidChar,
            ),
            (
                json!({ "identity": "has space" }),
                LaunchConfigError::IdentityInvalidChar,
            ),
            (
                json!({ "identity": "UPPER" }),
                LaunchConfigError::IdentityInvalidChar,
            ),
            (
                json!({ "identity": "a/b" }),
                LaunchConfigError::IdentityInvalidChar,
            ),
            (
                json!({ "identity": "a:b" }),
                LaunchConfigError::IdentityInvalidChar,
            ),
            (
                json!({ "identity": "\u{0}" }),
                LaunchConfigError::IdentityInvalidChar,
            ),
            (
                json!({ "identity": &format!("a{}", "b".repeat(MAX_IDENTITY_TOKEN_BYTES)) }),
                LaunchConfigError::IdentityTooLong,
            ),
        ];
        for (params, expected) in cases {
            assert_eq!(
                parse_application_params(&params).unwrap_err(),
                expected,
                "case {params}"
            );
        }
    }

    #[test]
    fn file_targets_accept_absolute_forms_only() {
        for raw in [
            "C:\\shows\\notes.md",
            "c:/shows/notes.md",
            "\\\\server\\share\\cue.txt",
            "/home/stage/cue.txt",
            "Z:\\a.b.c",
        ] {
            let target = FileTarget::try_new(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(target.capability_identity(), raw);
            assert_eq!(parse_file_params(&json!({ "path": raw })).unwrap(), target);
        }
    }

    #[test]
    fn file_targets_reject_traversal_relative_and_device_paths() {
        let cases: Vec<(Value, LaunchConfigError)> = vec![
            (
                json!({ "path": "relative.txt" }),
                LaunchConfigError::PathNotAbsolute,
            ),
            (json!({ "path": "" }), LaunchConfigError::PathNotAbsolute),
            (
                json!({ "path": "C:\\shows\\..\\secrets.txt" }),
                LaunchConfigError::PathTraversal,
            ),
            (
                json!({ "path": "/stage/../etc/passwd" }),
                LaunchConfigError::PathTraversal,
            ),
            (
                json!({ "path": "C:\\shows\\\\notes.md" }),
                LaunchConfigError::PathEmptySegment,
            ),
            (
                json!({ "path": "C:\\shows\\." }),
                LaunchConfigError::PathEmptySegment,
            ),
            (
                json!({ "path": "C:\\shows\\" }),
                LaunchConfigError::PathTrailingSeparator,
            ),
            (
                json!({ "path": "\\\\.\\PhysicalDrive0" }),
                LaunchConfigError::PathDeviceNamespace,
            ),
            (
                json!({ "path": "\\\\?\\C:\\show.txt" }),
                LaunchConfigError::PathDeviceNamespace,
            ),
            (
                json!({ "path": &format!("C:\\{}", "a".repeat(MAX_TARGET_BYTES)) }),
                LaunchConfigError::PathTooLong,
            ),
            (
                json!({ "path": "C:\\a|b.txt" }),
                LaunchConfigError::PathInvalidChar,
            ),
            (
                json!({ "path": "C:\u{0}\\a.txt" }),
                LaunchConfigError::PathInvalidChar,
            ),
        ];
        for (params, expected) in cases {
            assert_eq!(
                parse_file_params(&params).unwrap_err(),
                expected,
                "case {params}"
            );
        }
    }

    #[test]
    fn url_targets_require_absolute_authority_form() {
        for raw in [
            "https://example.com/live",
            "HTTP://Example.COM:8080/dashboard",
            "https://example.com/path?a=b#cue",
            "http://127.0.0.1:8000/",
        ] {
            let target = UrlTarget::try_new(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(target.capability_identity(), raw);
            assert_eq!(parse_url_params(&json!({ "url": raw })).unwrap(), target);
        }
        let mixed = UrlTarget::try_new("HTTPS://Host.example/x").expect("valid");
        assert_eq!(mixed.scheme(), "HTTPS");
        assert_eq!(mixed.host(), "Host.example");
    }

    #[test]
    fn url_targets_reject_structural_violations() {
        let cases = [
            (
                json!({ "url": "example.com/live" }),
                LaunchConfigError::UrlNotAbsoluteForm,
            ),
            (
                json!({ "url": "//example.com/live" }),
                LaunchConfigError::UrlNotAbsoluteForm,
            ),
            (
                json!({ "url": "https:/single" }),
                LaunchConfigError::UrlNotAbsoluteForm,
            ),
            (
                json!({ "url": "1https://example.com" }),
                LaunchConfigError::UrlNotAbsoluteForm,
            ),
            (
                json!({ "url": "we ird://example.com" }),
                LaunchConfigError::UrlForbiddenChar,
            ),
            (json!({ "url": "" }), LaunchConfigError::UrlNotAbsoluteForm),
            (
                json!({ "url": &format!("https://h/{}", "a".repeat(MAX_TARGET_BYTES)) }),
                LaunchConfigError::UrlTooLong,
            ),
            (
                json!({ "url": "https://user:pw@example.com" }),
                LaunchConfigError::UrlUserinfo,
            ),
            (
                json!({ "url": "https://user@example.com" }),
                LaunchConfigError::UrlUserinfo,
            ),
            (
                json!({ "url": "https:///path" }),
                LaunchConfigError::UrlMissingHost,
            ),
            (
                json!({ "url": "https://ex ample.com" }),
                LaunchConfigError::UrlForbiddenChar,
            ),
            (
                json!({ "url": "https://example.com/\n" }),
                LaunchConfigError::UrlForbiddenChar,
            ),
        ];
        for (params, expected) in cases {
            assert_eq!(
                parse_url_params(&params).unwrap_err(),
                expected,
                "case {params}"
            );
        }
    }

    #[test]
    fn malformed_param_objects_reject_for_every_kind() {
        for params in [
            json!("x"),
            json!([]),
            json!(null),
            json!({}),
            json!({ "identity": "a", "extra": 1 }),
            json!({ "identity": 5 }),
            json!({ "path": 5 }),
            json!({ "url": true }),
            json!({ "keys": "ctrl+a" }),
        ] {
            assert!(
                parse_application_params(&params).is_err()
                    && parse_file_params(&params).is_err()
                    && parse_url_params(&params).is_err(),
                "garbage must reject for every kind: {params}"
            );
        }
        assert_eq!(
            parse_application_params(&json!({ "path": "C:\\x" })).unwrap_err(),
            LaunchConfigError::UnexpectedField
        );
    }

    #[test]
    fn policy_enforces_closed_scheme_vocabulary_and_executable_denylist() {
        let https_only = LaunchPolicy::standard();
        assert_eq!(https_only.url_schemes(), [UrlScheme::Https]);

        let ok = UrlTarget::try_new("https://example.com/live").unwrap();
        assert_eq!(https_only.check_url(&ok), Ok(()));
        // Scheme comparison is case-insensitive per RFC 3986.
        let upper = UrlTarget::try_new("HTTPS://example.com/live").unwrap();
        assert_eq!(https_only.check_url(&upper), Ok(()));

        let http = UrlTarget::try_new("http://example.com/live").unwrap();
        assert_eq!(
            https_only.check_url(&http),
            Err(LaunchConfigError::PolicySchemeNotAllowed)
        );

        let widened = LaunchPolicy::new(&[UrlScheme::Http, UrlScheme::Http, UrlScheme::Https]);
        assert_eq!(widened.url_schemes(), [UrlScheme::Http, UrlScheme::Https]);
        assert_eq!(widened.check_url(&http), Ok(()));
        assert_eq!(widened.check_url(&ok), Ok(()));

        let empty = LaunchPolicy::new(&[]);
        assert_eq!(
            empty.check_url(&ok),
            Err(LaunchConfigError::PolicySchemeNotAllowed)
        );
    }

    #[test]
    fn policy_denies_executable_like_file_targets() {
        let policy = LaunchPolicy::standard();
        for blocked in [
            "C:\\tools\\run.exe",
            "C:\\tools\\RUN.EXE",
            "\\\\server\\share\\setup.msi",
            "/opt/tools/job.sh.btm.cmd",
            "C:\\bypass.PS1",
            "/home/u/drop.hta",
        ] {
            let target = FileTarget::try_new(blocked).expect("structurally valid");
            assert_eq!(
                policy.check_file(&target),
                Err(LaunchConfigError::PolicyExecutableTarget),
                "{blocked} must refuse"
            );
        }
        for allowed in [
            "C:\\shows\\notes.md",
            "/home/stage/cue.txt",
            "\\\\server\\share\\cue.txt",
            "C:\\docs\\readme",
        ] {
            let target = FileTarget::try_new(allowed).expect("structurally valid");
            assert_eq!(policy.check_file(&target), Ok(()), "{allowed}");
        }
    }

    #[test]
    fn bindings_carry_exact_capabilities_and_action_types() {
        let app = LaunchBinding::Application(ApplicationTarget::try_new("obs-studio").unwrap());
        assert_eq!(app.action_type(), ACTION_TYPE_LAUNCH_APPLICATION);
        assert_eq!(
            app.capability().to_string(),
            "os.application.launch:identity=obs-studio"
        );

        let file = LaunchBinding::File(FileTarget::try_new("C:\\shows\\notes.md").expect("valid"));
        assert_eq!(file.action_type(), ACTION_TYPE_LAUNCH_FILE);
        assert_eq!(
            file.capability().to_string(),
            "os.application.launch:identity=C:\\shows\\notes.md"
        );

        let url =
            LaunchBinding::Url(UrlTarget::try_new("https://example.com/live").expect("valid"));
        assert_eq!(url.action_type(), ACTION_TYPE_LAUNCH_URL);
        assert_eq!(
            url.capability().to_string(),
            "os.application.launch:identity=https://example.com/live"
        );
    }
}
