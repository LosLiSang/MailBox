# MailBox

轻量、现代、本地优先的多邮箱桌面客户端。基于 **Tauri 2 (Rust) + React 19 + TypeScript** 构建。

![MailBox](public/icon.png)

## 核心特性 (v0.2.1)

- **⚡ 秒级打开与离线缓存**：基于本地 SQLite 存储邮件头与已读正文，打开即刻呈现，后台按 UID 静默增量同步，无惧断网或慢速连接。
- **📜 加载更多**：列表底部一键加载更早的邮件，本地缓存不够时自动从服务器拉取更早的邮件头。
- **🔔 Windows 原生系统深度集成**：
  - **任务栏未读角标徽标**：类似 Outlook，在 Windows 任务栏应用图标右上角直观显示未读邮件总数，未读清空时自动复原。
  - **桌面新邮件横幅通知**：收到新邮件时在 Windows 任务栏右下角弹出原生 Toast 桌面通知，点击直接查看。
  - **系统托盘常驻**：关闭窗口后最小化到通知区，后台继续自动同步与通知；单击托盘图标恢复窗口，右键菜单「退出」才真正结束。
  - **开机自启动**：在「设置 → 启动与同步」中开启，开机后静默常驻托盘，不弹出主窗口。
- **🛡️ 安全与隐私保护**：
  - 登录授权码、密码与 OAuth Token 统一加密保存在操作系统原生凭据管理器（Windows Credential Manager / Keyring）中，明文绝不落盘。
  - 阅读正文通过 CSP Sandbox Iframe 隔离渲染，彻底杜绝恶意脚本执行。
  - 支持远程图片拦截、代理下载与信任发件人白名单机制，防范邮件隐蔽追踪像素。
- **📖 纯粹舒适的阅读体验**：
  - 原生支持智能深色模式、正文自适应色彩转换，夜间阅读更护眼。
  - 自定义界面字体与正文字体字号，自由调节列表舒适/紧凑密度与双栏宽度。
  - 列表与正文预缓存：后台预先拉取最近未读邮件原始正文，点开零等待。
- **📁 本地分类与自动化规则**：每个邮箱拥有独立分类与标签，支持按发件人模式自动归类。
- **🧠 智能分类（可选，默认关闭）**：发件人规则没有命中的新邮件，交给兼容 Jev schema 的决策模型按内容归类。
  - 内置 **Cloudflare Workers AI（Clef Flash）** 预设，填写 Account ID 与 API Token 即可使用；也支持任意兼容的自定义端点。
  - 只发送**发件人与主题**，不发送正文和附件；低于置信度阈值的不改动；模型失败时自动回退到规则，不影响同步。
  - 可为每个分类填写「分类说明」，提升判断准确度。配置方法见下方「智能分类配置」。
- **🌐 全面的协议与服务商适配**：
  - 内置支持 **Outlook / Hotmail**（无需注册 Azure，一键浏览器 OAuth 授权登录）。
  - 支持 **Gmail**（标准 OAuth 2.0 PKCE 流程）。
  - 支持 **QQ 邮箱、腾讯企业邮、iCloud、Yahoo、阿里邮箱**及任意标准 IMAP/SMTP 邮箱。
  - 内置支持全局与按账号独立的 HTTP / SOCKS5 代理配置及网络连通性测速。

---

## 数据存储

- 账号配置与本地数据库：`%APPDATA%\com.mailbox.desktop\`
  - `accounts.json`：账号连接参数
  - `settings.json`：全局偏好设置
  - `mail.db`：SQLite 本地邮件索引与正文缓存
  - `mail.corrupt-<时间戳>.db`：仅在本地缓存损坏时出现，是自动改名保留的旧库，确认无用后可删除
- 凭据信息：Windows 系统凭据管理器（服务名：`MailBox`，包括邮箱授权码、OAuth Token、代理密码与智能分类 API Token）
- 附件保存路径：`下载\MailBox\`

---

## 智能分类配置（Cloudflare Workers AI）

1. 登录 [Cloudflare 控制台](https://dash.cloudflare.com)，在 **Workers AI** 页面复制 **Account ID**。
2. 打开 [API Tokens](https://dash.cloudflare.com/profile/api-tokens) → **Create Token** → 使用 **Workers AI** 模板（或自定义权限 `Account → Workers AI`），资源只选自己的账户。Token 只显示一次，请立即复制。
3. 在 MailBox **设置 → 智能分类** 中选择「Cloudflare Workers AI（Clef Flash）」，填入 Account ID 与 Token，点击 **测试**。
4. 测试通过后勾选「开启智能分类」并保存。

> Workers AI 有每日免费额度，超出后按量计费，以 Cloudflare 官方价格为准。每次同步最多调用模型 50 次。

自定义端点需接收 `{ model?, state, questions: { category: { type: "choice", instructions, criteria } } }`，返回 `answers.category` 中的 `choice`（或 `value`）与 `confidence`（或 `probabilities`），使用 Bearer Token 认证；远程端点必须为 https。

---

## 本地开发与构建

### 环境要求
- Node.js >= 18，`pnpm` >= 9
- Rust >= 1.77
- Visual Studio C++ 生成工具 (Windows)

### 常用命令

```bash
# 安装依赖
pnpm install

# 启动开发调试
pnpm tauri dev

# 运行自动化测试
pnpm test                          # 前端 Vitest 单测
cargo test --manifest-path src-tauri/Cargo.toml  # Rust 核心库单测

# 打包生产安装包 (x86_64 Windows)
pnpm tauri build
```

打包产物位于 `src-tauri/target/release/bundle/nsis/` 与 `msi/` 目录下。

---

## 开源许可

本项目遵循 MIT 许可证发布。
