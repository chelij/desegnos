//! Whether Linux file dialogs have a desktop portal to talk to.
//!
//! rfd's portal backend returns nothing when the portal is missing, which the editor cannot
//! tell apart from the user cancelling. [`available`] asks the session bus, which also starts
//! the portal when it is installed but not yet running.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Plain explanation shown when a file dialog could not open at all.
pub const MESSAGE: &str = "File dialogs need the desktop portal. Install xdg-desktop-portal and a backend such as xdg-desktop-portal-gtk, then try again.";

/// Short enough to notice a portal the user installs while the editor is running.
const CACHE: Duration = Duration::from_secs(2);

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
const CHOOSER: &str = "org.freedesktop.portal.FileChooser";

static CHECKED: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// True when the portal service answers and offers file choices.
pub fn available() -> bool {
    if let Ok(cache) = CHECKED.lock()
        && let Some((at, answer)) = *cache
        && at.elapsed() < CACHE
    {
        return answer;
    }
    let answer = ask();
    if let Ok(mut cache) = CHECKED.lock() {
        *cache = Some((Instant::now(), answer));
    }
    answer
}

fn ask() -> bool {
    use zbus::blocking::Proxy;
    use zbus::blocking::connection::Builder;
    // Goal: never hold startup or a command for long when the portal is absent or wedged.
    let connection = Builder::session()
        .and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build());
    let Ok(connection) = connection else {
        return false;
    };
    let Ok(proxy) = Proxy::new(&connection, PORTAL, OBJECT, CHOOSER) else {
        return false;
    };
    // Reading a property activates the service when it is installed but not running, and
    // proves the portal exposes file choices.
    proxy.get_property::<u32>("version").is_ok()
}
