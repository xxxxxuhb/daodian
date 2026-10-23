// 到点 · 原型构建公共件
export const icon = (n, cls = "ico") => '<svg class="' + cls + '"><use href="#' + n + '"></use></svg>';

export const markSvg = (size = 38) =>
  '<svg class="mark" width="' + size + '" height="' + size + '" viewBox="0 0 64 64" fill="none" aria-hidden="true">' +
  '<circle cx="32" cy="32" r="18.4" stroke="currentColor" stroke-width="4.8"></circle>' +
  '<path d="M32 32 40.5 23.5" stroke="currentColor" stroke-width="4.6" stroke-linecap="round"></path>' +
  '<circle class="bead" cx="43.7" cy="20.3" r="6.8"></circle>' +
  "</svg>";

export const sprite = `
<svg width="0" height="0" style="position:absolute" aria-hidden="true">
  <symbol id="i-clock" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.4"/><path d="M12 7.6v4.9l3.3 1.9"/></symbol>
  <symbol id="i-plus" viewBox="0 0 24 24"><path d="M12 5.5v13M5.5 12h13"/></symbol>
  <symbol id="i-search" viewBox="0 0 24 24"><circle cx="10.8" cy="10.8" r="6.2"/><path d="M15.4 15.4 20 20"/></symbol>
  <symbol id="i-bell" viewBox="0 0 24 24"><path d="M6.6 9.6a5.4 5.4 0 0 1 10.8 0c0 3.6 1.3 4.9 1.3 4.9H5.3s1.3-1.3 1.3-4.9Z"/><path d="M10 18.3a2.1 2.1 0 0 0 4 0"/></symbol>
  <symbol id="i-check" viewBox="0 0 24 24"><path d="m5.5 12.6 4.2 4.2L18.8 7.7"/></symbol>
  <symbol id="i-x" viewBox="0 0 24 24"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/></symbol>
  <symbol id="i-cal" viewBox="0 0 24 24"><rect x="4" y="5.6" width="16" height="13.8" rx="2.4"/><path d="M4 10.2h16M8.6 3.8v3.6M15.4 3.8v3.6"/></symbol>
  <symbol id="i-list" viewBox="0 0 24 24"><path d="M8.5 7h10.5M8.5 12h10.5M8.5 17h10.5M5 7h.01M5 12h.01M5 17h.01"/></symbol>
  <symbol id="i-timeline" viewBox="0 0 24 24"><path d="M3.5 18.6h17"/><circle cx="8.4" cy="10.4" r="2.1"/><circle cx="16" cy="10.4" r="2.1"/><path d="M10.5 10.4h3.4"/></symbol>
  <symbol id="i-grip" viewBox="0 0 24 24"><path d="M9.5 7h.01M9.5 12h.01M9.5 17h.01M14.5 7h.01M14.5 12h.01M14.5 17h.01"/></symbol>
  <symbol id="i-trash" viewBox="0 0 24 24"><path d="M4.8 7h14.4M9.4 7V4.8h5.2V7M6.8 7l.9 12.2h8.6L17.2 7"/></symbol>
</svg>
`;

const navItem = (label, n, active) =>
  '<a class="nav-item' + (active ? " is-active" : "") + '"><span>' + label + '</span><i class="n">' + n + "</i></a>";

export const rail = (active, opts = {}) => {
  const query = opts.query || "";
  const foot = opts.foot || "关闭窗口只是收进托盘，<br>提醒会继续跑。";
  return '<aside class="rail">' +
    '<div class="brand">' + markSvg(38) + '<div><b>到点</b><small>临时的事用待办<br>分阶段的用计划</small></div></div>' +
    '<nav class="nav">' +
      '<div class="nav-group"><p>待办</p>' +
        navItem("今天", 5, active === "todo-today") +
        navItem("以后", 3, active === "todo-later") +
        navItem("逾期", 2, active === "todo-overdue") +
        navItem("已完成", 12, active === "todo-done") +
      "</div>" +
      '<div class="nav-group"><p>计划</p>' +
        navItem("进行中", 3, active === "plan-active") +
        navItem("已完成", 6, active === "plan-done") +
      "</div>" +
    "</nav>" +
    '<div class="rail-foot">' +
      '<div class="search' + (query ? " filled" : "") + '">' + icon("i-search", "ico s") +
        "<span>" + (query || "搜索标题、备注或阶段") + '</span><span class="q">Ctrl K</span></div>' +
      '<p class="tip">' + foot + "</p>" +
    "</div></aside>";
};

export const shell = (o) =>
  '<!doctype html>\n<html lang="zh-CN" data-theme="' + (o.theme || "celadon") + '">\n<head>\n<meta charset="utf-8">\n<title>' + o.title + "</title>\n" +
  '<link rel="stylesheet" href="' + (o.css || "theme.css") + '">\n' + (o.extraCss ? "<style>\n" + o.extraCss + "\n</style>\n" : "") +
  "</head>\n<body>\n" + sprite + "\n" + o.body + "\n</body>\n</html>\n";

const row = (o) =>
  '<article class="row' + (o.state ? " is-" + o.state : "") + (o.done ? " is-done" : "") + '">' +
    '<span class="check' + (o.done ? " is-done" : "") + '">' + (o.done ? icon("i-check", "ico s") : "") + "</span>" +
    '<div class="when"><span class="t">' + o.t + '</span><span class="d">' + o.d + "</span></div>" +
    "<div>" +
      "<h3>" + (o.pin ? '<span class="pin' + (o.pin === "low" ? " pin--low" : "") + '"></span>' : "") + o.title + "</h3>" +
      (o.note ? "<p>" + o.note + "</p>" : "") +
    "</div>" +
    '<div class="row__side">' + (o.pill || "") + "</div>" +
  "</article>";

export const todayMain = () => `
<main class="main">
  <header class="topbar">
    <div>
      <p class="eyebrow">待办</p>
      <h1>今天</h1>
      <p class="sub">临时接到、今天要办的，还有还没定时间的</p>
    </div>
    <div class="topbar__right">
      <div class="date"><b>10月23日</b>周四 · 09:23</div>
      <div class="seg"><span class="on">列表</span><span>日程</span></div>
      <button class="btn btn--primary">${icon("i-plus", "ico s")}新建</button>
    </div>
  </header>

  <div class="composer">
    <div class="composer__line">
      <span class="composer__dot"></span>
      <span class="composer__hint">记一件临时的事，按回车定个时间点</span>
      <span class="kbd">Ctrl N</span>
    </div>
    <div class="composer__row">
      <span class="chip">不设时间</span>
      <span class="chip on">今天 15:00</span>
      <span class="chip">明天 09:00</span>
      <span class="chip">${icon("i-cal", "ico s")}自选</span>
      <span class="grow"></span>
      <button class="btn btn--primary btn--sm">记下</button>
    </div>
  </div>

  <div class="group"><h2>已过时间</h2><span class="cnt">1</span><span class="rule"></span><span class="act">全部逾期 →</span></div>
  <div class="list">
    ${row({ t: "08:30", d: "今天", title: "把昨天的测试结果发群里", note: "顺手把压测结论一起附上", state: "overdue", pill: '<span class="pill pill--red">逾期 53 分钟</span>' })}
  </div>

  <div class="group"><h2>今天</h2><span class="cnt">3</span><span class="rule"></span></div>
  <div class="list">
    ${row({ t: "10:30", d: "今天", title: "站会同步进度", note: "说清楚灰度现在卡在哪儿", state: "soon", pill: '<span class="pill pill--amber">1 小时后</span>' })}
    ${row({ t: "14:00", d: "今天", title: "提测评审", note: "带上用例清单，重点讲支付链路", pin: "high", pill: '<span class="pill pill--plain">4 小时后</span>' })}
    ${row({ t: "18:00", d: "今天", title: "交周报" })}
  </div>

  <div class="group"><h2>没定时间</h2><span class="cnt">1</span><span class="rule"></span></div>
  <div class="list">
    ${row({ t: "—", d: "未定", title: "整理这周的需求变更", note: "空下来再看，不急", pin: "low" })}
  </div>

  <div class="main-foot">
    <span><b>5</b> 件没做完 · 今天还有 3 个时间点</span>
    <span class="k"><span><span class="kbd">空格</span>完成</span><span><span class="kbd">Enter</span>编辑</span><span><span class="kbd">Ctrl N</span>新建</span></span>
  </div>
</main>
`;

export const todayAside = () => `
<aside class="aside">
  <div class="aside__head">
    <p class="eyebrow">今天</p>
    <div class="big"><b>2</b><span>/ 7 件做完</span></div>
    <div class="bar"><i style="width:29%"></i></div>
  </div>
  <div class="tl">
    <div class="tl__item is-done"><i class="tl__t">08:30</i><span class="tl__dot"></span><span class="nm">发测试结果</span></div>
    <div class="tl__now"><i>现在 09:23</i><span></span></div>
    <div class="tl__item"><i class="tl__t">10:30</i><span class="tl__dot"></span><span><span class="nm">站会同步进度</span><span class="meta">待办</span></span></div>
    <div class="tl__item is-key"><i class="tl__t">14:00</i><span class="tl__dot"></span><span><span class="nm">提测评审</span><span class="meta">待办 · 重要</span></span></div>
    <div class="tl__item"><i class="tl__t">18:00</i><span class="tl__dot"></span><span><span class="nm">交周报</span><span class="meta">待办</span></span></div>
    <div class="tl__item is-key"><i class="tl__t">20:00</i><span class="tl__dot"></span><span><span class="nm">发布</span><span class="meta">计划 · 1.2 版本</span></span></div>
  </div>
  <div class="next-card">
    <p class="eyebrow">下一个提醒</p>
    <h4>提测评审</h4>
    <p>待办 · 今天 14:00</p>
    <div class="cd"><b>4:37</b><span>后到点</span></div>
  </div>
</aside>
`;

export const plansMain = () => `
<main class="main">
  <header class="topbar">
    <div>
      <p class="eyebrow">计划</p>
      <h1>进行中</h1>
      <p class="sub">一件事分成多个阶段，比如提测和发布，到了分别提醒</p>
    </div>
    <div class="topbar__right">
      <div class="date"><b>10月23日</b>周四 · 09:23</div>
      <div class="seg"><span class="on">卡片</span><span>${icon("i-timeline", "ico s")}时间轴</span></div>
      <button class="btn btn--primary">${icon("i-plus", "ico s")}新建</button>
    </div>
  </header>

  <div class="composer">
    <div class="composer__line">
      <span class="composer__dot"></span>
      <span class="composer__hint">先写个名字，再拆阶段，例如「1.2 版本」</span>
      <span class="kbd">Ctrl N</span>
    </div>
    <div class="composer__row">
      <span class="chip on">分 2 个阶段</span>
      <span class="chip">用模板：提测 → 发布</span>
      <span class="grow"></span>
      <button class="btn btn--primary btn--sm">记下</button>
    </div>
  </div>

  <article class="plan">
    <div class="plan__head">
      <span class="check"></span>
      <div class="plan__title">
        <h3><span class="pin"></span>1.2 版本</h3>
        <p>灰度先到 30%，重点看支付链路</p>
      </div>
      <span class="pill pill--amber">37 分钟后到点</span>
      <div class="plan__prog"><span class="frac">1/4</span><span class="segs"><i class="on"></i><i></i><i></i><i></i></span></div>
    </div>
    <ol class="stepper">
      <li class="done"><span class="dot"></span><span class="tm">10/21 18:00</span><span class="nm">需求确认</span><span class="side"></span></li>
      <li class="next"><span class="dot"></span><span class="tm">今天 10:00</span><span class="nm">提测</span><span class="side"><span class="pill pill--amber">37 分钟后</span></span></li>
      <li><span class="dot"></span><span class="tm">今天 20:00</span><span class="nm">发布</span><span class="side"><span class="pill pill--line">今晚</span></span></li>
      <li><span class="dot"></span><span class="tm">明天 10:00</span><span class="nm">观察</span><span class="side"></span></li>
    </ol>
  </article>

  <article class="plan">
    <div class="plan__head">
      <span class="check"></span>
      <div class="plan__title">
        <h3>灰度开关上线</h3>
        <p>先开 5%，看两天错误率再全量</p>
      </div>
      <span class="pill pill--plain">明天到点</span>
      <div class="plan__prog"><span class="frac">1/3</span><span class="segs"><i class="on"></i><i></i><i></i></span></div>
    </div>
    <ol class="stepper">
      <li class="done"><span class="dot"></span><span class="tm">10/22 14:00</span><span class="nm">方案评审</span><span class="side"></span></li>
      <li class="next"><span class="dot"></span><span class="tm">10/24 14:00</span><span class="nm">灰度 5%</span><span class="side"><span class="pill pill--line">明天</span></span></li>
      <li><span class="dot"></span><span class="tm">10/26 10:00</span><span class="nm">全量</span><span class="side"></span></li>
    </ol>
  </article>

  <article class="plan">
    <div class="plan__head">
      <span class="check"></span>
      <div class="plan__title">
        <h3>团队周会改版</h3>
        <p>还没想清楚节奏，先记着</p>
      </div>
      <span class="pill pill--plain">还没分阶段</span>
    </div>
    <div class="empty-stage">${icon("i-plus", "ico s")} 分阶段：议程 → 试跑 → 定稿</div>
  </article>

  <div class="main-foot">
    <span><b>3</b> 份计划 · 10 个阶段，下一个 37 分钟后</span>
    <span class="k"><span><span class="kbd">Enter</span>打开</span><span><span class="kbd">Ctrl N</span>新建</span></span>
  </div>
</main>
`;

export const plansAside = () => `
<aside class="aside">
  <div class="aside__head">
    <p class="eyebrow">本周</p>
    <div class="big"><b>3</b><span>份计划在跑</span></div>
    <div class="bars">
      <i class="moss" style="height:34%"></i>
      <i style="height:18%"></i>
      <i style="height:58%"></i>
      <i class="on" style="height:100%"></i>
      <i style="height:44%"></i>
      <i style="height:12%"></i>
      <i style="height:6%"></i>
    </div>
    <div class="bar-labels"><span>一</span><span>二</span><span>三</span><span class="on">四</span><span>五</span><span>六</span><span>日</span></div>
    <div class="stat-grid">
      <div class="stat"><b>7</b><span>本周到点</span></div>
      <div class="stat"><b>3</b><span>已完成</span></div>
    </div>
  </div>
  <div class="next-card" style="margin-top:auto">
    <p class="eyebrow">下一阶段</p>
    <h4>提测</h4>
    <p>1.2 版本 · 阶段 2 / 4</p>
    <div class="cd"><b>0:37</b><span>后到点</span></div>
  </div>
</aside>
`;
