(function (A) {
  A.icon = function (name, cls, style) {
    const body = A.ICONS[name] || A.ICONS["circle-dashed"];
    return `<svg class="icon ${cls || ""}" ${style ? `style="${style}"` : ""} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;
  };

  function hue(name) {
    let hash = 0x811c9dc5;
    for (const byte of new TextEncoder().encode(name.toLowerCase())) {
      hash = Math.imul(hash ^ byte, 0x01000193) >>> 0;
    }
    return hash % 360;
  }

  function initials(name) {
    const parts = name.split(/[-_. ]/).filter(Boolean);
    if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
    return name.replace(/[^a-z0-9]/gi, "").slice(0, 2).toUpperCase();
  }

  A.avatar = function (login, size) {
    size = size || 18;
    return `<span class="avatar" title="${login}" style="width:${size}px;height:${size}px;font-size:${Math.round(size * 0.45)}px;background:hsl(${hue(login)} 45% 42%)">${initials(login)}</span>`;
  };

  A.stack = function (logins, size, max) {
    max = max || 3;
    return `<span class="stack" style="display:inline-flex">${logins.slice(0, max).map((l) => A.avatar(l, size || 16)).join("")}</span>`;
  };

  A.orgBadge = function (name) {
    return `<span class="org-badge" style="background:hsl(${hue(name)} 45% 42%)">${initials(name)}</span>`;
  };

  A.labelChip = function (key) {
    const label = A.LABELS[key] || { name: key, color: "888888" };
    const n = parseInt(label.color, 16);
    const r = ((n >> 16) & 255) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
    const fg = 0.299 * r + 0.587 * g + 0.114 * b > 0.6 ? "#141518" : "#ffffff";
    return `<span class="label-chip" style="background:#${label.color};color:${fg}">${label.name}</span>`;
  };

  A.labelChips = function (keys, max) {
    const shown = keys.slice(0, max || keys.length).map(A.labelChip).join("");
    const rest = max && keys.length > max ? `<span class="muted" style="font-size:11px">+${keys.length - max}</span>` : "";
    return shown + rest;
  };

  A.ago = function (min) {
    if (min < 60) return `há ${min} min`;
    if (min < 1440) return `há ${Math.round(min / 60)} h`;
    const days = Math.round(min / 1440);
    if (days === 1) return "ontem";
    if (days < 14) return `há ${days} dias`;
    return `há ${Math.round(days / 7)} semanas`;
  };

  A.age = function (min) {
    if (min < 60) return `${min}m`;
    if (min < 1440) return `${Math.round(min / 60)}h`;
    const days = Math.round(min / 1440);
    if (days < 14) return `${days}d`;
    return `${Math.round(days / 7)}sem`;
  };

  A.issueState = function (i) {
    if (i.state === "open") return { icon: "circle-dot", color: "var(--success)", label: "Open" };
    if (i.stateReason === "not_planned") return { icon: "circle-slash", color: "var(--muted-fg)", label: "Not planned" };
    return { icon: "circle-check", color: "var(--magenta)", label: "Closed" };
  };

  A.prState = function (p) {
    if (p.state === "merged") return { icon: "git-merge", color: "var(--magenta)", label: "Merged" };
    if (p.state === "closed") return { icon: "git-pull-request-closed", color: "var(--danger)", label: "Closed" };
    if (p.draft) return { icon: "git-pull-request-draft", color: "var(--muted-fg)", label: "Draft" };
    return { icon: "git-pull-request", color: "var(--success)", label: "Open" };
  };

  A.stateOf = function (item) {
    return item.kind === "pr" ? A.prState(item) : A.issueState(item);
  };

  A.stateIcon = function (style, cls) {
    return `<span style="color:${style.color};display:inline-flex" title="${style.label}">${A.icon(style.icon, cls || "sm")}</span>`;
  };

  A.pill = function (style) {
    return `<span class="pill" style="color:${style.color};background:color-mix(in srgb, ${style.color} 16%, transparent);border:1px solid color-mix(in srgb, ${style.color} 50%, transparent)">${A.icon(style.icon, "xs")}${style.label}</span>`;
  };

  A.checksGlyph = function (checks) {
    const map = {
      success: ["check", "var(--success)", "CI ok"],
      failure: ["x", "var(--danger)", "CI falhou"],
      pending: ["circle-dashed", "var(--warning)", "CI em andamento"],
    };
    const [name, color, title] = map[checks] || ["minus", "var(--muted-fg)", "sem CI"];
    return `<span style="color:${color};display:inline-flex" title="${title}">${A.icon(name, "xs")}</span>`;
  };

  A.typeStyle = function (type) {
    if (type === "Bug") return { icon: "bug", color: "var(--danger)" };
    if (type === "Feature") return { icon: "lightbulb", color: "var(--blue)" };
    return { icon: "square-check", color: "var(--muted-fg)" };
  };

  A.typeTag = function (type) {
    const s = A.typeStyle(type);
    return `<span class="tag" style="color:${s.color}">${A.icon(s.icon, "xs")}${type}</span>`;
  };

  A.findPr = (number) => A.PRS.find((p) => p.number === number);
  A.findIssue = (number) => A.ISSUES.find((i) => i.number === number);
  A.repoShort = (repo) => repo.split("/")[1];

  A.prChip = function (number, opts) {
    const pr = A.findPr(number);
    if (!pr) return "";
    const s = A.prState(pr);
    const checks = pr.state === "open" && !(opts && opts.noChecks) ? A.checksGlyph(pr.checks) : "";
    return `<span class="tag" style="color:var(--fg);gap:5px" title="${pr.title}"><span style="color:${s.color};display:inline-flex">${A.icon(s.icon, "xs")}</span><span class="mono" style="font-size:11px">#${pr.number}</span>${checks}</span>`;
  };

  A.diffStat = function (add, del) {
    return `<span class="mono diff-add">+${add}</span><span class="mono diff-del">−${del}</span>`;
  };

  /* ---------- Chrome ---------- */

  A.titlebar = function (opts) {
    opts = opts || {};
    return `<div class="titlebar">
      <div class="tb-identity"><span class="app">Argon</span><span class="muted">@${A.ME}</span>
        <span class="tb-sync"><span class="dot"></span>Sincronizado há 1 min</span></div>
      <div class="tb-center"><div class="tb-search" ${opts.searchNote ? `data-note="${opts.searchNote}"` : ""}>${A.icon("search", "xs")}<span class="ph">${opts.search || "Buscar PRs, repos, comandos…"}</span><span class="kbd">Ctrl K</span></div></div>
      <div class="tb-right"><span class="tb-win">${A.icon("minus", "xs")}</span><span class="tb-win">${A.icon("rows", "xs")}</span><span class="tb-win">${A.icon("x", "xs")}</span></div>
    </div>`;
  };

  A.navItem = function (item) {
    const cls = item.accent === "warn" ? "count warn" : item.accent ? "count accent" : "count";
    const count = item.count === undefined || item.count === null ? "" : `<span class="${cls}">${item.count}</span>`;
    const lead = item.badge ? A.orgBadge(item.label) : A.icon(item.icon, "sm", item.iconColor ? `color:${item.iconColor}` : "");
    return `<div class="nav-item ${item.active ? "active" : ""}" data-nav="${item.id || ""}" ${item.note ? `data-note="${item.note}"` : ""}>
      ${lead}<span class="label">${item.label}</span>${item.admin ? '<span class="tag" style="height:16px;font-size:10px">admin</span>' : ""}${count}</div>`;
  };

  A.navGroup = function (group) {
    const isNew = group.isNew ? '<span class="new">novo</span>' : "";
    return `<div class="nav-group" ${group.note ? `data-note="${group.note}"` : ""}>
      <div class="nav-title">${group.title}${isNew}${group.titleExtra || ""}</div>
      ${group.items.map(A.navItem).join("")}
    </div>`;
  };

  A.GROUPS = {
    inbox: (active, counts) => ({
      title: "Inbox",
      items: [
        { id: "inbox", icon: "inbox", label: "Inbox", count: (counts && counts.inbox) || 18, active: active === "inbox" },
        { id: "needs", icon: "circle-dot", label: "Needs you", count: (counts && counts.needs) || 2, accent: true, active: active === "needs" },
      ],
    }),
    agents: (active) => ({
      title: "Agentes",
      items: [
        { id: "ag-all", icon: "bot", label: "Todas", count: 6, active: active === "ag-all" },
        { id: "ag-wait", icon: "hand", label: "Aguardando você", count: 1, accent: "warn", active: active === "ag-wait" },
        { id: "ag-run", icon: "loader-circle", label: "Rodando", count: 1 },
        { id: "ag-done", icon: "circle-check", label: "Concluídas", count: 4 },
        { id: "ag-arch", icon: "archive", label: "Arquivadas", count: 12 },
      ],
    }),
    yourPrs: (active) => ({
      title: "Seus PRs",
      items: [
        { id: "your", icon: "git-pull-request", label: "Your PRs", count: 4, active: active === "your" },
        { id: "waiting", icon: "clock", label: "Waiting on others", count: 1, active: active === "waiting" },
        { id: "changes", icon: "file-diff", label: "Changes requested", count: 0, active: active === "changes" },
        { id: "ready", icon: "git-merge", label: "Ready to merge", count: 1, active: active === "ready" },
      ],
    }),
    reviewed: (active) => ({
      title: "Revisados",
      items: [{ id: "done", icon: "check-check", label: "Already done", count: 3, active: active === "done" }],
    }),
    orgs: () => ({
      title: "Orgs",
      items: A.ORGS.map((o) => ({ id: "org-" + o.name, badge: true, label: o.name, count: o.count, admin: o.admin })),
    }),
  };

  A.dockLeft = function (groups) {
    return `<aside class="dock-left">
      <div class="dock-tabs"><div class="dock-tab active">Visões</div></div>
      <div class="views">${groups.map(A.navGroup).join("")}</div>
      <div class="accounts"><div class="accounts-inner">
        <div class="nav-title" style="justify-content:space-between">Contas<span class="muted" style="display:flex">${A.icon("settings", "xs")}</span></div>
        <div class="account">${A.avatar(A.ME, 22)}<div><div>@${A.ME}</div><div class="host mono">github.com</div></div></div>
      </div></div>
    </aside>`;
  };

  A.tabbar = function (tabs) {
    return `<div class="tabbar">${tabs
      .map(
        (t) => `<div class="tab ${t.active ? "active" : ""}" data-tab="${t.id}" ${t.note ? `data-note="${t.note}"` : ""}>
          ${t.icon ? `<span style="display:inline-flex;color:${t.iconColor || "inherit"}">${A.icon(t.icon, "sm")}</span>` : ""}
          <span class="truncate">${t.title}</span>
          ${t.closable === false ? "" : `<span class="x">${A.icon("x", "xs")}</span>`}</div>`
      )
      .join("")}</div>`;
  };

  A.statusbar = function (opts) {
    opts = opts || {};
    return `<div class="statusbar">
      <span class="sb-btn on" title="Visões (Ctrl+B)">${A.icon("panel-left", "xs")}</span>
      <span class="sb-btn" title="Painel inferior">${A.icon("panel-bottom", "xs")}</span>
      <span class="sb-btn">${A.icon("circle-dot", "xs")} ${opts.needs || "2 needs you"}</span>
      ${opts.extra || ""}
      <span class="sb-btn">${A.icon("bot", "xs")} 1 rodando · 1 aguardando</span>
      <span style="flex:1"></span>
      <span class="sb-btn">v0.1.5</span>
    </div>`;
  };

  /* ---------- Linha de PR (igual à do app) ---------- */

  A.prRow = function (pr, opts) {
    opts = opts || {};
    const reviewers = A.stack(pr.reviewers, 16);
    return `<div class="row ${pr.unread ? "unread" : ""} ${opts.selected ? "selected" : ""}" data-open="pr:${pr.number}">
      <div class="row-main">
        <span class="udot"></span>${A.stateIcon(A.prState(pr))}${A.avatar(pr.author, 18)}
        <span class="title">${pr.title}</span>
        <span class="col col-ci">${A.checksGlyph(pr.checks)}</span>
        <span class="col col-rev">${reviewers}</span>
        <span class="col col-diff">${A.diffStat(pr.add, pr.del)}</span>
        <span class="col col-com">${A.icon("message-square", "xs")}${pr.comments}</span>
        <span class="col col-age mono muted">${A.age(pr.upd)}</span>
      </div>
      <div class="row-sub">
        <span class="mono">${pr.repo} #${pr.number}</span><span class="sep">·</span>
        <span class="mono truncate" style="max-width:240px;flex-shrink:1">${pr.head}</span><span class="sep">·</span>
        <span class="truncate flex1">${pr.reason}</span>
      </div>
    </div>`;
  };

  A.sectionHeader = function (title, count, extra) {
    return `<div class="section-header"><b>${title}</b><span class="muted">${count}</span>${extra || ""}</div>`;
  };

  /* ---------- Cabeçalho de issue (espelha o header do PR) ---------- */

  A.issueHeader = function (issue, opts) {
    opts = opts || {};
    const [owner, repo] = issue.repo.split("/");
    const style = A.issueState(issue);
    const assignees = issue.assignees.length
      ? issue.assignees.map((a) => `<span class="item">${A.avatar(a, 16)}<span class="fg">${a}</span></span>`).join("")
      : '<span class="item muted">sem responsável</span>';
    const linked = issue.linked.length
      ? `<span class="item" ${opts.linkedNote ? `data-note="${opts.linkedNote}"` : ""}>${A.icon("git-pull-request", "xs")}${issue.linked.map((n) => A.prChip(n)).join(" ")}</span>`
      : `<span class="item">${A.icon("git-pull-request", "xs")}sem PR vinculado</span>`;
    const sub = issue.sub
      ? `<span class="item">${A.icon("list-tree", "xs")}${issue.sub.done}/${issue.sub.total} sub-issues</span>`
      : "";
    const milestone = issue.milestone ? `<span class="item">${A.icon("milestone", "xs")}${issue.milestone}</span>` : "";
    return `<div class="d-head">
      <div class="d-top">
        <div class="crumb mono">${owner}<span class="sep">/</span>${repo}<span class="sep">/</span><b>#${issue.number}</b></div>
        <div class="d-actions" ${opts.actionsNote ? `data-note="${opts.actionsNote}"` : ""}>${opts.actions || ""}</div>
      </div>
      <div class="d-title"><h1>${issue.title}</h1><span class="num">#${issue.number}</span></div>
      <div class="d-meta">${A.pill(style)}${A.typeTag(issue.type)}${A.avatar(issue.author, 18)}<b>${issue.author}</b><span>· aberta ${A.ago(issue.created)}</span>${opts.metaExtra || ""}</div>
      <div class="d-stats" ${opts.statsNote ? `data-note="${opts.statsNote}"` : ""}>
        <span class="item">${A.icon("users", "xs")}</span>${assignees}
        ${linked}${sub}${milestone}
        <span class="item" style="gap:4px">${A.labelChips(issue.labels)}</span>
      </div>
    </div>`;
  };

  A.issueActions = function (issue, extra) {
    return `<button class="btn ghost small">${A.icon("external-link", "xs")}Abrir no GitHub</button>
      <button class="btn ghost small only" title="Atualizar">${A.icon("refresh-cw", "xs")}</button>
      ${extra || ""}`;
  };

  A.thread318 = function (opts) {
    opts = opts || {};
    const t = A.THREAD_318;
    const items = t.items
      .map((it) => {
        if (it.t === "event") {
          const text = it.text.replace(/<span data-labels="([^"]+)"><\/span>/, (_, keys) => A.labelChips(keys.split(",")));
          return `<div class="event"><span class="ev-icon">${A.icon(it.icon, "xs")}</span>${A.avatar(it.who, 16)}<b>${it.who}</b>${text}<span>${A.ago(it.ago)}</span></div>`;
        }
        if (it.t === "xref") {
          const pr = A.findPr(it.pr);
          const s = A.prState(pr);
          return `<div class="event"><span class="ev-icon">${A.icon("link", "xs")}</span>${A.avatar(it.who, 16)}<b>${it.who}</b>vinculou um pull request<span>${A.ago(it.ago)}</span></div>
            <div class="xref" ${opts.xrefNote ? `data-note="${opts.xrefNote}"` : ""}>${A.stateIcon(s)}<span class="fg">${pr.title}</span><span class="mono muted">#${pr.number}</span>
              <span style="flex:1"></span>${A.checksGlyph(pr.checks)}<span class="muted" style="font-size:12px">CI em andamento</span>
              <span class="muted" style="font-size:12px">· review de ${pr.reviewers.join(", ")}</span>
              <button class="btn outline small" data-open="pr:${pr.number}">Abrir PR</button></div>`;
        }
        return `<div class="comment ${it.isNew ? "new" : ""}"><div class="comment-head">${A.avatar(it.who, 18)}<b>${it.who}</b>comentou ${A.ago(it.ago)}${it.isNew ? '<span class="badge-new">novo</span>' : ""}</div><div class="md">${it.html}</div></div>`;
      })
      .join("");
    return `<div class="thread">
      <div class="comment"><div class="comment-head">${A.avatar("marina-lopes", 18)}<b>marina-lopes</b>abriu esta issue ${A.ago(4300)}</div><div class="md">${t.body}</div></div>
      ${items}
      <div class="composer"><div class="area">Comentar… (markdown)</div><div class="foot">
        <button class="btn ghost small">Fechar como concluída</button><button class="btn primary small">Comentar</button></div></div>
    </div>`;
  };

  /* ---------- Faixa da proposta + anotações + tema ---------- */

  A.strip = function (opts) {
    const notes = (opts.notes || []).map((n, ix) => `<li><span class="n">${ix + 1}</span><span>${n}</span></li>`).join("");
    const pros = (opts.pros || []).map((p) => `<div class="pro"><b>+</b> ${p}</div>`).join("");
    const cons = (opts.cons || []).map((c) => `<div class="con"><b>−</b> ${c}</div>`).join("");
    return `<div class="strip">
        <a class="strip-back" href="index.html">${A.icon("chevron-left", "xs")}Propostas</a>
        <span class="strip-tag">${opts.tag}</span>
        <span class="strip-title">${opts.title}</span>
        <span class="strip-desc truncate">${opts.desc}</span>
        <span class="strip-actions">
          <button class="strip-btn on" data-action="notes">${A.icon("message-circle-question", "xs")}Anotações</button>
          <button class="strip-btn" data-action="theme">${A.icon("sun", "xs")}Tema</button>
        </span>
      </div>
      <div class="notes-panel"><h3 title="Recolher / expandir">${opts.title}<span class="caret">${A.icon("chevron-down", "xs")}</span></h3><p class="lead">${opts.lead || ""}</p><ol>${notes}</ol>
        ${pros || cons ? `<div class="pc">${pros}${cons}</div>` : ""}</div>`;
  };

  function clipOf(el) {
    let clip = { top: 0, left: 0, right: innerWidth, bottom: innerHeight };
    for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
      const cs = getComputedStyle(p);
      if (cs.overflowX === "visible" && cs.overflowY === "visible") continue;
      const r = p.getBoundingClientRect();
      clip = {
        top: Math.max(clip.top, r.top), left: Math.max(clip.left, r.left),
        right: Math.min(clip.right, r.right), bottom: Math.min(clip.bottom, r.bottom),
      };
    }
    return clip;
  }

  A.placeNotes = function () {
    let layer = document.getElementById("note-layer");
    if (!layer) {
      layer = document.createElement("div");
      layer.id = "note-layer";
      document.body.appendChild(layer);
    }
    if (!document.body.classList.contains("notes-on")) {
      layer.innerHTML = "";
      return;
    }
    const badges = [];
    const frame = (document.querySelector(".window") || document.body).getBoundingClientRect();
    document.querySelectorAll("[data-note]").forEach((el) => {
      const r = el.getBoundingClientRect();
      if (!r.width && !r.height) return;
      const clip = clipOf(el);
      if (r.bottom < clip.top + 4 || r.top > clip.bottom - 4 || r.right < clip.left + 4 || r.left > clip.right - 4) return;
      const x = Math.max(frame.left + 2, Math.min(r.right - 6, frame.right - 20));
      const y = Math.max(frame.top + 2, Math.max(clip.top - 6, Math.min(r.top - 9, frame.bottom - 20)));
      badges.push(`<span class="note-badge" style="left:${x}px;top:${y}px">${el.dataset.note}</span>`);
    });
    layer.innerHTML = badges.join("");
  };

  let notesQueued = false;
  function queueNotes() {
    if (notesQueued) return;
    notesQueued = true;
    requestAnimationFrame(() => {
      notesQueued = false;
      A.placeNotes();
    });
  }

  A.mountStrip = function (opts) {
    const host = document.getElementById("strip");
    host.innerHTML = A.strip(opts);
    let theme = "dark";
    try { theme = localStorage.getItem("argon-mock-theme") || "dark"; } catch (e) {}
    document.documentElement.setAttribute("data-theme", theme);
    document.body.classList.add("notes-on");
    const watched = document.getElementById("app") || document.body;
    const outsideLayer = (record) => {
      const node = record.target.nodeType === 1 ? record.target : record.target.parentElement;
      return !(node && node.closest("#note-layer"));
    };
    new MutationObserver((records) => {
      if (records.some(outsideLayer)) queueNotes();
    }).observe(watched, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "style"] });
    document.addEventListener("scroll", queueNotes, true);
    window.addEventListener("resize", queueNotes);
    queueNotes();
    const panel = host.querySelector(".notes-panel");
    panel.querySelector("h3").addEventListener("click", () => panel.classList.toggle("collapsed"));
    panel.addEventListener("mouseover", (event) => {
      const li = event.target.closest("li");
      document.querySelectorAll(".note-focus").forEach((el) => el.classList.remove("note-focus"));
      if (!li) return;
      const n = li.querySelector(".n").textContent;
      document.querySelectorAll(`[data-note="${n}"]`).forEach((el) => el.classList.add("note-focus"));
    });
    panel.addEventListener("mouseleave", () => {
      document.querySelectorAll(".note-focus").forEach((el) => el.classList.remove("note-focus"));
    });
    host.addEventListener("click", (event) => {
      const action = event.target.closest("[data-action]");
      if (!action) return;
      if (action.dataset.action === "notes") {
        document.body.classList.toggle("notes-on");
        action.classList.toggle("on", document.body.classList.contains("notes-on"));
        queueNotes();
      }
      if (action.dataset.action === "theme") {
        theme = theme === "dark" ? "light" : "dark";
        document.documentElement.setAttribute("data-theme", theme);
        try { localStorage.setItem("argon-mock-theme", theme); } catch (e) {}
      }
    });
  };
})(window.Argon);
