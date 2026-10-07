mod accounts;
mod db;
mod fonts;
mod imap_client;
mod mail;
mod net;
mod oauth;
mod render;
mod rules;
mod secrets;
mod settings;
mod sync;

use accounts::{AccountConfig, AccountStore, AuthKind};
use db::{Category, Db, View};
use imap_client::{Auth, Credentials};
use mail::Envelope;
use oauth::OAuthProvider;
use render::{MessageView, PartRoute};
use serde::{Deserialize, Serialize};
use settings::{RemoteImages, Settings, SettingsStore};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};
use sync::{SyncStats, INBOX};
use tauri::{
    http::{header, Response, StatusCode},
    webview::NewWindowResponse,
    AppHandle, Manager, State, WebviewWindowBuilder,
};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    db: Arc<Db>,
    store: Arc<AccountStore>,
    settings: Arc<SettingsStore>,
    attachments_dir: PathBuf,
    db_path: PathBuf,
    oauth_cancel: Arc<AtomicBool>,
}

impl AppState {
    fn proxy_for(&self, account: &AccountConfig) -> net::ProxyConfig {
        if account.use_proxy { self.settings.proxy() } else { net::ProxyConfig::default() }
    }

    /// 组装连接凭据：授权码从 keyring 读，OAuth 自动刷新 access token
    fn credentials<'a>(&self, account: &'a AccountConfig) -> Result<Credentials<'a>, String> {
        let proxy = self.proxy_for(account);
        let auth = match account.auth.oauth_provider() {
            None => Auth::Password(accounts::get_password(&account.email)?),
            Some(_) => Auth::OAuth(oauth::access_token(&account.email, &self.settings.get().oauth, &proxy)?),
        };
        Ok(Credentials { host: &account.host, port: account.port, username: &account.email, auth, proxy })
    }

    fn with_session<T>(&self, email: &str, f: impl FnOnce(&Credentials) -> Result<T, String>) -> Result<T, String> {
        let account = self.store.get(email)?;
        f(&self.credentials(&account)?)
    }

    /// 确保账号有默认分类（幂等，新增账号时调用）
    fn ensure_categories(&self, email: &str) {
        self.db.default_categories(email).ok();
    }

    /// 优先从本地缓存取原始邮件，没有再从服务器下载并缓存
    fn load_raw(&self, email: &str, folder: &str, uid: u32) -> Result<Vec<u8>, String> {
        if let Some(raw) = self.db.get_body(email, folder, uid)? {
            return Ok(raw);
        }
        let raw = self.with_session(email, |creds| {
            let mut s = imap_client::connect(creds).map_err(|e| e.to_string())?;
            s.select(folder).map_err(|e| e.to_string())?;
            let raw = imap_client::fetch_raw(&mut s, uid).map_err(|e| e.to_string());
            s.logout().ok();
            raw?.ok_or_else(|| "服务器上找不到这封邮件，可能已被删除".to_string())
        })?;
        self.db.put_body(email, folder, uid, &raw)?;
        Ok(raw)
    }
}

/// 在阻塞线程池里执行同步 IMAP / SQLite 调用，避免卡住 UI
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn app_state(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

// ---------- 账号 ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInput {
    email: String,
    display_name: String,
    provider: String,
    host: String,
    port: u16,
    use_proxy: bool,
    /// 新增时必填；编辑时留空表示不修改
    #[serde(default)]
    password: String,
}

impl AccountInput {
    fn config(&self, auth: AuthKind) -> Result<AccountConfig, String> {
        let config = AccountConfig {
            email: accounts::normalize_email(&self.email),
            display_name: self.display_name.trim().to_string(),
            provider: self.provider.clone(),
            host: self.host.trim().to_string(),
            port: self.port,
            auth,
            use_proxy: self.use_proxy,
        };
        accounts::validate(&config)?;
        Ok(config)
    }
}

#[tauri::command]
fn list_accounts(state: State<Arc<AppState>>) -> Result<Vec<AccountConfig>, String> {
    state.store.list()
}

/// 用密码 / 授权码登录的账号：先验证能登录，再保存
#[tauri::command]
async fn save_password_account(app: AppHandle, account: AccountInput, is_new: bool) -> Result<AccountConfig, String> {
    let state = app_state(&app);
    blocking(move || {
        let existing = state.store.get(&accounts::normalize_email(&account.email)).ok();
        if is_new && existing.is_some() {
            return Err("这个邮箱已经添加过了".into());
        }
        let auth = existing.as_ref().map_or(AuthKind::Password, |a| a.auth);
        let config = account.config(auth)?;

        if config.auth == AuthKind::Password {
            let password = if account.password.is_empty() {
                accounts::get_password(&config.email)?
            } else {
                account.password.clone()
            };
            imap_client::verify(&Credentials {
                host: &config.host,
                port: config.port,
                username: &config.email,
                auth: Auth::Password(password.clone()),
                proxy: state.proxy_for(&config),
            })
            .map_err(|e| format!("登录失败，请检查授权码和服务器: {e}"))?;
            if !account.password.is_empty() {
                accounts::set_password(&config.email, &password)?;
            }
        } else {
            // OAuth 账号只允许改显示名和代理开关，验证一下新设置下能否连上
            let creds = state.credentials(&config)?;
            imap_client::verify(&creds).map_err(|e| format!("连接失败: {e}"))?;
        }
        state.store.upsert(config.clone())?;
        state.ensure_categories(&config.email);
        Ok(config)
    })
    .await
}

/// 浏览器 OAuth 登录（新增或重新登录），完成后验证 IMAP 并保存
#[tauri::command]
async fn oauth_login(
    app: AppHandle,
    provider: OAuthProvider,
    login_hint: Option<String>,
    display_name: String,
    use_proxy: bool,
) -> Result<AccountConfig, String> {
    let state = app_state(&app);
    let opener = app.clone();
    blocking(move || {
        let proxy = if use_proxy { state.settings.proxy() } else { net::ProxyConfig::default() };
        let result = oauth::login(
            provider,
            &state.settings.get().oauth,
            &proxy,
            login_hint.as_deref(),
            &state.oauth_cancel,
            |url| opener.opener().open_url(url, None::<&str>).map_err(|e| format!("无法打开浏览器: {e}")),
        )?;

        let ep = provider.endpoints();
        let existing = state.store.get(&result.email).ok();
        let config = AccountConfig {
            email: result.email.clone(),
            display_name: existing.as_ref().map_or(display_name.trim().to_string(), |a| a.display_name.clone()),
            provider: match provider {
                OAuthProvider::Google => "gmail".into(),
                OAuthProvider::Microsoft => "outlook".into(),
            },
            host: ep.imap_host.into(),
            port: 993,
            auth: provider.into(),
            use_proxy,
        };
        imap_client::verify(&Credentials {
            host: &config.host,
            port: config.port,
            username: &config.email,
            auth: Auth::OAuth(result.token.access_token.clone()),
            proxy,
        })
        .map_err(|e| format!("已获得授权，但 IMAP 登录失败: {e}"))?;

        oauth::save_token(&config.email, &result.token)?;
        state.store.upsert(config.clone())?;
        Ok(config)
    })
    .await
}

#[tauri::command]
fn cancel_oauth(state: State<Arc<AppState>>) {
    state.oauth_cancel.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
async fn test_account(app: AppHandle, email: String) -> Result<(), String> {
    let state = app_state(&app);
    blocking(move || state.with_session(&email, |c| imap_client::verify(c).map_err(|e| e.to_string()))).await
}

#[tauri::command]
fn reorder_accounts(state: State<Arc<AppState>>, order: Vec<String>) -> Result<Vec<AccountConfig>, String> {
    state.store.reorder(&order)
}

#[tauri::command]
async fn remove_account(app: AppHandle, email: String) -> Result<(), String> {
    let state = app_state(&app);
    blocking(move || {
        state.store.delete(&email)?;
        state.db.delete_account(&email)
    })
    .await
}

// ---------- 设置 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    #[serde(flatten)]
    settings: Settings,
    has_proxy_password: bool,
}

#[tauri::command]
fn get_settings(state: State<Arc<AppState>>) -> Result<SettingsView, String> {
    Ok(SettingsView {
        settings: state.settings.get(),
        has_proxy_password: secrets::get(secrets::PROXY_PASSWORD_KEY)?.is_some(),
    })
}

/// proxy_password: None 不修改，Some("") 清除
#[tauri::command]
fn save_settings(
    state: State<Arc<AppState>>,
    settings: Settings,
    proxy_password: Option<String>,
) -> Result<SettingsView, String> {
    let saved = state.settings.save(settings)?;
    match proxy_password.as_deref() {
        None => {}
        Some("") => secrets::delete(secrets::PROXY_PASSWORD_KEY)?,
        Some(p) => secrets::set(secrets::PROXY_PASSWORD_KEY, p)?,
    }
    Ok(SettingsView { settings: saved, has_proxy_password: secrets::get(secrets::PROXY_PASSWORD_KEY)?.is_some() })
}

/// 用表单里尚未保存的代理配置，测试能否连到指定服务器
#[tauri::command]
async fn test_proxy(
    proxy: net::ProxyConfig,
    proxy_password: Option<String>,
    target: String,
) -> Result<u64, String> {
    blocking(move || {
        let mut proxy = proxy;
        net::validate_proxy(&proxy)?;
        proxy.password = match proxy_password {
            Some(p) => p,
            None => secrets::get(secrets::PROXY_PASSWORD_KEY)?.unwrap_or_default(),
        };
        let start = std::time::Instant::now();
        let (host, port) = target.rsplit_once(':').and_then(|(h, p)| Some((h, p.parse().ok()?))).unwrap_or((&target, 993));
        imap_client::open(host, port, &proxy).map_err(|e| e.to_string())?;
        Ok(start.elapsed().as_millis() as u64)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheStats {
    file_bytes: u64,
    accounts: Vec<AccountCache>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountCache {
    email: String,
    headers: u64,
    bodies: u64,
    body_bytes: u64,
}

#[tauri::command]
async fn cache_stats(app: AppHandle) -> Result<CacheStats, String> {
    let state = app_state(&app);
    blocking(move || {
        let accounts = state
            .store
            .list()?
            .into_iter()
            .map(|a| {
                let (headers, bodies, body_bytes) = state.db.stats(&a.email)?;
                Ok(AccountCache { email: a.email, headers, bodies, body_bytes })
            })
            .collect::<Result<_, String>>()?;
        let file_bytes = ["", "-wal", "-shm"]
            .iter()
            .filter_map(|suffix| {
                let mut p = state.db_path.clone().into_os_string();
                p.push(suffix);
                std::fs::metadata(p).ok().map(|m| m.len())
            })
            .sum();
        Ok(CacheStats { file_bytes, accounts })
    })
    .await
}

#[tauri::command]
async fn clear_cache(app: AppHandle, bodies_only: bool) -> Result<(), String> {
    let state = app_state(&app);
    blocking(move || if bodies_only { state.db.clear_bodies() } else { state.db.clear_all() }).await
}

// ---------- 邮件列表 ----------

#[tauri::command]
async fn list_cached(app: AppHandle, email: String, view: Option<i64>, limit: u32) -> Result<Vec<Envelope>, String> {
    let state = app_state(&app);
    blocking(move || {
        let v = match view {
            None => View::Inbox,
            Some(id) if id < 0 => View::All,
            Some(id) => View::Category(id),
        };
        state.db.list_envelopes(&email, INBOX, limit, v)
    })
    .await
}

#[tauri::command]
async fn sync_inbox(app: AppHandle, email: String) -> Result<SyncStats, String> {
    let state = app_state(&app);
    blocking(move || {
        let window = state.settings.get().sync.window;
        state.with_session(&email, |creds| sync::sync_folder(&state.db, creds, &email, INBOX, window))
    })
    .await
}

// ---------- 分类与规则 ----------

#[tauri::command]
fn list_categories(state: State<Arc<AppState>>, email: String) -> Result<Vec<Category>, String> {
    state.ensure_categories(&email);
    state.db.list_categories(&email)
}

#[tauri::command]
async fn create_category(app: AppHandle, email: String, name: String, color: String) -> Result<Category, String> {
    let state = app_state(&app);
    blocking(move || state.db.create_category(&email, &name, &color)).await
}

#[tauri::command]
async fn update_category(
    app: AppHandle,
    email: String,
    id: i64,
    name: Option<String>,
    color: Option<String>,
) -> Result<(), String> {
    let state = app_state(&app);
    blocking(move || state.db.update_category(&email, id, name.as_deref(), color.as_deref())).await
}

#[tauri::command]
async fn delete_category(app: AppHandle, email: String, id: i64) -> Result<bool, String> {
    let state = app_state(&app);
    blocking(move || state.db.delete_category(&email, id)).await
}

#[tauri::command]
fn reorder_categories(state: State<Arc<AppState>>, email: String, ids: Vec<i64>) -> Result<(), String> {
    state.db.reorder_categories(&email, &ids)
}

/// view: None 收件箱，Some(id) 移入分类，Some(-1) 移回收件箱
#[tauri::command]
async fn move_messages(app: AppHandle, email: String, uids: Vec<u32>, view: Option<i64>) -> Result<usize, String> {
    let state = app_state(&app);
    blocking(move || {
        let category = view.filter(|v| *v >= 0);
        state.db.move_messages(&email, &uids, category)
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RuleWithCategory {
    #[serde(flatten)]
    rule: rules::Rule,
    category_name: String,
    category_color: String,
}

#[tauri::command]
fn list_rules(state: State<Arc<AppState>>, email: String) -> Result<Vec<RuleWithCategory>, String> {
    let rules = state.db.list_rules(&email)?;
    let categories = state.db.list_categories(&email)?;
    Ok(rules
        .into_iter()
        .map(|rule| {
            let cat = categories.iter().find(|c| c.id == rule.category_id);
            RuleWithCategory {
                rule,
                category_name: cat.map_or_else(|| "已删除".into(), |c| c.name.clone()),
                category_color: cat.map_or_else(|| "#6e7781".into(), |c| c.color.clone()),
            }
        })
        .collect())
}

#[tauri::command]
async fn add_rule(
    app: AppHandle,
    email: String,
    pattern: String,
    category_id: i64,
    apply_existing: bool,
) -> Result<usize, String> {
    let state = app_state(&app);
    blocking(move || state.db.add_rule(&email, &pattern, category_id, apply_existing).map(|(_, moved)| moved)).await
}

#[tauri::command]
async fn delete_rule(app: AppHandle, email: String, id: i64) -> Result<(), String> {
    let state = app_state(&app);
    blocking(move || state.db.delete_rule(&email, id)).await
}

// ---------- 字体 ----------

#[tauri::command]
fn list_fonts() -> &'static [fonts::FontFamily] {
    fonts::system_fonts()
}

/// 发件人是否在信任列表里（完整地址或 @域名）
fn is_trusted(sender: &str, trusted: &[String]) -> bool {
    let sender = sender.to_lowercase();
    trusted.iter().any(|t| if t.starts_with('@') { sender.ends_with(t.as_str()) } else { *t == sender })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageResponse {
    #[serde(flatten)]
    view: MessageView,
    /// 当前是否显示了远程图片（手动放行、全局允许或信任发件人）
    remote_allowed: bool,
}

/// app_dark：前端当前实际生效的是否深色（已经考虑了「跟随系统」）
/// force_dark：阅读区手动切换，null 表示按设置自动决定
#[tauri::command]
async fn get_message(
    app: AppHandle,
    email: String,
    uid: u32,
    sender: String,
    allow_remote: bool,
    app_dark: bool,
    force_dark: Option<bool>,
) -> Result<MessageResponse, String> {
    let state = app_state(&app);
    blocking(move || {
        let settings = state.settings.get();
        let reading = &settings.reading;
        let reading_fonts = settings.appearance.fonts.clone();
        let remote_allowed = allow_remote
            || reading.remote_images == RemoteImages::Allow
            || is_trusted(&sender, &reading.trusted_senders);
        let raw = state.load_raw(&email, INBOX, uid)?;
        let route = PartRoute { account: email, folder: INBOX.into(), uid, part: 0 };
        let opts = render::RenderOptions {
            allow_remote: remote_allowed,
            app_dark,
            preference: reading.email_dark_mode,
            force_dark,
            mail_family: &reading_fonts.mail_family,
            mail_font_size: reading_fonts.mail_font_size,
        };
        Ok(MessageResponse { view: render::render(&raw, &route, &opts)?, remote_allowed })
    })
    .await
}

/// 本地立即更新，再同步到服务器；服务器失败时回滚本地
#[tauri::command]
async fn set_seen(app: AppHandle, email: String, uid: u32, seen: bool) -> Result<(), String> {
    let state = app_state(&app);
    blocking(move || {
        state.db.set_seen(&email, INBOX, uid, seen)?;
        let result = state.with_session(&email, |creds| {
            let mut s = imap_client::connect(creds).map_err(|e| e.to_string())?;
            s.select(INBOX).map_err(|e| e.to_string())?;
            let r = imap_client::store_seen(&mut s, uid, seen).map_err(|e| e.to_string());
            s.logout().ok();
            r
        });
        if result.is_err() {
            state.db.set_seen(&email, INBOX, uid, !seen).ok();
        }
        result
    })
    .await
}

/// 把附件写到 下载目录/MailBox 下，返回完整路径
fn write_attachment(state: &AppState, email: &str, uid: u32, part: u32) -> Result<PathBuf, String> {
    let raw = state.load_raw(email, INBOX, uid)?;
    let (name, _, data) = render::extract_part(&raw, part).ok_or("附件不存在")?;
    std::fs::create_dir_all(&state.attachments_dir).map_err(|e| e.to_string())?;
    let name = render::unique_filename(&render::sanitize_filename(&name), |n| state.attachments_dir.join(n).exists());
    let path = state.attachments_dir.join(name);
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(path)
}

/// 保存附件并在资源管理器中定位
#[tauri::command]
async fn save_attachment(app: AppHandle, email: String, uid: u32, part: u32) -> Result<String, String> {
    let state = app_state(&app);
    let path = blocking(move || write_attachment(&state, &email, uid, part)).await?;
    app.opener().reveal_item_in_dir(&path).ok();
    Ok(path.display().to_string())
}

#[tauri::command]
async fn open_attachment(app: AppHandle, email: String, uid: u32, part: u32) -> Result<String, String> {
    let state = app_state(&app);
    let path = blocking(move || write_attachment(&state, &email, uid, part)).await?;
    app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if !is_http_url(&parsed) {
        return Err("只能打开 http(s) 链接".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ---------- mailbox:// 协议：给阅读 iframe 提供内联图片 ----------

fn serve_part(state: &AppState, path: &str) -> Response<Vec<u8>> {
    let not_found = || Response::builder().status(StatusCode::NOT_FOUND).body(vec![]).unwrap();
    let Some(route) = render::parse_route(path) else { return not_found() };
    // 只读本地缓存，不触发网络请求（打开邮件时正文已缓存）
    let Ok(Some(raw)) = state.db.get_body(&route.account, &route.folder, route.uid) else {
        return not_found();
    };
    let Some((_, mime, data)) = render::extract_part(&raw, route.part) else { return not_found() };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .header("X-Content-Type-Options", "nosniff")
        .body(data)
        .unwrap()
}

fn is_http_url(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "mailto")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol(render::SCHEME, |ctx, request, responder| {
            let state = app_state(ctx.app_handle());
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(serve_part(&state, &path)));
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let db_path = app.path().app_data_dir()?.join("mail.db");
            let state = AppState {
                db: Arc::new(Db::open(&db_path)?),
                store: Arc::new(AccountStore::new(&config_dir)),
                settings: Arc::new(SettingsStore::load(&config_dir)),
                attachments_dir: app.path().download_dir()?.join("MailBox"),
                db_path,
                oauth_cancel: Arc::new(AtomicBool::new(false)),
            };
            app.manage(Arc::new(state));

            // 主窗口在这里创建（tauri.conf.json 里 create=false），
            // 以便拦截邮件里的链接：新窗口请求一律交给系统浏览器
            let handle = app.handle().clone();
            let window_config = app.config().app.windows.first().ok_or("缺少窗口配置")?.clone();
            WebviewWindowBuilder::from_config(app, &window_config)?
                .on_new_window(move |url, _| {
                    if is_http_url(&url) {
                        handle.opener().open_url(url.as_str(), None::<&str>).ok();
                    }
                    NewWindowResponse::Deny
                })
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_accounts,
            save_password_account,
            oauth_login,
            cancel_oauth,
            test_account,
            reorder_accounts,
            remove_account,
            get_settings,
            save_settings,
            test_proxy,
            cache_stats,
            clear_cache,
            list_cached,
            sync_inbox,
            list_categories,
            create_category,
            update_category,
            delete_category,
            reorder_categories,
            move_messages,
            list_rules,
            add_rule,
            delete_rule,
            list_fonts,
            get_message,
            set_seen,
            save_attachment,
            open_attachment,
            open_external,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_trusted_senders() {
        let trusted = vec!["@github.com".to_string(), "boss@corp.com".to_string()];
        let cases = [
            ("noreply@github.com", true),
            ("NoReply@GitHub.com", true),
            ("boss@corp.com", true),
            ("other@corp.com", false),
            ("x@notgithub.com", false),
            ("", false),
        ];
        for (sender, want) in cases {
            assert_eq!(is_trusted(sender, &trusted), want, "{sender}");
        }
    }
}
