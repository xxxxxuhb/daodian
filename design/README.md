# 到点 · 界面设计与两套主题

桌面待办 / 计划（Tauri 2 + Vue 3 + Naive UI）。版式沿用上一版，**配色整体换掉**，并做成 token 化的两套主题：

- **青瓷**（浅色）— 白天用
- **夜航**（深色）— 夜间、常驻托盘时用

同一套结构，只换颜色变量。打开 [overview.html](overview.html) 点着看，图片在 [out/](out/)。

## 三个判断（沿用）

**时间是主角，所以给它一条线。** 「今天」做成右栏时间轴：这一刻之前的变灰划掉，之后的按时间排开，待办和计划的阶段混在同一根线上（图 01 里 20:00 的「发布」来自计划）。计划另有横向时间轴，七天内所有阶段排在一起。

**「到点」是唯一允许打断人的动作。** 横幅只给两个选项：稍后 10 分钟、标记完成，不给「打开应用」；托盘常驻写在导航栏底部。

**导航保留原有结构，但让它安静下来。** 待办五类、计划两类位置不变，纯文字 + 数字计数 + 选中胶囊；图标只留给动作和状态。

## 两套主题

| | 青瓷 LIGHT | 夜航 DARK |
| --- | --- | --- |
| 底 | `#EBF0ED` | `#0E1116` |
| 卡片 | `#FAFCFB` | `#161A21` |
| 左栏 | `#0C1F1A` | `#080A0E` |
| 主色 | `#15637B` 深靛青 | `#5B8DF6` 雾蓝 |
| 墨 / 次墨 / 浅墨 | `#0E1613` / `#47564F` / `#74847C` | `#E7ECF2` / `#A7B2BF` / `#77828E` |
| 线 / 填充 | `#D3DFD9` / `#E1E8E4` | `#252C36` / `#1E242C` |
| 临近 | `#9C640E` 琥珀 | `#E0A24C` |
| 逾期 | `#A33C3C` 砖红 | `#E9646B` |
| 完成 | `#2C7A58` 苔绿 | `#45B98F` |
| 提醒卡 | `#0C1F1A` | `#1B2434` |

**为什么青瓷的主色是靛青而不是绿。** 绿做主色时，「下一个到点」和「已完成」在时间轴图例里几乎同色。主色压成深靛青后，三档状态色（琥珀 / 砖红 / 苔绿）和一个交互色各占一个位置，暗底亮底都不打架。

**深色不是把浅色反相。** 深色里底色和卡片只差一档明度，靠 1px 线分层次；主色提亮到雾蓝，状态色同时提亮一档；阴影加重、高光减弱（`inset 0 1px 0 rgba(255,255,255,.04)`）。

## 字号与形状

展示 30/600，小节 12/600 加 0.05em 字距，正文 14.5/600，辅助 12.5，微标 10.5 加 0.2em 字距；时间与数字统一 Georgia 衬线 + 表格数字。圆角 14（卡片）/ 10（输入）/ 999（胶囊）；阴影两级，抽屉用 `-30px 0 70px`。行状态用左侧 3px 竖条（红=逾期、琥珀=1 小时内），不做整块染色。

## 五屏 × 两主题

| 画面 | 青瓷 | 夜航 |
| --- | --- | --- |
| 待办 · 今天 | [01](out/celadon/01-today.png) | [01](out/night/01-today.png) |
| 计划 · 进行中 | [02](out/celadon/02-plans.png) | [02](out/night/02-plans.png) |
| 计划 · 时间轴 | [03](out/celadon/03-gantt.png) | [03](out/night/03-gantt.png) |
| 编辑计划抽屉 | [04](out/celadon/04-plan-drawer.png) | [04](out/night/04-plan-drawer.png) |
| 到点提醒 | [05](out/celadon/05-reminder.png) | [05](out/night/05-reminder.png) |

令牌对照看 [out/spec.png](out/spec.png)。

## 状态清单

逾期（红条 + 红时间）· 1 小时内（琥珀条 + 琥珀时间）· 未设时间（`—` / 未定）· 已完成（实心勾 + 标题划掉）· 空列表 · 搜索无结果 · 读取失败（顶部一条错误条）· 保存中（按钮 loading，不阻塞列表）。

## 落到代码

主题挂在 `<html data-theme="celadon|night">` 上，所有颜色走 [theme.css](theme.css) 里的 CSS 变量；切换主题只改这一层。

- `src/theme.ts` 的 `GlobalThemeOverrides.common` 直接取变量值：`primaryColor　→ var(--accent)`、`bodyColor → --bg`、`cardColor → --card`、`borderColor → --line`、`textColor1/2/3 → --ink/--ink-2/--ink-3`。
- Naive UI 的 `darkTheme` 在夜航下同时启用，避免弹层（下拉、日期面板）还留浅色底。
- 首选项存 `localStorage`，默认跟 `prefers-color-scheme`；托盘「退出」旁边可以放一个切换项。
- 列表行对应 `TodoPage.vue` 的 `.item`，计划卡对应 `PlanPage.vue`，`todoTone` 的三态正好映射红 / 琥珀 / 中性。

## 重新生成

```bash
cd design
node build.mjs        # 生成 celadon/ 与 night/ 各 5 个页面 + spec.html
node render.mjs celadon/01-today.html night/01-today.html ... spec.html overview.html
```

`render.mjs` 用本机 Chrome / Edge 无头截图，2 倍像素密度。原型是静态 HTML，交互只用状态和视觉表达。

旧版（米色 + 柿子红）的 8 张图归档在 [out/_v1-warm/](out/_v1-warm/)。
