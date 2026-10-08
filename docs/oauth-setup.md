# 配置 Gmail / Outlook 浏览器登录（OAuth）

Gmail 和 Outlook 的浏览器登录需要一个「OAuth 应用」来标识 MailBox。
Outlook 已内置 MailBox 项目的公共客户端注册，用户无需申请 Azure；Gmail 仍需配置自己的 Google 应用。

> 只想先用起来 Gmail，也可以跳过本文：开启两步验证后生成「应用专用密码」，
> 添加账号时选 Gmail → 「改用应用专用密码」。Outlook / Hotmail 个人邮箱已经不支持密码登录，只能走 OAuth。

国内网络访问 Google 需要代理：先在「设置 → 代理」填好并测试通过，添加账号时勾选「通过代理连接」。
浏览器授权页本身走的是系统浏览器，跟随浏览器的代理设置。

---

## Google（Gmail）

Google Cloud 控制台现在把 OAuth 配置放在 **Google Auth Platform** 下，分为品牌塑造、目标对象、数据访问、客户端几个页面。

1. 打开 [Google Cloud Console](https://console.cloud.google.com/)，顶部项目下拉框 → **新建项目**（名字随意，如 `MailBox`），创建后切换到这个项目。
2. （推荐）「API 和服务 → 库」搜索 **Gmail API** 并启用，这样下一步的范围列表里能直接找到 Gmail 权限。
3. 左侧菜单进入 **Google Auth Platform**，第一次使用点「开始」：
   - 应用名称：`MailBox`；用户支持邮箱、开发者联系邮箱：填你自己的
   - 目标对象（用户类型）：**外部（External）**
4. **目标对象（Audience）** → 测试用户 → **添加用户**，填入你要登录的 Gmail 地址。
   不加的话登录时会提示「已拒绝访问：未完成 Google 验证流程」。发布状态保持「测试」即可。
5. **数据访问（Data Access）** → 添加或移除范围：
   - 勾选 `https://mail.google.com/`（Gmail 完整权限，IMAP 需要这个）
   - 列表里找不到就在底部「手动添加范围」粘贴进去，点「添加到表格」→「更新」→ 保存
   - `openid`、`email` 属于基础范围，不加也能用
6. **客户端（Clients）** → **创建客户端**：
   - 应用类型：**桌面应用（Desktop app）**，名称随意
   - 不需要填重定向 URI，桌面应用默认允许 `http://127.0.0.1:<任意端口>`
   - 创建后复制 **客户端 ID**（`xxxx.apps.googleusercontent.com`）和 **客户端密钥**（`GOCSPX-...`）
   - ⚠️ 客户端密钥**只在创建时的弹窗里显示一次**，关掉后控制台只显示最后几位。
     错过了就进入这个客户端的详情页，点 **添加密钥（Add secret）** 生成一个新的，并立即复制
7. 回到 MailBox：「⚙ 设置 → 高级 → Google OAuth 应用」填入这两个值，点「保存」。
8. 「设置 → 代理」填好代理并测试 `imap.gmail.com:993` 通过，再「添加账号 → Gmail」，勾选「通过代理连接」，点「使用 Gmail 账号登录」。

注意：
- 桌面应用的「客户端密钥」按 Google 的说明并不保密，但仍然必须提交，所以两个都要填。
- 测试状态下 refresh token **7 天后过期**，届时在「设置 → 账号 → 编辑 → 重新登录」即可。
  想长期免登录，需要在权限请求页面把应用「发布」为正式版（个人使用时 Google 会显示「未经验证」警告，点「继续」即可）。

## Microsoft（Outlook / Hotmail / Live）

### 普通用户：直接登录

1. 网页版 Outlook → 设置 → 邮件 → 转发和 IMAP，确认允许设备和应用使用 IMAP。
2. MailBox → 添加账号 → Outlook，点击浏览器登录按钮，在微软页面完成登录并同意授权。
3. 公司/学校邮箱可能需要管理员批准，客户端不能绕过组织策略。

默认 Client ID 为 `88600fe5-f9d3-4b15-974d-1c299963a96e`。这是可公开的应用标识，不是密钥。
「设置 → 高级 → Microsoft OAuth 应用」留空即使用默认注册，包括旧版本已经保存的空值。
若之前设置了自定义 ID，清空并保存即可恢复默认；更换应用后，已有账号可能需要重新登录。

### 项目维护者 / 自定义应用

应用注册必须放在有注册权限的 Microsoft Entra 目录中；个人 Microsoft 账号不保证有可用目录。
Azure 注册可能要求银行卡验证；没有目录时，可由可信协作者在长期维护的目录中为项目注册。
应用注册本身不要求部署云资源，不应承诺任何账号均可免费创建租户。

1. Microsoft Entra ID → 应用注册 → 新注册：
   - 名称：`MailBox`
   - 支持的账户类型：任何组织目录中的账户和个人 Microsoft 账户
   - 重定向平台：公共客户端/本机（移动和桌面应用程序）
   - 回调 URI：`http://localhost/`，也可以注册 `http://localhost:34567/`
2. 应用 → 身份验证，确认回调属于「移动和桌面应用程序」，不要配置成 Web / SPA。
   MailBox 使用授权码 + PKCE 和随机空闲端口；微软匹配该 localhost 原生回调时忽略端口，因此不必固定监听 34567。
3. API 权限 → 添加权限 → Office 365 Exchange Online → 委托的权限：
   添加 `IMAP.AccessAsUser.All`。不要用 Microsoft Graph 的 `Mail.Read` 替代。
   登录还请求 `offline_access openid email profile`，用于续期和账号识别。
4. 从概述复制「应用程序（客户端）ID」，不是对象 ID 或租户 ID。
   自定义部署可填到 MailBox 高级设置中，覆盖默认应用。

**不需要** Client Secret，也不要创建或内置：MailBox 是公共桌面客户端，使用 PKCE。
本地代码配置不代表微软后台已经正确设置；公开发布前必须验证真实账号登录、续期和 IMAP 访问。
维护者应配置准确的应用名称、主页、隐私政策和联系方式，并明确目录与应用的长期管理归属。

## 工作原理

1. MailBox 在本机随机端口启动一个临时监听（只接受 127.0.0.1 / ::1）
2. 用系统浏览器打开授权页，登录并同意后浏览器跳回 `http://localhost:<端口>/?code=...`
3. MailBox 用授权码 + PKCE 换取 access token 和 refresh token
4. 令牌保存在 Windows 凭据管理器（`MailBox` / `oauth:<邮箱>`），不写入任何文件
5. IMAP 使用 `AUTHENTICATE XOAUTH2` 登录；access token 过期前自动用 refresh token 续期
