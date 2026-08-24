//! Launch actions (issue #11): application launch, file open, and URL
//! open behind engine action types
//! [`ACTION_TYPE_LAUNCH_APPLICATION`](spec::ACTION_TYPE_LAUNCH_APPLICATION),
//! [`ACTION_TYPE_LAUNCH_FILE`](spec::ACTION_TYPE_LAUNCH_FILE), and
//! [`ACTION_TYPE_LAUNCH_URL`](spec::ACTION_TYPE_LAUNCH_URL).
//!
//! All three kinds scope under the existing taxonomy row
//! `os.application.launch:<identity>`: opening a file or URL delegates to
//! the OS default-handler resolution, so every kind ultimately launches an
//! OS-selected handler for one user-approved target. Authority is per
//! exact target: bindings declare the approved identities at registration,
//! grants cover exactly one identity each, and the port recomputes the
//! target token from parameters before spawn — drift fails closed.
//!
//! Hard rules (SECURITY.md process-execution constraints): no shell
//! interpreter anywhere; no argument interpolation (application launches
//! carry zero arguments this milestone); clean environment, explicit
//! working directory, nulled standard handles on direct spawns; executable
//! revalidation (approved-map membership plus on-disk existence) before
//! every application launch; URL schemes restricted to a closed vocabulary
//! narrowed by a per-registration allowlist.

pub mod backend;
pub mod port;
pub mod spec;

#[doc(inline)]
pub use crate::launch::{
    backend::{
        FakeLaunchBackend, LaunchBackend, LaunchError, LaunchInvocation, UnsupportedLaunchBackend,
        current_platform, platform_launch_backend,
    },
    port::{
        CODE_CAPABILITY_MISMATCH, CODE_INVALID_CONFIG, CODE_MISSING_TARGET, CODE_PLATFORM_REFUSED,
        CODE_POLICY_REFUSED, CODE_UNSUPPORTED_PLATFORM, LaunchKind, LaunchPort,
        LaunchRegistrationError, register_launch_actions,
    },
    spec::{
        ACTION_TYPE_LAUNCH_APPLICATION, ACTION_TYPE_LAUNCH_FILE, ACTION_TYPE_LAUNCH_URL,
        ApplicationTarget, FileTarget, LaunchBinding, LaunchConfigError, LaunchPolicy,
        MAX_IDENTITY_TOKEN_BYTES, MAX_TARGET_BYTES, UrlScheme, UrlTarget, parse_application_params,
        parse_file_params, parse_url_params,
    },
};

/// Real Windows launch backend (CreateProcess-class spawns plus
/// ShellExecuteW-class default-handler opens through the pinned `open`
/// wrapper). Present only on Windows; every other platform reports
/// [`UnsupportedLaunchBackend`] through [`platform_launch_backend`].
#[cfg(target_os = "windows")]
#[doc(inline)]
pub use crate::launch::backend::WindowsLaunchBackend;
