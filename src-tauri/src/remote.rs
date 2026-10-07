//! 邮件正文里的远程图片由 Rust 下载（可经代理），再通过 mailbox:// 协议交给 WebView。
//!
//! 好处：
//! - 走「设置 → 代理」，Gmail / 国外 CDN 的图片在国内也能显示
//! - 不带 Cookie 和 Referer，部分防盗链图片也能加载，发件人也拿不到你的浏览器信息
//! - 内存缓存，切换深浅色等重新渲染时不重复下载

use crate::net::{self, ProxyConfig};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{Arc, Mutex},
};

/// 单张图片上限
const MAX_IMAGE_BYTES: u64 = 15 * 1024 * 1024;
/// 内存缓存总上限
const CACHE_BYTES: usize = 64 * 1024 * 1024;
/// 部分 CDN 会拒绝非浏览器的请求
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

// ---------- 纯函数 ----------

/// 禁止访问本机和内网地址：恶意邮件不能借这个功能探测局域网
pub fn is_blocked_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast()
        }
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00 // fc00::/7 唯一本地地址
                || (first & 0xffc0) == 0xfe80 // fe80::/10 链路本地
        }
        Err(_) => false,
    }
}

/// 只转发图片和字体；很多 CDN 给图片返回 octet-stream，也放行
pub fn allowed_mime(content_type: &str) -> bool {
    let ct = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    ct.is_empty()
        || ct.starts_with("image/")
        || ct.starts_with("font/")
        || ct == "application/octet-stream"
        || ct == "binary/octet-stream"
        || ct == "application/font-woff"
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cached {
    pub mime: String,
    pub data: Arc<Vec<u8>>,
}

/// 按插入顺序淘汰的简单缓存
pub struct ImageCache {
    map: HashMap<String, Cached>,
    order: VecDeque<String>,
    bytes: usize,
    cap: usize,
}

impl ImageCache {
    pub fn new(cap: usize) -> Self {
        Self { map: HashMap::new(), order: VecDeque::new(), bytes: 0, cap }
    }

    pub fn get(&self, url: &str) -> Option<Cached> {
        self.map.get(url).cloned()
    }

    pub fn insert(&mut self, url: String, item: Cached) {
        let size = item.data.len();
        // 单张超过总容量 1/4 的不缓存，避免一张大图把其他全挤掉
        if size > self.cap / 4 || self.map.contains_key(&url) {
            return;
        }
        while self.bytes + size > self.cap {
            let Some(old) = self.order.pop_front() else { break };
            if let Some(removed) = self.map.remove(&old) {
                self.bytes -= removed.data.len();
            }
        }
        self.bytes += size;
        self.order.push_back(url.clone());
        self.map.insert(url, item);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

// ---------- 下载 ----------

pub struct RemoteFetcher {
    cache: Mutex<ImageCache>,
    /// 代理配置不变时复用连接池
    agent: Mutex<Option<(ProxyConfig, ureq::Agent)>>,
}

impl RemoteFetcher {
    pub fn new() -> Self {
        Self { cache: Mutex::new(ImageCache::new(CACHE_BYTES)), agent: Mutex::new(None) }
    }

    fn agent(&self, proxy: &ProxyConfig) -> Result<ureq::Agent, String> {
        let mut slot = self.agent.lock().map_err(|e| e.to_string())?;
        if let Some((cfg, agent)) = slot.as_ref() {
            if cfg == proxy {
                return Ok(agent.clone());
            }
        }
        let agent = net::http_agent(proxy)?;
        *slot = Some((proxy.clone(), agent.clone()));
        Ok(agent)
    }

    pub fn fetch(&self, url: &str, proxy: &ProxyConfig) -> Result<Cached, String> {
        if let Some(hit) = self.cache.lock().map_err(|e| e.to_string())?.get(url) {
            return Ok(hit);
        }

        let parsed = tauri::Url::parse(url).map_err(|e| format!("图片地址无效: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("只支持 http(s) 图片".into());
        }
        if is_blocked_host(parsed.host_str().unwrap_or_default()) {
            return Err("不允许访问本机或内网地址".into());
        }

        let resp = self
            .agent(proxy)?
            .get(url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
            .call()
            .map_err(|e| format!("下载图片失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("图片服务器返回 {}", resp.status()));
        }
        let mime = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        if !allowed_mime(&mime) {
            return Err(format!("不是图片: {mime}"));
        }
        let data = resp
            .into_body()
            .with_config()
            .limit(MAX_IMAGE_BYTES)
            .read_to_vec()
            .map_err(|e| format!("读取图片失败: {e}"))?;

        let item = Cached {
            mime: if mime.is_empty() { "application/octet-stream".into() } else { mime },
            data: Arc::new(data),
        };
        self.cache.lock().map_err(|e| e.to_string())?.insert(url.to_string(), item.clone());
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_local_and_private_hosts() {
        let cases = [
            ("localhost", true),
            ("api.localhost", true),
            ("127.0.0.1", true),
            ("10.1.2.3", true),
            ("172.16.0.1", true),
            ("192.168.1.1", true),
            ("169.254.1.1", true),
            ("0.0.0.0", true),
            ("[::1]", true),
            ("fd00::1", true),
            ("fe80::1", true),
            ("", true),
            ("8.8.8.8", false),
            ("lh3.googleusercontent.com", false),
            ("[2606:4700::1111]", false),
        ];
        for (host, blocked) in cases {
            assert_eq!(is_blocked_host(host), blocked, "{host}");
        }
    }

    #[test]
    fn filters_mime_types() {
        let cases = [
            ("image/png", true),
            ("image/svg+xml; charset=utf-8", true),
            ("IMAGE/JPEG", true),
            ("font/woff2", true),
            ("application/octet-stream", true),
            ("", true),
            ("text/html; charset=utf-8", false),
            ("application/javascript", false),
        ];
        for (ct, ok) in cases {
            assert_eq!(allowed_mime(ct), ok, "{ct}");
        }
    }

    fn item(size: usize) -> Cached {
        Cached { mime: "image/png".into(), data: Arc::new(vec![0; size]) }
    }

    #[test]
    fn cache_evicts_oldest_and_skips_huge_items() {
        let mut c = ImageCache::new(100);
        c.insert("a".into(), item(20));
        c.insert("b".into(), item(20));
        c.insert("c".into(), item(20));
        c.insert("d".into(), item(20));
        c.insert("e".into(), item(20));
        assert_eq!(c.len(), 5);

        // 再放一张，最早的 a 被淘汰
        c.insert("f".into(), item(20));
        assert!(c.get("a").is_none());
        assert!(c.get("f").is_some());

        // 超过容量 1/4 的不缓存
        c.insert("huge".into(), item(30));
        assert!(c.get("huge").is_none());

        // 重复插入不重复计数
        c.insert("f".into(), item(20));
        assert_eq!(c.len(), 5);
    }
}
