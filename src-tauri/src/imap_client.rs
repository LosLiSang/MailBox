//! IMAP 网络层。imap 2.x 是同步阻塞 API，由调用方放到阻塞线程里执行。

use crate::mail::{parse_envelope, Envelope};
use imap::types::{Fetch, Flag};

pub struct Credentials<'a> {
    pub host: &'a str,
    pub port: u16,
    pub username: &'a str,
    /// QQ/163 邮箱这里填「授权码」，不是登录密码
    pub password: &'a str,
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("TLS 初始化失败: {0}")]
    Tls(#[from] native_tls::Error),
    #[error("IMAP 错误: {0}")]
    Imap(#[from] imap::Error),
}

pub type Session = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

pub fn connect(c: &Credentials) -> Result<Session, MailError> {
    let tls = native_tls::TlsConnector::builder().build()?;
    let client = imap::connect((c.host, c.port), c.host, &tls)?;
    // 注意：不要发 ID 命令。imap-proto 0.10 解析不了 `* ID (...)` 响应，
    // 会留下未读的 tagged OK，导致后续命令标签错位并 panic。
    // 网易邮箱要求 ID，需要等换成 async-imap 后再支持。
    Ok(client.login(c.username, c.password).map_err(|(e, _)| e)?)
}

/// 仅验证能否登录，用于添加账号时校验授权码。
pub fn verify(c: &Credentials) -> Result<(), MailError> {
    connect(c)?.logout().ok();
    Ok(())
}

fn is_seen(f: &Fetch) -> bool {
    f.flags().iter().any(|fl| matches!(fl, Flag::Seen))
}

fn flags_of(fetches: &[Fetch]) -> Vec<(u32, bool)> {
    fetches.iter().filter_map(|f| Some((f.uid?, is_seen(f)))).collect()
}

/// 按序号区间（如 "81:100"）拉取 (uid, seen)
pub fn fetch_flags_by_seq(s: &mut Session, range: &str) -> imap::error::Result<Vec<(u32, bool)>> {
    Ok(flags_of(&s.fetch(range, "(UID FLAGS)")?))
}

/// 按 UID 集合（如 "120:*"）拉取 (uid, seen)
pub fn fetch_flags_by_uid(s: &mut Session, set: &str) -> imap::error::Result<Vec<(u32, bool)>> {
    Ok(flags_of(&s.uid_fetch(set, "(UID FLAGS)")?))
}

/// 拉取邮件头。BODY.PEEK 不会把邮件标记为已读
pub fn fetch_headers(s: &mut Session, set: &str) -> imap::error::Result<Vec<Envelope>> {
    let fetches = s.uid_fetch(set, "(UID FLAGS BODY.PEEK[HEADER])")?;
    Ok(fetches
        .iter()
        .filter_map(|f| Some(parse_envelope(f.uid?, f.header()?, is_seen(f))))
        .collect())
}

/// 拉取完整原始邮件（含附件），不改变已读状态
pub fn fetch_raw(s: &mut Session, uid: u32) -> imap::error::Result<Option<Vec<u8>>> {
    let fetches = s.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")?;
    Ok(fetches
        .iter()
        .find(|f| f.uid == Some(uid))
        .and_then(|f| f.body())
        .map(<[u8]>::to_vec))
}

pub fn store_seen(s: &mut Session, uid: u32, seen: bool) -> imap::error::Result<()> {
    let op = if seen { "+FLAGS.SILENT" } else { "-FLAGS.SILENT" };
    s.uid_store(uid.to_string(), format!(r"{op} (\Seen)"))?;
    Ok(())
}
