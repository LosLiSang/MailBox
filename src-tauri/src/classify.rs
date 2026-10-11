//! 智能分类：调用兼容 Jev schema 的决策模型（如 Cloudflare Clef），
//! 把发件人规则没命中的新邮件按内容归类。
//!
//! 一封邮件对应一次请求：`state` 是发件人 + 主题，`questions.category` 是一道 `choice` 题，
//! 选项就是用户的分类（外加「收件箱」）。置信度低于阈值的留在原处，不强行归类。
//!
//! 组装请求、解析响应、阈值判断都是纯函数，表驱动单测覆盖；网络只在 [`classify`] 里。

use crate::{db::Category, mail::Envelope, net};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// 「收件箱」选项的键。分类的键用 `c<id>`，避免分类名里的特殊字符
pub const INBOX_KEY: &str = "inbox";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ClassifySettings {
    /// 默认关闭：开启后邮件的发件人和主题会发送到下面的端点
    pub enabled: bool,
    /// 兼容 Jev schema 的 HTTP 端点
    pub endpoint: String,
    /// 请求体里的 model 字段；空则不发送（Workers AI 按 URL 选模型）
    pub model: String,
    /// 置信度低于该值时不归类（0.5–0.99）
    pub threshold: f64,
    /// 是否经「设置 → 代理」访问端点
    pub use_proxy: bool,
}

impl Default for ClassifySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            model: String::new(),
            threshold: 0.7,
            use_proxy: false,
        }
    }
}

pub const THRESHOLD_MIN: f64 = 0.5;
pub const THRESHOLD_MAX: f64 = 0.99;

pub fn normalize(mut s: ClassifySettings) -> Result<ClassifySettings, String> {
    s.endpoint = s.endpoint.trim().to_string();
    s.model = s.model.trim().to_string();
    s.threshold = if s.threshold.is_finite() {
        s.threshold.clamp(THRESHOLD_MIN, THRESHOLD_MAX)
    } else {
        0.7
    };
    if !s.endpoint.is_empty() {
        let url = tauri::Url::parse(&s.endpoint).map_err(|_| "智能分类端点不是有效的网址")?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        // 邮件内容和 token 不能明文发到远程服务器；本机服务允许 http
        if url.scheme() != "https" && !(url.scheme() == "http" && local) {
            return Err("智能分类端点必须使用 https（本机服务可用 http://localhost）".into());
        }
    }
    if s.enabled && s.endpoint.is_empty() {
        return Err("开启智能分类需要填写端点".into());
    }
    Ok(s)
}

/// 发给模型的邮件内容：只用同步时已有的发件人和主题，不下载正文
pub fn mail_state(env: &Envelope) -> String {
    let from = match (env.from_name.trim(), env.from_address.trim()) {
        ("", addr) => addr.to_string(),
        (name, "") => name.to_string(),
        (name, addr) => format!("{name} <{addr}>"),
    };
    format!("From: {from}\nSubject: {}", env.subject.trim())
}

pub fn category_key(id: i64) -> String {
    format!("c{id}")
}

/// 键 -> 分类；`inbox` -> None
pub fn parse_category_key(key: &str) -> Option<Option<i64>> {
    if key == INBOX_KEY {
        return Some(None);
    }
    key.strip_prefix('c')?.parse().ok().map(Some)
}

/// 选项描述：用户填了「分类说明」就用说明，否则用分类名
pub fn criteria(categories: &[Category]) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        INBOX_KEY.into(),
        "需要本人阅读或处理、不属于以下任何分类的邮件".into(),
    );
    for c in categories {
        let desc = c.description.trim();
        let text = if desc.is_empty() {
            c.name.clone()
        } else {
            format!("{}：{desc}", c.name)
        };
        map.insert(category_key(c.id), text.into());
    }
    map
}

pub fn build_request(model: &str, state: &str, categories: &[Category]) -> Value {
    let mut body = json!({
        "state": state,
        "questions": {
            "category": {
                "type": "choice",
                "instructions": "这封邮件应该归入哪个分类？",
                "criteria": criteria(categories),
            }
        }
    });
    if !model.is_empty() {
        body["model"] = model.into();
    }
    body
}

/// 解析 `category` 题的答案，返回 (选中键, 置信度)。
/// 兼容两种外层（`{answers}` / Workers AI 的 `{result: {answers}}`）
/// 和两种字段名（`choice` / `value`）；没有 confidence 时用选中项的概率。
pub fn parse_answer(resp: &Value) -> Result<(String, f64), String> {
    let answers = resp
        .get("answers")
        .or_else(|| resp.pointer("/result/answers"))
        .ok_or_else(|| describe_error(resp))?;
    let ans = answers
        .get("category")
        .ok_or("模型响应缺少 category 答案")?;
    let key = ans
        .get("choice")
        .or_else(|| ans.get("value"))
        .and_then(Value::as_str)
        .ok_or("模型响应缺少选中项")?
        .to_string();
    let confidence = ans
        .get("confidence")
        .and_then(Value::as_f64)
        .or_else(|| {
            ans.pointer(&format!("/probabilities/{key}"))
                .and_then(Value::as_f64)
        })
        .ok_or("模型响应缺少置信度")?;
    Ok((key, confidence))
}

/// 从错误响应里提取可读信息（Workers AI: `{success:false, errors:[{message}]}`）
fn describe_error(resp: &Value) -> String {
    let msg = resp
        .pointer("/errors/0/message")
        .or_else(|| resp.pointer("/error/message"))
        .or_else(|| resp.get("error"))
        .and_then(Value::as_str);
    match msg {
        Some(m) => format!("模型返回错误: {m}"),
        None => "模型响应格式无法识别".into(),
    }
}

/// 阈值判断：返回 Some(归类) 表示采纳；None 表示不归类（低置信度或未知选项）。
/// Some(None) 表示模型认为应留在收件箱。
pub fn decide(
    key: &str,
    confidence: f64,
    threshold: f64,
    categories: &[Category],
) -> Option<Option<i64>> {
    if confidence < threshold {
        return None;
    }
    match parse_category_key(key)? {
        None => Some(None),
        Some(id) => categories.iter().any(|c| c.id == id).then_some(Some(id)),
    }
}

/// 一次分类所需的上下文
pub struct Classifier {
    agent: ureq::Agent,
    settings: ClassifySettings,
    token: Option<String>,
    categories: Vec<Category>,
}

impl Classifier {
    /// 未开启或没有分类时返回 None
    pub fn new(
        settings: ClassifySettings,
        token: Option<String>,
        proxy: &net::ProxyConfig,
        categories: Vec<Category>,
    ) -> Result<Option<Self>, String> {
        if !settings.enabled || settings.endpoint.is_empty() || categories.is_empty() {
            return Ok(None);
        }
        let proxy = if settings.use_proxy {
            proxy.clone()
        } else {
            net::ProxyConfig::default()
        };
        Ok(Some(Self {
            agent: net::http_agent(&proxy)?,
            settings,
            token,
            categories,
        }))
    }

    /// 返回 (选中键, 置信度)，不做阈值判断，供「测试」按钮展示
    pub fn ask(&self, state: &str) -> Result<(String, f64), String> {
        let body = build_request(&self.settings.model, state, &self.categories);
        let mut req = self.agent.post(&self.settings.endpoint);
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        let mut resp = req
            .send_json(&body)
            .map_err(|e| format!("请求模型失败: {e}"))?;
        let status = resp.status().as_u16();
        let json: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("模型响应无法解析（HTTP {status}）: {e}"))?;
        parse_answer(&json).map_err(|e| format!("{e}（HTTP {status}）"))
    }

    /// 给一封邮件归类；None 表示不改变（低置信度、未知选项）
    pub fn classify(&self, env: &Envelope) -> Result<Option<Option<i64>>, String> {
        let (key, confidence) = self.ask(&mail_state(env))?;
        Ok(decide(
            &key,
            confidence,
            self.settings.threshold,
            &self.categories,
        ))
    }

    pub fn categories(&self) -> &[Category] {
        &self.categories
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(id: i64, name: &str, description: &str) -> Category {
        Category {
            id,
            name: name.into(),
            color: "#000".into(),
            count: 0,
            description: description.into(),
        }
    }

    #[test]
    fn builds_request() {
        let cats = [cat(1, "通知", "平台自动发送的通知"), cat(3, "重要", "")];
        let body = build_request("clef-flash", "From: a\nSubject: b", &cats);
        assert_eq!(body["model"], "clef-flash");
        assert_eq!(body["state"], "From: a\nSubject: b");
        let q = &body["questions"]["category"];
        assert_eq!(q["type"], "choice");
        assert_eq!(q["criteria"]["c1"], "通知：平台自动发送的通知");
        assert_eq!(q["criteria"]["c3"], "重要");
        assert!(q["criteria"][INBOX_KEY].is_string());

        // 不填 model 时不发送该字段
        assert!(build_request("", "s", &cats).get("model").is_none());
    }

    #[test]
    fn formats_mail_state() {
        let env = |name: &str, addr: &str| Envelope {
            uid: 1,
            subject: " 周报 ".into(),
            from_name: name.into(),
            from_address: addr.into(),
            date: None,
            seen: false,
            category_id: None,
        };
        let cases = [
            (
                env("GitHub", "noreply@github.com"),
                "From: GitHub <noreply@github.com>\nSubject: 周报",
            ),
            (env("", "a@b.c"), "From: a@b.c\nSubject: 周报"),
            (env("张三", ""), "From: 张三\nSubject: 周报"),
        ];
        for (e, want) in cases {
            assert_eq!(mail_state(&e), want);
        }
    }

    #[test]
    fn parses_answers() {
        let cases: &[(&str, Result<(&str, f64), &str>)] = &[
            // Jev / Hugging Face 形式
            (
                r#"{"answers":{"category":{"type":"choice","choice":"c1","confidence":0.91,"probabilities":{"c1":0.95}}}}"#,
                Ok(("c1", 0.91)),
            ),
            // Workers AI 外层 result + value 字段，无 confidence 时取概率
            (
                r#"{"success":true,"result":{"answers":{"category":{"value":"inbox","probabilities":{"inbox":0.8,"c1":0.2}}}}}"#,
                Ok(("inbox", 0.8)),
            ),
            (
                r#"{"success":false,"errors":[{"code":10000,"message":"Authentication error"}]}"#,
                Err("Authentication error"),
            ),
            (r#"{"answers":{}}"#, Err("缺少 category")),
            (
                r#"{"answers":{"category":{"choice":"c1"}}}"#,
                Err("缺少置信度"),
            ),
            (r#"{"foo":1}"#, Err("无法识别")),
        ];
        for (raw, want) in cases {
            let got = parse_answer(&serde_json::from_str(raw).unwrap());
            match (got, want) {
                (Ok((k, c)), Ok((wk, wc))) => assert_eq!((k.as_str(), c), (*wk, *wc), "{raw}"),
                (Err(e), Err(frag)) => assert!(e.contains(frag), "{raw} -> {e}"),
                (got, _) => panic!("{raw} -> {got:?}"),
            }
        }
    }

    #[test]
    fn decides_with_threshold() {
        let cats = [cat(1, "通知", ""), cat(2, "重要", "")];
        let cases = [
            ("c1", 0.9, Some(Some(1))),
            ("c1", 0.6, None),           // 低于阈值
            ("inbox", 0.95, Some(None)), // 模型认为留在收件箱
            ("c9", 0.99, None),          // 分类已被删除
            ("bogus", 0.99, None),
        ];
        for (key, conf, want) in cases {
            assert_eq!(decide(key, conf, 0.7, &cats), want, "{key} {conf}");
        }
    }

    #[test]
    fn normalizes_settings() {
        let ok = |endpoint: &str| ClassifySettings {
            enabled: true,
            endpoint: endpoint.into(),
            ..Default::default()
        };
        assert!(normalize(ok("https://api.cloudflare.com/x")).is_ok());
        assert!(normalize(ok("http://localhost:8080/v1/decide")).is_ok());
        assert!(normalize(ok("http://example.com/x")).is_err());
        assert!(normalize(ok("not a url")).is_err());
        assert!(normalize(ok("")).is_err());
        // 关闭时可以不填端点
        assert!(normalize(ClassifySettings::default()).is_ok());

        let mut s = ok(" https://a.b/c ");
        s.threshold = 5.0;
        let s = normalize(s).unwrap();
        assert_eq!(
            (s.endpoint.as_str(), s.threshold),
            ("https://a.b/c", THRESHOLD_MAX)
        );
    }
}
