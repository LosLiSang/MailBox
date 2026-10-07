//! IMAP 网络层：连接、登录、拉取最近邮件头。
//! imap 2.x 是同步阻塞 API，由调用方放到阻塞线程里执行。

use crate::mail::{parse_envelope, recent_range, Envelope};

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

type Session = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

fn login(c: &Credentials) -> Result<Session, MailError> {
    let tls = native_tls::TlsConnector::builder().build()?;
    let client = imap::connect((c.host, c.port), c.host, &tls)?;
    Ok(client.login(c.username, c.password).map_err(|(e, _)| e)?)
}

/// 仅验证能否登录，用于添加账号时校验授权码。
pub fn verify(c: &Credentials) -> Result<(), MailError> {
    login(c)?.logout().ok();
    Ok(())
}

pub fn fetch_recent(c: &Credentials, limit: u32) -> Result<Vec<Envelope>, MailError> {
    let mut session = login(c)?;

    let mailbox = session.select("INBOX")?;
    let Some(range) = recent_range(mailbox.exists, limit) else {
        session.logout().ok();
        return Ok(vec![]);
    };

    // BODY.PEEK 不会把邮件标记为已读
    let fetches = session.fetch(range, "(UID FLAGS BODY.PEEK[HEADER])")?;

    let mut envelopes: Vec<Envelope> = fetches
        .iter()
        .filter_map(|f| {
            let uid = f.uid?;
            let header = f.header()?;
            let seen = f.flags().iter().any(|fl| matches!(fl, imap::types::Flag::Seen));
            Some(parse_envelope(uid, header, seen))
        })
        .collect();

    session.logout().ok();

    // 最新的在前
    envelopes.sort_by(|a, b| b.uid.cmp(&a.uid));
    Ok(envelopes)
}
