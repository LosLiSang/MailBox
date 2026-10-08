# MailBox

自用多邮箱客户端，Tauri 2（Rust）+ React + TypeScript。

## 当前功能

- 本地 SQLite 缓存（`%APPDATA%\com.mailbox.app\mail.db`），打开即显示，后台按 UID 增量同步收件箱（首次最近 200 封）
- 阅读正文：首次打开从服务器下载并缓存，sandbox iframe + CSP 渲染，默认拦截远程图片，内联图片走 `mailbox://` 协议
- 附件打开 / 保存到 `下载\MailBox`，已读状态双向同步
- 多账号：配置存 `%APPDATA%\com.mailbox.app\accounts.json`，授权码 / OAuth 令牌存系统凭据管理器（keyring）
- 服务商：Gmail、Outlook（浏览器 OAuth 登录；Outlook 内置项目应用，无需用户注册 Azure，Gmail 配置见 [docs/oauth-setup.md](docs/oauth-setup.md)）、QQ、腾讯企业邮、iCloud、Yahoo、阿里邮箱、自定义 IMAP
  - 网易 163/126 暂不支持（需要 IMAP ID 命令，待更换 IMAP 库）
- 设置：账号管理、同步数量与自动同步、缓存统计与清理、远程图片策略与信任发件人、HTTP / SOCKS5 代理、主题与密度

## 开发

```bash
pnpm install
pnpm tauri dev      # 启动应用
pnpm test           # 前端单测
cd src-tauri && cargo test --lib   # Rust 单测
```

QQ / 网易邮箱需在网页版设置中开启 IMAP 并使用**授权码**登录。
