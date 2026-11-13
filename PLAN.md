# 校园网自动重连工具（CampusNet Reconnector）— 详细规划

> 状态：待审阅
> 日期：2026-11-13
> 技术栈：Tauri v2 + Rust + TypeScript（前端）

---

## 1. 项目概述

一款常驻系统托盘的轻量桌面工具，解决校园网"掉线后需要手动打开认证页面重新登录"的问题：

- 按用户设定的时间间隔，周期性检测当前网络是否可以上外网（是否通过认证）。
- **仅当**当前连接的 Wi-Fi 是用户指定的校园网 SSID，且检测到"已连 Wi-Fi 但未通过认证"时，自动向**用户指定的认证网页**提交账号密码完成登录。
- 其他 Wi-Fi（宿舍路由器、手机热点、家里网络）下完全不动作，不打扰。
- 通过 GitHub Actions 自动构建并发布 Release，版本号自动从项目配置中读取。

### 1.1 设计目标

| 目标 | 做法 |
|---|---|
| 轻量、内存占用小 | 托盘常驻 + WebView 懒加载（设置窗口关闭即销毁 WebView），后台检测逻辑纯 Rust，不依赖浏览器 |
| 不误动作 | SSID 白名单 + 明确的"需认证"判定（204 检测），双重条件同时满足才提交登录 |
| 适配任意学校 | 认证网页由用户直接指定 URL，配合"配置驱动的 HTTP 提交模板"（方法/字段/成功判定均可配置），不做内置厂商预设 |
| 凭据安全 | 密码存入 Windows 凭据管理器（keyring），不落明文配置文件 |
| 可观测 | 托盘图标状态（绿/黄/红）+ 环形日志，方便排查"为什么没自动登录" |

### 1.2 非目标（第一版不做）

- 跨平台（macOS / Linux）——**本工具只做 Windows 版**，代码直接使用 Windows API，不做平台抽象。
- 替代系统 Wi-Fi 连接管理工具——本工具不做"扫描并连接 Wi-Fi"，只做"已连接指定 SSID 时的认证补救"。（后续可作为可选增强：掉线后自动调系统 API 重连指定 SSID。）
- 破解/绕过认证——只做合法的"重新提交自己的账号密码"。
- 内置各厂商认证系统的专用协议实现——认证网页由用户自己指定，软件只负责向该网页提交配置好的请求。

---

## 2. 功能需求

### 2.1 核心功能（M1 必须有）

1. **连通性检测**
   - 周期性向多个"网络可达性探测端点"之一发 HTTP GET（如 `http://www.gstatic.com/generate_204`、`http://connect.rom.miui.com/generate_204`、`http://www.msftconnecttest.com/connecttest.txt`，可在设置里选择，默认 204 端点）。
   - 判定规则：
     - 返回 `204`（或 200 且命中预期内容）→ **已联网**
     - 返回 `302/301`，且 `Location` 指向非探测端点 → **被重定向到认证门户 → 需要认证**（同时自动记录门户 URL，用于登录）
     - 超时 / 连接失败 → **无网**（连 Wi-Fi 可能都没连上）
   - 探测超时：默认 5s，可配置。

2. **SSID 白名单**
   - 用户在设置中添加一个或多个"校园网 SSID"（如 `CampusNet`、`CampusNet-5G`）。
   - 通过 Windows WLAN 原生 API（`windows` crate，`WlanQueryInterface`）读取当前连接的 SSID；仅当当前 SSID 命中白名单时，检测到"需认证"才会触发登录。
   - 白名单为空视为"任意网络都触发"（不推荐，默认要求至少填一个）。

3. **认证提交引擎**
   - **认证网页由用户指定**：设置中直接填写认证页 URL（如 `http://10.10.10.10/login` 或门户完整地址）；302 重定向发现的 URL 仅作为提示/一键填充参考，不自动替代用户配置。
   - **提交模板**：用户配置请求方法（GET/POST）、表单（form）或 JSON 字段模板（`{username}` `{password}` 等占位符）、额外请求头；软件按模板向指定 URL 提交账号密码。
   - **成功判定**：支持多重条件（HTTP 状态码、响应体包含/不包含某关键字、响应体正则、再次探测 204）。判定成功与否写日志。
   - 典型用法：校园网认证页基本都是一个登录表单，用浏览器开发者工具（F12 → Network）看一次提交的字段名，填进模板即可。

4. **重试策略**
   - 登录失败时指数退避重试（如 10s → 30s → 60s，封顶 5 次），避免频繁 POST 打爆门户或触发账号锁定。
   - 连续失败超过阈值后暂停自动登录并在托盘/通知中提示，等用户手动处理或重置。

5. **托盘 UI**
   - 托盘图标反映状态：绿=已联网、黄=需认证/正在登录、红=无网/连续失败、灰=已暂停。
   - 托盘菜单：立即检测、暂停/恢复、打开设置、打开日志、退出。
   - 气泡通知（可选开关）：登录成功/失败、进入长时间失败状态时提醒。

6. **设置窗口（按需创建）**
   - 基本：检测间隔（如 30s ~ 10min）、探测端点、SSID 白名单、开机自启（tauri autostart 插件）。
   - 认证：认证页 URL（必填）、账号、密码、提交模板（方法、字段、成功判定，默认提供常见表单模板可直接改）。
   - 高级：重试参数、检测超时、是否开机后立即检测一次。
   - 日志窗口：环形缓冲最近 N 条（如 500 条），含时间戳与动作原因。

### 2.2 增强功能（M2+，可裁剪）

- 掉线后自动调用系统 API 重连指定 SSID（WlanConnect）。
- 多认证配置（不同 SSID 绑定不同账号/认证页模板）。
- 内置更新检查（Tauri updater 插件，指向 GitHub Releases）。
- 系统休眠/唤醒事件感知（唤醒后立即检测一次）。
- 迷你状态窗口（悬浮小窗显示剩余倒计时）——可选，注意内存权衡。

---

## 3. 技术选型与理由

| 层 | 选择 | 理由 |
|---|---|---|
| 应用框架 | **Tauri v2** | 安装包小（~3-10MB vs Electron 100MB+），Rust 后端性能好；v2 的 tray、autostart、notification、deep-link 插件成熟，权限系统（capabilities）细粒度 |
| 后端语言 | Rust（tauri 主进程即后端） | 检测循环、HTTP、WLAN API 全在 Rust，前端只在打开设置时才存在 |
| HTTP | `reqwest`（blocking 或 tauri 的 tokio runtime 内 async）+ `rustls` | 避免 OpenSSL 依赖；门户多为 HTTP 明文，也支持 |
| SSID 获取 | `windows` crate（WLAN 原生 API） | 不 spawn `netsh` 子进程（更快、更可靠、无编码问题） |
| 凭据存储 | `keyring` crate → Windows 凭据管理器 | 密码不进配置文件、不进 git |
| 配置存储 | `serde` + JSON，存于 `%APPDATA%/<app>/config.json` | 简单直观，方便用户手动备份/迁移；只存非敏感项 |
| 前端 | 原生 HTML/TS + 轻量方案（详见 §5.6） | 设置页很简单，不需要 React/Vue 全家桶；或用 Svelte（编译产物小）。倾向：**无框架 TS + 少量手写组件**，最大化轻量 |
| CI/CD | GitHub Actions + `tauri-action` | 官方维护，自动读 `tauri.conf.json` 版本号打 Release |

### 3.1 关于"为什么 Tauri v2 也能做到小内存"

WebView2 进程只在窗口存在时创建。策略：

1. 启动时**不创建任何窗口**（`tauri.conf.json` 中 `app.windows: []`），只启动托盘。
2. 设置窗口用 Rust 侧 `WebviewWindowBuilder` **懒创建**，用户关闭时销毁整个窗口（不是 hide），下次打开重建（配置由 Rust 侧持有，重建成本 ~200ms）。
3. 因此常驻内存 ≈ Rust 主进程（约 10-20MB）+ 托盘，打开设置时才短暂出现 WebView2（约 40-80MB，关闭后释放）。
4. HTTP 客户端复用连接池；检测循环用 `std::thread` + 简单睡眠或 tokio interval（Tauri 自带 tokio，不额外引依赖成本）。

---

## 4. 系统架构

```
┌─────────────────────────────────────────────────────────┐
│                    Rust 主进程 (常驻)                     │
│                                                         │
│  ┌──────────────┐   ┌───────────────────────────────┐   │
│  │ 托盘模块      │   │ 检测调度器 (Monitor Loop)       │   │
│  │ tray + 菜单   │◄──┤  interval 定时 → 触发检测流程    │   │
│  │ 状态图标/通知  │   └──────────────┬────────────────┘   │
│  └──────▲───────┘                  │                     │
│         │                  ┌───────▼────────┐            │
│  ┌──────┴───────┐          │  状态机 (State)  │           │
│  │ 设置窗口(懒建) │◄────────►│ Idle/Checking/  │           │
│  │ Webview      │  events  │ NeedAuth/...    │           │
│  └──────▲───────┘          └───────┬────────┘            │
│         │                          │                      │
│  ┌──────┴──────────────────────────▼──────────────────┐  │
│  │ 服务层                                              │  │
│  │  • NetProbe     探测 204 / 解析 302 Location         │  │
│  │  • SsidWatcher  WLAN API 读当前 SSID                 │  │
│  │  • PortalAuth   认证提交引擎 (用户指定 URL + 模板)    │  │
│  │  • ConfigStore  配置读写 (JSON)                      │  │
│  │  • KeyringStore 账号密码存取 (Windows 凭据管理器)      │  │
│  │  • LogRing      环形日志 (内存 + 可选文件)            │  │
│  └────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

- 单向数据流：调度器驱动状态机，状态机调用服务层，结果广播给托盘与（若打开的）设置窗口。
- 配置变更即时生效（保存后调度器重载间隔）。

---

## 5. 核心模块详细设计

### 5.1 状态机

```
            ┌────────────────────────────────────────────┐
            │        interval 触发 或 用户"立即检测"        │
            ▼                                            │
  ┌──────────────┐   SSID 不在白名单 / 已联网             │
  │   Checking   ├──────────────────────────────────► Idle(Online)
  │ (探测中)      │                                     │
  └──────┬───────┘                                      │
         │ 302 → 需认证 且 SSID 命中                      │
         ▼                                              │
  ┌──────────────┐   登录成功                            │
  │   Authing    ├──────────────────────────────────► Idle(Online)
  │ (提交登录)    │                                     │
  └──────┬───────┘                                      │
         │ 登录失败 (≤max_retry)                         │
         ▼                                              │
  ┌──────────────┐                                      │
  │  Backoff     │  退避后回到 Checking                   │
  └──────┬───────┘                                      │
         │ 连续失败 > max_retry                          │
         ▼                                              │
  ┌──────────────┐                                      │
  │  GaveUp(红)  │  等待：用户重置 / 网络变化 / 下轮检测     │
  └──────────────┘                                      │
```

补充规则：

- 无网（探测超时且 SSID 不在白名单或无 Wi-Fi）→ `Idle(Offline)`，不尝试登录。
- SSID 命中白名单但探测超时 → 可能是"连上了但门户不可达"，仍尝试登录一次（门户 IP 可能固定，可配置直连门户地址探测）。
- 用户点"暂停"→ `Paused(灰)`，停表；恢复时立即检测一次。
- 检测流程全程带超时与并发去重（上一轮未完成则跳过本轮，避免堆积）。

### 5.2 NetProbe（探测模块）

```rust
pub enum ProbeResult {
    Online,                       // 204 或预期内容
    NeedAuth { portal_url: Option<String> },  // 302 Location ≠ 探测端点
    Offline,                      // 超时/连接失败
}
```

- 探测端点列表（设置中可多选，任一成功即 Online，增强误判容错）：
  - `http://www.gstatic.com/generate_204`（Google）
  - `http://connect.rom.miui.com/generate_204`（小米，国内可达）
  - `http://wifi.vivo.com.cn/generate_204`（vivo，国内可达）
  - `http://www.msftconnecttest.com/connecttest.txt`（微软，期望内容 `Microsoft Connect Test`）
- 默认：`connect.rom.miui.com` + `msftconnecttest` 双端点（国内校园网环境更稳）。
- 判断 302 是否为门户：`Location` 的 host ≠ 探测端点 host 即视为门户；记录完整 URL 供 PortalAuth 使用。

### 5.3 SsidWatcher（SSID 模块）

- Windows：`windows` crate → `WlanOpenHandle` / `WlanEnumInterfaces` / `WlanQueryInterface(wlan_intf_opcode_current_connection)` 取 SSID 字节串转 UTF-8（处理中文 SSID 的编码回退）。
- 结果缓存 2-5s，避免每轮检测都调用；同时监听 `WLAN_NOTIFICATION`（可选）在网络变化时提前触发一轮检测。
- 仅 Windows 平台，直接调用 Windows API，不做平台抽象。

### 5.4 PortalAuth（认证提交引擎）

**认证网页由用户指定**：设置中必填 `login_url`。软件对该 URL 按用户配置的提交模板发请求，302 探测发现的 URL 仅作为"一键填充"提示，不自动覆盖用户配置。

`PortalProfile` 结构：

```jsonc
{
  "login_url": "http://10.10.10.10:8080/login",  // 用户指定的认证网页，必填
  "method": "POST",
  "fields": {                        // 占位符: {username} {password} {portal_url} {ip}...
    "username": "{username}",
    "password": "{password}"
  },
  "extra_headers": {},
  "body_type": "form",               // form | json
  "success": [
    { "type": "status", "op": "eq", "value": 200 },
    { "type": "body_contains", "value": "login_ok" },
    { "type": "reprobe" }             // 登录后重新探测 204
  ]
}
```

**不做内置厂商预设**：各校认证系统协议差异大（有的还需 challenge-response 摘要算法），统一用"用户自己指定 URL + 配置字段"的通用模板覆盖；遇到特殊系统（如深澜的摘要算法）时通过模板的高级能力（见下）或后续迭代按需支持。

- 模板编辑：设置窗口提供表单（URL、方法、字段键值、成功条件），并支持"从 302 URL 一键填充认证页地址"。
- 高级能力（按需迭代）：支持"登录前先 GET 一个接口"的前置步骤（应对需要先取 token 的系统）、响应体正则提取占位符。
- 默认模板：预填一份最常见的 `POST form: username/password` 模板，用户只需改成自己学校实际字段名（用浏览器 F12 → Network 看一次表单提交即可确定）。
- 安全：请求发出前日志中脱敏密码；`body` 记录截断。

### 5.5 ConfigStore / KeyringStore

- `config.json`：间隔、探测端点、SSID 白名单、PortalProfile（不含密码）、重试参数、通知开关、自启动。
- 密码：`keyring` crate，service = 应用名，account = 用户名；config 中只存"账号"与"密码已设置"标志。
- 提供"导出/导入配置"（不含密码）方便换机。

### 5.6 设置窗口前端

- 方案：**无框架**，Vite + TypeScript + 手写组件，样式用一份小 CSS（或内置 naive 的小型 CSS 方案）。产物小、加载快、无 npm 重依赖。
- 前端只做：读/写配置（`invoke`）、测试登录按钮、日志展示（`listen` 事件流）。
- 所有业务逻辑（探测、登录、重试）都在 Rust；前端是纯视图。

### 5.7 日志

- 内存环形缓冲（`VecDeque` 上限 500 条）+ 可选写文件 `%APPDATA%/<app>/logs/app.log`（按天滚动，保留 7 天）。
- 事件通过 Tauri `emit` 推给日志窗口实时刷新。

---

## 6. 配置文件示例（config.json）

```jsonc
{
  "interval_secs": 60,                // 检测间隔
  "probe_timeout_secs": 5,
  "probe_endpoints": [
    "http://connect.rom.miui.com/generate_204"
  ],
  "ssids": ["CampusNet", "CampusNet-5G"],
  "portal": {
    "login_url": "http://10.10.10.10:8080/login",
    "method": "POST",
    "fields": { "username": "{username}", "password": "{password}" },
    "body_type": "form",
    "success": [{ "type": "reprobe" }]
  },
  "username": "20230101",
  "retry": { "max_attempts": 5, "backoff_base_secs": 10, "backoff_cap_secs": 300 },
  "notify": true,
  "autostart": false
  // 密码不在此文件，位于 Windows 凭据管理器
}
```

---

## 7. 项目目录结构

```
compus-wifi/
├── .github/workflows/
│   ├── ci.yml                  # PR/push 检查：fmt + clippy + test
│   ├── release.yml             # tag 触发：自动读版本构建发布
│   └── version-bump.yml        # 手动触发：升版本 → 提交 → 打 tag
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json         # 版本号唯一来源 (version 字段)
│   ├── capabilities/default.json
│   ├── icons/
│   └── src/
│       ├── main.rs / lib.rs
│       ├── state.rs            # 状态机
│       ├── monitor.rs          # 调度循环
│       ├── services/
│       │   ├── probe.rs
│       │   ├── ssid.rs
│       │   ├── portal.rs           # 认证提交模板引擎（用户指定 URL + 字段模板）
│       │   ├── config.rs
│       │   └── keyring.rs
│       ├── tray.rs
│       ├── logging.rs
│       └── commands.rs         # #[tauri::command] 供前端调用
├── src/                        # 前端 (Vite + TS，无框架)
│   ├── index.html
│   ├── main.ts
│   └── style.css
├── package.json
└── PLAN.md / README.md / LICENSE
```

---

## 8. GitHub Actions 设计

### 8.1 版本号管理策略

**唯一来源：`src-tauri/tauri.conf.json` 的 `version` 字段。**

- 升版本方式（推荐 A，二选一，可并存）：
  - **A. 手动触发工作流**（version-bump.yml）：填入新版本号（如 `0.2.0`），脚本自动同步写入 `tauri.conf.json`、`Cargo.toml`、`package.json`，提交 `chore: bump to vX.Y.Z`，创建并推送 tag `vX.Y.Z` → 触发 release.yml。
  - B. 本地运行小脚本 `pnpm bump 0.2.0` 同样效果，push tag 触发发布。
- 不用 git tag 手打，避免 tag 与 conf.json 版本不一致。

### 8.2 ci.yml（质量门禁）

```yaml
name: CI
on: [push, pull_request]
# 仅 windows-latest（Windows 专用项目）
# 步骤: checkout → rust-toolchain(fmt/clippy/test) → pnpm install → pnpm build (前端类型检查)
```

- `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`、前端 `tsc --noEmit`。
- 用 `Swatinem/rust-cache` 加速。

### 8.3 release.yml（自动读版本构建发布）

```yaml
name: Release
on:
  push:
    tags: ['v*']
jobs:
  build:
    permissions:
      contents: write
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4        # 按 package.json 的 packageManager
      - uses: actions/setup-node@v4       # node + pnpm install
      - run: pnpm install --frozen-lockfile
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          # tauri-action 自动读取 tauri.conf.json 的 version，
          # 以 __VERSION__ 占位符拼 tag / release 名
          tagName: v__VERSION__
          releaseName: 'CampusNet Reconnector v__VERSION__'
          releaseDraft: false            # 或 true 人工审核后发布（可配置）
          prerelease: false
          # 自动产出: Windows .msi + .exe(NSIS) 安装包，并附 sha256
```

- `tauri-action` 内部逻辑就是读 `tauri.conf.json` 的 `version` 替换 `__VERSION__` 并创建 GitHub Release —— 满足"自动读取版本号进行构建发布"。
- 若后续开启自动更新，再补 `includeUpdaterJson: true` 与签名密钥（`TAURI_SIGNING_PRIVATE_KEY`）。
- 可选：`releaseDraft: true` 起步（先人工确认再 Publish，避免误发），稳定后改 false。**待确认**。

### 8.4 version-bump.yml（一键升版本）

```yaml
on:
  workflow_dispatch:
    inputs:
      version: { description: 'e.g. 0.2.0', required: true }
# 步骤: 校验 semver 格式 → 用脚本改 tauri.conf.json / Cargo.toml / package.json
#      → cargo check 确认 Cargo.lock 更新 → commit → tag vX.Y.Z → push (触发 release)
```

---

## 9. 安全与隐私考量

1. 密码只进 Windows 凭据管理器；配置文件、日志、错误信息中不出现明文密码（日志统一脱敏）。
2. 认证门户通常为 HTTP 明文传输——密码在校内网内可被嗅探，这是校园网固有情况，README 中明确告知；不做自研加密伪装。
3. Tauri capabilities 最小化：设置窗口只开放必要的 `invoke` 权限；不开 shell-open 等。
4. 自动更新（如启用）强制校验签名。

---

## 10. 性能与内存优化清单

| 措施 | 说明 |
|---|---|
| 无窗口启动 | `app.windows: []`，仅托盘 |
| 窗口懒建 + 关闭即销毁 | WebView2 内存仅在设置页打开时占用 |
| HTTP 连接池复用 | reqwest Client 全局单例 |
| 检测去重 | 上一轮未结束跳过本轮，防堆积 |
| SSID 查询缓存 | 2-5s 内复用结果 |
| 依赖瘦身 | rustls 代替 native-tls；不引入重量级前端框架 |
| 日志环形缓冲 | 不无限增长 |

预估：常驻 ~15-25MB（Rust 主进程 + 托盘）；打开设置时 ~60-90MB（WebView2），关闭后释放。安装包（NSIS exe）预计 3-6MB。

---

## 11. 开发里程碑

| 阶段 | 内容 | 产出 |
|---|---|---|
| **M0 骨架** | Tauri v2 工程初始化（无窗口 + 托盘 + 配置读写 + CI） | 可运行空壳，GitHub 仓库 + CI 绿 |
| **M1 核心** | NetProbe + SsidWatcher + 状态机 + 调度循环 + 托盘状态图标 | 检测可用（含单测） |
| **M2 认证** | PortalAuth 提交引擎（用户指定 URL + 字段模板）+ 重试退避 | 真实环境自动重连跑通 |
| **M3 UI** | 设置窗口（懒建）+ 日志窗口 + 凭据管理 + 开机自启 | 完整可用 v0.1.0 |
| **M4 发布** | release.yml + version-bump.yml 跑通首次 Release | GitHub Release 带安装包 |
| **M5 增强** | 自动重连 SSID / 前置请求步骤 / 更新器 / 唤醒感知 | 后续迭代 |

每个 M 内先写 Rust 单元测试（探测解析、模板渲染、状态机转移用固定响应 fixture）。

---

## 12. 风险与待确认问题

| # | 问题 | 影响 | 默认方案 |
|---|---|---|---|
| 1 | 认证页的提交字段名需要用户自查一次（F12 → Network 看表单提交） | 决定模板默认值 | 软件预填最常见 `username/password` POST 表单模板，用户对照修改 |
| 2 | 认证页是否为 HTTPS？ | 影响 reqwest 证书处理（自签名证书需跳过验证开关，默认不跳） | 默认严格校验，提供高级选项 |
| 3 | 需要支持多 SSID 绑定不同账号吗？ | M1 可不做，结构上预留 | 单账号起步 |
| 4 | Release 先用草稿（draft）人工发布，还是直接自动发布？ | 发布流程 | 建议 draft 起步 |
| 5 | 前端是否接受"无框架手写 TS"？ | 开发体验 vs 轻量 | 接受（产物最小）；如想更顺手可换 Svelte |
| 6 | 检测间隔默认值 | 对认证页的压力 | 60s，可 10s-1h 调节 |
| 7 | 仓库名 | GitHub 仓库命名 | 建议 `campus-net-reconnector` 或你指定 |

---

## 13. 首版交付物清单（验收标准）

- [ ] 托盘常驻，图标状态正确反映四类状态
- [ ] 指定 SSID 下断网（被踢下线）后 ≤ 检测间隔+重试时间 内自动重新登录成功
- [ ] 非指定 SSID 下不做任何登录动作
- [ ] 设置修改即时生效；密码不出现在任何文件/日志
- [ ] 连续失败有退避与上限，不轰击门户
- [ ] push tag 后 GitHub 自动出 Release（msi + nsis exe），版本号与 `tauri.conf.json` 一致
- [ ] CI：fmt/clippy/test 全绿
- [ ] 常驻内存 ≤ 30MB（任务管理器实测记录进 README）
