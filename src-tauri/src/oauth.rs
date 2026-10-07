//! Gmail / Outlook 的 OAuth 2.0 登录（授权码 + PKCE + 本地回环回调）。
//!
//! 1. 本地随机端口起一个临时 HTTP 监听
//! 2. 用系统浏览器打开授权页，用户登录后浏览器跳回 http://127.0.0.1:<port>/?code=...
//! 3. 用 code + PKCE verifier 换取 access token / refresh token
//! 4. IMAP 用 XOAUTH2 认证；access token 过期前用 refresh token 自动续期

use crate::{net, secrets, settings::OAuthApps};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Url;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OAuthProvider {
    Google,
    Microsoft,
}

pub struct Endpoints {
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scope: &'static str,
    pub imap_host: &'static str,
    /// 回调地址用的主机名：微软注册的是 http://localhost（任意端口），谷歌桌面应用用 127.0.0.1
    pub redirect_host: &'static str,
}

impl OAuthProvider {
    pub fn endpoints(self) -> Endpoints {
        match self {
            Self::Google => Endpoints {
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
                token_url: "https://oauth2.googleapis.com/token",
                scope: "https://mail.google.com/ openid email",
                imap_host: "imap.gmail.com",
                redirect_host: "127.0.0.1",
            },
            Self::Microsoft => Endpoints {
                auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                scope: "https://outlook.office.com/IMAP.AccessAsUser.All offline_access openid email profile",
                imap_host: "outlook.office365.com",
                redirect_host: "localhost",
            },
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
        }
    }

    /// (client_id, client_secret)
    fn client(self, apps: &OAuthApps) -> Result<(String, Option<String>), String> {
        let (id, secret) = match self {
            Self::Google => (&apps.google_client_id, Some(&apps.google_client_secret)),
            Self::Microsoft => (&apps.microsoft_client_id, None),
        };
        if id.is_empty() || secret.is_some_and(|s| s.is_empty()) {
            return Err(format!(
                "还没有配置 {} OAuth 应用。请先到「设置 → 高级」填写 Client ID{}",
                self.name(),
                if secret.is_some() { " 和 Client Secret" } else { "" }
            ));
        }
        Ok((id.clone(), secret.filter(|s| !s.is_empty()).cloned()))
    }
}

// ---------- 纯函数 ----------

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

pub fn new_pkce() -> Result<Pkce, String> {
    let verifier = random_token(32)?;
    Ok(Pkce { challenge: pkce_challenge(&verifier), verifier })
}

pub fn authorize_url(
    ep: &Endpoints,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
    login_hint: Option<&str>,
) -> String {
    let mut url = Url::parse(ep.auth_url).unwrap();
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", ep.scope)
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            // Google：要求返回 refresh token，并且每次都弹出同意页以确保拿到
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");
        if let Some(hint) = login_hint.filter(|h| !h.is_empty()) {
            q.append_pair("login_hint", hint);
        }
    }
    url.into()
}

#[derive(Debug, PartialEq)]
pub enum Callback {
    Code(String),
    Denied(String),
    /// favicon 等无关请求，继续等待
    Ignore,
}

/// 解析回调请求行，如 `GET /?code=xx&state=yy HTTP/1.1`
pub fn parse_callback(request_line: &str, expected_state: &str) -> Result<Callback, String> {
    let target = request_line.split_whitespace().nth(1).unwrap_or_default();
    let url = Url::parse(&format!("http://localhost{target}")).map_err(|e| e.to_string())?;
    if url.path() != "/" {
        return Ok(Callback::Ignore);
    }
    let get = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned());
    if let Some(err) = get("error") {
        let desc = get("error_description").unwrap_or_default();
        return Ok(Callback::Denied(if desc.is_empty() { err } else { format!("{err}: {desc}") }));
    }
    match (get("code"), get("state")) {
        (Some(code), Some(state)) if state == expected_state => Ok(Callback::Code(code)),
        (Some(_), _) => Err("回调 state 不匹配，可能是过期或伪造的登录请求，请重试".into()),
        _ => Ok(Callback::Ignore),
    }
}

/// IMAP AUTHENTICATE XOAUTH2 的明文载荷（发送时由 imap 库做 base64）
pub fn xoauth2_payload(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

/// 从 id_token（JWT）中取邮箱地址。只用于显示/识别账号，不做签名校验
pub fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    ["email", "preferred_username", "upn"]
        .iter()
        .filter_map(|k| claims.get(k)?.as_str())
        .find(|v| v.contains('@'))
        .map(|v| v.to_lowercase())
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub id_token: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredToken {
    pub provider: OAuthProvider,
    pub access_token: String,
    pub refresh_token: String,
    /// unix 秒
    pub expires_at: u64,
}

impl StoredToken {
    /// 提前 2 分钟视为过期，避免请求途中失效
    pub fn is_fresh(&self, now: u64) -> bool {
        now + 120 < self.expires_at
    }
}

/// 把 token 响应合并进已有 token（刷新时服务器可能不返回新的 refresh token）
pub fn merge_token(
    provider: OAuthProvider,
    resp: TokenResponse,
    previous_refresh: Option<&str>,
    now: u64,
) -> Result<StoredToken, String> {
    if let Some(err) = resp.error {
        let expired = err == "invalid_grant";
        let desc = resp.error_description.unwrap_or_default();
        return Err(if expired {
            format!("{} 登录已过期或被撤销，请在「设置 → 账号」中重新登录。({desc})", provider.name())
        } else {
            format!("{} 授权失败: {err} {desc}", provider.name())
        });
    }
    let access_token = resp.access_token.ok_or("授权服务器没有返回 access token")?;
    let refresh_token = resp
        .refresh_token
        .or_else(|| previous_refresh.map(str::to_string))
        .ok_or("授权服务器没有返回 refresh token，请在授权页确认允许离线访问")?;
    Ok(StoredToken {
        provider,
        access_token,
        refresh_token,
        expires_at: now + resp.expires_in.unwrap_or(3600),
    })
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------- 网络 ----------

fn post_token(
    agent: &ureq::Agent,
    ep: &Endpoints,
    form: &[(&str, &str)],
) -> Result<TokenResponse, String> {
    let resp = agent
        .post(ep.token_url)
        .send_form(form.iter().copied())
        .map_err(|e| format!("请求授权服务器失败（如在国内需配置代理）: {e}"))?;
    resp.into_body()
        .read_json::<TokenResponse>()
        .map_err(|e| format!("授权服务器响应无法解析: {e}"))
}

const SUCCESS_PAGE: &str = "<!doctype html><meta charset=utf-8><title>MailBox</title>\
<body style=\"font-family:system-ui;text-align:center;padding-top:80px\">\
<h2>✅ 登录成功</h2><p>可以关闭此页面，回到 MailBox。</p>";

fn error_page(msg: &str) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>MailBox</title>\
<body style=\"font-family:system-ui;text-align:center;padding-top:80px\">\
<h2>❌ 登录失败</h2><p>{}</p>",
        crate::render::escape_html(msg)
    )
}

fn respond(mut stream: TcpStream, status: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).ok();
}

/// 等待浏览器回调，返回授权码。同时监听 IPv4 和 IPv6，避免 localhost 解析到 ::1 时连不上
fn wait_for_code(listeners: &[TcpListener], state: &str, cancel: &AtomicBool) -> Result<String, String> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("已取消登录".into());
        }
        if Instant::now() > deadline {
            return Err("等待浏览器登录超时（5 分钟），请重试".into());
        }
        for listener in listeners {
            let Ok((stream, _)) = listener.accept() else { continue };
            stream.set_nonblocking(false).ok();
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            match parse_callback(&line, state) {
                Ok(Callback::Code(code)) => {
                    respond(stream, "200 OK", SUCCESS_PAGE);
                    return Ok(code);
                }
                Ok(Callback::Denied(reason)) => {
                    respond(stream, "200 OK", &error_page(&reason));
                    return Err(format!("授权被拒绝: {reason}"));
                }
                Ok(Callback::Ignore) => respond(stream, "404 Not Found", ""),
                Err(e) => {
                    respond(stream, "400 Bad Request", &error_page(&e));
                    return Err(e);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub struct LoginResult {
    pub email: String,
    pub token: StoredToken,
}

/// 完整的浏览器登录流程。`open_browser` 由调用方提供（用 tauri opener 打开系统浏览器）
pub fn login(
    provider: OAuthProvider,
    apps: &OAuthApps,
    proxy: &net::ProxyConfig,
    login_hint: Option<&str>,
    cancel: &AtomicBool,
    open_browser: impl FnOnce(&str) -> Result<(), String>,
) -> Result<LoginResult, String> {
    let (client_id, client_secret) = provider.client(apps)?;
    let ep = provider.endpoints();

    let v4 = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("无法启动本地回调监听: {e}"))?;
    let port = v4.local_addr().map_err(|e| e.to_string())?.port();
    let mut listeners = vec![v4];
    if let Ok(v6) = TcpListener::bind(("::1", port)) {
        listeners.push(v6);
    }
    for l in &listeners {
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
    }

    let redirect_uri = format!("http://{}:{port}/", ep.redirect_host);
    let pkce = new_pkce()?;
    let state = random_token(16)?;
    cancel.store(false, Ordering::SeqCst);

    open_browser(&authorize_url(&ep, &client_id, &redirect_uri, &state, &pkce.challenge, login_hint))?;
    let code = wait_for_code(&listeners, &state, cancel)?;

    let agent = net::http_agent(proxy)?;
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
    ];
    if let Some(secret) = &client_secret {
        form.push(("client_secret", secret));
    }
    if provider == OAuthProvider::Microsoft {
        form.push(("scope", ep.scope));
    }
    let resp = post_token(&agent, &ep, &form)?;
    let email = resp
        .id_token
        .as_deref()
        .and_then(email_from_id_token)
        .or_else(|| login_hint.map(str::to_lowercase))
        .ok_or("无法从登录结果中获取邮箱地址")?;
    let token = merge_token(provider, resp, None, now_secs())?;
    Ok(LoginResult { email, token })
}

pub fn save_token(email: &str, token: &StoredToken) -> Result<(), String> {
    secrets::set(&secrets::oauth_token_key(email), &serde_json::to_string(token).map_err(|e| e.to_string())?)
}

/// 取可用的 access token，必要时自动刷新并保存
pub fn access_token(email: &str, apps: &OAuthApps, proxy: &net::ProxyConfig) -> Result<String, String> {
    let stored = secrets::get(&secrets::oauth_token_key(email))?
        .ok_or("找不到登录凭据，请在「设置 → 账号」中重新登录")?;
    let token: StoredToken = serde_json::from_str(&stored).map_err(|_| "登录凭据已损坏，请重新登录")?;
    if token.is_fresh(now_secs()) {
        return Ok(token.access_token);
    }

    let provider = token.provider;
    let (client_id, client_secret) = provider.client(apps)?;
    let ep = provider.endpoints();
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", token.refresh_token.as_str()),
        ("client_id", client_id.as_str()),
    ];
    if let Some(secret) = &client_secret {
        form.push(("client_secret", secret));
    }
    if provider == OAuthProvider::Microsoft {
        form.push(("scope", ep.scope));
    }
    let resp = post_token(&net::http_agent(proxy)?, &ep, &form)?;
    let refreshed = merge_token(provider, resp, Some(&token.refresh_token), now_secs())?;
    save_token(email, &refreshed)?;
    Ok(refreshed.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = new_pkce().unwrap();
        assert_eq!(p.verifier.len(), 43);
        assert_eq!(p.challenge, pkce_challenge(&p.verifier));
    }

    #[test]
    fn builds_authorize_url() {
        let ep = OAuthProvider::Microsoft.endpoints();
        let url = authorize_url(&ep, "cid", "http://localhost:5000/", "st", "ch", Some("a@outlook.com"));
        let parsed = Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert!(url.starts_with(ep.auth_url));
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["redirect_uri"], "http://localhost:5000/");
        assert_eq!(q["scope"], ep.scope);
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["login_hint"], "a@outlook.com");

        let no_hint = authorize_url(&ep, "cid", "r", "s", "c", Some(""));
        assert!(!no_hint.contains("login_hint"));
    }

    #[test]
    fn parses_callbacks() {
        let cases = [
            ("GET /?code=abc&state=S HTTP/1.1", Ok(Callback::Code("abc".into()))),
            ("GET /?state=S&code=a%2Fb&scope=x HTTP/1.1", Ok(Callback::Code("a/b".into()))),
            ("GET /?error=access_denied&state=S HTTP/1.1", Ok(Callback::Denied("access_denied".into()))),
            (
                "GET /?error=x&error_description=no+consent HTTP/1.1",
                Ok(Callback::Denied("x: no consent".into())),
            ),
            ("GET /favicon.ico HTTP/1.1", Ok(Callback::Ignore)),
            ("GET / HTTP/1.1", Ok(Callback::Ignore)),
            ("GET /?code=abc&state=EVIL HTTP/1.1", Err(())),
        ];
        for (line, want) in cases {
            match (parse_callback(line, "S"), want) {
                (Ok(got), Ok(want)) => assert_eq!(got, want, "{line}"),
                (Err(_), Err(())) => {}
                (got, _) => panic!("{line} -> {got:?}"),
            }
        }
    }

    #[test]
    fn builds_xoauth2_payload() {
        assert_eq!(xoauth2_payload("a@gmail.com", "tok"), "user=a@gmail.com\x01auth=Bearer tok\x01\x01");
    }

    #[test]
    fn extracts_email_from_id_token() {
        let jwt = |claims: &str| format!("h.{}.sig", URL_SAFE_NO_PAD.encode(claims));
        let cases = [
            (jwt(r#"{"email":"A@Gmail.com"}"#), Some("a@gmail.com")),
            (jwt(r#"{"preferred_username":"b@outlook.com","name":"B"}"#), Some("b@outlook.com")),
            (jwt(r#"{"email":"not-an-email","upn":"c@corp.com"}"#), Some("c@corp.com")),
            (jwt(r#"{"sub":"123"}"#), None),
            ("garbage".to_string(), None),
            ("a.!!!.c".to_string(), None),
        ];
        for (token, want) in cases {
            assert_eq!(email_from_id_token(&token).as_deref(), want, "{token}");
        }
    }

    fn resp(json: &str) -> TokenResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn merges_tokens() {
        let p = OAuthProvider::Google;
        let t = merge_token(p, resp(r#"{"access_token":"A","refresh_token":"R","expires_in":3599}"#), None, 1000).unwrap();
        assert_eq!(t, StoredToken { provider: p, access_token: "A".into(), refresh_token: "R".into(), expires_at: 4599 });

        // 刷新时没有返回新的 refresh token，沿用旧的
        let t = merge_token(p, resp(r#"{"access_token":"A2","expires_in":60}"#), Some("R"), 0).unwrap();
        assert_eq!((t.refresh_token.as_str(), t.expires_at), ("R", 60));

        let err = merge_token(p, resp(r#"{"error":"invalid_grant","error_description":"expired"}"#), Some("R"), 0);
        assert!(err.unwrap_err().contains("重新登录"));

        let err = merge_token(p, resp(r#"{"access_token":"A"}"#), None, 0);
        assert!(err.unwrap_err().contains("refresh token"));
    }

    #[test]
    fn token_freshness() {
        let t = StoredToken {
            provider: OAuthProvider::Microsoft,
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1000,
        };
        assert!(t.is_fresh(800));
        assert!(!t.is_fresh(880));
        assert!(!t.is_fresh(2000));
    }

    #[test]
    fn requires_client_config() {
        let mut apps = OAuthApps::default();
        assert!(OAuthProvider::Google.client(&apps).unwrap_err().contains("Client Secret"));
        assert!(OAuthProvider::Microsoft.client(&apps).unwrap_err().contains("Client ID"));

        apps.google_client_id = "g".into();
        assert!(OAuthProvider::Google.client(&apps).is_err());
        apps.google_client_secret = "s".into();
        assert_eq!(OAuthProvider::Google.client(&apps).unwrap(), ("g".into(), Some("s".into())));

        apps.microsoft_client_id = "m".into();
        assert_eq!(OAuthProvider::Microsoft.client(&apps).unwrap(), ("m".into(), None));
    }
}
