# CampusNet Reconnector

常驻系统托盘的校园网自动重连工具（Windows）。检测到"已连上指定 Wi-Fi 但未通过认证"时，自动向**你指定的认证网页**提交账号密码完成登录。Tauri v2 + Rust，常驻内存约 15-25MB。

## 功能

- 周期探测（generate_204 类端点）判定：已联网 / 需认证（302 重定向）/ 无网
- **SSID 白名单**：仅在连接指定校园网 Wi-Fi 时才可能触发自动登录；宿舍路由器、热点等绝不误动作
- **认证页自定义**：软件不内置厂商协议。用浏览器 F12 看一次认证页提交的字段名，填入模板（支持 form/JSON、自定义请求头、成功判定条件、可选前置请求）
- 托盘状态图标（绿/黄/红/灰）、立即检测、暂停、通知
- 失败指数退避（10s→30s→…→封顶），到达最大次数后停止并通知，不会轰击认证服务器
- 密码仅存于 **Windows 凭据管理器**，不落配置文件、不进日志
- 设置窗口懒创建、关闭即销毁 WebView2，空闲时仅 Rust 主进程常驻

## 使用

1. 从 [Releases](../../releases) 下载：
   - **Portable**：`campus-net-reconnector_*_x64_portable.exe` — 单文件免安装，放到任意目录直接运行
   - **Installer**：`.msi` 或 `setup.exe` — 向导式安装，可卸载
2. 右键托盘图标 → 设置：
   - 填写校园网 SSID 白名单
   - 填写认证页 URL 与字段模板（默认 `username/password` POST 表单，按需修改）
   - 填写账号密码并保存
3. 完成。掉线后会自动重新登录，可在"日志"窗口查看每次判定原因

### 模板占位符

| 占位符 | 含义 |
|---|---|
| `{username}` `{password}` | 账号密码 |
| `{portal_url}` | 探测 302 发现的重定向地址（可选提示用） |
| `{step1_body}` | 前置请求的响应（或其正则提取结果） |

成功判定格式（每行一条，全部满足才算成功）：`status=200` / `contains=xxx` / `notcontains=xxx` / `regex=xxx` / `reprobe`（登录后再探测一次 204）。留空默认 HTTP 2xx。

## 隐私与安全说明

校园网认证页多为 HTTP 明文，密码在提交时于校内网络可见——这是认证协议本身的限制，与本工具无关。密码存于本机凭据管理器，配置文件（`%APPDATA%\campus-net-reconnector\config.json`）不含密码。

高级选项：设置环境变量 `CNR_INSECURE_TLS=1` 可跳过 HTTPS 证书校验（仅当认证服务器使用自签名证书时使用）。

## 开发

```bash
pnpm install
pnpm dev      # 开发模式
pnpm check    # 前端类型检查
cargo test    # Rust 单测（在 src-tauri/ 下）
pnpm build    # 打包
```

## 发布

版本号唯一来源是 `src-tauri/tauri.conf.json`：

- GitHub 仓库页面 → Actions → **Version bump** → 填入版本号运行：自动同步三处版本号、提交、打 tag，并触发 Release 构建
- Release 工作流由 `tauri-apps/tauri-action` 自动读取版本号，产出 Windows `.msi` / NSIS `setup.exe` 安装包，并额外上传 **portable 单文件版**（裸 exe + sha256），默认先出 **Draft**，人工确认后 Publish

## 许可

MIT
