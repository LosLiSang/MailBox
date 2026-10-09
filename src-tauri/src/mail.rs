//! 纯函数：把原始邮件头字节解析成前端需要的 Envelope。
//! 不涉及网络，便于用表驱动单测覆盖各种编码。

use mail_parser::MessageParser;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub uid: u32,
    pub subject: String,
    pub from_name: String,
    pub from_address: String,
    /// RFC 3339 格式；解析失败时为 None
    pub date: Option<String>,
    pub seen: bool,
    /// 本地分类；None 表示在收件箱
    pub category_id: Option<i64>,
}

pub fn parse_envelope(uid: u32, header: &[u8], seen: bool) -> Envelope {
    let msg = MessageParser::default().parse(header);

    let (subject, from_name, from_address, date) = match &msg {
        Some(m) => {
            let from = m.from().and_then(|a| a.first());
            (
                m.subject().unwrap_or_default().to_string(),
                from.and_then(|a| a.name()).unwrap_or_default().to_string(),
                from.and_then(|a| a.address())
                    .unwrap_or_default()
                    .to_string(),
                m.date().map(|d| d.to_rfc3339()),
            )
        }
        None => Default::default(),
    };

    Envelope {
        uid,
        subject: if subject.is_empty() {
            "(无主题)".into()
        } else {
            subject
        },
        from_name,
        from_address,
        date,
        seen,
        category_id: None,
    }
}

/// 计算拉取最近 `limit` 封邮件的序号区间，如 exists=100, limit=20 -> "81:100"。
pub fn recent_range(exists: u32, limit: u32) -> Option<String> {
    if exists == 0 || limit == 0 {
        return None;
    }
    let start = exists.saturating_sub(limit) + 1;
    Some(format!("{start}:{exists}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers_in_various_encodings() {
        // (原始头, 期望主题, 期望发件人名, 期望地址)
        let cases: &[(&[u8], &str, &str, &str)] = &[
            (
                b"From: Alice <alice@example.com>\r\nSubject: Hello\r\nDate: Tue, 1 Apr 2025 10:00:00 +0800\r\n\r\n",
                "Hello",
                "Alice",
                "alice@example.com",
            ),
            (
                // UTF-8 Base64 encoded-word
                b"From: =?UTF-8?B?5byg5LiJ?= <zs@qq.com>\r\nSubject: =?UTF-8?B?5L2g5aW9?=\r\n\r\n",
                "你好",
                "张三",
                "zs@qq.com",
            ),
            (
                // GBK Base64 encoded-word（国内邮箱常见）
                b"From: =?GBK?B?1cXI/Q==?= <zs@163.com>\r\nSubject: =?GBK?B?xOO6ww==?=\r\n\r\n",
                "你好",
                "张三",
                "zs@163.com",
            ),
            (
                b"From: bare@example.com\r\n\r\n",
                "(无主题)",
                "",
                "bare@example.com",
            ),
        ];

        for (raw, subject, name, addr) in cases {
            let env = parse_envelope(1, raw, false);
            assert_eq!(
                env.subject,
                *subject,
                "subject for {:?}",
                String::from_utf8_lossy(raw)
            );
            assert_eq!(env.from_name, *name);
            assert_eq!(env.from_address, *addr);
        }
    }

    #[test]
    fn parses_date_to_rfc3339() {
        let env = parse_envelope(7, b"Date: Tue, 1 Apr 2025 10:00:00 +0800\r\n\r\n", true);
        assert_eq!(env.date.as_deref(), Some("2025-04-01T10:00:00+08:00"));
        assert!(env.seen);
        assert_eq!(env.uid, 7);
    }

    #[test]
    fn computes_recent_range() {
        let cases = [
            (100, 20, Some("81:100")),
            (5, 20, Some("1:5")),
            (20, 20, Some("1:20")),
            (0, 20, None),
            (10, 0, None),
        ];
        for (exists, limit, want) in cases {
            assert_eq!(
                recent_range(exists, limit).as_deref(),
                want,
                "{exists},{limit}"
            );
        }
    }
}
