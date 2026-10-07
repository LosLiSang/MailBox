//! 应用设置：存 settings.json；代理密码单独存 keyring。

use crate::net::{validate_proxy, ProxyConfig};
pub use crate::render::DarkPreference;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
};

const FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncSettings {
    /// 首次同步 / 每次最多拉取的邮件数
    pub window: u32,
    /// 自动同步间隔（分钟），0 表示关闭
    pub auto_sync_minutes: u32,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self { window: 200, auto_sync_minutes: 5 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RemoteImages {
    #[default]
    Block,
    Allow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReadingSettings {
    pub remote_images: RemoteImages,
    /// 拦截模式下仍然自动显示图片的发件人（邮箱地址或 @域名）
    pub trusted_senders: Vec<String>,
    pub mark_read_on_open: bool,
    /// 深色主题下邮件正文如何显示
    pub email_dark_mode: DarkPreference,
}

impl Default for ReadingSettings {
    fn default() -> Self {
        Self {
            remote_images: RemoteImages::Block,
            trusted_senders: vec![],
            mark_read_on_open: true,
            email_dark_mode: DarkPreference::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    /// system / light / dark
    pub theme: String,
    /// comfortable / compact
    pub density: String,
}

/// 自己在 Google Cloud / Azure 注册的 OAuth 应用
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct OAuthApps {
    pub google_client_id: String,
    /// Google 桌面应用的 client secret 按官方说明不视为机密，但仍必须提交
    pub google_client_secret: String,
    pub microsoft_client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub proxy: ProxyConfig,
    pub sync: SyncSettings,
    pub reading: ReadingSettings,
    pub appearance: AppearanceSettings,
    pub oauth: OAuthApps,
}

pub const WINDOW_MIN: u32 = 50;
pub const WINDOW_MAX: u32 = 5000;

/// 校验并规整：去空白、去重、限制范围
pub fn normalize(mut s: Settings) -> Result<Settings, String> {
    s.proxy.host = s.proxy.host.trim().to_string();
    s.proxy.username = s.proxy.username.trim().to_string();
    validate_proxy(&s.proxy)?;

    if !(WINDOW_MIN..=WINDOW_MAX).contains(&s.sync.window) {
        return Err(format!("同步数量需在 {WINDOW_MIN}–{WINDOW_MAX} 之间"));
    }
    s.sync.auto_sync_minutes = s.sync.auto_sync_minutes.min(24 * 60);

    let mut trusted: Vec<String> = s
        .reading
        .trusted_senders
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    trusted.sort();
    trusted.dedup();
    s.reading.trusted_senders = trusted;

    if !matches!(s.appearance.theme.as_str(), "light" | "dark") {
        s.appearance.theme = "system".into();
    }
    if s.appearance.density != "compact" {
        s.appearance.density = "comfortable".into();
    }

    for v in [&mut s.oauth.google_client_id, &mut s.oauth.google_client_secret, &mut s.oauth.microsoft_client_id] {
        *v = v.trim().to_string();
    }
    Ok(s)
}

pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join(FILE);
        // 文件不存在或损坏时用默认值，避免设置文件问题导致应用打不开
        let current = fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
            .and_then(|s| normalize(s).ok())
            .unwrap_or_else(|| normalize(Settings::default()).unwrap());
        Self { path, current: RwLock::new(current) }
    }

    pub fn get(&self) -> Settings {
        self.current.read().unwrap().clone()
    }

    /// 带上 keyring 里的代理密码，供建立连接时使用
    pub fn proxy(&self) -> ProxyConfig {
        let mut proxy = self.get().proxy;
        if !proxy.username.is_empty() {
            proxy.password = crate::secrets::get(crate::secrets::PROXY_PASSWORD_KEY)
                .ok()
                .flatten()
                .unwrap_or_default();
        }
        proxy
    }

    pub fn save(&self, settings: Settings) -> Result<Settings, String> {
        let settings = normalize(settings)?;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        *self.current.write().unwrap() = settings.clone();
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::ProxyKind;

    #[test]
    fn defaults_are_valid_and_old_files_get_defaults() {
        let s = normalize(Settings::default()).unwrap();
        assert_eq!(s.sync.window, 200);
        assert_eq!(s.appearance.theme, "system");
        assert!(s.reading.mark_read_on_open);

        // 旧版本没有的字段用默认值补齐
        let partial: Settings = serde_json::from_str(r#"{"sync":{"window":500}}"#).unwrap();
        assert_eq!(partial.sync.window, 500);
        assert_eq!(partial.sync.auto_sync_minutes, 5);
        assert_eq!(partial.reading.remote_images, RemoteImages::Block);
        assert_eq!(partial.reading.email_dark_mode, DarkPreference::Auto);
    }

    #[test]
    fn normalizes_values() {
        let mut s = Settings::default();
        s.reading.trusted_senders = vec![" A@QQ.com ".into(), "a@qq.com".into(), "".into(), "@github.com".into()];
        s.appearance.theme = "purple".into();
        s.appearance.density = "".into();
        s.sync.auto_sync_minutes = 99999;
        s.proxy = ProxyConfig { kind: ProxyKind::Socks5, host: " 127.0.0.1 ".into(), port: 7890, ..Default::default() };

        let s = normalize(s).unwrap();
        assert_eq!(s.reading.trusted_senders, ["@github.com", "a@qq.com"]);
        assert_eq!(s.appearance.theme, "system");
        assert_eq!(s.appearance.density, "comfortable");
        assert_eq!(s.sync.auto_sync_minutes, 1440);
        assert_eq!(s.proxy.host, "127.0.0.1");
    }

    #[test]
    fn rejects_invalid() {
        let cases: [fn(&mut Settings); 3] = [
            |s| s.sync.window = 10,
            |s| s.sync.window = 100_000,
            |s| s.proxy = ProxyConfig { kind: ProxyKind::Http, host: "".into(), port: 1, ..Default::default() },
        ];
        for mutate in cases {
            let mut s = Settings::default();
            mutate(&mut s);
            assert!(normalize(s).is_err());
        }
    }

    #[test]
    fn proxy_password_never_serialized() {
        let mut s = Settings::default();
        s.proxy.password = "secret".into();
        assert!(!serde_json::to_string(&s).unwrap().contains("secret"));
    }
}
