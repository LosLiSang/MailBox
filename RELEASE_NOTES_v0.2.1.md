# MailBox v0.2.1

修复版本。修复了 MailBox 在托盘中运行时再次打开会报「数据库错误: database disk image is malformed」的问题。直接覆盖安装 v0.2.0 即可，本地数据和设置都会保留。

---

## 🐛 修复

### 重复打开导致数据库报错（#12 / #13）
- **只运行一个实例**：MailBox 已经在后台或托盘中运行时，再次从桌面图标或开始菜单打开，会直接调出原来的窗口，不会启动第二个进程。之前两个进程同时写入本地缓存，就会出现上述报错。
- **短暂冲突时等待**：后台同步和界面操作同时读写缓存时会稍等片刻，不再立即报「database is locked」。
- **缓存损坏时自动恢复**：启动时如果发现本地缓存文件确实已损坏，会把它改名保留为 `mail.corrupt-<时间戳>.db`，再新建一个缓存，程序可以正常打开：
  - 尽量从旧缓存中迁移**分类（含说明）和发件人规则**；
  - 邮件列表和正文会从服务器重新同步；
  - 确认一切正常后，可以删除 `%APPDATA%\com.mailbox.desktop\` 下的 `mail.corrupt-*` 文件。

## ⚠️ 升级说明
- 安装前请先在托盘图标上右键 →「退出」，关闭正在运行的旧版本。

---

## 📦 下载与安装 (Windows x64)

| 文件名 | 格式 | 描述 |
| :--- | :--- | :--- |
| `MailBox_0.2.1_x64-setup.exe` | NSIS 安装程序 | 推荐：安装到用户目录，无需管理员权限 |
| `MailBox_0.2.1_x64_en-US.msi` | MSI 安装包 | Windows Installer 标准部署包 |
| `MailBox_0.2.1_x64.exe` | 独立便携版 | 免安装单个执行文件（需系统已安装 WebView2） |

### SHA256 校验码

```text
84c4bf0a62905488d6b6cd34093b243c368dace6ba50d8e61ceff867a123f62b  MailBox_0.2.1_x64-setup.exe
052f058242c4fce38a3f95484a543957df11f42f9d3f1216385befe36481be33  MailBox_0.2.1_x64_en-US.msi
63b15248e3ba01517d6595379e7d4d83a44e3b52e41583e003582b12bb53b81b  MailBox_0.2.1_x64.exe
```

**完整变更**：https://github.com/LosLiSang/MailBox/compare/v0.2.0...v0.2.1
