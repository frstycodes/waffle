// What Waffle tells Discord about itself: Discord's web client in Chrome on Windows, the profile Dissent and
// Abaddon present. A client that names itself, or claims a rare platform, stands out to Discord's automation
// checks and puts the account at risk. IDENTIFY, X-Super-Properties, every REST request and sign-in carry
// the same details, so they never contradict each other.
//
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

use base64::Engine;

use super::*;

// The language Waffle's interface is in, which is what Discord's X-Discord-Locale names.
const UI_LOCALE: &str = "en-US";
// Chrome's reduced user agent freezes the OS and the minor version digits, so only the major moves.
// Bump with transport.rs's PROFILE, the newest Chrome it can imitate; Sec-Ch-Ua follows it.
const CHROME_MAJOR: u32 = 149;
// Used until the live build number is read at the first sign-in or connect, and when that read fails.
const FALLBACK_BUILD_NUMBER: u64 = 679_544;

static BUILD_NUMBER: AtomicU64 = AtomicU64::new(FALLBACK_BUILD_NUMBER);

pub struct Identity {
    common: Value,
    pub user_agent: String,
    brands: String,
    timezone: String,
    launch_signature: String,
    heartbeat_session_id: String,
}

pub fn identity() -> &'static Identity {
    static IDENTITY: OnceLock<Identity> = OnceLock::new();
    IDENTITY.get_or_init(Identity::detect)
}

// Reads the web client's current build number from Discord's login page, once per run, as Abaddon and Dissent
// do: a build number that falls behind marks a client that isn't Discord's. Await before the first IDENTIFY.
pub async fn load_build_number() {
    static LOADED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    LOADED
        .get_or_init(|| async {
            if let Some(build) = fetch_build_number().await {
                BUILD_NUMBER.store(build, Ordering::Relaxed);
            }
        })
        .await;
}

async fn fetch_build_number() -> Option<u64> {
    let req = transport::plain(transport::Method::GET, "https://discord.com/login").header(transport::header::USER_AGENT, &identity().user_agent);
    let page = req.timeout(std::time::Duration::from_secs(5)).send().await.ok()?.text().await.ok()?;
    parse_build_number(&page)
}

fn parse_build_number(page: &str) -> Option<u64> {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| regex::Regex::new(r#""BUILD_NUMBER":\s*"(\d+)""#).expect("the pattern is valid"));
    pattern.captures(page)?[1].parse().ok()
}

impl Identity {
    fn detect() -> Self {
        let version = format!("{CHROME_MAJOR}.0.0.0");
        let user_agent = format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{version} Safari/537.36");
        let common = json!({
            "os": "Windows",
            "browser": "Chrome",
            "device": "",
            "system_locale": sys_locale::get_locale().unwrap_or_else(|| UI_LOCALE.to_owned()),
            "has_client_mods": false,
            "browser_user_agent": user_agent,
            "browser_version": version,
            "os_version": "10",
            "referrer": "",
            "referring_domain": "",
            "referrer_current": "",
            "referring_domain_current": "",
            "release_channel": "stable",
            "client_event_source": null,
            "client_launch_id": uuid_v4(),
        });
        Self {
            common,
            user_agent,
            brands: brands(CHROME_MAJOR),
            timezone: iana_time_zone::get_timezone().unwrap_or_default(),
            launch_signature: launch_signature(),
            heartbeat_session_id: uuid_v4(),
        }
    }

    fn with(&self, extra: Value) -> Value {
        let mut properties = self.common.clone();
        properties["client_build_number"] = BUILD_NUMBER.load(Ordering::Relaxed).into();
        if let (Some(properties), Value::Object(extra)) = (properties.as_object_mut(), extra) {
            properties.extend(extra);
        }
        properties
    }

    // What IDENTIFY carries as `properties`.
    pub fn properties(&self) -> Value {
        self.with(json!({ "is_fast_connect": true }))
    }

    fn super_properties(&self) -> Value {
        self.with(json!({ "client_app_state": "focused", "client_heartbeat_session_id": self.heartbeat_session_id, "launch_signature": self.launch_signature }))
    }

    // The headers Chrome and Discord's web client put on every API request. The page's Referer is the caller's.
    pub fn headers(&self, req: transport::Request) -> transport::Request {
        let super_properties = base64::engine::general_purpose::STANDARD.encode(self.super_properties().to_string());
        let req = req
            .header(transport::header::USER_AGENT, &self.user_agent)
            .header(transport::header::ACCEPT, "*/*")
            .header(transport::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(transport::header::ORIGIN, "https://discord.com")
            .header("Priority", "u=1, i")
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-origin")
            .header("Sec-Ch-Ua", &self.brands)
            .header("Sec-Ch-Ua-Mobile", "?0")
            .header("Sec-Ch-Ua-Platform", "\"Windows\"")
            .header("X-Debug-Options", "bugReporterEnabled")
            .header("X-Discord-Locale", UI_LOCALE)
            .header("X-Super-Properties", super_properties);
        match self.timezone.is_empty() {
            true => req,
            false => req.header("X-Discord-Timezone", &self.timezone),
        }
    }
}

// Chrome's Sec-Ch-Ua for a major version: the GREASE brand's punctuation, its version and the order of the
// three brands all derive from the major (Chromium's user_agent_utils.cc, GetGreasedUserAgentBrandVersion).
fn brands(major: u32) -> String {
    const CHARS: [&str; 11] = [" ", "(", ":", "-", ".", "/", ")", ";", "=", "?", "_"];
    const ORDERS: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    let seed = major as usize;
    let list = [
        format!("\"Not{}A{}Brand\";v=\"{}\"", CHARS[seed % 11], CHARS[(seed + 1) % 11], ["8", "99", "24"][seed % 3]),
        format!("\"Chromium\";v=\"{major}\""),
        format!("\"Google Chrome\";v=\"{major}\""),
    ];
    let mut shuffled = [""; 3];
    for (brand, &at) in list.iter().zip(&ORDERS[seed % 6]) {
        shuffled[at] = brand;
    }
    shuffled.join(", ")
}

// A UUIDv4 with the bits Discord reads to detect client mods cleared (docs.discord.food, launch signature).
fn launch_signature() -> String {
    const MASK: [u8; 16] = [0xff, 0x7f, 0xef, 0xef, 0xf7, 0xef, 0xf7, 0xff, 0xdf, 0x7e, 0xff, 0xbf, 0xfe, 0xff, 0xf7, 0xff];
    let mut bytes = uuid_bytes();
    for (byte, mask) in bytes.iter_mut().zip(MASK) {
        *byte &= mask;
    }
    format_uuid(bytes)
}

fn uuid_v4() -> String {
    format_uuid(uuid_bytes())
}

fn uuid_bytes() -> [u8; 16] {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes
}

fn format_uuid(b: [u8; 16]) -> String {
    let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_and_the_header_agree_on_every_shared_field() {
        let id = Identity::detect();
        let identify = id.properties();
        assert_eq!(identify["browser"], "Chrome");
        assert_eq!(identify["browser_user_agent"], id.user_agent.as_str());
        assert!(!id.user_agent.contains("Waffle"));
        let header = id.super_properties();
        for (key, value) in id.common.as_object().expect("an object") {
            assert_eq!(&header[key], value, "{key}");
        }
        assert_eq!(identify["client_build_number"], header["client_build_number"]);
    }

    #[test]
    fn brands_match_what_chrome_sends() {
        assert_eq!(brands(131), r#""Google Chrome";v="131", "Chromium";v="131", "Not_A Brand";v="24""#);
    }

    #[test]
    fn reads_the_build_number_from_the_login_page() {
        assert_eq!(parse_build_number(r#"window.GLOBAL_ENV = {"BUILD_NUMBER":"679544","PROJECT_ENV":"prod"}"#), Some(679_544));
        assert_eq!(parse_build_number("<html></html>"), None);
    }

    #[test]
    fn launch_signature_keeps_the_mod_bits_clear() {
        let signature = launch_signature().replace('-', "");
        let bytes: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&signature[i * 2..i * 2 + 2], 16).expect("hex")).collect();
        assert_eq!(bytes[1] & 0x80, 0);
        assert_eq!(bytes[9] & 0x81, 0);
        assert_eq!(bytes[6] >> 4, 4, "still a v4 UUID");
    }
}
