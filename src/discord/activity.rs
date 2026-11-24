// Discord's activities (embedded apps): running instances, the launcher's shelf, starting, joining and leaving one,
// the proxy ticket its frame carries, the OAuth an activity asks for, and activity posts in chats.
use serde_json::{Value, json};

use super::{API, CDN, Client, app_icon_url, parse_user};

// A running activity (Discord's embedded app) in a voice channel or call, from EMBEDDED_ACTIVITY_UPDATE_V2:
// who's in it, by user and gateway session. No participants means it ended.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivityInstance {
    pub application_id: String,
    pub launch_id: String,
    // What the activity's frame calls its instance: the composite id when Discord sends one.
    pub instance_id: String,
    pub location_id: String,
    pub channel_id: String,
    pub guild_id: Option<String>,
    pub participants: Vec<ActivityParticipant>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivityParticipant {
    pub user_id: String,
    pub session_id: String,
}

pub fn parse_activity_instance(d: &Value) -> Option<ActivityInstance> {
    let launch_id = d["launch_id"].as_str()?.to_owned();
    let location = &d["location"];
    Some(ActivityInstance {
        application_id: d["application_id"].as_str()?.to_owned(),
        instance_id: d["composite_instance_id"].as_str().unwrap_or(&launch_id).to_owned(),
        launch_id,
        location_id: location["id"].as_str()?.to_owned(),
        channel_id: location["channel_id"].as_str()?.to_owned(),
        guild_id: location["guild_id"].as_str().map(str::to_owned),
        participants: d["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some(ActivityParticipant { user_id: p["user_id"].as_str()?.to_owned(), session_id: p["session_id"].as_str().unwrap_or_default().to_owned() }))
            .collect(),
    })
}

// One app on the activity launcher (GET /activities/shelf), in Discord's shelf order.
#[derive(Clone, Debug, PartialEq)]
pub struct ShelfApp {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    // The app's cover art for its detail, and the wide background its stage tile sits on.
    pub cover: Option<String>,
    pub background: Option<String>,
    // How many can play; None when the app sets no limit.
    pub max_players: Option<u32>,
    // Runs on desktop (Discord's "web" platform); phone-only apps can't run here.
    pub desktop: bool,
    pub age_gate: bool,
    // Discord's NEW label, while it lasts.
    pub new: bool,
}

impl ShelfApp {
    // An app known only by id and name (not on the shelf we have): assume it runs here.
    pub fn bare(id: String, name: String, description: String, icon: Option<String>) -> Self {
        Self { id, name, description, icon, cover: None, background: None, max_players: None, desktop: true, age_gate: false, new: false }
    }
}

pub(super) fn parse_shelf(body: &Value) -> Vec<ShelfApp> {
    let apps = body["applications"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut ranked: Vec<(i64, ShelfApp)> = body["activities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a["application_id"].as_str()?;
            let app = apps.iter().find(|app| app["id"].as_str() == Some(id))?;
            let assets = body["assets"][id].as_array().map(Vec::as_slice).unwrap_or_default();
            let asset = |name: &str| assets.iter().find(|a| a["name"].as_str() == Some(name)).and_then(|a| a["id"].as_str()).map(|asset| app_asset_url(id, asset));
            let web = &a["client_platform_config"]["web"];
            let shelf = ShelfApp {
                id: id.to_owned(),
                name: app["name"].as_str()?.to_owned(),
                description: app["description"].as_str().unwrap_or_default().to_owned(),
                icon: app["icon"].as_str().map(|icon| app_icon_url(id, icon)),
                cover: asset("embedded_cover").or_else(|| asset("embedded_background")),
                background: asset("embedded_background").or_else(|| asset("embedded_cover")),
                max_players: app["max_participants"].as_u64().filter(|n| *n > 0).map(|n| n as u32),
                desktop: a["supported_platforms"].as_array().is_some_and(|p| p.iter().any(|p| p.as_str() == Some("web"))),
                age_gate: a["requires_age_gate"].as_bool().unwrap_or(false),
                new: web["label_type"].as_u64() == Some(1) && web["label_until"].as_str().is_none_or(|until| chrono::DateTime::parse_from_rfc3339(until).is_ok_and(|t| t > chrono::Utc::now())),
            };
            Some((a["shelf_rank"].as_i64().unwrap_or(i64::MAX), shelf))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, app)| app).collect()
}

// What an activity's AUTHORIZE asks for, as Discord's consent screen shows it.
#[derive(Clone, Debug)]
pub struct OAuthRequest {
    pub client_id: String,
    pub scopes: Vec<String>,
    pub state: String,
    pub app_name: String,
    // Already granted these scopes: Discord skips the consent screen when the app also asked for none.
    pub authorized: bool,
    pub integration_type: u8,
}

// An app's art (cover, background) at Retina size.
pub fn app_asset_url(app_id: &str, asset_id: &str) -> String {
    format!("{CDN}/app-assets/{app_id}/{asset_id}.png?size=1024")
}

impl Client {
    /// An app's named art (embedded_background, embedded_cover …) by asset id, for apps not on the shelf.
    pub async fn app_assets(&self, app_id: &str) -> Result<Vec<(String, String)>, String> {
        let body = self.run(self.request(crate::discord::Method::GET, &format!("/oauth2/applications/{app_id}/assets"))).await?;
        Ok(body.as_array().into_iter().flatten().filter_map(|a| Some((a["name"].as_str()?.to_owned(), a["id"].as_str()?.to_owned()))).collect())
    }

    /// The apps the activity launcher offers, for a server (None: a DM call).
    pub async fn activity_shelf(&self, guild_id: Option<&str>) -> Result<Vec<ShelfApp>, String> {
        let req = self.request(crate::discord::Method::GET, "/activities/shelf");
        let req = match guild_id {
            Some(id) => req.query(&[("guild_id", id)]),
            None => req,
        };
        Ok(parse_shelf(&self.run(req).await?))
    }

    /// Starts an activity as Discord's client does: it runs the app's launch command (its primary entry
    /// point) as an interaction in the channel. EMBEDDED_ACTIVITY_UPDATE_V2 then reports the instance.
    pub async fn start_activity(&self, app_id: &str, channel_id: &str, guild_id: Option<&str>, session_id: &str, nonce: &str) -> Result<(), String> {
        let index = self.run(self.request(crate::discord::Method::GET, &format!("/applications/{app_id}/application-command-index"))).await?;
        let command = index["application_commands"].as_array().into_iter().flatten().find(|c| c["type"].as_u64() == Some(4)).ok_or("This app has no way to start it")?;
        let body = json!({
            "type": 2,
            "application_id": app_id,
            "guild_id": guild_id,
            "channel_id": channel_id,
            "session_id": session_id,
            "nonce": nonce,
            "data": { "version": command["version"], "id": command["id"], "name": command["name"], "type": 4, "options": [], "application_command": command },
        });
        self.run(self.request(crate::discord::Method::POST, "/interactions").json(&body)).await.map(drop)
    }

    /// Joins an activity already running in the channel.
    pub async fn join_activity(&self, app_id: &str, channel_id: &str, guild_id: Option<&str>, session_id: &str) -> Result<(), String> {
        let body = json!({ "session_id": session_id, "guild_id": guild_id });
        self.run(self.request(crate::discord::Method::POST, &format!("/activities/{channel_id}/{app_id}")).json(&body)).await.map(drop)
    }

    pub async fn leave_activity(&self, instance: &ActivityInstance, session_id: &str) -> Result<(), String> {
        let path = format!("/applications/{}/activities/{}/instances/{}/leave", instance.application_id, instance.location_id, instance.launch_id);
        self.run(self.request(crate::discord::Method::POST, &path).json(&json!({ "session_id": session_id }))).await.map(drop)
    }

    /// The ticket the activity's frame carries so its proxy (`/.proxy/…`) lets it through.
    pub async fn activity_proxy_ticket(&self, app_id: &str, channel_id: &str) -> Result<String, String> {
        let body = json!({ "use_stateless_ticket": true, "channel_id": channel_id });
        let res = self.run(self.request(crate::discord::Method::POST, &format!("/applications/{app_id}/proxy-tickets")).json(&body)).await?;
        res["ticket"].as_str().map(str::to_owned).ok_or_else(|| "No proxy ticket".into())
    }

    /// What an activity's AUTHORIZE would grant, before asking: Discord's RPC asks the same GET first. A user
    /// install when the app supports one, else a server install, as Discord's client picks.
    pub async fn oauth_request(&self, client_id: &str, scopes: Vec<String>, state: String) -> Result<OAuthRequest, String> {
        let app = self.run(self.request(crate::discord::Method::GET, &format!("/applications/{client_id}/rpc"))).await.unwrap_or_default();
        let integration_type = if app["integration_types_config"]["1"].is_object() { 1 } else { 0 };
        let query = oauth_query(client_id, &scopes, &state, integration_type);
        let info = self.run(self.request(crate::discord::Method::GET, "/oauth2/authorize").query(&query)).await?;
        Ok(OAuthRequest {
            app_name: info["application"]["name"].as_str().unwrap_or("This app").to_owned(),
            authorized: info["authorized"].as_bool().unwrap_or(false),
            client_id: client_id.to_owned(),
            scopes,
            state,
            integration_type,
        })
    }

    /// Grants it: the authorization code the activity trades for its token.
    pub async fn oauth_authorize(&self, req: &OAuthRequest) -> Result<String, String> {
        let query = oauth_query(&req.client_id, &req.scopes, &req.state, req.integration_type);
        let body = json!({ "authorize": true, "permissions": "0", "integration_type": req.integration_type });
        let res = self.run(self.request(crate::discord::Method::POST, "/oauth2/authorize").query(&query).json(&body)).await?;
        let location = res["location"].as_str().ok_or("No authorization came back")?;
        oauth_code(location).ok_or_else(|| "No authorization code came back".into())
    }

    /// The user behind an activity's access token (AUTHENTICATE), with its scopes and expiry.
    pub async fn oauth_me(&self, access_token: &str) -> Result<Value, String> {
        self.run(super::transport::request(crate::discord::Method::GET, format!("{API}/oauth2/@me")).bearer_auth(access_token)).await
    }
}

// The activity a message posts. Not verified: `activity_instance` carries only the instance's id, as Discord's
// client reads it (web build 2026-11-23), and who started it rides on the launch interaction.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MessageActivity {
    pub instance_id: String,
    pub app_id: String,
    // The app as the post carries it, for a card whose app isn't on any shelf we've loaded.
    pub app_name: Option<String>,
    pub app_icon: Option<String>,
    pub starter: Option<String>,
}

pub(super) fn parse_message_activity(m: &Value) -> Option<MessageActivity> {
    let instance_id = m["activity_instance"]["id"].as_str()?.to_owned();
    let app_id = m["application_id"].as_str().or(m["application"]["id"].as_str())?.to_owned();
    let user = if m["interaction_metadata"]["user"].is_object() { &m["interaction_metadata"]["user"] } else { &m["interaction"]["user"] };
    let starter = user.is_object().then(|| parse_user(user).name);
    let app_name = m["application"]["name"].as_str().map(str::to_owned);
    let app_icon = m["application"]["icon"].as_str().map(|icon| app_icon_url(&app_id, icon));
    Some(MessageActivity { instance_id, app_id, app_name, app_icon, starter })
}

// Why Discord refused to start or join an activity (its REST error codes), in Waffle's words.
pub(super) fn launch_error(code: u64) -> Option<&'static str> {
    Some(match code {
        50106 => "You don't have access to it.",
        50107 => "It needs a server boost level this server doesn't have.",
        50148 => "You can't start activities in the AFK channel.",
        50165 => "It's for 18 and over.",
        50209 => "It's in preview, and this server is too big to try it.",
        50231 => "It doesn't run on desktop yet.",
        _ => return None,
    })
}

// Why an activity's launch interaction failed: the gateway's INTERACTION_FAILURE reason codes (Discord's
// client's ActivityLaunchReasonCodes, read from its web build 2026-11-23), in Waffle's words.
pub fn launch_failure(reason: u64) -> &'static str {
    match reason {
        3..=5 => "That activity isn't around any more.",
        6 | 14..=16 => "It doesn't run on desktop yet.",
        7 => "It isn't available to you yet.",
        8 | 19 => "It can't run in this kind of channel.",
        9 => "You can't start activities in the AFK channel.",
        10 => "It's in preview, and this server is too big to try it.",
        11 => "It's for 18 and over.",
        12 => "This server needs you verified before you can play here.",
        13 => "A moderator turned off Use Activities here.",
        18 => "You don't have access to it.",
        20 => "It isn't available where you are.",
        _ => "It didn't start. Try again in a moment.",
    }
}

fn oauth_query(client_id: &str, scopes: &[String], state: &str, integration_type: u8) -> Vec<(&'static str, String)> {
    vec![("client_id", client_id.to_owned()), ("response_type", "code".into()), ("scope", scopes.join(" ")), ("state", state.to_owned()), ("integration_type", integration_type.to_string())]
}

// The `code` in the redirect Discord answers an authorize with.
pub(super) fn oauth_code(location: &str) -> Option<String> {
    let query = location.split_once('?')?.1;
    query.split('&').find_map(|pair| pair.strip_prefix("code=")).map(str::to_owned)
}

