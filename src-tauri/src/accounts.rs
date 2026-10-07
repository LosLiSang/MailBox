//! 账号配置：非敏感信息存 JSON 文件，授权码存系统凭据管理器（keyring）。

use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

const KEYRING_SERVICE: &str = "MailBox";
const CONFIG_FILE: &str = "accounts.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountConfig {
    /// 邮箱地址，同时作为账号唯一标识
    pub email: String,
    pub display_name: String,
    pub host: String,
    pub port: u16,
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
    let valid_email = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'));
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

// ---------- 文件 & keyring ----------

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

    pub fn save(&self, account: AccountConfig, password: &str) -> Result<(), String> {
        set_secret(&account.email, password)?;
        let mut accounts = self.list()?;
        upsert(&mut accounts, account);
        self.write(accounts)
    }

    pub fn delete(&self, email: &str) -> Result<(), String> {
        let mut accounts = self.list()?;
        remove(&mut accounts, email);
        self.write(accounts)?;
        delete_secret(email)
    }
}

fn entry(email: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, email).map_err(|e| e.to_string())
}

fn set_secret(email: &str, password: &str) -> Result<(), String> {
    entry(email)?.set_password(password).map_err(|e| format!("保存授权码失败: {e}"))
}

pub fn get_secret(email: &str) -> Result<String, String> {
    entry(email)?
        .get_password()
        .map_err(|e| format!("读取授权码失败，请重新添加账号: {e}"))
}

fn delete_secret(email: &str) -> Result<(), String> {
    match entry(email)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(email: &str, name: &str) -> AccountConfig {
        AccountConfig { email: email.into(), display_name: name.into(), host: "imap.qq.com".into(), port: 993 }
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
    fn upsert_and_remove_keep_order() {
        let mut list = vec![acc("a@qq.com", "A"), acc("b@qq.com", "B")];

        upsert(&mut list, acc("c@qq.com", "C"));
        upsert(&mut list, acc("a@qq.com", "A2"));
        let names: Vec<_> = list.iter().map(|a| a.display_name.as_str()).collect();
        assert_eq!(names, ["A2", "B", "C"]);

        assert!(remove(&mut list, "b@qq.com"));
        assert!(!remove(&mut list, "missing@qq.com"));
        let emails: Vec<_> = list.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(emails, ["a@qq.com", "c@qq.com"]);
    }
}
