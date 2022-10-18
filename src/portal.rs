use std::collections::HashMap;
use zbus::{dbus_proxy, Result};
use zbus::zvariant::{Fd, OwnedValue, Value};

pub type Options<'a> = HashMap<&'static str, Value<'a>>;

#[dbus_proxy(
    interface = "org.freedesktop.portal.Request",
    default_service = "org.freedesktop.portal.Desktop"
)]
pub trait Request {
    #[dbus_proxy(signal)]
    fn response(&self, code: u32, results: HashMap<String, OwnedValue>);
}

#[dbus_proxy(
    interface = "org.freedesktop.portal.Session",
    default_service = "org.freedesktop.portal.Desktop"
)]
pub trait Session {}

#[dbus_proxy(
    interface = "org.freedesktop.portal.ScreenCast",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
pub trait DesktopScreenCast {
    #[dbus_proxy(object = "Request")]
    fn create_session(&self, options: &Options<'_>);
    #[dbus_proxy(object = "Request")]
    fn select_sources(&self, s: &SessionProxy<'_>, options: &Options<'_>);
    #[dbus_proxy(object = "Request")]
    fn start(
        &self,
        s: &SessionProxy<'_>,
        parent_window: &str,
        options: &Options<'_>
    );
    fn open_pipe_wire_remote(
        &self,
        s: &SessionProxy<'_>,
        options: &Options<'_>
    ) -> Result<Fd>;
}
