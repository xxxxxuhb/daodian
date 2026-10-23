import { writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { shell, rail, icon, todayMain, todayAside, plansMain, plansAside } from "./lib.mjs";

const DIR = dirname(fileURLToPath(import.meta.url));
const THEMES = ["celadon", "night"];
const w = (folder, name, html) => {
  mkdirSync(join(DIR, folder), { recursive: true });
  writeFileSync(join(DIR, folder, name), html);
  console.log("wrote", folder + "/" + name);
};
const page = (theme, file, title, body, extraCss) =>
  w(theme, file, shell({ title, theme, css: "../theme.css", body, extraCss }));

/* ---------------- 时间轴视图 ---------------- */
const days = [["10/23", "四", true], ["10/24", "五"], ["10/25", "六"], ["10/26", "日"], ["10/27", "一"], ["10/28", "二"], ["10/29", "三"]];
const head = days.map(d => '<div class="' + (d[2] ? "today" : "") + '">' + d[0] + " " + d[1] + "</div>").join("");

const ganttRow = (name, meta, from, to, pins) =>
  '<div class="gantt__row">' +
    '<div class="gantt__label">' + name + "<span>" + meta + "</span></div>" +
    '<div class="gantt__track"><span class="gantt__seg" style="left:' + from + "%;width:" + (to - from) + '%"></span>' +
      pins + "</div>" +
  "</div>";

const pin = (pos, label, cls) =>
  '<span class="pin ' + cls + '" style="left:' + pos + '%"><span>' + label + "</span><i></i></span>";

const gantt =
  '<div class="gantt">' +
    '<div class="gantt__head"><div>计划 / 10月</div>' + head + "</div>" +
    '<div class="gantt__body">' +
      '<span class="gantt__now"></span>' +
      ganttRow("1.2 版本", "4 个阶段 · 1 完成", 6, 20.3,
        pin(6, "提测", "next") + pin(11.9, "发布", "future") + pin(20.3, "观察", "future")) +
      ganttRow("灰度开关上线", "3 个阶段 · 1 完成", 22.6, 83.3,
        pin(22.6, "方案评审", "done") + pin(48.9, "灰度 5%", "future") + pin(83.3, "全量", "future")) +
      ganttRow("团队周会改版", "3 个阶段", 34.6, 91.7,
        pin(34.6, "议程", "future") + pin(62.9, "试跑", "future") + pin(91.7, "定稿", "future")) +
      ganttRow("数据看板 V2", "2 个阶段", 51.1, 97.6,
        pin(51.1, "口径确认", "future") + pin(97.6, "上线", "future")) +
      ganttRow("值班表改版", "2 个阶段 · 1 完成", 36.9, 77.4,
        pin(36.9, "排班规则", "done") + pin(77.4, "上线", "future")) +
      ganttRow("搜索埋点", "3 个阶段", 20.3, 91.7,
        pin(20.3, "埋点方案", "future") + pin(65.4, "接数", "future") + pin(91.7, "看板", "future")) +
    "</div>" +
  "</div>" +
  '<div class="legend">' +
    '<span><i style="background:var(--done)"></i>已完成</span>' +
    '<span><i style="background:var(--accent)"></i>下一个到点</span>' +
    '<span><i style="background:var(--fill-2)"></i>还没到</span>' +
    '<span style="margin-left:auto;color:var(--ink-4)">拖动节点可以直接改时间</span>' +
  "</div>";

const ganttMain = `
<main class="main">
  <header class="topbar">
    <div>
      <p class="eyebrow">计划</p>
      <h1>时间轴</h1>
      <p class="sub">七个计划排在同一条线上，谁先到点一眼看得出来</p>
    </div>
    <div class="topbar__right">
      <div class="date"><b>10月23日</b>周四 · 09:23</div>
      <div class="seg"><span>卡片</span><span class="on">${icon("i-timeline", "ico s")}时间轴</span></div>
      <button class="btn btn--primary">${icon("i-plus", "ico s")}新建</button>
    </div>
  </header>
  <div class="toolbar">
    <span class="chip on">本周</span>
    <span class="chip">本月</span>
    <span class="grow"></span>
    <span class="chip">全部计划</span>
    <span class="chip">只看重要</span>
  </div>
  ${gantt}
  <p class="foot-note">每个点都是一个阶段时间；细线是计划从第一个阶段到最后一个阶段的跨度。</p>
</main>
`;

const ganttAside = `
<aside class="aside">
  <div class="aside__head">
    <p class="eyebrow">本周节点</p>
    <div class="big"><b>7</b><span>个到点</span></div>
    <div class="bar"><i style="width:43%;background:var(--accent)"></i></div>
  </div>
  <div class="tl">
    <div class="tl__item is-key"><i class="tl__t">10:00</i><span class="tl__dot"></span><span><span class="nm">提测</span><span class="meta">1.2 版本 · 今天</span></span></div>
    <div class="tl__item"><i class="tl__t">20:00</i><span class="tl__dot"></span><span><span class="nm">发布</span><span class="meta">1.2 版本 · 今天</span></span></div>
    <div class="tl__item"><i class="tl__t">14:00</i><span class="tl__dot"></span><span><span class="nm">方案评审</span><span class="meta">灰度开关 · 明天</span></span></div>
    <div class="tl__item"><i class="tl__t">10:00</i><span class="tl__dot"></span><span><span class="nm">议程</span><span class="meta">周会改版 · 周六</span></span></div>
    <div class="tl__item"><i class="tl__t">10:00</i><span class="tl__dot"></span><span><span class="nm">灰度 5%</span><span class="meta">灰度开关 · 周日</span></span></div>
    <div class="tl__item"><i class="tl__t">10:00</i><span class="tl__dot"></span><span><span class="nm">试跑</span><span class="meta">周会改版 · 下周一</span></span></div>
    <div class="tl__item"><i class="tl__t">20:00</i><span class="tl__dot"></span><span><span class="nm">全量</span><span class="meta">灰度开关 · 下周二</span></span></div>
  </div>
  <div class="next-card" style="margin-top:auto">
    <p class="eyebrow">最近一个</p>
    <h4>提测 · 1.2 版本</h4>
    <p>今天 10:00</p>
    <div class="cd"><b>0:37</b><span>后到点</span></div>
  </div>
</aside>
`;

/* ---------------- 编辑计划抽屉 ---------------- */
const stage = (o) =>
  '<div class="stage' + (o.focus ? " is-focus" : "") + '">' +
    '<div class="stage__idx">' + o.idx + "</div>" +
    "<div>" +
      '<div class="stage__name">' + o.name + (o.focus ? '<span class="caret"></span>' : "") +
        (o.done ? '<span class="pill pill--moss">已完成</span>' : "") + "</div>" +
      '<div class="stage__time">' + icon("i-cal", "ico s") + o.time + "</div>" +
    "</div>" +
    '<div class="stage__grip">' + icon("i-grip", "ico s") + "</div>" +
  "</div>";

const drawer = `
<div class="app is-dim">${rail("plan-active")}${plansMain()}${plansAside()}</div>
<div class="scrim"></div>
<div class="drawer">
  <div class="drawer__head">
    <div><p class="eyebrow">计划 · 编辑</p><div class="drawer__title">1.2 版本</div></div>
    <button class="icon-btn">${icon("i-x", "ico s")}</button>
  </div>
  <div class="drawer__body">
    <div class="field">
      <div class="field__label"><span>计划名称</span><span>Ctrl S 保存</span></div>
      <div class="input input--big is-focus">1.2 版本<span class="caret"></span></div>
    </div>
    <div class="field">
      <div class="field__label"><span>重要程度</span></div>
      <div class="seg"><span class="on">重要</span><span>普通</span><span>可缓</span></div>
    </div>
    <div class="field">
      <div class="field__label"><span>阶段</span><span>4 个 · 到点分别提醒</span></div>
      <div class="field__hint" style="margin:-2px 0 10px">每个阶段一个时间点。发布往后挪，观察跟着挪。</div>
      ${stage({ idx: "01", name: "需求确认", time: "10月21日 周二 18:00", done: true })}
      ${stage({ idx: "02", name: "提测", time: "今天 10:00", focus: true })}
      ${stage({ idx: "03", name: "发布", time: "今天 20:00" })}
      ${stage({ idx: "04", name: "观察", time: "明天 10:00" })}
      <button class="btn btn--dash" style="width:100%;justify-content:center;margin-top:6px">${icon("i-plus", "ico s")}再加一个阶段</button>
    </div>
    <div class="field">
      <div class="field__label"><span>备注</span><span>0 / 500</span></div>
      <div class="input textarea">灰度先到 30%，重点看支付链路。有问题先回滚，别硬扛。</div>
    </div>
  </div>
  <div class="drawer__foot">
    <button class="btn btn--primary">保存</button>
    <button class="btn">整份标为完成</button>
    <button class="btn btn--ghost" style="margin-left:auto;color:var(--danger)">删除</button>
  </div>
</div>
`;

/* ---------------- 到点提醒 ---------------- */
const reminder = `
<div class="app is-dim no-aside">${rail("todo-today")}${todayMain()}</div>
<div class="scrim" style="background:var(--scrim)"></div>
<div class="toast">
  <div class="toast__top">${icon("i-bell", "ico s")}<span>到点提醒</span><time>14:00</time></div>
  <h3>提测评审</h3>
  <p>待办 · 今天 14:00 · 只提醒这一次</p>
  <div class="toast__acts">
    <button class="btn btn--sm">稍后 10 分钟</button>
    <button class="btn btn--sm btn--primary">标记完成</button>
  </div>
</div>
<div class="tray">
  <div class="tray__head"><span class="mark mark--sm"></span><b>到点</b><span>3 件未完成</span></div>
  <div class="tray__item">打开主窗口</div>
  <div class="tray__item">暂停提醒 1 小时</div>
  <div class="tray__item">下一个：提测 10:00</div>
  <div class="tray__sep"></div>
  <div class="tray__item">退出</div>
</div>
`;

/* ---------------- 输出两套主题 ---------------- */
const extra = ".toolbar{display:flex;align-items:center;gap:6px;margin:2px 0 16px}\n" +
  ".toolbar .grow{flex:1}\n.foot-note{margin-top:14px;font-size:11.5px;color:var(--ink-4)}";

for (const t of THEMES) {
  page(t, "01-today.html", "到点 · 待办 今天", '<div class="app">' + rail("todo-today") + todayMain() + todayAside() + "</div>");
  page(t, "02-plans.html", "到点 · 计划 进行中", '<div class="app">' + rail("plan-active") + plansMain() + plansAside() + "</div>");
  page(t, "03-gantt.html", "到点 · 计划 时间轴", '<div class="app">' + rail("plan-active") + ganttMain + ganttAside + "</div>", extra);
  page(t, "04-plan-drawer.html", "到点 · 编辑计划", drawer);
  page(t, "05-reminder.html", "到点 · 提醒", reminder);
}

/* ---------------- 两套主题对照 ---------------- */
const swatch = (n, hex) =>
  '<div class="sw"><i style="background:' + hex + '"></i><b>' + n + "</b><small>" + hex + "</small></div>";

const panel = (theme, name, sub, tag, a, b, note) => `
<section class="panel" data-theme="${theme}">
  <div class="panel__head"><h2>${name}</h2><span>${sub}</span><em>${tag}</em></div>
  <div class="sw-row">${a.map(function (s) { return swatch(s[0], s[1]); }).join("")}</div>
  <div class="sw-row" style="margin-top:8px">${b.map(function (s) { return swatch(s[0], s[1]); }).join("")}</div>
  <div class="comp-row">
    <button class="btn btn--primary">记下</button>
    <button class="btn">详细写</button>
    <span class="pill pill--red">逾期 53 分钟</span>
    <span class="pill pill--amber">1 小时后</span>
    <span class="pill pill--moss">已完成</span>
  </div>
  <div class="comp-row">
    <span class="chip on">今天 15:00</span>
    <span class="chip">明天 09:00</span>
    <span class="check"></span>
    <span class="check is-done">${icon("i-check", "ico s")}</span>
    <div class="seg"><span class="on">重要</span><span>普通</span><span>可缓</span></div>
  </div>
  <div class="comp-row">
    <div class="input is-focus" style="width:100%;height:38px;font-size:13px">提测评审<span class="caret"></span></div>
  </div>
  <p class="note">${note}</p>
</section>
`;

const celadonA = [["底", "#EBF0ED"], ["卡片", "#FAFCFB"], ["栏", "#0C1F1A"], ["墨", "#0E1613"], ["次墨", "#47564F"], ["主色", "#15637B"]];
const celadonB = [["临近", "#9C640E"], ["逾期", "#A33C3C"], ["完成", "#2C7A58"], ["线", "#D3DFD9"], ["填充", "#E1E8E4"], ["提醒卡", "#0C1F1A"]];
const nightA = [["底", "#0E1116"], ["卡片", "#161A21"], ["栏", "#080A0E"], ["墨", "#E7ECF2"], ["次墨", "#A7B2BF"], ["主色", "#5B8DF6"]];
const nightB = [["临近", "#E0A24C"], ["逾期", "#E9646B"], ["完成", "#45B98F"], ["线", "#252C36"], ["填充", "#1E242C"], ["提醒卡", "#1B2434"]];

const spec = `
<div class="board">
  <div class="board__head">
    <div>
      <p class="eyebrow">到点 · DAODIAN</p>
      <h1 style="margin-top:8px">两套主题</h1>
    </div>
    <p>同一套版式与组件，只换令牌<br>浅色「青瓷」· 深色「夜航」· 1400 × 880</p>
  </div>
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:18px">
    ${panel("celadon", "青瓷", "浅色 · 白天使用", "LIGHT", celadonA, celadonB,
      "<b>青瓷</b>用青瓷釉的冷灰绿做底，主色压到深靛青。状态色只用三档：<b>临近</b>琥珀、<b>逾期</b>砖红、<b>完成</b>苔绿。")}
    ${panel("night", "夜航", "深色 · 夜间与常驻", "DARK", nightA, nightB,
      "<b>夜航</b>用近黑的蓝灰分层，主色提亮到雾蓝，状态色同步提亮一档，暗底上仍然分得清。")}
  </div>
  <p class="note" style="margin-top:20px;text-align:center">
    版式、字号、间距、圆角、动效完全一致；切换主题只改 <b>背景 / 卡片 / 文字 / 主色 / 三档状态色</b> 这几组变量。
  </p>
</div>
`;

writeFileSync(join(DIR, "spec.html"), shell({
  title: "到点 · 两套主题",
  theme: "night",
  css: "theme.css",
  body: spec,
  extraCss: "body{background:#08090C}\n.board__head{border-color:#1B2027}\n.board__head p{color:#5A636E}\n.eyebrow{color:#5A636E}",
}));
console.log("wrote spec.html");
