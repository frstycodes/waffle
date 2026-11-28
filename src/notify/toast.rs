// Notifications through notify-rust: Windows toasts under Waffle's own AppUserModelID, and the freedesktop
// notification service elsewhere.

// Nothing to set up: each notification listens for its own click.
pub fn install() {}

pub fn post(title: &str, body: &str, link: String) {
    let (title, body) = (title.to_owned(), body.to_owned());
    // Each notification waits for its click on its own thread, which ends once it's clicked or dismissed.
    std::thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification.summary(&title).body(&body);
        #[cfg(target_os = "windows")]
        {
            aumid::register();
            notification.app_id(super::APP_ID);
        }
        #[cfg(not(target_os = "windows"))]
        notification.action("default", "Open");
        let Ok(handle) = notification.show() else { return };
        // A body click is `Default` on Windows; freedesktop servers report it as the "default" action.
        #[cfg(target_os = "windows")]
        let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
            if *response == notify_rust::NotificationResponse::Default {
                super::clicked(link);
            }
        });
        #[cfg(not(target_os = "windows"))]
        handle.wait_for_action(|action| {
            if action == "default" {
                super::clicked(link);
            }
        });
    });
}

#[cfg(target_os = "windows")]
mod aumid {
    use std::sync::Once;

    use ::windows::{
        Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW},
        core::{HSTRING, PCWSTR},
    };

    // Toasts from an unpackaged app show the name and icon registered under its AppUserModelID; without
    // one, Windows drops them, which is why notify-rust's default borrows PowerShell's.
    pub fn register() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let icon = crate::store::data_dir().map(|dir| dir.join("notification-icon.png"));
            let icon = icon.filter(|path| std::fs::create_dir_all(path.parent().unwrap_or(path)).is_ok() && std::fs::write(path, crate::settings::Prefs::load().app_icon.bytes()).is_ok());
            let key = format!(r"Software\Classes\AppUserModelId\{}", super::super::APP_ID);
            let mut values = vec![("DisplayName", "Waffle".to_owned())];
            values.extend(icon.map(|path| ("IconUri", path.to_string_lossy().into_owned())));
            // SAFETY: plain registry calls on our own key, closed below.
            unsafe {
                let mut handle = HKEY::default();
                if RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut handle, None).is_err() {
                    return;
                }
                for (name, value) in values {
                    let wide: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
                    let _ = RegSetValueExW(handle, &HSTRING::from(name), None, REG_SZ, Some(&wide));
                }
                let _ = RegCloseKey(handle);
            }
        });
    }
}
