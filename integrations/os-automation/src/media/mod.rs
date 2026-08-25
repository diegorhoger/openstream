//! Media transport and volume actions (issue #12) behind engine action
//! types [`ACTION_TYPE_MEDIA_TRANSPORT`] and [`ACTION_TYPE_AUDIO_VOLUME`],
//! scoped by the existing taxonomy rows `os.media.emit` and
//! `audio.control:<device>` respectively.
//!
//! Authority mapping (no new capability vocabulary):
//!
//! - Transport (`play_pause` / `next_track` / `previous_track`) scopes
//!   under the unqualified `os.media.emit` row: dispatching OS
//!   media-transport commands is exactly media-playback authority. The
//!   soundboard half of that row (audio rendered on an engine-managed
//!   audio path) remains future work and is not claimed here.
//! - Volume scopes under `audio.control:<device>` with exactly one
//!   declared device scope, `master` (the OS default render endpoint):
//!   step up/down within a bounded per-dispatch magnitude plus mute
//!   toggle. Any other device scope rejects at graph validation before
//!   grants are consulted, and the port refuses it defensively with
//!   [`port::CODE_DEVICE_SCOPE_UNSUPPORTED`] — a scoped request never
//!   silently degrades into master control.
//!
//! Hard rules: the adapters only ever *send* one-shot control events — no
//! audio capture, no session/process enumeration, no playback-state
//! polling or logging exists anywhere, and none may be added without a
//! security ADR and human gate.

pub mod backend;
pub mod port;
pub mod spec;

#[doc(inline)]
pub use crate::media::{
    backend::{
        FakeMediaBackend, MediaDeviceController, MediaError, MediaInvocation,
        UnsupportedMediaBackend, current_platform, platform_media_backend,
    },
    port::{
        ACTION_TYPE_AUDIO_VOLUME, ACTION_TYPE_MEDIA_TRANSPORT, AudioVolumePort,
        CODE_CAPABILITY_MISMATCH, CODE_DEVICE_SCOPE_UNSUPPORTED, CODE_INVALID_MEDIA_CONFIG,
        CODE_INVALID_VOLUME_CONFIG, CODE_PLATFORM_REFUSED, CODE_UNSUPPORTED_PLATFORM,
        MASTER_DEVICE_SCOPE, MediaTransportPort, register_media_actions,
    },
    spec::{
        MAX_MEDIA_TOKEN_BYTES, MAX_VOLUME_STEPS, MediaCommand, MediaConfigError, StepDirection,
        VolumeOperation, parse_media_params, parse_volume_params,
    },
};

/// Real Windows media/volume backend (SendInput-class synthesis of the
/// standard media and volume keys through the pinned audited `enigo`
/// wrapper). Present only on Windows; every other platform reports
/// [`UnsupportedMediaBackend`] through [`platform_media_backend`].
#[cfg(target_os = "windows")]
#[doc(inline)]
pub use crate::media::backend::WindowsMediaBackend;
