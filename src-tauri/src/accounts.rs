//! 账号配置：非敏感信息存 JSON 文件，凭据（授权码 / OAuth token）存系统凭据管理器。

use crate::{oauth::OAuthProvider, secrets};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const CONFIG_FILE: &str = "accounts.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    #[default]
    Password,
    Google,
    Microsoft,
}

impl AuthKind {
    pub fn oauth_provider(self) -> Option<OAuthProvider> {
        match self {
            Self::Password => None,
            Self::Google => Some(OAuthProvider::Google),
            Self::Microsoft => Some(OAuthProvider::Microsoft),
        }
    }
}

impl From<OAuthProvider> for AuthKind {
    fn from(p: OAuthProvider) -> Self {
        match p {
            OAuthProvider::Google => Self::Google,
            OAuthProvider::Microsoft => Self::Microsoft,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountConfig {
    /// 邮箱地址，同时作为账号唯一标识
    pub email: String,
    pub display_name: String,
    /// 服务商 id（qq / gmail / outlook / icloud / custom ...），只用于界面显示
    pub provider: String,
    pub host: String,
    pub port: u16,
    pub auth: AuthKind,
    /// 是否经过「设置 → 代理」连接
    pub use_proxy: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConfigFile {
    accounts: Vec<AccountConfig>,
}

// ---------- 纯函数 ----------

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn validate(account: &AccountConfig) -> Result<(), String> {
    let email = &account.email;
    let valid_email = email.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
    });
    if !valid_email {
        return Err(format!("邮箱地址格式不正确: {email}"));
    }
    if account.host.trim().is_empty() {
        return Err("IMAP 服务器不能为空".into());
    }
    if account.port == 0 {
        return Err("端口不能为 0".into());
    }
    Ok(())
}

/// 新增或更新（按 email 匹配），保持原有顺序；新账号追加在末尾。
pub fn upsert(accounts: &mut Vec<AccountConfig>, account: AccountConfig) {
    match accounts.iter_mut().find(|a| a.email == account.email) {
        Some(existing) => *existing = account,
        None => accounts.push(account),
    }
}

pub fn remove(accounts: &mut Vec<AccountConfig>, email: &str) -> bool {
    let before = accounts.len();
    accounts.retain(|a| a.email != email);
    accounts.len() != before
}

/// 按给定顺序重排；未出现在 order 里的账号保持原相对顺序放在末尾
pub fn reorder(accounts: &mut Vec<AccountConfig>, order: &[String]) {
    let rank = |a: &AccountConfig| order.iter().position(|e| *e == a.email).unwrap_or(usize::MAX);
    accounts.sort_by_key(rank);
}

// ---------- 文件 ----------

pub struct AccountStore {
    path: PathBuf,
}

impl AccountStore {
    pub fn new(config_dir: &Path) -> Self {
        Self { path: config_dir.join(CONFIG_FILE) }
    }

    pub fn list(&self) -> Result<Vec<AccountConfig>, String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str::<ConfigFile>(&text)
                .map(|c| c.accounts)
                .map_err(|e| format!("配置文件损坏 {}: {e}", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(e.to_string()),
        }
    }

    fn write(&self, accounts: Vec<AccountConfig>) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&ConfigFile { accounts }).map_err(|e| e.to_string())?;
        // 先写临时文件再改名，避免写一半崩溃导致配置损坏
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn get(&self, email: &str) -> Result<AccountConfig, String> {
        self.list()?
            .into_iter()
            .find(|a| a.email == email)
            .ok_or_else(|| format!("账号不存在: {email}"))
    }

    pub fn upsert(&self, account: AccountConfig) -> Result<(), String> {
        let mut accounts = self.list()?;
        upsert(&mut accounts, account);
        self.write(accounts)
    }

    pub fn reorder(&self, order: &[String]) -> Result<Vec<AccountConfig>, String> {
        let mut accounts = self.list()?;
        reorder(&mut accounts, order);
        self.write(accounts.clone())?;
        Ok(accounts)
    }

    /// 删除配置和所有相关凭据
    pub fn delete(&self, email: &str) -> Result<(), String> {
        let mut accounts = self.list()?;
        remove(&mut accounts, email);
        self.write(accounts)?;
        secrets::delete(&secrets::account_password_key(email))?;
        secrets::delete(&secrets::oauth_token_key(email))
    }
}

pub fn get_password(email: &str) -> Result<String, String> {
    secrets::get(&secrets::account_password_key(email))?
        .ok_or_else(|| "找不到保存的授权码，请在「设置 → 账号」中更新".to_string())
}

pub fn set_password(email: &str, password: &str) -> Result<(), String> {
    secrets::set(&secrets::account_password_key(email), password)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(email: &str, name: &str) -> AccountConfig {
        AccountConfig {
            email: email.into(),
            display_name: name.into(),
            host: "imap.qq.com".into(),
            port: 993,
            ..Default::default()
        }
    }

    #[test]
    fn validates_accounts() {
        let cases = [
            (acc("a@qq.com", ""), true),
            (acc("a@sub.example.com", ""), true),
            (acc("aqq.com", ""), false),
            (acc("@qq.com", ""), false),
            (acc("a@qq", ""), false),
            (acc("a@.com", ""), false),
            (AccountConfig { host: " ".into(), ..acc("a@qq.com", "") }, false),
            (AccountConfig { port: 0, ..acc("a@qq.com", "") }, false),
        ];
        for (account, ok) in cases {
            assert_eq!(validate(&account).is_ok(), ok, "{account:?}");
        }
    }

    #[test]
    fn normalizes_email() {
        assert_eq!(normalize_email("  Foo@QQ.com "), "foo@qq.com");
    }

    #[test]
    fn upsert_remove_and_reorder_keep_order() {
        let mut list = vec![acc("a@qq.com", "A"), acc("b@qq.com", "B")];

        upsert(&mut list, acc("c@qq.com", "C"));
        upsert(&mut list, acc("a@qq.com", "A2"));
        let names: Vec<_> = list.iter().map(|a| a.display_name.as_str()).collect();
        assert_eq!(names, ["A2", "B", "C"]);

        reorder(&mut list, &["c@qq.com".into(), "a@qq.com".into()]);
        let emails: Vec<_> = list.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(emails, ["c@qq.com", "a@qq.com", "b@qq.com"]);

        assert!(remove(&mut list, "b@qq.com"));
        assert!(!remove(&mut list, "missing@qq.com"));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn reads_config_written_by_older_versions() {
        let old = r#"{"accounts":[{"email":"a@qq.com","displayName":"","host":"imap.qq.com","port":993}]}"#;
        let file: ConfigFile = serde_json::from_str(old).unwrap();
        let a = &file.accounts[0];
        assert_eq!((a.auth, a.use_proxy, a.provider.as_str()), (AuthKind::Password, false, ""));
    }
}
