mod accounts;
mod db;
mod imap_client;
mod mail;
mod render;
mod sync;

use accounts::{AccountConfig, AccountStore};
use db::Db;
use imap_client::Credentials;
use mail::Envelope;
use render::{MessageView, PartRoute};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
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
    attachments_dir: PathBuf,
}

/// 在阻塞线程池里执行同步 IMAP / SQLite 调用，避免卡住 UI
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// 读取账号配置和授权码，再执行一个需要 IMAP 连接的操作
fn with_session<T>(
    store: &AccountStore,
    email: &str,
    f: impl FnOnce(&Credentials) -> Result<T, String>,
) -> Result<T, String> {
    let account = store.get(email)?;
    let password = accounts::get_secret(email)?;
    f(&Credentials {
        host: &account.host,
        port: account.port,
        username: &account.email,
        password: &password,
    })
}

/// 优先从本地缓存取原始邮件，没有再从服务器下载并缓存
fn load_raw(state: &AppState, email: &str, folder: &str, uid: u32) -> Result<Vec<u8>, String> {
    if let Some(raw) = state.db.get_body(email, folder, uid)? {
        return Ok(raw);
    }
    let raw = with_session(&state.store, email, |creds| {
        let mut s = imap_client::connect(creds).map_err(|e| e.to_string())?;
        s.select(folder).map_err(|e| e.to_string())?;
        let raw = imap_client::fetch_raw(&mut s, uid).map_err(|e| e.to_string());
        s.logout().ok();
        raw?.ok_or_else(|| "服务器上找不到这封邮件，可能已被删除".to_string())
    })?;
    state.db.put_body(email, folder, uid, &raw)?;
    Ok(raw)
}

fn app_state(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

// ---------- 账号 ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewAccount {
    email: String,
    display_name: String,
    host: String,
    port: u16,
    password: String,
}

#[tauri::command]
fn list_accounts(state: State<Arc<AppState>>) -> Result<Vec<AccountConfig>, String> {
    state.store.list()
}

/// 先验证能登录，再保存配置与授权码
#[tauri::command]
async fn add_account(app: AppHandle, account: NewAccount) -> Result<AccountConfig, String> {
    let state = app_state(&app);
    let config = AccountConfig {
        email: accounts::normalize_email(&account.email),
        display_name: account.display_name.trim().to_string(),
        host: account.host.trim().to_string(),
        port: account.port,
    };
    accounts::validate(&config)?;

    let password = account.password;
    blocking(move || {
        imap_client::verify(&Credentials {
            host: &config.host,
            port: config.port,
            username: &config.email,
            password: &password,
        })
        .map_err(|e| format!("登录失败，请检查授权码和服务器: {e}"))?;
        state.store.save(config.clone(), &password)?;
        Ok(config)
    })
    .await
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

// ---------- 邮件列表 ----------

#[tauri::command]
async fn list_cached(app: AppHandle, email: String, limit: u32) -> Result<Vec<Envelope>, String> {
    let state = app_state(&app);
    blocking(move || state.db.list_envelopes(&email, INBOX, limit)).await
}

#[tauri::command]
async fn sync_inbox(app: AppHandle, email: String) -> Result<SyncStats, String> {
    let state = app_state(&app);
    blocking(move || {
        with_session(&state.store, &email, |creds| sync::sync_folder(&state.db, creds, &email, INBOX))
    })
    .await
}

// ---------- 阅读 ----------

#[tauri::command]
async fn get_message(app: AppHandle, email: String, uid: u32, allow_remote: bool) -> Result<MessageView, String> {
    let state = app_state(&app);
    blocking(move || {
        let raw = load_raw(&state, &email, INBOX, uid)?;
        let route = PartRoute { account: email, folder: INBOX.into(), uid, part: 0 };
        render::render(&raw, &route, allow_remote)
    })
    .await
}

/// 本地立即更新，再同步到服务器；服务器失败时回滚本地
#[tauri::command]
async fn set_seen(app: AppHandle, email: String, uid: u32, seen: bool) -> Result<(), String> {
    let state = app_state(&app);
    blocking(move || {
        state.db.set_seen(&email, INBOX, uid, seen)?;
        let result = with_session(&state.store, &email, |creds| {
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
    let raw = load_raw(state, email, INBOX, uid)?;
    let (name, _, data) = render::extract_part(&raw, part).ok_or("附件不存在")?;
    std::fs::create_dir_all(&state.attachments_dir).map_err(|e| e.to_string())?;
    let name = render::unique_filename(&render::sanitize_filename(&name), |n| {
        state.attachments_dir.join(n).exists()
    });
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
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
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
            let state = AppState {
                db: Arc::new(Db::open(&app.path().app_data_dir()?.join("mail.db"))?),
                store: Arc::new(AccountStore::new(&config_dir)),
                attachments_dir: app.path().download_dir()?.join("MailBox"),
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
            add_account,
            remove_account,
            list_cached,
            sync_inbox,
            get_message,
            set_seen,
            save_attachment,
            open_attachment,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
