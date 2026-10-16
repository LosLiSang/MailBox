# 配置 Gmail / Outlook 浏览器登录（OAuth）

## 先看结论

| 邮箱 | 需要做什么 |
| --- | --- |
| **Outlook / Hotmail / Live** | **什么都不用配置。**「添加账号 → Outlook」点浏览器登录即可，无需注册 Azure。 |
| **Gmail：应用专用密码**（更简单） | Google 账号开启两步验证 → 生成[应用专用密码](https://myaccount.google.com/apppasswords) → 「添加账号 → Gmail → 改用应用专用密码」。 |
| **Gmail：浏览器登录（OAuth）** | 需要在 Google Cloud 创建自己的 OAuth 应用，按下面 [Gmail OAuth 配置](#gmail-oauth-配置) 一步步做，约 10 分钟。 |

MailBox 里对应的位置：**⚙ 设置 → 高级**。那里的步骤编号、按钮名称和本文一致，每个按钮会直接打开对应的控制台页面。

---

## Gmail OAuth 配置

### 第 0 步：准备代理（国内网络必做）

Google Cloud 控制台、Google 登录页和 `imap.gmail.com` 在国内都无法直连。

1. 浏览器能打开 <https://console.cloud.google.com/>（系统代理 / 浏览器代理）。浏览器授权页走的是系统浏览器，跟随浏览器的代理设置。
2. MailBox「设置 → 代理」：类型选 SOCKS5 或 HTTP，填好地址端口（Clash / v2rayN 常见为 `127.0.0.1:7890`），点「测试」，`imap.gmail.com:993` 显示「连接成功」后点「保存」。

### 第 1 步：新建项目

- 入口：设置页「新建项目」按钮，或 <https://console.cloud.google.com/projectcreate>
- **项目名称**随意（如 `MailBox`），「位置」保持「无组织」，点 **创建**。
- 创建完成后，确认页面**左上角的项目下拉框**显示的是刚建的项目；不是的话点下拉框切换过去。后面所有步骤都在这个项目里操作。

### 第 2 步：启用 Gmail API

- 入口：设置页「打开 Gmail API」按钮，或 <https://console.cloud.google.com/apis/library/gmail.googleapis.com>
- 页面中间的蓝色 **启用（Enable）** 按钮，点一下。已经启用会显示「管理」，跳过即可。

### 第 3 步：Google Auth Platform → 开始配置

- 入口：设置页「打开 Google Auth Platform」按钮，或 <https://console.cloud.google.com/auth/overview>
- 第一次进入会看到「Google Auth Platform 尚未配置」，点 **开始（Get started）**，按向导填写：
  1. **应用信息**：应用名称 `MailBox`，用户支持邮箱选你自己 → 下一步
  2. **目标对象**：选 **外部（External）** → 下一步
  3. **联系信息**：填你自己的邮箱 → 下一步
  4. 勾选同意用户数据政策 → **创建**
- 完成后左侧菜单会出现：概览、品牌塑造、**目标对象**、**客户端**、**数据访问** 等页面。

### 第 4 步：目标对象 → 添加测试用户

- 入口：设置页「打开目标对象」按钮，或 <https://console.cloud.google.com/auth/audience>
- 「发布状态」保持 **测试（Testing）**。
- 往下找到 **测试用户（Test users）** → **+ 添加用户（Add users）**，填入你要在 MailBox 登录的 Gmail 地址（可以填多个），点 **保存**。

> [!WARNING]
> **一定要把自己加为测试用户。** 不加的话，登录时会提示「已拒绝访问：MailBox 未完成 Google 验证流程」（`Error 403: access_denied`）。

### 第 5 步：数据访问 → 添加 Gmail 范围

- 入口：设置页「打开数据访问」按钮，或 <https://console.cloud.google.com/auth/scopes>
- 点 **添加或移除范围（Add or remove scopes）**，右侧弹出面板：
  - 在过滤框搜索 `mail.google.com`，勾选 `https://mail.google.com/`（Gmail API，「阅读、撰写、发送和永久删除…」）。IMAP 登录只认这个完整权限。
  - 列表里找不到（通常是第 2 步没启用 Gmail API）：在面板底部 **手动添加范围** 粘贴 `https://mail.google.com/`，点 **添加到表格**。
  - 点面板底部 **更新（Update）**，再点页面底部 **保存（Save）**。
- `openid`、`email` 属于基础范围，不加也能用。

### 第 6 步：客户端 → 创建客户端

- 入口：设置页「打开客户端」按钮，或 <https://console.cloud.google.com/auth/clients>
- 点 **+ 创建客户端（Create client）**：
  - **应用类型**：**桌面应用（Desktop app）**。不要选「Web 应用」。
  - **名称**随意，点 **创建**。
  - 不需要填重定向 URI：桌面应用默认允许 `http://127.0.0.1:<任意端口>`。
- 弹窗里显示 **客户端 ID**（`xxxx.apps.googleusercontent.com`）和 **客户端密钥**（`GOCSPX-...`），**马上复制**或点「下载 JSON」。

> [!IMPORTANT]
> **客户端密钥只在这个弹窗里显示一次。** 关掉后控制台只显示最后几位。
> 错过了：点进这个客户端的详情页 → **添加密钥（Add secret）** 生成新的，立即复制；旧密钥可以停用。

### 第 7 步：填入 MailBox 并登录

1. MailBox「⚙ 设置 → 高级 → Google OAuth 应用」：
   - **Client ID（客户端 ID）** 粘贴 `...apps.googleusercontent.com`
   - **Client Secret（客户端密钥）** 粘贴 `GOCSPX-...`
   - 首尾空格会自动去掉；格式明显不对（比如两个值填反）时输入框下方会给出提示
   - 点底部 **保存**
2. 「设置 → 账号 → 添加账号 → Gmail」，勾选 **通过代理连接**，点 **使用 Gmail 账号登录**。
3. 浏览器里选择你的 Gmail 账号。会出现「Google 尚未验证此应用」——这是正常的（应用是你自己的），点 **继续**；勾选允许访问 Gmail，点 **继续**。
4. 浏览器显示登录成功后回到 MailBox，账号已添加。

> [!NOTE]
> **测试状态下登录 7 天后过期。** Google 对「测试」状态应用的 refresh token 只给 7 天有效期，到期后同步会提示重新登录：
> 「设置 → 账号 → 编辑 → 重新登录」即可。
> 想长期免登录：在「目标对象」页点 **发布应用（Publish app）**，改为「正式版」。个人使用不需要提交审核，登录时仍会显示「未经验证」警告，点「继续」即可。

桌面应用的「客户端密钥」按 Google 的说明并不保密，但仍然必须提交，所以两个都要填。

---

## 常见错误

| 现象 / 错误 | 原因 | 处理 |
| --- | --- | --- |
| MailBox 提示「还没有配置 Google OAuth 应用」 | 「设置 → 高级」里 Client ID 或 Secret 没填、没保存 | 按上面第 1～7 步获取并保存。对话框里可以直接点「去『设置 → 高级』配置」 |
| 「已拒绝访问：未完成 Google 验证流程」/ `Error 403: access_denied` | 登录用的 Gmail 不在测试用户列表 | [第 4 步](#第-4-步目标对象--添加测试用户)添加测试用户；确认添加的地址和登录的一致 |
| `access_denied`（自己点了「取消」） | 授权页没有点「继续」/没有勾选 Gmail 权限 | 重新登录，在权限页勾选「阅读、撰写、发送和永久删除…」 |
| `Error 400: redirect_uri_mismatch` | 客户端类型选成了「Web 应用」 | [第 6 步](#第-6-步客户端--创建客户端)重新创建一个 **桌面应用** 类型的客户端，替换 ID 和 Secret |
| `Error 401: invalid_client` / `unauthorized_client` | Client ID 或 Secret 复制错了、填反了、属于别的项目，或该密钥已被停用 | 回到客户端详情页核对；Secret 丢了就「添加密钥」重新生成 |
| `invalid_scope` 或登录成功但 IMAP 报 `AUTHENTICATE failed` | 没加 `https://mail.google.com/` 范围，或授权时没勾 Gmail 权限 | [第 5 步](#第-5-步数据访问--添加-gmail-范围)补上范围，再「编辑 → 重新登录」 |
| 约 7 天后提示登录已过期 / `invalid_grant` | 应用处于「测试」状态，refresh token 到期；或你在 Google 账号里撤销了授权 | 「设置 → 账号 → 编辑 → 重新登录」；想长期免登录就发布应用 |
| 浏览器一直转圈或「无法访问此网站」 | 浏览器没走代理 | 先确认浏览器能打开 `accounts.google.com` |
| 授权成功但 MailBox 连接超时 | MailBox 没走代理 | 「设置 → 代理」测试通过，账号勾选「通过代理连接」 |

---

## Outlook / Hotmail / Live

### 普通用户：直接登录（无需配置）

1. 网页版 Outlook → 设置 → 邮件 → 转发和 IMAP，确认允许设备和应用使用 IMAP。
2. MailBox →「添加账号 → Outlook」，点 **使用 Outlook / Hotmail 账号登录**，在微软页面完成登录并同意授权。
3. 公司/学校邮箱可能需要管理员批准，客户端不能绕过组织策略。

「设置 → 高级 → Microsoft OAuth 应用」**保持留空**即可，MailBox 使用内置的公共客户端注册（Client ID `88600fe5-f9d3-4b15-974d-1c299963a96e`，可公开的应用标识，不是密钥）。
若之前填过自定义 ID，展开「使用自己的 Microsoft 应用」清空并保存即恢复默认；更换应用后，已有账号可能需要重新登录。

### 自定义应用（高级）

> 只有项目维护者，或需要用自己组织的应用注册时才需要看这一节。普通用户请跳过。

应用注册必须放在有注册权限的 Microsoft Entra 目录中；个人 Microsoft 账号不保证有可用目录。
Azure 注册可能要求银行卡验证；没有目录时，可由可信协作者在长期维护的目录中为项目注册。
应用注册本身不要求部署云资源，不应承诺任何账号均可免费创建租户。

1. [Microsoft Entra 管理中心](https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade) → 应用注册 → 新注册：
   - 名称：`MailBox`
   - 支持的账户类型：任何组织目录中的账户和个人 Microsoft 账户
   - 重定向平台：公共客户端/本机（移动和桌面应用程序）
   - 回调 URI：`http://localhost/`，也可以注册 `http://localhost:34567/`
2. 应用 → 身份验证，确认回调属于「移动和桌面应用程序」，不要配置成 Web / SPA。
   MailBox 使用授权码 + PKCE 和随机空闲端口；微软匹配该 localhost 原生回调时忽略端口，因此不必固定监听 34567。
3. API 权限 → 添加权限 → Office 365 Exchange Online → 委托的权限：
   添加 `IMAP.AccessAsUser.All`。不要用 Microsoft Graph 的 `Mail.Read` 替代。
   登录还请求 `offline_access openid email profile`，用于续期和账号识别。
4. 从概述复制「应用程序（客户端）ID」（GUID 格式），不是对象 ID 或租户 ID。
   填到 MailBox「设置 → 高级 → Microsoft OAuth 应用 → 使用自己的 Microsoft 应用」，覆盖默认应用。

**不需要** Client Secret，也不要创建或内置：MailBox 是公共桌面客户端，使用 PKCE。
本地代码配置不代表微软后台已经正确设置；公开发布前必须验证真实账号登录、续期和 IMAP 访问。
维护者应配置准确的应用名称、主页、隐私政策和联系方式，并明确目录与应用的长期管理归属。

常见错误：`AADSTS50011`（回调不匹配）→ 检查第 1～2 步的平台类型和 `http://localhost/`；`AADSTS65001` / 需要管理员批准 → 组织策略限制，联系管理员；IMAP 报 `AUTHENTICATE failed` → 检查第 3 步权限，并确认网页版 Outlook 已允许 IMAP。

---

## 工作原理

1. MailBox 在本机随机端口启动一个临时监听（只接受 127.0.0.1 / ::1）
2. 用系统浏览器打开授权页，登录并同意后浏览器跳回 `http://localhost:<端口>/?code=...`
3. MailBox 用授权码 + PKCE 换取 access token 和 refresh token
4. 令牌保存在 Windows 凭据管理器（`MailBox` / `oauth:<邮箱>`），不写入任何文件
5. IMAP 使用 `AUTHENTICATE XOAUTH2` 登录；access token 过期前自动用 refresh token 续期
