# MailBox

自用多邮箱客户端，Tauri 2（Rust）+ React + TypeScript。

## 当前功能

- 本地 SQLite 缓存（`%APPDATA%\com.mailbox.app\mail.db`），打开即显示，后台按 UID 增量同步收件箱（首次最近 200 封）
- 阅读正文：首次打开从服务器下载并缓存，sandbox iframe + CSP 渲染，默认拦截远程图片，内联图片走 `mailbox://` 协议
- 附件打开 / 保存到 `下载\MailBox`，已读状态双向同步
- 多账号：配置存 `%APPDATA%\com.mailbox.app\accounts.json`，授权码存系统凭据管理器（keyring）
- QQ / Foxmail / 163 / 126 / yeah.net 自动识别服务器

## 开发

```bash
pnpm install
pnpm tauri dev      # 启动应用
pnpm test           # 前端单测
cd src-tauri && cargo test --lib   # Rust 单测
```

QQ / 网易邮箱需在网页版设置中开启 IMAP 并使用**授权码**登录。
