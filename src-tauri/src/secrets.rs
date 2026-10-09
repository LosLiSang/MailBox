//! 敏感信息统一存系统凭据管理器。
//!
//! Windows 凭据单条上限约 2.5KB（按 UTF-16 约 1280 字符），微软的 OAuth refresh token
//! 可能超过这个长度，所以长值会拆成多条：主条目存标记 `\0chunks:N`，分片存 `<key>#i`。

const SERVICE: &str = "MailBox";
const CHUNK_CHARS: usize = 1000;
const MARKER: &str = "\0chunks:";

pub fn account_password_key(email: &str) -> String {
    // 与早期版本保持一致：直接用邮箱地址作为键
    email.to_string()
}

pub fn oauth_token_key(email: &str) -> String {
    format!("oauth:{email}")
}

pub const PROXY_PASSWORD_KEY: &str = "proxy";

// ---------- 纯函数 ----------

pub fn split_chunks(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    chars
        .chunks(CHUNK_CHARS)
        .map(|c| c.iter().collect())
        .collect()
}

/// 主条目如果是分片标记，返回分片数量
pub fn parse_marker(stored: &str) -> Option<usize> {
    stored.strip_prefix(MARKER)?.parse().ok()
}

fn chunk_key(key: &str, i: usize) -> String {
    format!("{key}#{i}")
}

// ---------- keyring ----------

fn entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())
}

fn raw_get(key: &str) -> Result<Option<String>, String> {
    match entry(key)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取凭据失败: {e}")),
    }
}

fn raw_set(key: &str, value: &str) -> Result<(), String> {
    entry(key)?
        .set_password(value)
        .map_err(|e| format!("保存凭据失败: {e}"))
}

fn raw_delete(key: &str) -> Result<(), String> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除凭据失败: {e}")),
    }
}

pub fn get(key: &str) -> Result<Option<String>, String> {
    let Some(stored) = raw_get(key)? else {
        return Ok(None);
    };
    let Some(n) = parse_marker(&stored) else {
        return Ok(Some(stored));
    };
    let mut value = String::new();
    for i in 0..n {
        value.push_str(&raw_get(&chunk_key(key, i))?.ok_or("凭据分片缺失，请重新登录")?);
    }
    Ok(Some(value))
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    delete(key)?;
    if value.chars().count() <= CHUNK_CHARS {
        return raw_set(key, value);
    }
    let chunks = split_chunks(value);
    for (i, chunk) in chunks.iter().enumerate() {
        raw_set(&chunk_key(key, i), chunk)?;
    }
    raw_set(key, &format!("{MARKER}{}", chunks.len()))
}

pub fn delete(key: &str) -> Result<(), String> {
    if let Some(n) = raw_get(key)?.as_deref().and_then(parse_marker) {
        for i in 0..n {
            raw_delete(&chunk_key(key, i))?;
        }
    }
    raw_delete(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_into_chunks() {
        assert!(split_chunks("").is_empty());
        assert_eq!(split_chunks("abc"), ["abc"]);

        let long: String = "授".repeat(CHUNK_CHARS) + "权码";
        let chunks = split_chunks(&long);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), CHUNK_CHARS);
        assert_eq!(chunks[1], "权码");
        assert_eq!(chunks.concat(), long);
    }

    #[test]
    fn parses_marker() {
        let cases = [
            ("\0chunks:3", Some(3)),
            ("\0chunks:x", None),
            ("chunks:3", None),
            ("abcdefghijklmnop", None),
            ("", None),
        ];
        for (stored, want) in cases {
            assert_eq!(parse_marker(stored), want, "{stored:?}");
        }
    }
}
