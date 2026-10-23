<p align="center">
  <img src="design/logo/out/app-icon-256.png" width="88" height="88" alt="到点">
</p>

<h1 align="center">到点</h1>

<p align="center">
  临时的事用待办，分阶段的用计划。<br>
  给每一件事一个时间点，到点提醒一次。
</p>

---

## 它解决什么

多数待办工具只有一个「截止日期」。可真实的事情分成两类：

- **临时的事**——领导丢过来一句、群里 @ 你一下。只有一件事、一个时间点，处理完就完了。
- **分阶段的事**——一个版本要提测、发布、观察；一次改版要出方案、试跑、定稿。它是一件事，但有好几个「到点」，每个都得提醒。

到点把这两类放在同一个窗口里：**一件事一个时间点，或者一件事几个阶段、几个时间点**。到点弹系统通知，窗口关了也在。

## 长什么样

<table>
<tr>
<td width="50%"><img src="design/out/celadon/01-today.png" alt="待办 · 今天"><br><sub>待办 · 今天（青瓷）</sub></td>
<td width="50%"><img src="design/out/night/03-gantt.png" alt="计划 · 时间轴"><br><sub>计划 · 时间轴（夜航）</sub></td>
</tr>
</table>

## 功能

**待办**　顶部一行记下，时间点四选一（不设时间 / 今天 15:00 / 明天 09:00 / 自选）。分「今天 / 以后 / 逾期 / 已完成」四栏，逾期和快到点的行左边有颜色条。点开是详细编辑：时间、重要程度、备注。

**计划**　一件事拆成最多 20 个阶段，每个阶段一个时间点。卡片上是一条阶段进度线，下一个到点的阶段单独标出来；也可以切成时间轴视图，把几份计划的阶段排在同一条日历线上，谁先到点一眼看出来。

**提醒**　到点弹系统通知，同时窗口里出现一张「珠子落盘」卡片——标志上那颗珠从盘沿落到盘心。稍后不用点按钮：把珠子往右拖，落在 10 分钟 / 30 分钟 / 1 小时哪个刻度，就延后到哪。

<img src="design/notify/out/card.png" width="420" alt="提醒卡片">

**窗口与托盘**　关窗口不退出，缩进托盘继续跑；托盘菜单「退出」才真的关。有到点没做完的事，托盘图标会换成「珠已落盘」的样子。

**两套主题**　青瓷（浅）/ 夜航（深），左下角切换，记在本机。颜色全部走 CSS 变量，版式只有一套。

## 快捷键

| 键 | 作用 |
| --- | --- |
| `Ctrl N` | 新建当前板块的条目 |
| `Ctrl K` | 跳到搜索框（标题、备注、阶段名） |
| `Esc` | 关掉编辑抽屉 |
| `Ctrl S` | 在编辑抽屉里保存 |

## 提醒的规则

提醒是为了不漏，不是为了多响。

- **同一条只响一次**；阶段里错过的那些只记账，不逐条补发。
- **免打扰 22:00–08:00**：到点照算，但不弹；第二天 8 点合并成一条。
- **每小时最多 3 条**，超出的先攒着，整点后合并。
- **同一批合并**：「3 件到点 · 提测评审、交周报 等 3 件」，不连着弹三条。
- **发不出去会退避**：同一条最多试 3 次，之后只记账，不会再每 20 秒刷一次通知中心。

Windows 上通知走自己注册的 AppUserModelID，所以开发态（还没建开始菜单快捷方式）也能弹出来。

## 技术栈

| | |
| --- | --- |
| 桌面壳 | Tauri 2（Rust） |
| 界面 | Vue 3 + TypeScript + Vite，没有组件库，组件和样式都是自己的 |
| 存储 | 本机 JSON，无网络请求 |
| 通知 | Windows：WinRT toast + 自注册 AUMID；其他平台：`tauri-plugin-notification` |

```
Vue 3 (src/)
  AppShell ─ 待办 / 计划 / 主题
     │ invoke                        ▲ 事件 data-changed
     ▼                               │
Tauri 命令 (store/)  ──────────►  store.json
     ▲
     │
remind.rs  线程：算下一个到点 → 睡到那时 → 检查 → 发通知 → 记账
     ├─ win_toast.rs                Windows：WinRT toast
     └─ tauri-plugin-notification    macOS / Linux
```

## 目录

```
.
├─ src/                     前端
│  ├─ features/shell/       外壳、主题、提醒卡片、快捷键
│  ├─ features/todos/       待办页、行、编辑器
│  ├─ features/plans/       计划页、卡片、时间轴、编辑器
│  ├─ domain/               类型、时间、排序、展示格式
│  ├─ data/ · api/          读写封装（invoke 到 Rust）
│  ├─ ui/                   Icon、Mark（标志）、TimeField
│  └─ styles.css            设计令牌 + 两套主题的全部样式
├─ src-tauri/
│  ├─ src/domain/           模型、调度（哪条该响）、旧数据迁移
│  ├─ src/store/            JSON 读写 + 命令
│  ├─ src/remind.rs         提醒线程（调度 / 合并 / 免打扰 / 记账）
│  ├─ src/win_toast.rs      Windows 通知
│  ├─ src/tray.rs           托盘、关窗隐藏
│  └─ icons/                应用图标、托盘角标
├─ design/                  设计资料（见下）
└─ public/favicon.svg
```

## 开发

```bash
pnpm install
pnpm tauri dev        # 桌面窗口，带热更新
pnpm build            # 只构建前端（vue-tsc + vite）
pnpm tauri build      # 打包安装包
```

Rust 侧单独检查：

```bash
cd src-tauri
cargo check --tests
```

## 数据

Windows：`%APPDATA%\com.daodian.app\store.json`

- 所有记录都在这个文件里，不上传。
- 更早版本的 `todos.json` 会在第一次打开时自动导入，原文件保留。
- 备份或换机器，拷这一个文件就行。

## 日志

启动、窗口创建、数据文件、托盘、提醒线程和崩溃信息会写入 `logs\daodian.log`。

- 优先放在 `daodian.exe` 旁边；安装目录不可写时退到 `%LOCALAPPDATA%\daodian\logs`。
- 单个日志到 2 MB 后轮换为 `daodian.log.1`，最多保留当前和上一份。
- 排查启动失败时，先查看最后几行 `ERROR`。

## 设计资料

`design/` 里是这套界面的源文件和说明：

- [design/README.md](design/README.md) —— 两套主题的设计说明、色板、组件
- [design/logo/README.md](design/logo/README.md) —— 标志的概念、构造、尺寸与用法
- [design/notify/方案.md](design/notify/方案.md) —— 提醒方案：系统通知、珠子落盘、打扰预算

原型是可点的静态 HTML，图在各自的 `out/` 下。

## 已知限制

- **Windows 通知暂时没有「完成 / 稍后」按钮**，点通知也不会跳回应用；要处理得打开窗口。
- 免打扰时段和每小时上限写死在 [remind.rs](src-tauri/src/remind.rs) 顶部，还没有设置界面。
- 只在 Windows 上验证过；macOS / Linux 走官方通知插件，没有实测。
- 不做云同步——设计上就是单机、本地、可备份。
