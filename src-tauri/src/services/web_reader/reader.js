// Sagar Library: read-only browser reader for the content server.
// Book HTML arrives sanitized by the server; the page CSP blocks scripts from it.
(() => {
  "use strict";

  const API = "/reader/api";
  const app = document.getElementById("app");
  const narrow = window.matchMedia("(max-width: 760px)");

  const store = {
    get(key, fallback) {
      try { const v = localStorage.getItem("sagar.reader." + key); return v === null ? fallback : v; } catch { return fallback; }
    },
    set(key, value) {
      try { localStorage.setItem("sagar.reader." + key, value); } catch { /* storage unavailable */ }
    },
  };

  const state = {
    query: "",
    books: null,
    detail: null, // { book, chapters }
    chapterId: null,
    mode: ["both", "original", "translation"].includes(store.get("mode")) ? store.get("mode") : "both",
    size: Math.min(28, Math.max(14, Number(store.get("size", 18)) || 18)),
    sidebar: !narrow.matches,
    pendingFragment: null,
    token: 0,
  };
  document.documentElement.style.setProperty("--reading-size", state.size + "px");

  // ---------- helpers ----------
  const ICONS = {
    book: "M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5v14zM4 19.5A2.5 2.5 0 0 0 6.5 22H20v-5",
    search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-4.35-4.35",
    x: "M18 6 6 18M6 6l12 12",
    back: "M19 12H5M12 19l-7-7 7-7",
    prev: "M15 18l-6-6 6-6",
    next: "M9 18l6-6-6-6",
    list: "M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01",
    columns: "M3 4h18v16H3zM12 4v16",
    original: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6M8 13h8M8 17h5",
    translation: "M5 8l6 6M4 14l6-6 2-3M2 5h12M7 2h1M22 22l-5-10-5 10M14 18h6",
    smaller: "M5 12h14",
    larger: "M12 5v14M5 12h14",
    retry: "M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5",
    alert: "M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z",
  };
  function icon(name) {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("class", "icon");
    svg.setAttribute("aria-hidden", "true");
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", ICONS[name]);
    svg.append(path);
    return svg;
  }
  function h(tag, attrs = {}, ...children) {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
      if (v === null || v === undefined || v === false) continue;
      if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
      else if (k === "class") el.className = v;
      else el.setAttribute(k, v === true ? "" : v);
    }
    for (const c of children.flat()) if (c !== null && c !== undefined && c !== false) el.append(c);
    return el;
  }
  function button(label, iconName, onclick, { labelClass = null, ...attrs } = {}) {
    return h("button", { type: "button", class: "button", onclick, ...attrs }, icon(iconName), h("span", { class: labelClass }, label));
  }
  function iconButton(label, iconName, onclick, attrs = {}) {
    return h("button", { type: "button", class: "icon-button", "aria-label": label, title: label, onclick, ...attrs }, icon(iconName));
  }
  async function getJson(path) {
    const response = await fetch(API + path, { headers: { Accept: "application/json" }, credentials: "same-origin" });
    if (!response.ok) {
      const error = new Error("HTTP " + response.status);
      error.status = response.status;
      throw error;
    }
    return response.json();
  }
  function describe(error) {
    if (error && error.status === 404) return "This item is no longer in the Sagar library.";
    if (error && error.status === 401) return "Sign-in is required. Reload the page and enter the content server username and password.";
    if (error && (error.status === 503 || error.status === 408)) return "The desktop app is busy. Try again in a moment.";
    return "Could not reach the Sagar desktop app. Check that the content server is still running.";
  }
  function stateView(message, retry) {
    return h("div", { class: "state", role: retry ? "alert" : "status" },
      retry ? icon("alert") : null,
      h("p", {}, message),
      retry ? button("Try again", "retry", retry) : null);
  }
  const enc = encodeURIComponent;
  const bookHash = (bookId, chapterId) => "#/books/" + enc(bookId) + (chapterId ? "/" + enc(chapterId) : "");

  // ---------- routing ----------
  function route() {
    const parts = location.hash.replace(/^#\/?/, "").split("/").filter(Boolean).map((p) => { try { return decodeURIComponent(p); } catch { return p; } });
    if (parts[0] === "books" && parts[1]) showBook(parts[1], parts[2] || null);
    else showLibrary();
  }
  window.addEventListener("hashchange", route);

  // ---------- library ----------
  let searchTimer = 0;
  function showLibrary() {
    state.detail = null;
    state.chapterId = null;
    document.title = "Sagar Library";
    const input = h("input", {
      type: "search", value: state.query, placeholder: "Search title or author", "aria-label": "Search books", autocomplete: "off",
      oninput: (e) => {
        state.query = e.target.value;
        clear.hidden = !state.query;
        clearTimeout(searchTimer);
        searchTimer = setTimeout(loadBooks, 250);
      },
      onkeydown: (e) => { if (e.key === "Escape" && state.query) { e.preventDefault(); clearSearch(); } },
    });
    const clear = h("button", { type: "button", class: "icon-button clear", "aria-label": "Clear search", title: "Clear search", hidden: !state.query, onclick: () => clearSearch() }, icon("x"));
    function clearSearch() { state.query = ""; input.value = ""; clear.hidden = true; input.focus(); loadBooks(); }
    const results = h("main", { class: "library", id: "results", "aria-live": "polite" });
    app.replaceChildren(
      h("header", { class: "topbar" },
        h("h1", { class: "brand" }, icon("book"), "Sagar Library"),
        h("div", { class: "search", role: "search" }, icon("search"), input, clear)),
      results);
    if (state.books) renderBooks(results, state.books);
    loadBooks();
  }
  async function loadBooks() {
    const token = ++state.token;
    const results = document.getElementById("results");
    if (!results) return;
    if (!state.books) results.replaceChildren(stateView("Loading library…"));
    try {
      const q = state.query.trim();
      const books = await getJson("/books" + (q ? "?q=" + enc(q) : ""));
      if (token !== state.token) return;
      state.books = books;
      renderBooks(results, books);
    } catch (error) {
      if (token !== state.token) return;
      state.books = null;
      results.replaceChildren(stateView(describe(error), loadBooks));
    }
  }
  function renderBooks(results, books) {
    const q = state.query.trim();
    if (!books.length) {
      results.replaceChildren(stateView(q ? "No books match “" + q + "”." : "The Sagar library has no books yet."));
      return;
    }
    const summary = h("p", { class: "summary" }, q ? books.length + (books.length === 1 ? " book matches" : " books match") + " “" + q + "”" : books.length + (books.length === 1 ? " book" : " books"));
    results.replaceChildren(summary, h("ul", { class: "grid" }, books.map((book) => h("li", {}, bookCard(book)))));
  }
  function bookCard(book) {
    const cover = h("div", { class: "cover" });
    const img = h("img", { src: book.coverUrl, alt: "", loading: "lazy", decoding: "async" });
    img.addEventListener("error", () => cover.replaceChildren(icon("book")), { once: true });
    cover.append(img);
    const pct = book.chapterCount ? Math.round((book.translatedCount / book.chapterCount) * 100) : 0;
    const bar = h("span");
    bar.style.width = pct + "%";
    return h("a", { class: "card", href: bookHash(book.id) },
      cover,
      h("div", { class: "card-body" },
        h("span", { class: "card-title" }, book.title || "Untitled"),
        book.author ? h("span", { class: "card-author" }, book.author) : null,
        h("div", { class: "progress" },
          book.translatedCount + " of " + book.chapterCount + " chapters translated",
          h("div", { class: "bar", "aria-hidden": "true" }, bar))));
  }

  // ---------- book ----------
  async function showBook(bookId, chapterId) {
    if (state.detail && state.detail.book.id === bookId) {
      openChapter(chapterId);
      return;
    }
    const token = ++state.token;
    state.detail = null;
    app.replaceChildren(stateView("Loading book…"));
    try {
      const detail = await getJson("/books/" + enc(bookId));
      if (token !== state.token) return;
      state.detail = detail;
      renderBookShell();
      openChapter(chapterId);
    } catch (error) {
      if (token !== state.token) return;
      app.replaceChildren(
        h("header", { class: "topbar" }, button("Library", "back", () => { location.hash = "#/"; })),
        stateView(describe(error), () => showBook(bookId, chapterId)));
    }
  }

  function renderBookShell() {
    const { book, chapters } = state.detail;
    document.title = book.title + " · Sagar Library";
    state.sidebar = !narrow.matches;
    const filter = h("input", {
      type: "search", placeholder: "Filter chapters", "aria-label": "Filter chapters", autocomplete: "off",
      oninput: (e) => {
        const q = e.target.value.trim().toLowerCase();
        for (const li of list.children) li.hidden = !!q && !li.dataset.title.includes(q);
      },
    });
    const list = h("ol", { class: "chapter-list", "aria-label": "Chapters" },
      chapters.map((c, i) => h("li", { "data-title": (c.title || "").toLowerCase() },
        h("a", { href: bookHash(book.id, c.id), "data-id": c.id, onclick: () => { if (narrow.matches) setSidebar(false); } },
          h("span", { class: "num", "aria-hidden": "true" }, String(i + 1)),
          h("span", { class: "name" }, c.title || "Chapter " + (i + 1)),
          h("span", { class: "dot" + (c.translated ? " done" : ""), title: c.translated ? "Translated" : "Not translated" }),
          h("span", { class: "sr-only" }, c.translated ? " (translated)" : " (not translated)")))));
    const nav = h("nav", { class: "chapters", id: "chapter-nav", "aria-label": "Chapter list", hidden: !state.sidebar,
      onkeydown: (e) => { if (e.key === "Escape" && narrow.matches) { setSidebar(false); toggle.focus(); } } },
      h("div", { class: "search" }, icon("search"), filter), list);
    const toggle = iconButton("Show chapter list", "list", () => setSidebar(!state.sidebar), { "aria-controls": "chapter-nav", "aria-expanded": String(state.sidebar), id: "chapter-toggle" });

    const modes = h("div", { class: "segmented", role: "group", "aria-label": "Reading view" },
      button("Side by side", "columns", () => setMode("both"), { "data-mode": "both", class: "button wide-only", labelClass: "label-optional" }),
      button("Original", "original", () => setMode("original"), { "data-mode": "original", labelClass: "label-optional" }),
      button("Translation", "translation", () => setMode("translation"), { "data-mode": "translation", labelClass: "label-optional" }));
    for (const b of modes.querySelectorAll("button")) b.setAttribute("title", b.textContent.trim() + " view");

    const sourceLang = book.sourceLanguage || null;
    app.replaceChildren(h("div", { class: "book-page" },
      h("header", { class: "topbar" },
        iconButton("Back to library", "back", () => { location.hash = "#/"; }),
        toggle,
        h("div", { class: "book-heading" }, h("h1", {}, book.title || "Untitled"), book.author ? h("p", {}, book.author) : null),
        h("div", { class: "toolbar" },
          modes,
          iconButton("Smaller text", "smaller", () => setSize(state.size - 1), { id: "size-down" }),
          iconButton("Larger text", "larger", () => setSize(state.size + 1), { id: "size-up" }))),
      h("div", { class: "book-body" },
        nav,
        h("main", { class: "reader", id: "reader" },
          h("div", { class: "chapter-bar" },
            h("h2", { id: "chapter-title", tabindex: "-1" }),
            h("span", { class: "badge", id: "chapter-status" }),
            iconButton("Previous chapter", "prev", () => step(-1), { id: "prev" }),
            iconButton("Next chapter", "next", () => step(1), { id: "next" })),
          h("div", { class: "panes", id: "panes" },
            h("section", { class: "pane", id: "pane-original", "aria-labelledby": "label-original" },
              h("h3", { class: "pane-label", id: "label-original" }, "Original"),
              h("div", { class: "content", id: "content-original", lang: sourceLang })),
            h("section", { class: "pane", id: "pane-translation", "aria-labelledby": "label-translation" },
              h("h3", { class: "pane-label", id: "label-translation" }, "Translation"),
              h("div", { class: "content", id: "content-translation", lang: book.targetLanguage || null })))))));
    for (const id of ["content-original", "content-translation"]) document.getElementById(id).addEventListener("click", onContentClick);
    applyMode();
    updateSizeButtons();
  }

  function setSidebar(open) {
    state.sidebar = open;
    const nav = document.getElementById("chapter-nav");
    const toggle = document.getElementById("chapter-toggle");
    if (!nav || !toggle) return;
    nav.hidden = !open;
    toggle.setAttribute("aria-expanded", String(open));
    const label = open ? "Hide chapter list" : "Show chapter list";
    toggle.setAttribute("aria-label", label);
    toggle.title = label;
    if (open) {
      const current = nav.querySelector('[aria-current="page"]');
      if (current) current.scrollIntoView({ block: "nearest" });
      if (narrow.matches) (current || nav.querySelector("input")).focus();
    }
  }
  narrow.addEventListener("change", () => { if (state.detail) { setSidebar(!narrow.matches); applyMode(); } });

  function effectiveMode() { return state.mode === "both" && narrow.matches ? "translation" : state.mode; }
  function setMode(mode) { state.mode = mode; store.set("mode", mode); applyMode(); }
  function applyMode() {
    const mode = effectiveMode();
    const panes = document.getElementById("panes");
    if (!panes) return;
    document.getElementById("pane-original").hidden = mode === "translation";
    document.getElementById("pane-translation").hidden = mode === "original";
    panes.classList.toggle("single", mode !== "both");
    for (const b of document.querySelectorAll(".segmented button")) b.setAttribute("aria-pressed", String(b.dataset.mode === mode));
  }
  function setSize(size) {
    state.size = Math.min(28, Math.max(14, size));
    store.set("size", String(state.size));
    document.documentElement.style.setProperty("--reading-size", state.size + "px");
    updateSizeButtons();
  }
  function updateSizeButtons() {
    const down = document.getElementById("size-down");
    const up = document.getElementById("size-up");
    if (down) down.disabled = state.size <= 14;
    if (up) up.disabled = state.size >= 28;
  }

  function step(delta) {
    const { book, chapters } = state.detail;
    const index = chapters.findIndex((c) => c.id === state.chapterId) + delta;
    if (index >= 0 && index < chapters.length) location.hash = bookHash(book.id, chapters[index].id);
  }

  async function openChapter(chapterId) {
    const { book, chapters } = state.detail;
    if (!chapters.length) {
      document.getElementById("panes").replaceChildren(stateView("This book has no chapters in its reading order."));
      return;
    }
    let chapter = chapters.find((c) => c.id === chapterId);
    if (!chapter) {
      const last = store.get("last." + book.id);
      chapter = chapters.find((c) => c.id === last) || chapters[0];
      history.replaceState(null, "", bookHash(book.id, chapter.id));
    }
    const index = chapters.indexOf(chapter);
    state.chapterId = chapter.id;
    store.set("last." + book.id, chapter.id);

    const title = document.getElementById("chapter-title");
    title.textContent = chapter.title || "Chapter " + (index + 1);
    document.getElementById("prev").disabled = index === 0;
    document.getElementById("next").disabled = index === chapters.length - 1;
    for (const a of document.querySelectorAll(".chapter-list a")) {
      if (a.dataset.id === chapter.id) { a.setAttribute("aria-current", "page"); a.scrollIntoView({ block: "nearest" }); }
      else a.removeAttribute("aria-current");
    }
    const status = document.getElementById("chapter-status");
    const original = document.getElementById("content-original");
    const translation = document.getElementById("content-translation");
    status.textContent = "Loading…";
    status.className = "badge";
    original.replaceChildren(stateView("Loading chapter…"));
    translation.replaceChildren();

    const token = ++state.token;
    try {
      const view = await getJson("/books/" + enc(book.id) + "/chapters/" + enc(chapter.id));
      if (token !== state.token) return;
      original.innerHTML = view.originalHtml || "";
      if (!view.originalHtml) original.replaceChildren(h("p", { class: "empty" }, "This chapter has no readable text."));
      if (view.translatedHtml) {
        translation.innerHTML = view.translatedHtml;
        status.textContent = view.status === "reviewed" ? "Reviewed" : "Translated";
        status.className = "badge done";
      } else {
        const message = view.status === "skipped" ? "This chapter keeps the original text." : "This chapter has not been translated yet.";
        translation.replaceChildren(h("p", { class: "empty" }, message));
        status.textContent = view.status === "skipped" ? "Kept original" : "Not translated";
      }
      for (const pane of document.querySelectorAll(".pane")) pane.scrollTop = 0;
      if (state.pendingFragment) {
        scrollToFragment(original, state.pendingFragment);
        scrollToFragment(translation, state.pendingFragment);
        state.pendingFragment = null;
      } else if (document.activeElement && document.activeElement.closest && document.activeElement.closest(".chapter-list")) {
        // Keep focus in the list for keyboard browsing.
      } else {
        title.focus({ preventScroll: true });
      }
    } catch (error) {
      if (token !== state.token) return;
      status.textContent = "Error";
      original.replaceChildren(stateView(describe(error), () => openChapter(chapter.id)));
      translation.replaceChildren();
    }
  }

  function scrollToFragment(container, fragment) {
    let target = null;
    try { target = container.querySelector('[id="' + CSS.escape(fragment) + '"]'); } catch { target = null; }
    if (!target) return;
    target.scrollIntoView({ block: "start" });
    target.classList.add("target-flash");
    setTimeout(() => target.classList.remove("target-flash"), 1700);
  }

  // Links inside book content: in-chapter anchors scroll; links to other
  // chapters open them here. Everything else is inert.
  function onContentClick(event) {
    const link = event.target.closest("a");
    if (!link || !event.currentTarget.contains(link)) return;
    event.preventDefault();
    const href = link.getAttribute("href") || "";
    if (href.startsWith("#")) {
      let fragment = href.slice(1);
      try { fragment = decodeURIComponent(fragment); } catch { /* keep raw */ }
      scrollToFragment(event.currentTarget, fragment);
      return;
    }
    const marker = "/resources/";
    const at = href.indexOf(marker);
    if (!href.startsWith(API + "/books/") || at < 0) return;
    let [path, fragment] = href.slice(at + marker.length).split("#");
    try { path = decodeURIComponent(path); } catch { /* keep raw */ }
    const { book, chapters } = state.detail;
    const target = chapters.find((c) => c.href === path);
    if (!target) return;
    if (target.id === state.chapterId) {
      if (fragment) scrollToFragment(event.currentTarget, fragment);
      return;
    }
    state.pendingFragment = fragment || null;
    location.hash = bookHash(book.id, target.id);
  }

  document.addEventListener("keydown", (e) => {
    if (!state.detail || e.altKey || e.ctrlKey || e.metaKey) return;
    const tag = (e.target.tagName || "").toLowerCase();
    if (tag === "input" || tag === "textarea") return;
    if (e.key === "[" ) step(-1);
    else if (e.key === "]") step(1);
  });

  route();
})();
