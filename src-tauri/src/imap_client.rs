//! IMAP 网络层。imap 2.x 是同步阻塞 API，由调用方放到阻塞线程里执行。

use crate::{
    mail::{parse_envelope, Envelope},
    net::{self, ProxyConfig},
    oauth,
};
use imap::types::{Fetch, Flag};

pub enum Auth {
    /// 密码 / 授权码 / 应用专用密码
    Password(String),
    /// OAuth access token，用 XOAUTH2 认证
    OAuth(String),
}

pub struct Credentials<'a> {
    pub host: &'a str,
    pub port: u16,
    pub username: &'a str,
    pub auth: Auth,
    pub proxy: ProxyConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("{0}")]
    Net(String),
    #[error("TLS 握手失败: {0}")]
    Tls(String),
    #[error("IMAP 错误: {0}")]
    Imap(#[from] imap::Error),
}

pub type Session = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;
type Client = imap::Client<native_tls::TlsStream<std::net::TcpStream>>;

struct XOAuth2(String);

impl imap::Authenticator for XOAuth2 {
    type Response = String;
    fn process(&self, challenge: &[u8]) -> String {
        // 首次质询为空，返回凭据；认证失败时服务器会带着错误详情再次质询，
        // 按 XOAUTH2 协议回空串，服务器随后返回 NO 结束认证
        if challenge.is_empty() { self.0.clone() } else { String::new() }
    }
}

/// 建立 TLS 连接并读取服务器问候，按配置走代理
pub fn open(host: &str, port: u16, proxy: &ProxyConfig) -> Result<Client, MailError> {
    let tcp = net::connect_tcp(proxy, host, port).map_err(MailError::Net)?;
    let tls = native_tls::TlsConnector::new().map_err(|e| MailError::Tls(e.to_string()))?;
    let stream = tls.connect(host, tcp).map_err(|e| MailError::Tls(e.to_string()))?;
    let mut client = imap::Client::new(stream);
    client.read_greeting()?;
    Ok(client)
}

pub fn connect(c: &Credentials) -> Result<Session, MailError> {
    let client = open(c.host, c.port, &c.proxy)?;
    // 注意：不要发 ID 命令。imap-proto 0.10 解析不了 `* ID (...)` 响应，
    // 会留下未读的 tagged OK，导致后续命令标签错位并 panic。
    let session = match &c.auth {
        Auth::Password(p) => client.login(c.username, p),
        Auth::OAuth(token) => {
            client.authenticate("XOAUTH2", &XOAuth2(oauth::xoauth2_payload(c.username, token)))
        }
    };
    Ok(session.map_err(|(e, _)| e)?)
}

/// 仅验证能否登录
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

/// 预缓存前先检查大小，避免后台下载大型附件。PEEK 不改变已读状态。
pub fn fetch_raw_bounded(s: &mut Session, uid: u32, max_bytes: u32) -> imap::error::Result<Option<Vec<u8>>> {
    let sizes = s.uid_fetch(uid.to_string(), "(UID RFC822.SIZE)")?;
    let size = sizes.iter().find(|f| f.uid == Some(uid)).and_then(|f| f.size);
    if !matches!(size, Some(bytes) if bytes <= max_bytes) {
        return Ok(None);
    }
    fetch_raw(s, uid)
}

pub fn store_seen_many(s: &mut Session, uids: &[u32], seen: bool) -> imap::error::Result<()> {
    if uids.is_empty() { return Ok(()); }
    let op = if seen { "+FLAGS.SILENT" } else { "-FLAGS.SILENT" };
    s.uid_store(crate::sync::uid_set(uids), format!(r"{op} (\Seen)"))?;
    Ok(())
}

pub fn store_seen(s: &mut Session, uid: u32, seen: bool) -> imap::error::Result<()> {
    store_seen_many(s, &[uid], seen)
}
