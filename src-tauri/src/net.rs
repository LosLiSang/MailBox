//! 网络出口：直连或经代理（HTTP CONNECT / SOCKS5）建立 TCP 连接，
//! IMAP 和 OAuth 的 HTTP 请求都走这里，保证代理设置对两者一致生效。

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IO_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProxyKind {
    #[default]
    None,
    Http,
    Socks5,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxyConfig {
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// 代理密码不写进 settings.json，运行时从 keyring 填入
    #[serde(skip)]
    pub password: String,
}

impl ProxyConfig {
    pub fn enabled(&self) -> bool {
        self.kind != ProxyKind::None && !self.host.trim().is_empty() && self.port != 0
    }
}

pub fn validate_proxy(p: &ProxyConfig) -> Result<(), String> {
    if p.kind == ProxyKind::None {
        return Ok(());
    }
    if p.host.trim().is_empty() {
        return Err("代理服务器地址不能为空".into());
    }
    if p.host.contains("://") {
        return Err("代理地址只填主机名或 IP，不要带 http:// 或 socks5://".into());
    }
    if p.port == 0 {
        return Err("代理端口不能为 0".into());
    }
    Ok(())
}

/// 生成 HTTP CONNECT 请求
pub fn connect_request(
    target_host: &str,
    target_port: u16,
    username: &str,
    password: &str,
) -> String {
    let mut req = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\n"
    );
    if !username.is_empty() {
        let token =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    req.push_str("\r\n");
    req
}

/// 解析 CONNECT 响应状态行，2xx 视为成功
pub fn parse_connect_status(head: &str) -> Result<(), String> {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.splitn(3, ' ');
    let (version, code) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    if !version.starts_with("HTTP/") {
        return Err(format!("代理返回了无法识别的响应: {line}"));
    }
    match code.parse::<u16>() {
        Ok(200..=299) => Ok(()),
        Ok(407) => Err("代理需要认证，请检查代理用户名和密码".into()),
        Ok(_) => Err(format!("代理拒绝连接: {line}")),
        Err(_) => Err(format!("代理返回了无法识别的响应: {line}")),
    }
}

fn tcp_connect(host: &str, port: u16) -> io::Result<TcpStream> {
    let mut last_err = io::Error::new(io::ErrorKind::NotFound, format!("无法解析地址 {host}"));
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 读取到 `\r\n\r\n` 为止，逐字节读，避免吞掉后面的 TLS 数据
fn read_http_head(stream: &mut TcpStream) -> io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "代理提前关闭了连接",
            ));
        }
        buf.push(byte[0]);
        if buf.len() > 16 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "代理响应头过长"));
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// 建立到目标的 TCP 连接，按配置走代理
pub fn connect_tcp(proxy: &ProxyConfig, host: &str, port: u16) -> Result<TcpStream, String> {
    let stream = if !proxy.enabled() {
        tcp_connect(host, port).map_err(|e| format!("连接 {host}:{port} 失败: {e}"))?
    } else {
        let (ph, pp) = (proxy.host.trim(), proxy.port);
        let proxy_err = |e: io::Error| format!("连接代理 {ph}:{pp} 失败: {e}");
        match proxy.kind {
            ProxyKind::Http => {
                let mut s = tcp_connect(ph, pp).map_err(proxy_err)?;
                s.set_read_timeout(Some(CONNECT_TIMEOUT)).ok();
                s.write_all(
                    connect_request(host, port, &proxy.username, &proxy.password).as_bytes(),
                )
                .map_err(proxy_err)?;
                parse_connect_status(&read_http_head(&mut s).map_err(proxy_err)?)?;
                s
            }
            ProxyKind::Socks5 => {
                // 把域名交给代理解析（相当于 socks5h），国内 DNS 被污染时也能连上 Gmail
                let target = (host, port);
                let result = if proxy.username.is_empty() {
                    socks::Socks5Stream::connect((ph, pp), target)
                } else {
                    socks::Socks5Stream::connect_with_password(
                        (ph, pp),
                        target,
                        &proxy.username,
                        &proxy.password,
                    )
                };
                result.map_err(proxy_err)?.into_inner()
            }
            ProxyKind::None => unreachable!(),
        }
    };
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
    Ok(stream)
}

/// 发 HTTPS 请求用的 agent。HTTP 状态码不当成错误，由调用方读出错误详情
pub fn http_agent(proxy: &ProxyConfig) -> Result<ureq::Agent, String> {
    let proxy = if proxy.enabled() {
        let protocol = match proxy.kind {
            ProxyKind::Http => ureq::ProxyProtocol::Http,
            _ => ureq::ProxyProtocol::Socks5h,
        };
        let mut b = ureq::Proxy::builder(protocol)
            .host(proxy.host.trim())
            .port(proxy.port);
        if !proxy.username.is_empty() {
            b = b.username(&proxy.username).password(&proxy.password);
        }
        Some(b.build().map_err(|e| format!("代理配置无效: {e}"))?)
    } else {
        None
    };
    let config = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .proxy(proxy)
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build();
    Ok(config.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxy(kind: ProxyKind, host: &str, port: u16) -> ProxyConfig {
        ProxyConfig {
            kind,
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    #[test]
    fn validates_proxy() {
        let cases = [
            (proxy(ProxyKind::None, "", 0), true),
            (proxy(ProxyKind::Http, "127.0.0.1", 7890), true),
            (proxy(ProxyKind::Socks5, "localhost", 1080), true),
            (proxy(ProxyKind::Http, " ", 7890), false),
            (proxy(ProxyKind::Http, "http://127.0.0.1", 7890), false),
            (proxy(ProxyKind::Socks5, "127.0.0.1", 0), false),
        ];
        for (p, ok) in cases {
            assert_eq!(validate_proxy(&p).is_ok(), ok, "{p:?}");
        }
    }

    #[test]
    fn enabled_only_when_complete() {
        assert!(!proxy(ProxyKind::None, "127.0.0.1", 7890).enabled());
        assert!(!proxy(ProxyKind::Http, "", 7890).enabled());
        assert!(proxy(ProxyKind::Http, "127.0.0.1", 7890).enabled());
    }

    #[test]
    fn builds_connect_request() {
        assert_eq!(
            connect_request("imap.gmail.com", 993, "", ""),
            "CONNECT imap.gmail.com:993 HTTP/1.1\r\nHost: imap.gmail.com:993\r\n\r\n"
        );
        // base64("u:p") = "dTpw"
        assert!(connect_request("h", 1, "u", "p").contains("Proxy-Authorization: Basic dTpw\r\n"));
    }

    #[test]
    fn parses_connect_status() {
        let cases = [
            ("HTTP/1.1 200 Connection established\r\n\r\n", Ok(())),
            ("HTTP/1.0 200 OK\r\n\r\n", Ok(())),
            (
                "HTTP/1.1 407 Proxy Authentication Required\r\n\r\n",
                Err("认证"),
            ),
            ("HTTP/1.1 502 Bad Gateway\r\n\r\n", Err("拒绝")),
            ("SSH-2.0-OpenSSH\r\n\r\n", Err("无法识别")),
            ("", Err("无法识别")),
        ];
        for (head, want) in cases {
            match (parse_connect_status(head), want) {
                (Ok(()), Ok(())) => {}
                (Err(e), Err(fragment)) => assert!(e.contains(fragment), "{head:?} -> {e}"),
                (got, _) => panic!("{head:?} -> {got:?}"),
            }
        }
    }
}
