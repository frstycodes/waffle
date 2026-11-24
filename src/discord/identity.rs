// What Waffle tells Discord about itself: the fields Discord's own client fills in its IDENTIFY and its
// X-Super-Properties header, with true values. Waffle names itself and its version; it never claims to
// be Discord's client or a browser. The same details go out on the gateway, every REST request and
// sign-in, so they never contradict each other.
//
// Developer settings can switch to minimal: what Waffle sent before (os, browser and device at IDENTIFY,
// no identity headers on requests), to compare the two on real accounts. Sign-in keeps Waffle's user agent.
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use base64::Engine;

use super::*;

// The language Waffle's interface is in, which is what Discord's X-Discord-Locale names.
const UI_LOCALE: &str = "en-US";

// Full unless Developer settings turned it off. Requests follow it at once; IDENTIFY at the next connect.
static FULL: AtomicBool = AtomicBool::new(true);

pub fn set_full_identity(on: bool) {
    FULL.store(on, Ordering::Relaxed);
}

pub struct Identity {
    full: Value,
    minimal: Value,
    pub user_agent: String,
    super_properties: String,
    timezone: String,
}

pub fn identity() -> &'static Identity {
    static IDENTITY: OnceLock<Identity> = OnceLock::new();
    IDENTITY.get_or_init(Identity::detect)
}

impl Identity {
    fn detect() -> Self {
        let os = match std::env::consts::OS {
            "macos" => "Mac OS X",
            "windows" => "Windows",
            "linux" => "Linux",
            other => other,
        };
        let os_version = os_version();
        let version = env!("CARGO_PKG_VERSION");
        let user_agent = format!("Waffle/{version} ({os} {os_version})").replace(" )", ")");
        let full = json!({
            "os": os,
            "os_version": os_version,
            "os_arch": std::env::consts::ARCH,
            "browser": "Waffle",
            "browser_version": version,
            "browser_user_agent": user_agent,
            "device": "",
            "system_locale": sys_locale::get_locale().unwrap_or_else(|| UI_LOCALE.to_owned()),
            "release_channel": "stable",
        });
        let minimal = json!({ "os": std::env::consts::OS, "browser": "Waffle", "device": "" });
        let super_properties = base64::engine::general_purpose::STANDARD.encode(full.to_string());
        let timezone = iana_time_zone::get_timezone().unwrap_or_default();
        Self { full, minimal, user_agent, super_properties, timezone }
    }

    // What IDENTIFY carries as `properties`.
    pub fn properties(&self) -> Value {
        self.properties_for(FULL.load(Ordering::Relaxed))
    }

    fn properties_for(&self, full: bool) -> Value {
        if full { self.full.clone() } else { self.minimal.clone() }
    }

    // The headers Discord's client puts on every API request, with Waffle's own values; none when minimal.
    pub fn headers(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.headers_for(req, FULL.load(Ordering::Relaxed))
    }

    fn headers_for(&self, req: reqwest::RequestBuilder, full: bool) -> reqwest::RequestBuilder {
        if !full {
            return req;
        }
        let req = req.header(reqwest::header::USER_AGENT, &self.user_agent).header("X-Super-Properties", &self.super_properties).header("X-Discord-Locale", UI_LOCALE);
        match self.timezone.is_empty() {
            true => req,
            false => req.header("X-Discord-Timezone", &self.timezone),
        }
    }
}

// The OS release, read once; empty where it can't be read.
fn os_version() -> String {
    let (program, args): (&str, &[&str]) = match std::env::consts::OS {
        "macos" => ("sw_vers", &["-productVersion"]),
        "linux" => ("uname", &["-r"]),
        _ => return String::new(),
    };
    std::process::Command::new(program).args(args).output().ok().and_then(|out| String::from_utf8(out.stdout).ok()).map(|v| v.trim().to_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_waffle_everywhere_it_identifies() {
        let id = Identity::detect();
        let properties = id.properties_for(true);
        assert_eq!(properties["browser"], "Waffle");
        assert_eq!(properties["browser_user_agent"], id.user_agent.as_str());
        assert!(id.user_agent.starts_with("Waffle/"));
        let decoded = base64::engine::general_purpose::STANDARD.decode(&id.super_properties).expect("base64");
        assert_eq!(serde_json::from_slice::<Value>(&decoded).expect("json"), properties, "the header says what IDENTIFY says");
    }

    #[test]
    fn minimal_sends_only_what_waffle_sent_before() {
        let id = Identity::detect();
        assert_eq!(id.properties_for(false), json!({ "os": std::env::consts::OS, "browser": "Waffle", "device": "" }));
        let req = id.headers_for(reqwest::Client::new().get("https://example.invalid"), false).build().expect("request");
        assert!(req.headers().is_empty(), "no identity headers");
    }
}
