//! 发件人规则的匹配逻辑。纯函数，表驱动单测覆盖。
//!
//! pattern 三种写法（存库前已规整为小写）：
//! - `@domain.com`   匹配整个域名
//! - `a@qq.com`      精确匹配地址
//! - `微信`          匹配发件人名字或地址里包含该关键词（至少 2 个字符）

/// 匹配返回 Some(归类)；不匹配返回 None
pub fn match_sender(rules: &[Rule], address: &str, name: &str) -> Option<i64> {
    let addr = address.trim().to_lowercase();
    let sender_name = name.trim().to_lowercase();
    // 先进先匹配：按规则顺序，第一条命中的生效
    rules
        .iter()
        .find(|r| matches(r.pattern.trim(), &addr, &sender_name))
        .map(|r| r.category_id)
}

fn matches(pattern: &str, addr: &str, name: &str) -> bool {
    if let Some(domain) = pattern.strip_prefix('@') {
        // 含子域名：x@sub.github.com 也算 @github.com
        !domain.is_empty()
            && addr
                .rsplit_once('@')
                .is_some_and(|(_, d)| d == domain || d.ends_with(&format!(".{domain}")))
    } else if pattern.contains('@') {
        addr == pattern
    } else {
        // 关键词：中文两个字起（如「微信」），单个字符误伤太多
        pattern.chars().count() >= 2 && (name.contains(pattern) || addr.contains(pattern))
    }
}

/// 规整用户输入：小写、去空白；不合法（空 / @ 后面为空 / 关键词只有 1 个字符）返回 None
pub fn normalize_pattern(raw: &str) -> Option<String> {
    let p = raw.trim().to_lowercase();
    if p.is_empty() {
        return None;
    }
    if let Some(domain) = p.strip_prefix('@') {
        return (!domain.is_empty() && !domain.contains([' ', '@', '/'])).then_some(p);
    }
    if p.contains('@') {
        // 粗略校验邮箱
        return (p.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
        }))
        .then_some(p);
    }
    (p.chars().count() >= 2).then_some(p)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: i64,
    pub pattern: String,
    pub category_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(list: &[(&str, i64)]) -> Vec<Rule> {
        list.iter()
            .enumerate()
            .map(|(i, (pattern, category_id))| Rule {
                id: i as i64,
                pattern: pattern.to_string(),
                category_id: *category_id,
            })
            .collect()
    }

    #[test]
    fn matches_patterns() {
        let r = rules(&[("@github.com", 1), ("boss@corp.com", 2), ("微信", 3)]);
        assert_eq!(match_sender(&r, "noreply@github.com", ""), Some(1));
        assert_eq!(match_sender(&r, "x@sub.github.com", ""), Some(1));
        assert_eq!(match_sender(&r, "boss@corp.com", "老板"), Some(2));
        assert_eq!(match_sender(&r, "other@corp.com", ""), None);
        // 关键词匹配名字或地址
        assert_eq!(match_sender(&r, "wx@t.tencent.com", "微信团队"), Some(3));
        assert_eq!(match_sender(&r, "wechat@qq.com", "QQ团队"), None);
        // 顺序优先：两条都能命中时取前面的
        let r2 = rules(&[("@qq.com", 1), ("a@qq.com", 2)]);
        assert_eq!(match_sender(&r2, "a@qq.com", ""), Some(1));
    }

    #[test]
    fn single_letter_keyword_is_ignored() {
        let r = rules(&[("a", 1)]);
        assert_eq!(match_sender(&r, "apple@qq.com", "Apple"), None);
    }

    #[test]
    fn normalizes_patterns() {
        let cases = [
            ("  @GitHub.COM ", Some("@github.com".to_string())),
            (" A@QQ.com ", Some("a@qq.com".to_string())),
            ("微信", Some("微信".to_string())),
            ("", None),
            ("@ ", None),
            ("@ok.com", Some("@ok.com".to_string())),
            ("a@qq", None),
            ("x", None),
            ("@bad domain", None),
        ];
        for (input, want) in cases {
            assert_eq!(normalize_pattern(input), want, "{input}");
        }
    }
}
