mod accounts;
mod imap_client;
mod mail;

use accounts::{AccountConfig, AccountStore};
use imap_client::Credentials;
use mail::Envelope;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

fn store(app: &AppHandle) -> Result<AccountStore, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(AccountStore::new(&dir))
}

/// 在阻塞线程池里执行同步 IMAP 调用，避免卡住 UI
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

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
fn list_accounts(app: AppHandle) -> Result<Vec<AccountConfig>, String> {
    store(&app)?.list()
}

/// 先验证能登录，再保存配置与授权码
#[tauri::command]
async fn add_account(app: AppHandle, account: NewAccount) -> Result<AccountConfig, String> {
    let store = store(&app)?;
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
        store.save(config.clone(), &password)?;
        Ok(config)
    })
    .await
}

#[tauri::command]
fn remove_account(app: AppHandle, email: String) -> Result<(), String> {
    store(&app)?.delete(&email)
}

#[tauri::command]
async fn fetch_recent(app: AppHandle, email: String, limit: u32) -> Result<Vec<Envelope>, String> {
    let store = store(&app)?;
    blocking(move || {
        let account = store.get(&email)?;
        let password = accounts::get_secret(&email)?;
        imap_client::fetch_recent(
            &Credentials {
                host: &account.host,
                port: account.port,
                username: &account.email,
                password: &password,
            },
            limit,
        )
        .map_err(|e| e.to_string())
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_accounts,
            add_account,
            remove_account,
            fetch_recent
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
