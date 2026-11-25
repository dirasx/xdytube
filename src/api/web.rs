//! Browser sign-in: the YouTube website's own session cookies.
//!
//! The TV sign-in can read the account (feeds, history, likes), but YouTube no
//! longer lets it play videos, so nothing watched through it reaches the account.
//! With the website's cookies the app reports playback the way youtube.com does:
//! the video lands in the account's watch history and the position you stopped at
//! is what other devices resume from.
//!
//! Signing in happens in a real browser (Edge, Chrome, Brave or Chromium) started
//! with its own empty profile. The app reads that profile's youtube.com cookies over
//! the browser's local debugging port once sign-in is done, then closes the browser
//! and deletes the profile. Nothing else from the user's browsers is read.
use super::http;
use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, Permissions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

const WEB_VERSION: &str = "2.20251120.01.00";
static WATCH_LATER_LOCK: Mutex<()> = Mutex::new(());
const ORIGIN: &str = "https://www.youtube.com";
const SIGN_IN_URL: &str = "https://accounts.google.com/ServiceLogin?service=youtube&uilel=3&passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin%3Faction_handle_signin%3Dtrue%26app%3Ddesktop%26next%3Dhttps%253A%252F%252Fwww.youtube.com%252F";
pub const SIGN_IN_NETWORK_HINT: &str = "If Google sign-in is stuck or the connection closed, check your network. If you use a VPN, try another server and start Connect again.";

#[derive(Debug)]
pub struct SignInTimeout;

impl std::fmt::Display for SignInTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sign-in timed out.")
    }
}

impl std::error::Error for SignInTimeout {}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Session {
    cookies: BTreeMap<String, String>,
    account: Option<String>,
}

fn session() -> &'static Mutex<Option<Session>> {
    static S: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    S.get_or_init(|| {
        let s: Option<Session> = crate::credentials::load("web_session");
        Mutex::new(s.filter(|s| has_login(&s.cookies)))
    })
}

fn save(s: Option<&Session>) -> Result<()> {
    crate::credentials::save("web_session", s)
}

fn has_login(c: &BTreeMap<String, String>) -> bool {
    (c.contains_key("SAPISID") || c.contains_key("__Secure-3PAPISID")) && (c.contains_key("SID") || c.contains_key("__Secure-3PSID"))
}

/// Whether the browser sign-in is connected.
pub fn connected() -> bool {
    session().lock().is_some()
}

/// The account name the browser session belongs to.
pub fn account_name() -> Option<String> {
    session().lock().as_ref().and_then(|s| s.account.clone())
}

pub fn disconnect() -> Result<()> {
    let _edit = WATCH_LATER_LOCK.lock();
    let mut s = session().lock();
    save(None)?;
    *s = None;
    Ok(())
}

/// Stores a set of youtube.com cookies and checks they belong to a signed-in account.
fn connect(cookies: BTreeMap<String, String>) -> Result<String> {
    let _edit = WATCH_LATER_LOCK.lock();
    if !has_login(&cookies) {
        return Err(anyhow!("These cookies aren't from a signed-in YouTube session."));
    }
    {
        let mut s = session().lock();
        *s = Some(Session { cookies, account: None });
    }
    let name = match fetch_account_name() {
        Ok(n) => n,
        Err(e) => {
            *session().lock() = None;
            return Err(e.context("YouTube didn't accept the session"));
        }
    };
    let mut s = session().lock();
    if let Some(st) = s.as_mut() {
        st.account = Some(name.clone());
        if let Err(error) = save(Some(st)) {
            *s = None;
            return Err(error);
        }
    } else {
        return Err(anyhow!("The YouTube history connection was cancelled."));
    }
    Ok(name)
}

fn cookie_header() -> Option<String> {
    let s = session().lock();
    let s = s.as_ref()?;
    Some(s.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "))
}

fn sha1_hex(s: &str) -> String {
    use sha1::{Digest, Sha1};
    Sha1::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The `Authorization` header youtube.com sends with its API calls.
fn sapisid_auth() -> Option<String> {
    let s = session().lock();
    let c = &s.as_ref()?.cookies;
    let ts = chrono::Utc::now().timestamp();
    let parts: Vec<String> = [("SAPISIDHASH", "SAPISID"), ("SAPISID1PHASH", "__Secure-1PAPISID"), ("SAPISID3PHASH", "__Secure-3PAPISID")]
        .iter()
        .filter_map(|(scheme, name)| {
            let v = c.get(*name).or_else(|| if *name == "SAPISID" { c.get("__Secure-3PAPISID") } else { None })?;
            Some(format!("{scheme} {ts}_{}", sha1_hex(&format!("{ts} {v} {ORIGIN}"))))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A call to the YouTube website's API as the signed-in user.
pub fn web_call(endpoint: &str, mut body: Value) -> Result<Value> {
    let cookie = cookie_header().ok_or_else(|| anyhow!("browser sign-in not connected"))?;
    let auth = sapisid_auth().ok_or_else(|| anyhow!("browser sign-in not connected"))?;
    let st = crate::store::settings_snapshot();
    body["context"] = json!({ "client": { "clientName": "WEB", "clientVersion": WEB_VERSION, "hl": st.language, "gl": st.region } });
    let (status, v) = http::post_json_status(
        &format!("https://www.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false"),
        &body,
        &[
            ("Cookie", &cookie),
            ("Authorization", &auth),
            ("Origin", ORIGIN),
            ("X-Origin", ORIGIN),
            ("Referer", "https://www.youtube.com/"),
            ("X-Goog-AuthUser", "0"),
            ("X-Youtube-Bootstrap-Logged-In", "true"),
            ("X-Youtube-Client-Name", "1"),
            ("X-Youtube-Client-Version", WEB_VERSION),
        ],
    )?;
    if status == 401 || status == 403 {
        return Err(anyhow!("YouTube refused the browser session (HTTP {status}). Connect it again."));
    }
    if status >= 400 {
        return Err(anyhow!("HTTP {status}"));
    }
    Ok(v)
}

fn watch_later_add_request(video_id: &str) -> Value {
    json!({
        "playlistId": "WL",
        "actions": [{ "action": "ACTION_ADD_VIDEO", "addedVideoId": video_id }]
    })
}

fn playlist_entry_id(page: &Value, video_id: &str) -> Option<String> {
    let mut videos = Vec::new();
    crate::api::json::find_all(page, "playlistVideoRenderer", &mut videos);
    videos.into_iter().find_map(|video| {
        (video.get("videoId").and_then(Value::as_str) == Some(video_id))
            .then(|| video.get("setVideoId").and_then(Value::as_str).map(str::to_string))
            .flatten()
    })
}

fn watch_later_entry(video_id: &str) -> Result<Option<String>> {
    let mut page = web_call("browse", json!({ "browseId": "VLWL" }))?;
    if page.get("error").is_some() || crate::api::json::find(&page, "playlistVideoListRenderer").is_none() {
        return Err(anyhow!("YouTube did not return the Watch later playlist."));
    }
    let mut seen = std::collections::HashSet::new();
    loop {
        if let Some(id) = playlist_entry_id(&page, video_id) {
            return Ok(Some(id));
        }
        let mut continuations = Vec::new();
        crate::api::json::find_all(&page, "continuationItemRenderer", &mut continuations);
        let Some(token) = continuations.into_iter().find_map(crate::api::innertube::continuation_token) else {
            return Ok(None);
        };
        if !seen.insert(token.clone()) {
            return Err(anyhow!("YouTube returned a repeated playlist page."));
        }
        page = web_call("browse", json!({ "continuation": token }))?;
    }
}

fn session_key() -> Option<String> {
    use sha2::{Digest, Sha256};
    let session = session().lock();
    let cookies = &session.as_ref()?.cookies;
    let bytes = serde_json::to_vec(cookies).ok()?;
    Some(Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect())
}

fn session_matches(current: Option<&str>, expected: &str) -> bool {
    current.is_some_and(|current| current == expected)
}

fn parse_watch_later_page(response: &Value) -> Result<crate::api::types::PlaylistPage> {
    use crate::api::types::{Item, PlaylistItem, PlaylistPage};
    if response.get("error").is_some() || crate::api::json::find(response, "playlistVideoListRenderer").is_none() {
        return Err(anyhow!("YouTube did not return Watch later for the connected history account."));
    }
    let page = crate::api::innertube::collect_items(response);
    let videos: Vec<_> = page.items.into_iter().filter_map(|item| match item { Item::Video(video) => Some(video), _ => None }).collect();
    let info = PlaylistItem { id: "WL".into(), title: "Watch later".into(), channel_name: account_name(), thumbnail: videos.first().map(|v| v.thumbnail.clone()), ..Default::default() };
    Ok(PlaylistPage { info, videos, continuation: page.continuation, description: None })
}

pub fn watch_later_playlist() -> Result<(crate::api::types::PlaylistPage, String)> {
    let _edit = WATCH_LATER_LOCK.lock();
    let key = session_key().ok_or_else(|| anyhow!("Connect YouTube watch history in Settings to view Watch later."))?;
    let page = web_call("browse", json!({"browseId": "VLWL"}))?;
    Ok((parse_watch_later_page(&page)?, key))
}

pub fn watch_later_continue(token: &str, expected: &str) -> Result<crate::api::types::Page> {
    let _edit = WATCH_LATER_LOCK.lock();
    if !session_matches(session_key().as_deref(), expected) { return Err(anyhow!("YouTube history account changed; reopen Watch later.")); }
    let response = web_call("browse", json!({"continuation": token}))?;
    if response.get("error").is_some() { return Err(anyhow!("YouTube could not load the next Watch later page.")); }
    Ok(watch_later_continuation_page(&response))
}

fn watch_later_continuation_page(response: &Value) -> crate::api::types::Page {
    crate::api::innertube::collect_items(response)
}

/// Adds or removes one item from the YouTube Watch later playlist.
pub fn set_watch_later(video_id: &str, add: bool) -> Result<()> {
    set_watch_later_checked(video_id, add, None)
}

pub fn remove_watch_later_in_session(video_id: &str, expected: &str) -> Result<()> {
    set_watch_later_checked(video_id, false, Some(expected))
}

fn set_watch_later_checked(video_id: &str, add: bool, expected: Option<&str>) -> Result<()> {
    let _edit = WATCH_LATER_LOCK.lock();
    if let Some(expected) = expected {
        if !session_matches(session_key().as_deref(), expected) { return Err(anyhow!("YouTube history account changed; reopen Watch later.")); }
    }
    if video_id.is_empty() {
        return Err(anyhow!("The video has no ID."));
    }
    let existing = watch_later_entry(video_id)?;
    let request = match (add, existing) {
        (true, None) => watch_later_add_request(video_id),
        (false, Some(entry_id)) => json!({
            "playlistId": "WL",
            "actions": [{ "action": "ACTION_REMOVE_VIDEO", "setVideoId": entry_id }]
        }),
        _ => return Ok(()),
    };
    let result = web_call("browse/edit_playlist", request)?;
    if result.get("error").is_some() {
        return Err(anyhow!("YouTube could not update Watch later."));
    }
    let present = watch_later_entry(video_id)?.is_some();
    if present != add {
        return Err(anyhow!("YouTube did not confirm the Watch later change."));
    }
    Ok(())
}

fn fetch_account_name() -> Result<String> {
    let r = web_call("account/account_menu", json!({}))?;
    let header = crate::api::json::find(&r, "activeAccountHeaderRenderer").ok_or_else(|| anyhow!("not signed in"))?;
    let name = header.get("accountName").map(crate::api::json::text).filter(|s| !s.is_empty());
    let handle = header.get("channelHandle").map(crate::api::json::text).filter(|s| !s.is_empty());
    name.or(handle).ok_or_else(|| anyhow!("not signed in"))
}

/// Regular videos of a page, without the Shorts shelf (its items carry no channel or length).
fn long_videos(p: crate::api::types::Page) -> crate::api::types::Page {
    use crate::api::types::Item;
    let mut seen = std::collections::HashSet::new();
    crate::api::types::Page {
        items: p.items.into_iter().filter(|i| matches!(i, Item::Video(v) if !v.is_short && !v.channel_name.is_empty() && seen.insert(v.id.clone()))).collect(),
        continuation: p.continuation,
    }
}

/// The account's own home feed, the same one youtube.com shows.
pub fn home() -> Result<crate::api::types::Page> {
    let r = web_call("browse", json!({ "browseId": "FEwhat_to_watch" }))?;
    Ok(long_videos(crate::api::innertube::collect_items(&r)))
}

/// Next page of a feed loaded with the browser session.
pub fn browse_continue(token: &str) -> Result<crate::api::types::Page> {
    let r = web_call("browse", json!({ "continuation": token }))?;
    Ok(long_videos(crate::api::innertube::collect_items(&r)))
}

// ───────────── sign-in through a browser ─────────────

#[cfg(test)]
#[test]
fn browser_choices_include_chrome_and_yandex_once() {
    let dir = std::env::temp_dir().join(format!("xdytube-browser-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let yandex = dir.join("yandex");
    let chrome = dir.join("chrome");
    let chrome_alternate = dir.join("chrome-alternate");
    for path in [&yandex, &chrome, &chrome_alternate] {
        std::fs::write(path, []).unwrap();
    }
    let choices = browser_choices([
        ("Yandex Browser", yandex.clone()),
        ("Google Chrome", chrome.clone()),
        ("Google Chrome", chrome_alternate),
        ("Brave", dir.join("missing")),
    ]);
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(choices.iter().map(|browser| browser.name).collect::<Vec<_>>(), ["Yandex Browser", "Google Chrome"]);
    assert_eq!(choices[0].executable, yandex);
    assert_eq!(choices[1].executable, chrome);
}

#[derive(Clone)]
pub struct Browser {
    pub name: &'static str,
    executable: PathBuf,
}

fn browser_choices(candidates: impl IntoIterator<Item = (&'static str, PathBuf)>) -> Vec<Browser> {
    let mut browsers: Vec<Browser> = Vec::new();
    for (name, executable) in candidates {
        if executable.is_file() && !browsers.iter().any(|browser| browser.name == name) {
            browsers.push(Browser { name, executable });
        }
    }
    browsers
}

/// Installed Chromium-based browsers that the sign-in can open.
pub fn installed_browsers() -> Vec<Browser> {
    let mut candidates = Vec::new();
    let mut directories = vec![PathBuf::from("/Applications")];
    directories.extend(dirs::home_dir().map(|home| home.join("Applications")));
    let apps = [
        ("Google Chrome", "Google Chrome.app/Contents/MacOS/Google Chrome"),
        ("Yandex Browser", "Yandex.app/Contents/MacOS/Yandex"),
        ("Microsoft Edge", "Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
        ("Brave", "Brave Browser.app/Contents/MacOS/Brave Browser"),
        ("Chromium", "Chromium.app/Contents/MacOS/Chromium"),
        ("Vivaldi", "Vivaldi.app/Contents/MacOS/Vivaldi"),
    ];
    for (name, relative_path) in apps {
        for directory in &directories {
            candidates.push((name, directory.join(relative_path)));
        }
    }
    browser_choices(candidates)
}

fn local_agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).http_status_as_error(false).build().into()
}

/// Asks the browser for its cookies over the DevTools protocol.
fn cdp_cookies(port: u16) -> Result<Vec<Value>> {
    let mut resp = local_agent().get(&format!("http://127.0.0.1:{port}/json/version")).call()?;
    let v: Value = serde_json::from_str(&resp.body_mut().read_to_string()?)?;
    let ws = v.get("webSocketDebuggerUrl").and_then(Value::as_str).ok_or_else(|| anyhow!("no debugger url"))?.to_string();
    let stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let (mut sock, _) = tungstenite::client(ws.as_str(), stream).map_err(|e| anyhow!("devtools: {e}"))?;
    sock.send(tungstenite::Message::text(json!({ "id": 1, "method": "Storage.getCookies" }).to_string()))?;
    loop {
        let msg = sock.read()?;
        if let tungstenite::Message::Text(t) = msg {
            let v: Value = serde_json::from_str(t.as_str())?;
            if v.get("id").and_then(Value::as_i64) == Some(1) {
                let _ = sock.close(None);
                return Ok(v.pointer("/result/cookies").and_then(Value::as_array).cloned().unwrap_or_default());
            }
        }
    }
}

fn cdp_close(port: u16) {
    let Ok(mut resp) = local_agent().get(&format!("http://127.0.0.1:{port}/json/version")).call() else { return };
    let Ok(v) = serde_json::from_str::<Value>(&resp.body_mut().read_to_string().unwrap_or_default()) else { return };
    let Some(ws) = v.get("webSocketDebuggerUrl").and_then(Value::as_str) else { return };
    let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else { return };
    if let Ok((mut sock, _)) = tungstenite::client(ws, stream) {
        let _ = sock.send(tungstenite::Message::text(json!({ "id": 2, "method": "Browser.close" }).to_string()));
        let _ = sock.flush();
    }
}

fn youtube_cookies(list: &[Value]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for c in list {
        let domain = c.get("domain").and_then(Value::as_str).unwrap_or("");
        if !(domain == "youtube.com" || domain.ends_with(".youtube.com")) {
            continue;
        }
        if let (Some(n), Some(v)) = (c.get("name").and_then(Value::as_str), c.get("value").and_then(Value::as_str)) {
            out.insert(n.to_string(), v.to_string());
        }
    }
    out
}

const SIGN_IN_PROFILE_PREFIX: &str = "attempt-";

fn open_sign_in_profiles(data_dir: &Path) -> Result<(PathBuf, File)> {
    let root = data_dir.join("signin-profiles");
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&root)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(root.join("lock"))?;
    lock.try_lock().context("another browser sign-in is using this xdytube profile")?;

    // The lock covers both cleanup and the lifetime of each attempt.
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with(SIGN_IN_PROFILE_PREFIX) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    let _ = fs::remove_dir_all(data_dir.join("signin-browser"));
    Ok((root, lock))
}

pub fn clean_up_sign_in_profiles() {
    let _ = open_sign_in_profiles(&crate::store::data_dir());
}

fn new_sign_in_profile(root: &Path) -> std::io::Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(SIGN_IN_PROFILE_PREFIX)
        .permissions(Permissions::from_mode(0o700))
        .tempdir_in(root)
}

/// Opens a browser window for the user to sign in to YouTube and waits until they
/// have. Returns the account name. `cancel` stops waiting and closes the browser.
pub fn browser_sign_in(browser: Browser, cancel: Arc<AtomicBool>) -> Result<String> {
    let (root, _lock) = open_sign_in_profiles(&crate::store::data_dir())?;
    let profile = new_sign_in_profile(&root)?;
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
    let mut cmd = std::process::Command::new(&browser.executable);
    cmd.arg(format!("--user-data-dir={}", profile.path().display()))
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--remote-allow-origins=http://127.0.0.1")
        .args(["--no-first-run", "--no-default-browser-check", "--disable-sync", "--new-window"])
        .arg(SIGN_IN_URL);
    if let Some(p) = crate::store::proxy_setting() {
        cmd.arg(format!("--proxy-server={p}"));
    }
    let mut child = cmd.spawn().context("couldn't start the browser")?;
    let started = Instant::now();
    let result = loop {
        std::thread::sleep(Duration::from_millis(1500));
        if cancel.load(Ordering::Relaxed) {
            break Err(anyhow!("cancelled"));
        }
        if let Ok(Some(_)) = child.try_wait() {
            // the user closed the window; the browser may have handed off to an
            // already running copy, but with our own profile that doesn't happen
            break Err(anyhow!("The browser was closed before sign-in finished."));
        }
        if started.elapsed() > Duration::from_secs(20 * 60) {
            break Err(SignInTimeout.into());
        }
        let Ok(list) = cdp_cookies(port) else { continue };
        let cookies = youtube_cookies(&list);
        if has_login(&cookies) && cookies.contains_key("LOGIN_INFO") {
            break Ok(cookies);
        }
    };
    cdp_close(port);
    for _ in 0..20 {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let _ = child.kill();
    let _ = child.wait();
    for _ in 0..8 {
        if fs::remove_dir_all(profile.path()).is_ok() || !profile.path().exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    connect(result?)
}

/// Reads a Netscape-format cookies.txt (as saved by "Get cookies.txt" style
/// browser extensions) and connects with its youtube.com cookies.
pub fn import_cookies_txt(path: &std::path::Path) -> Result<String> {
    let text = std::fs::read_to_string(path)?;
    let mut cookies = BTreeMap::new();
    for line in text.lines() {
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 7 {
            continue;
        }
        let domain = f[0].trim_start_matches('.');
        if domain == "youtube.com" || domain.ends_with(".youtube.com") {
            cookies.insert(f[5].to_string(), f[6].trim_end_matches('\r').to_string());
        }
    }
    if cookies.is_empty() {
        return Err(anyhow!("No youtube.com cookies in that file."));
    }
    connect(cookies)
}

// ───────────── reporting what is watched ─────────────

fn sts() -> Option<u64> {
    static STS: Mutex<Option<(u64, Instant)>> = Mutex::new(None);
    if let Some((v, at)) = *STS.lock() {
        if at.elapsed() < Duration::from_secs(6 * 3600) {
            return Some(v);
        }
    }
    let api = String::from_utf8(http::get_bytes("https://www.youtube.com/iframe_api", &[]).ok()?).ok()?;
    let id: String = api
        .split("/s/player/")
        .nth(1)
        .or_else(|| api.split(r"player\/").nth(1))?
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    let js = String::from_utf8(http::get_bytes(&format!("https://www.youtube.com/s/player/{id}/player_ias.vflset/en_US/base.js"), &[]).ok()?).ok()?;
    let v: u64 = ["signatureTimestamp:", "sts:"]
        .iter()
        .find_map(|k| js.split(k).nth(1).and_then(|t| t.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()))?;
    *STS.lock() = Some((v, Instant::now()));
    Some(v)
}

struct Watch {
    id: String,
    playback: String,
    watchtime: String,
    cpn: String,
    len: f64,
    started: Instant,
    last_ping: Instant,
    last_cmt: f64,
    pings: u32,
    playing: bool,
}

impl Watch {
    fn begin(id: &str, len: f64) -> Result<Self> {
        let mut body = json!({ "videoId": id, "racyCheckOk": true, "contentCheckOk": true });
        if let Some(sts) = sts() {
            body["playbackContext"] = json!({ "contentPlaybackContext": { "signatureTimestamp": sts, "html5Preference": "HTML5_PREF_WANTS" } });
        }
        let r = web_call("player", body)?;
        let url = |k: &str| r.pointer(&format!("/playbackTracking/{k}/baseUrl")).and_then(Value::as_str).map(str::to_string);
        let playback = url("videostatsPlaybackUrl").ok_or_else(|| anyhow!("no playback tracking"))?;
        let watchtime = url("videostatsWatchtimeUrl").ok_or_else(|| anyhow!("no watchtime tracking"))?;
        let len = r
            .pointer("/videoDetails/lengthSeconds")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .filter(|l: &f64| *l > 0.0)
            .unwrap_or(len);
        let cpn: String = {
            use rand::Rng;
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut rng = rand::rng();
            (0..16).map(|_| A[rng.random_range(0..A.len())] as char).collect()
        };
        let w = Watch { id: id.to_string(), playback, watchtime, cpn, len, started: Instant::now(), last_ping: Instant::now(), last_cmt: 0.0, pings: 0, playing: true };
        w.get(&w.playback, &[("cmt", "0".into()), ("rtn", "0".into()), ("state", "playing".into())])?;
        Ok(w)
    }

    /// Sends a stats ping: the base URL from the player response with the fields
    /// youtube.com's own player adds. Fields already in the base URL are replaced,
    /// never repeated.
    fn get(&self, base: &str, fields: &[(&str, String)]) -> Result<()> {
        // the ping carries the sign-in cookies: only ever to YouTube itself
        if !is_youtube_https(base) {
            return Err(anyhow!("unexpected stats host"));
        }
        let rt = self.started.elapsed().as_secs_f64();
        let mut add: Vec<(&str, String)> = vec![
            ("ver", "2".into()),
            ("cpn", self.cpn.clone()),
            ("c", "WEB".into()),
            ("cver", WEB_VERSION.into()),
            ("cplayer", "UNIPLAYER".into()),
            ("cbr", "Chrome".into()),
            ("cbrver", "140.0.0.0".into()),
            ("cos", "Windows".into()),
            ("cosver", "10.0".into()),
            ("cplatform", "DESKTOP".into()),
            ("fs", "0".into()),
            ("volume", "100".into()),
            ("muted", "0".into()),
            ("lact", "1200".into()),
            ("len", format!("{:.3}", self.len)),
            ("rt", format!("{rt:.3}")),
        ];
        add.extend(fields.iter().cloned());
        let (path, query) = base.split_once('?').unwrap_or((base, ""));
        let mut pairs: Vec<(String, String)> = query
            .split('&')
            .filter(|kv| !kv.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (k.to_string(), v.to_string())
            })
            .collect();
        for (k, v) in add {
            match pairs.iter_mut().find(|(pk, _)| pk == k) {
                Some(p) => p.1 = v,
                None => pairs.push((k.to_string(), v)),
            }
        }
        let url = format!("{path}?{}", pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&"));
        let cookie = cookie_header().unwrap_or_default();
        let referer = format!("https://www.youtube.com/watch?v={}", self.id);
        http::get_bytes(&url, &[("Cookie", &cookie), ("Referer", &referer), ("Origin", ORIGIN)])?;
        Ok(())
    }

    /// Reports the stretch watched since the last report, ending at `cmt`.
    fn report(&mut self, cmt: f64, playing: bool, last: bool) {
        let st = if cmt >= self.last_cmt && cmt - self.last_cmt < 120.0 { self.last_cmt } else { cmt };
        let rtn = self.started.elapsed().as_secs_f64();
        let mut fields = vec![
            ("cmt", format!("{cmt:.3}")),
            ("st", format!("{st:.3}")),
            ("et", format!("{cmt:.3}")),
            ("state", if playing { "playing" } else { "paused" }.to_string()),
            ("rtn", format!("{:.0}", rtn.max(1.0))),
            ("rti", format!("{:.0}", rtn.max(1.0))),
            ("idpj", "-1".into()),
            ("ldpj", "-1".into()),
        ];
        if last {
            fields.push(("final", "1".into()));
        }
        if let Err(e) = self.get(&self.watchtime, &fields) {
            if cfg!(test) {
                eprintln!("watchtime ping failed: {e}");
            }
        }
        self.last_cmt = cmt;
        self.last_ping = Instant::now();
        self.pings += 1;
        self.playing = playing;
    }

    /// youtube.com reports at 10, 20 and 30 seconds, then every 40 seconds.
    fn due(&self) -> bool {
        let every = if self.pings < 3 { 10 } else { 40 };
        self.last_ping.elapsed() >= Duration::from_secs(every)
    }
}

enum Event {
    Start { id: String, len: f64 },
    Progress { id: String, t: f64, playing: bool },
    Stop { id: String, t: f64 },
}

fn reporter() -> &'static Mutex<mpsc::Sender<Event>> {
    static TX: OnceLock<Mutex<mpsc::Sender<Event>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Event>();
        std::thread::Builder::new()
            .name("watch-report".into())
            .spawn(move || {
                let mut cur: Option<Watch> = None;
                while let Ok(ev) = rx.recv() {
                    match ev {
                        Event::Start { id, len } => {
                            if let Some(mut w) = cur.take() {
                                let t = w.last_cmt;
                                w.report(t, false, true);
                            }
                            cur = Watch::begin(&id, len).ok();
                        }
                        Event::Progress { id, t, playing } => {
                            if let Some(w) = cur.as_mut().filter(|w| w.id == id) {
                                if w.due() || playing != w.playing {
                                    w.report(t, playing, false);
                                }
                            }
                        }
                        Event::Stop { id, t } => {
                            if let Some(mut w) = cur.take() {
                                if w.id == id {
                                    w.report(t, false, true);
                                } else {
                                    cur = Some(w);
                                }
                            }
                        }
                    }
                }
            })
            .ok();
        Mutex::new(tx)
    })
}

/// A video started playing.
pub fn report_start(id: &str, len: f64) {
    if connected() && crate::store::settings_snapshot().history_enabled {
        let _ = reporter().lock().send(Event::Start { id: id.to_string(), len });
    }
}

/// Playback position update; cheap to call often.
pub fn report_progress(id: &str, t: f64, playing: bool) {
    if connected() {
        let _ = reporter().lock().send(Event::Progress { id: id.to_string(), t, playing });
    }
}

/// The video was closed or replaced at position `t`.
pub fn report_stop(id: &str, t: f64) {
    if connected() {
        let _ = reporter().lock().send(Event::Stop { id: id.to_string(), t });
    }
}

/// `https://youtube.com/...` or `https://<anything>.youtube.com/...`, nothing else.
fn is_youtube_https(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // no user info or port tricks ("youtube.com@evil.example")
    !host.contains(['@', ':']) && (host == "youtube.com" || host.ends_with(".youtube.com"))
}

#[cfg(test)]
mod host_tests {
    use super::is_youtube_https;

    #[::core::prelude::v1::test]
    fn only_youtube() {
        assert!(is_youtube_https("https://s.youtube.com/api/stats/playback?x=1"));
        assert!(is_youtube_https("https://www.youtube.com/api/stats/watchtime"));
        assert!(!is_youtube_https("http://s.youtube.com/api"));
        assert!(!is_youtube_https("https://youtube.com.evil.example/api"));
        assert!(!is_youtube_https("https://www.youtube.com@evil.example/api"));
        assert!(!is_youtube_https("https://evilyoutube.com/api"));
    }
}

#[cfg(test)]
mod browser_profile_tests {
    use super::{open_sign_in_profiles, new_sign_in_profile};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn each_attempt_has_a_private_profile_and_stale_profiles_are_cleaned() {
        let data = tempfile::tempdir().unwrap();
        let (root, lock) = open_sign_in_profiles(data.path()).unwrap();
        let first = new_sign_in_profile(&root).unwrap();
        let second = new_sign_in_profile(&root).unwrap();
        assert_ne!(first.path(), second.path());
        assert_eq!(first.path().metadata().unwrap().permissions().mode() & 0o777, 0o700);
        assert!(open_sign_in_profiles(data.path()).is_err());

        let finished = first.path().to_path_buf();
        first.close().unwrap();
        assert!(!finished.exists());
        let stale = second.keep();
        let unrelated = root.join("other-file");
        let ordinary = root.join("other-directory");
        std::fs::write(&unrelated, b"keep").unwrap();
        std::fs::create_dir(&ordinary).unwrap();
        std::fs::create_dir(data.path().join("signin-browser")).unwrap();
        drop(lock);

        let _guard = open_sign_in_profiles(data.path()).unwrap();
        assert!(!stale.exists());
        assert!(!data.path().join("signin-browser").exists());
        assert!(unrelated.exists());
        assert!(ordinary.exists());
    }
}

#[cfg(test)]
mod watch_later_tests {
    use super::{playlist_entry_id, watch_later_add_request, parse_watch_later_page, session_matches, watch_later_continuation_page};
    use serde_json::json;

    #[test]
    fn addition_uses_the_youtube_watch_later_playlist() {
        assert_eq!(watch_later_add_request("video-123"), json!({
            "playlistId": "WL",
            "actions": [{ "action": "ACTION_ADD_VIDEO", "addedVideoId": "video-123" }]
        }));
    }

    #[test]
    fn playlist_uses_browser_response_and_continuation() {
        let response = json!({"contents":{"playlistVideoListRenderer":{"contents":[
            {"playlistVideoRenderer":{"videoId":"one", "title":{"simpleText":"One"}}},
            {"continuationItemRenderer":{"continuationEndpoint":{"continuationCommand":{"token":"next-page"}}}}
        ]}}});
        let page = parse_watch_later_page(&response).unwrap();
        assert_eq!(page.videos[0].id, "one");
        assert_eq!(page.continuation.as_deref(), Some("next-page"));
        assert!(parse_watch_later_page(&json!({"error":{"message":"not signed in"}})).is_err());
    }

    #[test]
    fn stale_playlist_session_cannot_be_used_for_removal() {
        assert!(session_matches(Some("same"), "same"));
        assert!(!session_matches(Some("other"), "same"));
        assert!(!session_matches(None, "same"));
        assert!(super::remove_watch_later_in_session("fixture-video", "stale-session").is_err());
        assert!(super::watch_later_continue("fixture-token", "stale-session").is_err());
    }

    #[test]
    fn continuation_keeps_videos_without_channel_names() {
        let page = watch_later_continuation_page(&json!({"items":[{"playlistVideoRenderer":{"videoId":"two", "title":{"simpleText":"Two"}}}]}));
        assert!(matches!(&page.items[..], [crate::api::types::Item::Video(video)] if video.id == "two"));
    }

    #[test]
    fn removal_uses_the_matching_playlist_entry_id() {
        let page = json!({"contents": [
            {"playlistVideoRenderer": {"videoId": "other", "setVideoId": "entry-other"}},
            {"playlistVideoRenderer": {"videoId": "video-123", "setVideoId": "entry-123"}}
        ]});
        assert_eq!(playlist_entry_id(&page, "video-123"), Some("entry-123".into()));
        assert_eq!(playlist_entry_id(&page, "missing"), None);
    }
}
