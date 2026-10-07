//! 把原始邮件转成可安全显示的阅读视图。
//!
//! 安全策略（配合前端 `<iframe sandbox>`，不允许脚本、表单、同源）：
//! - 注入 CSP：禁止脚本；默认只允许本地 `mailbox://` 和 data: 图片，远程资源需用户点击后放行
//! - `<base target="_blank">`：链接一律新窗口打开，由 Rust 拦截后交给系统浏览器
//! - `cid:` 内联图片改写为 `mailbox://` 协议 URL，由 Rust 从本地缓存返回
//!
//! 深色模式：先用 [`classify`] 判断邮件是哪一类，再用 [`choose_mode`] 决定怎么渲染。
//! 只有纯文本类邮件能安全地换成我们自己的深色配色；营销邮件强行改色容易变形，
//! 默认保持原样（白底），用户可以手动切换成反色。

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
    pub content_kind: ContentKind,
    pub render_mode: RenderMode,
}

// ---------- 深色模式 ----------

/// 邮件正文的类型，决定在深色模式下能不能安全地改配色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    /// 纯文本或没有指定任何颜色的 HTML（个人来信、简单通知）
    Plain,
    /// 邮件自己声明了深色适配（color-scheme / prefers-color-scheme）
    Adaptive,
    /// 指定了文字颜色，但没有背景和复杂排版（Outlook / Foxmail 写的信）
    Colored,
    /// 有背景色、背景图或表格排版（营销邮件、账单、通知模板）
    Designed,
    /// 邮件本身就是深色设计（如 Steam），原样显示即可，反色反而会变成浅色
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderMode {
    /// 原样显示，白底
    Light,
    /// 用我们的深色配色（只用于 Plain）
    Dark,
    /// 交给邮件自己的深色样式
    Adaptive,
    /// 整体反色，图片再反回来
    Invert,
}

/// 设置里的「邮件深色显示」
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DarkPreference {
    /// 智能：只对能安全改色的邮件使用深色
    #[default]
    Auto,
    /// 尽量都用深色（排版复杂的邮件也反色）
    Always,
    /// 邮件正文始终白底
    Never,
}

pub struct RenderOptions {
    pub allow_remote: bool,
    /// 应用当前是否处于深色主题
    pub app_dark: bool,
    pub preference: DarkPreference,
    /// 阅读区里手动切换：Some(true) 深色，Some(false) 浅色
    pub force_dark: Option<bool>,
}

/// 从属性或样式中取出一个值，如 `background-color: #fff;` 里的 `#fff`
fn value_after(s: &str) -> &str {
    let s = s.trim_start_matches([' ', '"', '\'', '\t']);
    let end = s.find([';', '"', '\'', '}', '>', '!']).unwrap_or(s.len());
    s[..end].trim()
}

/// 白色 / 透明之类的背景等同于没有设置背景
fn is_trivial_background(value: &str) -> bool {
    let v = value.trim().trim_end_matches(" !important");
    matches!(
        v,
        "" | "none" | "transparent" | "inherit" | "initial" | "unset" | "white" | "#fff" | "#ffffff"
            | "rgb(255,255,255)" | "rgb(255, 255, 255)" | "none transparent" | "transparent none"
    )
}

/// 把 #rgb / #rrggbb / rgb(r,g,b) / 常见颜色名解析成 RGB
pub fn parse_color(value: &str) -> Option<(u8, u8, u8)> {
    let v = value.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    // background 简写里可能带 url()/repeat 等，取第一个像颜色的部分；rgb(...) 内部有空格，整段取出
    let token = if let Some(start) = v.find("rgb") {
        let end = v[start..].find(')').map_or(v.len(), |e| start + e + 1);
        &v[start..end]
    } else {
        v.split_whitespace().find(|t| t.starts_with('#')).unwrap_or(&v)
    };
    if let Some(hex) = token.strip_prefix('#') {
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        let n = |s: &str| u8::from_str_radix(s, 16).ok();
        return match hex.len() {
            3 | 4 => {
                let d = |i: usize| n(&hex[i..i + 1].repeat(2));
                Some((d(0)?, d(1)?, d(2)?))
            }
            6 | 8 => Some((n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?)),
            _ => None,
        };
    }
    if let Some(inner) = token.strip_prefix("rgba(").or_else(|| token.strip_prefix("rgb(")) {
        let parts: Vec<u8> = inner
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .take(3)
            .map(|s| s.trim().parse::<f32>().ok().map(|f| f.clamp(0.0, 255.0) as u8))
            .collect::<Option<_>>()?;
        return (parts.len() == 3).then(|| (parts[0], parts[1], parts[2]));
    }
    match token {
        "black" => Some((0, 0, 0)),
        "white" => Some((255, 255, 255)),
        _ => None,
    }
}

/// 相对亮度 0（黑）~ 1（白），按 WCAG 公式
pub fn luminance((r, g, b): (u8, u8, u8)) -> f32 {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// 取邮件里的非白背景色。第一个通常是最外层的页面底色
fn background_colors(lower: &str) -> Vec<(u8, u8, u8)> {
    ["background-color:", "background:", "bgcolor="]
        .iter()
        .flat_map(|m| lower.match_indices(m).map(move |(i, _)| (i, &lower[i + m.len()..])))
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .filter_map(|rest| {
            let v = value_after(rest);
            (!is_trivial_background(v)).then(|| parse_color(v)).flatten()
        })
        .collect()
}

/// 去掉 HTML 注释和 CSS 注释。营销邮件常在注释里写「prefers-color-scheme: dark」之类的说明文字，
/// 不去掉会被误判为支持深色
pub fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        let next = [("<!--", "-->"), ("/*", "*/")]
            .iter()
            .filter_map(|(open, close)| rest.find(open).map(|i| (i, *open, *close)))
            .min_by_key(|(i, _, _)| *i);
        let Some((i, open, close)) = next else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..i]);
        let after = &rest[i + open.len()..];
        match after.find(close) {
            Some(j) => rest = &after[j + close.len()..],
            None => return out,
        }
    }
}

/// 邮件声明的配色方案：Some(true) 声明支持深色，Some(false) 声明了但不含深色（如 normal / light），
/// None 没有声明。来源：`<meta name="color-scheme" content=...>` 和 CSS 的 `color-scheme:` 属性
pub fn declared_dark_support(lower: &str) -> Option<bool> {
    let mut values = Vec::new();
    for (i, _) in lower.match_indices("<meta") {
        let tag = &lower[i..lower[i..].find('>').map_or(lower.len(), |e| i + e)];
        if tag.contains("color-scheme") {
            if let Some(c) = tag.find("content=") {
                values.push(value_after(&tag[c + 8..]).to_string());
            }
        }
    }
    for (i, _) in lower.match_indices("color-scheme:") {
        // 排除 prefers-color-scheme: 媒体查询
        if !lower[..i].ends_with("prefers-") {
            values.push(value_after(&lower[i + "color-scheme:".len()..]).to_string());
        }
    }
    (!values.is_empty()).then(|| values.iter().any(|v| v.contains("dark")))
}

/// 按启发式规则给邮件分类（输入为正文 HTML）
pub fn classify(html: &str) -> ContentKind {
    let lower = strip_comments(&html.to_ascii_lowercase());

    let has_dark_media = lower.contains("prefers-color-scheme: dark") || lower.contains("prefers-color-scheme:dark");
    match declared_dark_support(&lower) {
        Some(true) => return ContentKind::Adaptive,
        // 明确声明只支持浅色（color-scheme: normal / light），即使有深色媒体查询也不信任
        Some(false) => {}
        None if has_dark_media => return ContentKind::Adaptive,
        None => {}
    }

    let has_background = ["background-color:", "background:", "bgcolor="]
        .iter()
        .any(|m| lower.match_indices(m).any(|(i, _)| !is_trivial_background(value_after(&lower[i + m.len()..]))))
        || lower.contains("background-image:")
        || lower.contains("background=");
    let tables = lower.matches("<table").count();
    if has_background || tables >= 3 {
        // 最外层底色是深色 → 邮件本身就是深色设计
        if background_colors(&lower).first().is_some_and(|&c| luminance(c) < 0.18) {
            return ContentKind::Dark;
        }
        return ContentKind::Designed;
    }

    // 去掉 background-color 后再找 color，避免误判
    let text_colors = lower.replace("background-color", "");
    if text_colors.contains("color:") || text_colors.contains("color=") {
        return ContentKind::Colored;
    }
    ContentKind::Plain
}

fn dark_variant(kind: ContentKind) -> RenderMode {
    match kind {
        ContentKind::Plain => RenderMode::Dark,
        ContentKind::Adaptive => RenderMode::Adaptive,
        ContentKind::Colored | ContentKind::Designed => RenderMode::Invert,
        // 已经是深色，原样就是深色
        ContentKind::Dark => RenderMode::Light,
    }
}

pub fn choose_mode(kind: ContentKind, app_dark: bool, pref: DarkPreference, force_dark: Option<bool>) -> RenderMode {
    if !app_dark {
        return RenderMode::Light;
    }
    match (force_dark, pref) {
        (Some(true), _) => dark_variant(kind),
        (Some(false), _) | (None, DarkPreference::Never) => RenderMode::Light,
        (None, DarkPreference::Always) => dark_variant(kind),
        (None, DarkPreference::Auto) => match kind {
            ContentKind::Plain => RenderMode::Dark,
            ContentKind::Adaptive => RenderMode::Adaptive,
            ContentKind::Colored => RenderMode::Invert,
            // 排版复杂的邮件反色容易改变品牌色、让图片和文字不协调，默认保持原样
            ContentKind::Designed | ContentKind::Dark => RenderMode::Light,
        },
    }
}

const BASE_STYLE: &str = r#"body{margin:16px;font-family:system-ui,"Microsoft YaHei UI",sans-serif;font-size:14px;line-height:1.6;word-wrap:break-word}img{max-width:100%;height:auto}"#;

fn mode_style(mode: RenderMode) -> &'static str {
    match mode {
        RenderMode::Light => "html{background:#fff;color:#1f2328;color-scheme:light}",
        // 背景和应用深色面板一致；引用的历史邮件用左边框区分
        RenderMode::Dark => "html{background:#181b20;color:#e6e8eb;color-scheme:dark}a{color:#7aa2f7}\
             blockquote{margin:8px 0;padding-left:12px;border-left:3px solid #3a3f47;color:#a0a8b3}\
             hr{border:none;border-top:1px solid #2a2e35}",
        // 只声明配色方案，其余交给邮件自己的 prefers-color-scheme 样式
        RenderMode::Adaptive => "html{color-scheme:dark}",
        // invert(.9) 把白底变成 #1a1a1a、黑字变成 #e6e6e6，hue-rotate 让彩色大致保持色相；
        // 图片和视频再反转一次恢复原色
        RenderMode::Invert => "html{background:#fff;color:#1f2328;color-scheme:light;filter:invert(.9) hue-rotate(180deg)}\
             img,video,picture,svg,[style*=background-image],[background]{filter:invert(1) hue-rotate(180deg)}",
    }
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
pub fn wrap_document(body: &str, allow_remote: bool, base: &str, mode: RenderMode) -> String {
    let remote = if allow_remote { " https: http:" } else { "" };
    let csp = format!(
        "default-src 'none'; script-src 'none'; img-src {base} data:{remote}; \
         style-src 'unsafe-inline'{remote}; font-src data:{remote}; media-src {base}"
    );
    // Plain 邮件不含颜色样式，我们的样式放在前面即可；其余模式也不覆盖邮件自己的样式
    format!(
        r#"<meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="{csp}"><base target="_blank"><style>{}{BASE_STYLE}</style>{body}"#,
        mode_style(mode)
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

pub fn render(raw: &[u8], route_base: &PartRoute, opts: &RenderOptions) -> Result<MessageView, String> {
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

    let content_kind = classify(&body);
    let render_mode = choose_mode(content_kind, opts.app_dark, opts.preference, opts.force_dark);

    Ok(MessageView {
        subject: msg.subject().unwrap_or("(无主题)").to_string(),
        from: format_addresses(msg.from()),
        to: format_addresses(msg.to()),
        cc: format_addresses(msg.cc()),
        date: msg.date().map(|d| d.to_rfc3339()),
        has_remote_content: has_remote_content(&body),
        html: wrap_document(&body, opts.allow_remote, &base, render_mode),
        attachments,
        content_kind,
        render_mode,
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

    fn light() -> RenderOptions {
        RenderOptions { allow_remote: false, app_dark: false, preference: DarkPreference::Auto, force_dark: None }
    }

    #[test]
    fn classifies_content() {
        use ContentKind::*;
        let cases = [
            ("<pre>纯文本邮件</pre>", Plain),
            (r#"<div dir="ltr">Hi,<br>see you<blockquote>old</blockquote></div>"#, Plain),
            (r#"<div style="background-color: #ffffff">白底等于没背景</div>"#, Plain),
            (r#"<body style="background:transparent">x</body>"#, Plain),
            (r#"<meta name="color-scheme" content="light dark"><p>x</p>"#, Adaptive),
            ("<style>@media (prefers-color-scheme: dark){body{background:#000}}</style>", Adaptive),
            (r#"<meta name="color-scheme" content="light dark only"><p>x</p>"#, Adaptive),
            // 注释里提到深色、但声明只支持浅色（SpaceX 邮件）→ 不是深色适配
            (
                r##"<meta name="color-scheme" content="normal"><style>/* skip prefers-color-scheme: dark path */
                   :root{color-scheme: normal}</style><table bgcolor="#fefefe"><tr><td>x</td></tr></table>"##,
                Designed,
            ),
            ("<!-- @media (prefers-color-scheme: dark) --><p>纯文本</p>", Plain),
            (r#"<p style="color:#1F497D">Outlook 正文</p>"#, Colored),
            (r#"<font color="red">重要</font>"#, Colored),
            ("<style>a:link{color:#0563C1}</style><p>x</p>", Colored),
            (r##"<td bgcolor="#f4f4f4">x</td>"##, Designed),
            (r#"<div style="background-color:#1a73e8;color:#fff">按钮</div>"#, Designed),
            (r#"<div style="background: url(x.png)">x</div>"#, Designed),
            (r#"<div style="background-image:url(x)">x</div>"#, Designed),
            ("<table><tr><td><table><tr><td><table></table></td></tr></table></td></tr></table>", Designed),
            // 最外层是深色底（Steam 风格），内部按钮是亮色也不影响
            (r##"<body style="background-color:#2f261c"><a style="background:#ebbb67">买</a></body>"##, Dark),
            (r#"<table bgcolor="black"><tr><td>x</td></tr></table>"#, Dark),
            (r#"<div style="background: rgb(20, 20, 30) url(x.png)">x</div>"#, Dark),
            // 最外层是白底、里面有深色横幅 → 仍然是浅色设计
            (r##"<div style="background:#f4f4f4"><div style="background:#000">banner</div></div>"##, Designed),
        ];
        for (html, want) in cases {
            assert_eq!(classify(html), want, "{html}");
        }
    }

    #[test]
    fn chooses_render_mode() {
        use ContentKind::*;
        use DarkPreference::*;
        use RenderMode as M;
        let cases = [
            // 浅色主题下一律原样
            (Plain, false, Always, Some(true), M::Light),
            (Designed, false, Auto, None, M::Light),
            // 智能
            (Plain, true, Auto, None, M::Dark),
            (Adaptive, true, Auto, None, M::Adaptive),
            (Colored, true, Auto, None, M::Invert),
            (Designed, true, Auto, None, M::Light),
            // 总是深色
            (Designed, true, Always, None, M::Invert),
            (Plain, true, Always, None, M::Dark),
            // 从不
            (Plain, true, Never, None, M::Light),
            // 手动切换优先
            (Designed, true, Auto, Some(true), M::Invert),
            (Plain, true, Always, Some(false), M::Light),
            (Adaptive, true, Never, Some(true), M::Adaptive),
            // 邮件本身就是深色，任何情况下都不反色
            (Dark, true, Always, None, M::Light),
            (Dark, true, Auto, Some(true), M::Light),
        ];
        for (kind, dark, pref, force, want) in cases {
            assert_eq!(choose_mode(kind, dark, pref, force), want, "{kind:?} dark={dark} {pref:?} {force:?}");
        }
    }

    #[test]
    fn strips_comments() {
        let cases = [
            ("a<!-- x -->b/* y */c", "abc"),
            ("<!--[if mso]><style>p{}</style><![endif]-->d", "d"),
            ("keep /* unterminated", "keep "),
            ("no comments", "no comments"),
        ];
        for (input, want) in cases {
            assert_eq!(strip_comments(input), want, "{input}");
        }
    }

    #[test]
    fn detects_declared_color_scheme() {
        let cases = [
            (r#"<meta name="color-scheme" content="light dark">"#, Some(true)),
            (r#"<meta name="supported-color-schemes" content="light">"#, Some(false)),
            (":root{color-scheme: normal}", Some(false)),
            (":root{color-scheme:light dark}", Some(true)),
            ("@media (prefers-color-scheme: dark){}", None),
            ("<p>nothing</p>", None),
        ];
        for (html, want) in cases {
            assert_eq!(declared_dark_support(html), want, "{html}");
        }
    }

    #[test]
    fn parses_colors_and_luminance() {
        let cases = [
            ("#fff", Some((255, 255, 255))),
            ("#2F261C", Some((0x2f, 0x26, 0x1c))),
            ("#2f261cff", Some((0x2f, 0x26, 0x1c))),
            ("rgb(20, 20, 30)", Some((20, 20, 30))),
            ("rgba(0,0,0,.5)", Some((0, 0, 0))),
            ("rgb(20 20 30 / 50%)", Some((20, 20, 30))),
            ("black", Some((0, 0, 0))),
            ("#1a73e8 !important", Some((0x1a, 0x73, 0xe8))),
            ("url(x.png) no-repeat #000", Some((0, 0, 0))),
            ("red", None),
            ("#zz", None),
        ];
        for (v, want) in cases {
            assert_eq!(parse_color(v), want, "{v}");
        }
        assert!(luminance((0, 0, 0)) < 0.01);
        assert!(luminance((255, 255, 255)) > 0.99);
        assert!(luminance((0x2f, 0x26, 0x1c)) < 0.18);
        assert!(luminance((0xf4, 0xf4, 0xf4)) > 0.18);
    }

    #[test]
    fn mode_styles() {
        let doc = |m| wrap_document("<p>x</p>", false, "mailbox://localhost/", m);
        assert!(doc(RenderMode::Light).contains("background:#fff"));
        assert!(doc(RenderMode::Dark).contains("background:#181b20"));
        assert!(doc(RenderMode::Adaptive).contains("color-scheme:dark"));
        assert!(!doc(RenderMode::Adaptive).contains("background:"));
        let invert = doc(RenderMode::Invert);
        assert!(invert.contains("filter:invert(.9) hue-rotate(180deg)"));
        assert!(invert.contains("img,video"));
    }

    #[test]
    fn csp_blocks_remote_unless_allowed() {
        let blocked = wrap_document("<p>x</p>", false, "mailbox://localhost/", RenderMode::Light);
        assert!(blocked.contains("script-src 'none'"));
        assert!(!blocked.contains("https:"));
        assert!(blocked.contains(r#"<base target="_blank">"#));
        assert!(blocked.ends_with("<p>x</p>"));

        let allowed = wrap_document("<p>x</p>", true, "mailbox://localhost/", RenderMode::Light);
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
        let view = render(MULTIPART, &route(), &light()).unwrap();
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
        let view = render(raw, &route(), &light()).unwrap();
        assert!(view.html.contains("&lt;b&gt;not bold&lt;/b&gt;"));
        assert!(view.attachments.is_empty());
        assert!(!view.has_remote_content);
        assert_eq!(view.content_kind, ContentKind::Plain);
        assert_eq!(view.render_mode, RenderMode::Light);

        let dark = RenderOptions { app_dark: true, ..light() };
        let view = render(raw, &route(), &dark).unwrap();
        assert_eq!(view.render_mode, RenderMode::Dark);
    }
}
