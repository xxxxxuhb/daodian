# 到点 · 标志

## 概念

**表盘 + 指针 + 珠。** 圆是表盘，斜线是走到 2 点方向的指针，指针尽头那颗实心珠就是「到点」——时间走到的那一格。三笔构成，没有多余装饰，缩小到托盘 16px 还剩一个能认的轮廓。

珠用主色（青瓷 `#15637B`），表盘与指针用墨色，两色只在这一点上分开：颜色落在「到点」那一处。

## 文件

| 文件 | 用途 |
| --- | --- |
| `mark.svg` | 单色标记，`currentColor`，随文字颜色走 |
| `mark-accent.svg` | 双色标记（墨 + 主色珠） |
| `mark-small.svg` | 简化版，用于 ≤ 28px：描边加粗、珠放大 |
| `lockup-horizontal.svg` | 横向组合（标记 + 到点 + DAODIAN） |
| `lockup-vertical.svg` | 纵向组合，用于启动画面 |
| `app-icon.svg` | 应用图标，64 网格，圆角 22.5% |
| `out/app-icon-*.png` | 1024 / 512 / 256 / 128 / 64 / 32 / 16，透明底 |
| `out/logo-sheet.png` | 规范图 |

## 尺寸与留白

- 标记最小 **20px**（界面上），托盘 16px 用 `mark-small`。
- 四周留白 ≥ 珠直径的 1.5 倍；标记与中文间距 ≈ 半个珠。
- 横向组合中，标记的**外径**等于中文字面高；不要按外框去对齐，会显得标记偏小。

## 颜色

| 场景 | 表盘 / 指针 | 珠 |
| --- | --- | --- |
| 浅色 | `#0E1613` | `#15637B` |
| 深墨底 | `#EAF3EE` | `#7EC3B0` |
| 深色主题 | `#E7ECF2` | `#5B8DF6` |
| 反白 | `#FAFCFB` | `#FAFCFB` |

## 不要

- 不要把珠放大到盖住指针；不要给圆加描边或投影；不要改成渐变。
- 不要在小于 20px 时用完整版（珠会糊在一起）。
- 字标不要换字体：中文宋体 / Songti SC（字距 0.06em），英文系统无衬线（字距 0.42em）。

## 已替换到项目

- `src/ui/Mark.vue`：左上角标志，描边走 `currentColor`，珠走 `--mark-hand`（随主题）。
- `src/styles.css`：`.mark` 只保留尺寸与配色，不再用 CSS 画表盘。
- `public/favicon.svg` + `index.html`：网页 favicon 换成应用图标。
- `src-tauri/icons/`：整套桌面图标（含 `.ico` / `.icns` / Store 图标）已由 CLI 重生成。

重生成图标：

```bash
cd design/logo && pnpm tauri icon out/app-icon-1024.png   # 会重写 src-tauri/icons
```
