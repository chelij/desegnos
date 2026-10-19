use std::{
    ffi::{OsStr, OsString},
    io,
    os::unix::process::CommandExt,
    path::Path,
    process::Command,
};

const EXPLICIT_SYNC: &str = "__NV_DISABLE_EXPLICIT_SYNC";

fn needs_workaround(
    nvidia_loaded: bool,
    backend: Option<&OsStr>,
    wayland_display: Option<&OsStr>,
    wayland_socket: Option<&OsStr>,
    explicit_sync: Option<&OsStr>,
) -> bool {
    let wayland_allowed = backend.is_none_or(|value| {
        value.to_str().is_some_and(|backends| {
            backends
                .split(',')
                .any(|backend| matches!(backend.trim(), "wayland" | "*"))
        })
    });
    nvidia_loaded
        && wayland_allowed
        && [wayland_display, wayland_socket]
            .into_iter()
            .flatten()
            .any(|value| !value.is_empty())
        && explicit_sync.is_none()
}

fn compatibility_command(
    executable: &Path,
    arguments: impl IntoIterator<Item = OsString>,
) -> Command {
    let mut command = Command::new(executable);
    command.args(arguments).env(EXPLICIT_SYNC, "1");
    command
}

pub fn prepare() -> io::Result<()> {
    if needs_workaround(
        Path::new("/sys/module/nvidia").is_dir(),
        std::env::var_os("GDK_BACKEND").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_SOCKET").as_deref(),
        std::env::var_os(EXPLICIT_SYNC).as_deref(),
    ) {
        // WebKit bug 324551: NVIDIA explicit sync rejects GTK's first shared-memory frame.
        // Re-exec before GTK starts; avoid mutating the environment of a threaded process.
        return Err(
            compatibility_command(&std::env::current_exe()?, std::env::args_os().skip(1)).exec(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_nvidia_wayland_without_a_user_override_needs_compatibility() {
        let cases = [
            (true, Some("wayland"), Some("wayland-1"), None, None, true),
            (true, None, Some("wayland-1"), None, None, true),
            (
                true,
                Some("x11,wayland"),
                Some("wayland-1"),
                None,
                None,
                true,
            ),
            (true, Some("*"), None, Some("3"), None, true),
            (false, Some("wayland"), Some("wayland-1"), None, None, false),
            (true, Some("x11"), Some("wayland-1"), None, None, false),
            (true, Some("broadway"), Some("wayland-1"), None, None, false),
            (true, None, None, None, None, false),
            (true, None, Some(""), Some(""), None, false),
            (true, None, Some("wayland-1"), None, Some("0"), false),
            (true, None, Some("wayland-1"), None, Some("1"), false),
            (true, None, Some("wayland-1"), None, Some(""), false),
        ];
        for (nvidia, backend, display, socket, setting, expected) in cases {
            assert_eq!(
                needs_workaround(
                    nvidia,
                    backend.map(OsStr::new),
                    display.map(OsStr::new),
                    socket.map(OsStr::new),
                    setting.map(OsStr::new),
                ),
                expected,
                "Unexpected startup policy for backend {backend:?}, display {display:?}, socket {socket:?}, override {setting:?}"
            );
        }
    }

    #[test]
    fn relaunch_preserves_arguments_and_changes_only_the_child_environment() {
        let original = std::env::var_os(EXPLICIT_SYNC);
        let status = compatibility_command(
            Path::new("/bin/sh"),
            [
                "-c",
                "test \"$__NV_DISABLE_EXPLICIT_SYNC\" = 1 && test \"$1\" = 'argument with spaces'",
                "desegni-startup-check",
                "argument with spaces",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .status()
        .expect("The private compatibility child must start");
        assert!(status.success());
        assert_eq!(std::env::var_os(EXPLICIT_SYNC), original);
        assert!(!needs_workaround(
            true,
            Some(OsStr::new("wayland")),
            Some(OsStr::new("wayland-1")),
            None,
            Some(OsStr::new("1")),
        ));
    }
}
