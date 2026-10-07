# MailBox

自用多邮箱客户端，Tauri 2（Rust）+ React + TypeScript。

## 当前功能

- 通过 IMAP 拉取收件箱最近 50 封邮件头（支持 GBK / UTF-8 等编码）
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
