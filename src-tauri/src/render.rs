//! 把原始邮件转成可安全显示的阅读视图。
//!
//! 安全策略（配合前端 `<iframe sandbox>`，不允许脚本、表单、同源）：
//! - 注入 CSP：禁止脚本；默认只允许本地 `mailbox://` 和 data: 图片，远程资源需用户点击后放行
//! - `<base target="_blank">`：链接一律新窗口打开，由 Rust 拦截后交给系统浏览器
//! - `cid:` 内联图片改写为 `mailbox://` 协议 URL，由 Rust 从本地缓存返回

use mail_parser::{Address, Message, MessageParser, MimeHeaders, PartType};
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use serde::Serialize;
use std::collections::HashMap;

pub const SCHEME: &str = "mailbox";

/// Windows / Android 上自定义协议要写成 http://<scheme>.localhost
pub fn protocol_base() -> String {
    if cfg!(any(windows, target_os = "android")) {
        format!("http://{SCHEME}.localhost/")
    } else {
        format!("{SCHEME}://localhost/")
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub part: u32,
    pub name: String,
    pub mime: String,
    pub size: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub subject: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: Option<String>,
    /// 完整的 HTML 文档，用作 iframe 的 srcdoc
    pub html: String,
    pub has_remote_content: bool,
    pub attachments: Vec<Attachment>,
}

// ---------- 路由 ----------

#[derive(Debug, PartialEq)]
pub struct PartRoute {
    pub account: String,
    pub folder: String,
    pub uid: u32,
    pub part: u32,
}

pub fn part_url(base: &str, r: &PartRoute) -> String {
    let enc = |s: &str| utf8_percent_encode(s, NON_ALPHANUMERIC).to_string();
    format!("{base}{}/{}/{}/{}", enc(&r.account), enc(&r.folder), r.uid, r.part)
}

/// 解析协议请求路径 `/<account>/<folder>/<uid>/<part>`
pub fn parse_route(path: &str) -> Option<PartRoute> {
    let mut it = path.trim_start_matches('/').split('/');
    let dec = |s: &str| percent_decode_str(s).decode_utf8().ok().map(|c| c.into_owned());
    let route = PartRoute {
        account: dec(it.next()?)?,
        folder: dec(it.next()?)?,
        uid: it.next()?.parse().ok()?,
        part: it.next()?.parse().ok()?,
    };
    (it.next().is_none() && !route.account.is_empty() && !route.folder.is_empty()).then_some(route)
}

// ---------- HTML 处理（纯函数） ----------

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn text_to_html(text: &str) -> String {
    format!(
        r#"<pre style="white-space:pre-wrap;word-break:break-word;font:inherit;margin:0">{}</pre>"#,
        escape_html(text)
    )
}

/// 把 `cid:xxx` 替换为 `lookup` 返回的 URL；找不到的保持原样
pub fn rewrite_cid(html: &str, mut lookup: impl FnMut(&str) -> Option<String>) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut last = 0;
    let mut search = 0;
    while let Some(pos) = lower[search..].find("cid:").map(|p| p + search) {
        let start = pos + 4;
        let end = html[start..]
            .find(|c: char| matches!(c, '"' | '\'' | ')' | '>' | ' ' | '\t' | '\r' | '\n'))
            .map_or(html.len(), |e| e + start);
        let cid = &html[start..end];
        if let Some(url) = (!cid.is_empty()).then(|| lookup(cid)).flatten() {
            out.push_str(&html[last..pos]);
            out.push_str(&url);
            last = end;
        }
        search = end.max(start);
    }
    out.push_str(&html[last..]);
    out
}

/// 是否引用了远程图片/样式（追踪像素通常藏在这里）
pub fn has_remote_content(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    ["src=", "background=", "url(", "srcset="].iter().any(|marker| {
        lower.match_indices(marker).any(|(i, _)| {
            let rest = lower[i + marker.len()..].trim_start_matches(['"', '\'', ' ']);
            rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("//")
        })
    })
}

/// 包装成带 CSP 的完整文档。meta 放在最前面，浏览器会把它归入隐式的 <head>
pub fn wrap_document(body: &str, allow_remote: bool, base: &str) -> String {
    let remote = if allow_remote { " https: http:" } else { "" };
    let csp = format!(
        "default-src 'none'; script-src 'none'; img-src {base} data:{remote}; \
         style-src 'unsafe-inline'{remote}; font-src data:{remote}; media-src {base}"
    );
    format!(
        r#"<meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="{csp}"><base target="_blank"><style>html{{background:#fff;color:#1f2328;color-scheme:light}}body{{margin:16px;font-family:system-ui,"Microsoft YaHei UI",sans-serif;font-size:14px;line-height:1.6;word-wrap:break-word}}img{{max-width:100%;height:auto}}</style>{body}"#
    )
}

pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() { "attachment".into() } else { trimmed.into() }
}

/// 生成不冲突的文件名：a.pdf -> a (1).pdf -> a (2).pdf
pub fn unique_filename(name: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !exists(candidate))
        .unwrap()
}

// ---------- 邮件解析 ----------

fn format_addresses(addr: Option<&Address>) -> String {
    addr.map(|a| {
        a.iter()
            .map(|x| match (x.name(), x.address()) {
                (Some(n), Some(e)) if !n.is_empty() => format!("{n} <{e}>"),
                (_, Some(e)) => e.to_string(),
                (Some(n), None) => n.to_string(),
                _ => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    })
    .unwrap_or_default()
}

fn mime_of(part: &mail_parser::MessagePart) -> String {
    part.content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
        .to_ascii_lowercase()
}

pub fn parse(raw: &[u8]) -> Option<Message<'_>> {
    MessageParser::default().parse(raw)
}

pub fn render(raw: &[u8], route_base: &PartRoute, allow_remote: bool) -> Result<MessageView, String> {
    let msg = parse(raw).ok_or("无法解析邮件")?;
    let base = protocol_base();

    let cid_map: HashMap<String, u32> = msg
        .parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let cid = p.content_id()?.trim().trim_start_matches('<').trim_end_matches('>');
            Some((cid.to_string(), i as u32))
        })
        .collect();

    let body = match msg.html_part(0) {
        Some(p) if p.is_text_html() => msg.body_html(0).unwrap_or_default().into_owned(),
        _ => text_to_html(&msg.body_text(0).unwrap_or_default()),
    };

    let mut inline_parts = Vec::new();
    let body = rewrite_cid(&body, |cid| {
        let part = *cid_map.get(cid)?;
        inline_parts.push(part);
        Some(part_url(&base, &PartRoute { part, ..clone_route(route_base) }))
    });

    let attachments = msg
        .attachments
        .iter()
        .filter(|id| !inline_parts.contains(id))
        .filter_map(|&id| {
            let part = msg.part(id)?;
            let fallback = if matches!(part.body, PartType::Message(_)) { "邮件.eml" } else { "附件" };
            Some(Attachment {
                part: id,
                name: part.attachment_name().unwrap_or(fallback).to_string(),
                mime: mime_of(part),
                size: part.len(),
            })
        })
        .collect();

    Ok(MessageView {
        subject: msg.subject().unwrap_or("(无主题)").to_string(),
        from: format_addresses(msg.from()),
        to: format_addresses(msg.to()),
        cc: format_addresses(msg.cc()),
        date: msg.date().map(|d| d.to_rfc3339()),
        has_remote_content: has_remote_content(&body),
        html: wrap_document(&body, allow_remote, &base),
        attachments,
    })
}

fn clone_route(r: &PartRoute) -> PartRoute {
    PartRoute { account: r.account.clone(), folder: r.folder.clone(), uid: r.uid, part: r.part }
}

/// 取出某个 MIME 部分：(文件名, MIME 类型, 内容)
pub fn extract_part(raw: &[u8], part: u32) -> Option<(String, String, Vec<u8>)> {
    let msg = parse(raw)?;
    let p = msg.part(part)?;
    if p.is_multipart() {
        return None;
    }
    let name = p.attachment_name().unwrap_or("attachment").to_string();
    Some((name, mime_of(p), p.contents().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> PartRoute {
        PartRoute { account: "a@qq.com".into(), folder: "INBOX".into(), uid: 42, part: 0 }
    }

    #[test]
    fn route_roundtrip() {
        let r = PartRoute { account: "张三+x@qq.com".into(), folder: "INBOX/子 文件夹".into(), uid: 7, part: 3 };
        let url = part_url("mailbox://localhost/", &r);
        let path = url.strip_prefix("mailbox://localhost").unwrap();
        assert_eq!(parse_route(path), Some(r));
    }

    #[test]
    fn rejects_bad_routes() {
        for path in ["/", "/a", "/a/INBOX/1", "/a/INBOX/x/1", "/a/INBOX/1/2/3", "//INBOX/1/2", "/a/INBOX/-1/2"] {
            assert_eq!(parse_route(path), None, "{path}");
        }
    }

    #[test]
    fn escapes_html() {
        assert_eq!(escape_html(r#"<a href="x">&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;");
    }

    #[test]
    fn rewrites_cid() {
        let lookup = |cid: &str| (cid == "img1@x").then(|| "URL".to_string());
        let cases = [
            (r#"<img src="cid:img1@x">"#, r#"<img src="URL">"#),
            (r#"<img src='CID:img1@x'>"#, r#"<img src='URL'>"#),
            (r#"<td style="background:url(cid:img1@x)">"#, r#"<td style="background:url(URL)">"#),
            (r#"<img src="cid:unknown"> <img src="cid:img1@x">"#, r#"<img src="cid:unknown"> <img src="URL">"#),
            ("cid:", "cid:"),
            ("纯文本 no cid", "纯文本 no cid"),
        ];
        for (input, want) in cases {
            assert_eq!(rewrite_cid(input, lookup), want, "{input}");
        }
    }

    #[test]
    fn detects_remote_content() {
        let cases = [
            (r#"<img src="https://t.co/p.gif">"#, true),
            (r#"<img src='http://x/p.gif'>"#, true),
            (r#"<img src=//cdn.x/p.gif>"#, true),
            (r#"<td background="https://x/bg.png">"#, true),
            (r#"<div style="background: url('https://x/bg.png')">"#, true),
            (r#"<a href="https://example.com">link</a>"#, false),
            (r#"<img src="data:image/png;base64,AAA">"#, false),
            (r#"<img src="mailbox://localhost/a/INBOX/1/2">"#, false),
            ("plain text https://example.com", false),
        ];
        for (html, want) in cases {
            assert_eq!(has_remote_content(html), want, "{html}");
        }
    }

    #[test]
    fn csp_blocks_remote_unless_allowed() {
        let blocked = wrap_document("<p>x</p>", false, "mailbox://localhost/");
        assert!(blocked.contains("script-src 'none'"));
        assert!(!blocked.contains("https:"));
        assert!(blocked.contains(r#"<base target="_blank">"#));
        assert!(blocked.ends_with("<p>x</p>"));

        let allowed = wrap_document("<p>x</p>", true, "mailbox://localhost/");
        assert!(allowed.contains("img-src mailbox://localhost/ data: https: http:"));
        assert!(allowed.contains("script-src 'none'"));
    }

    #[test]
    fn sanitizes_and_dedups_filenames() {
        let cases = [
            ("报价单.pdf", "报价单.pdf"),
            ("a/b\\c:d*?.txt", "a_b_c_d__.txt"),
            ("  ..  ", "attachment"),
            ("name. ", "name"),
            ("", "attachment"),
        ];
        for (input, want) in cases {
            assert_eq!(sanitize_filename(input), want, "{input:?}");
        }

        let taken = ["a.pdf", "a (1).pdf", "noext", ".env"];
        let exists = |n: &str| taken.contains(&n);
        assert_eq!(unique_filename("b.pdf", exists), "b.pdf");
        assert_eq!(unique_filename("a.pdf", exists), "a (2).pdf");
        assert_eq!(unique_filename("noext", exists), "noext (1)");
        assert_eq!(unique_filename(".env", exists), ".env (1)");
    }

    const MULTIPART: &[u8] = b"From: =?UTF-8?B?5byg5LiJ?= <zs@qq.com>\r\n\
To: a@qq.com, Bob <b@qq.com>\r\n\
Subject: =?GBK?B?xOO6ww==?=\r\n\
Date: Tue, 1 Apr 2025 10:00:00 +0800\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"OUT\"\r\n\
\r\n\
--OUT\r\n\
Content-Type: multipart/related; boundary=\"REL\"\r\n\
\r\n\
--REL\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>hi <img src=\"cid:logo@x\"><script>alert(1)</script><img src=\"https://track.example/p.gif\"></p>\r\n\
--REL\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo@x>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--REL--\r\n\
--OUT\r\n\
Content-Type: application/pdf; name=\"=?UTF-8?B?5oql5Lu35Y2VLnBkZg==?=\"\r\n\
Content-Disposition: attachment; filename=\"=?UTF-8?B?5oql5Lu35Y2VLnBkZg==?=\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0=\r\n\
--OUT--\r\n";

    #[test]
    fn renders_multipart_message() {
        let view = render(MULTIPART, &route(), false).unwrap();
        assert_eq!(view.subject, "你好");
        assert_eq!(view.from, "张三 <zs@qq.com>");
        assert_eq!(view.to, "a@qq.com, Bob <b@qq.com>");
        assert_eq!(view.date.as_deref(), Some("2025-04-01T10:00:00+08:00"));
        assert!(view.has_remote_content);

        // 内联图片被改写，且不出现在附件列表里
        assert!(!view.html.contains("cid:logo@x"));
        assert!(view.html.contains(&format!("{}a%40qq%2Ecom/INBOX/42/", protocol_base())));

        assert_eq!(view.attachments.len(), 1);
        let att = &view.attachments[0];
        assert_eq!((att.name.as_str(), att.mime.as_str(), att.size), ("报价单.pdf", "application/pdf", 5));

        let (name, mime, data) = extract_part(MULTIPART, att.part).unwrap();
        assert_eq!((name.as_str(), mime.as_str(), data.as_slice()), ("报价单.pdf", "application/pdf", &b"%PDF-"[..]));
    }

    #[test]
    fn renders_plain_text_escaped() {
        let raw = b"Subject: t\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n<b>not bold</b>\r\nline2\r\n";
        let view = render(raw, &route(), false).unwrap();
        assert!(view.html.contains("&lt;b&gt;not bold&lt;/b&gt;"));
        assert!(view.attachments.is_empty());
        assert!(!view.has_remote_content);
    }
}
