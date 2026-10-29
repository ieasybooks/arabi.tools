// arabiTools.search.find(texts, query, { ignoreMarks, ignoreTatweel }) returns
// { segment, start, end } in document order, using original UTF-16 grapheme spans.
// attach(toolbar, { texts, show, element, label }) returns { refresh, dispose }.
// show reveals a segment inside its viewer and returns its exact-text element (or
// a Promise of it); refresh reapplies highlights after an adapter changes pages.
// Highlights use CSS ranges or range-positioned backgrounds, leaving text nodes
// untouched in both paths so Arabic shaping and native selection stay intact.
// Without Intl.Segmenter, search is unavailable rather than guessing boundaries.
(() => {
  const app = (window.arabiTools ??= {});
  if (app.search) return;
  const segmenter = typeof Intl.Segmenter === "function"
    ? new Intl.Segmenter(undefined, { granularity: "grapheme" }) : null;
  const customHighlights = typeof Highlight === "function" && !!window.CSS?.highlights;
  const allHighlights = customHighlights ? new Highlight() : null;
  const activeHighlights = customHighlights ? new Highlight() : null;
  if (customHighlights) {
    activeHighlights.priority = 1;
    CSS.highlights.set("arabi-search-match", allHighlights);
    CSS.highlights.set("arabi-search-active", activeHighlights);
  }

  function comparison(text, { ignoreMarks = true, ignoreTatweel = true } = {}) {
    let survivors = "";
    const origins = [];
    for (const { segment, index } of segmenter.segment(text)) {
      const origin = { start: index, end: index + segment.length };
      for (const character of segment) {
        if (ignoreMarks && /[\u064b-\u0652]/u.test(character)) continue;
        if (ignoreTatweel && character === "\u0640") continue;
        survivors += character;
        for (let unit = 0; unit < character.length; unit++) origins.push(origin);
      }
    }
    let value = "";
    const starts = new Map();
    const ends = new Map();
    // Removing tatweel can attach its surviving mark to the previous letter.
    for (const { segment, index } of segmenter.segment(survivors)) {
      starts.set(value.length, origins[index].start);
      value += segment.normalize("NFC");
      ends.set(value.length, origins[index + segment.length - 1].end);
    }
    return { value, starts, ends };
  }

  function matchesIn(buffer, query, segment) {
    const matches = [];
    let position = buffer.value.indexOf(query);
    while (position !== -1) {
      const end = position + query.length;
      if (buffer.starts.has(position) && buffer.ends.has(end)) {
        matches.push({ segment, start: buffer.starts.get(position), end: buffer.ends.get(end) });
      }
      // Include overlapping literal matches, in their original document order.
      position = buffer.value.indexOf(query, position + 1);
    }
    return matches;
  }

  function find(texts, query, options = {}) {
    if (!segmenter) return [];
    const needle = comparison(query, options).value;
    if (!needle.trim()) return [];
    return texts.flatMap((text, segment) => matchesIn(comparison(text, options), needle, segment));
  }

  function rangeFor(element, start, end) {
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    const range = new Range();
    let offset = 0;
    let started = false;
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const next = offset + node.length;
      if (!started && start < next) {
        range.setStart(node, start - offset);
        started = true;
      }
      if (started && end <= next) {
        range.setEnd(node, end - offset);
        return range;
      }
      offset = next;
    }
    return null;
  }

  function attach(toolbar, adapter) {
    const field = toolbar.querySelector("[data-search-query]");
    const marks = toolbar.querySelector("[data-search-marks]");
    const tatweel = toolbar.querySelector("[data-search-tatweel]");
    const previous = toolbar.querySelector("[data-search-previous]");
    const next = toolbar.querySelector("[data-search-next]");
    const clear = toolbar.querySelector("[data-search-clear]");
    const count = toolbar.querySelector("[data-search-count]");
    const label = toolbar.querySelector("[data-search-active]");
    const status = toolbar.querySelector("[data-search-status]");
    const events = new AbortController();
    let generation = 0;
    let navigation = 0;
    let disposed = false;
    let matches = [];
    let active = 0;
    let index = null;
    let indexOptions = "";
    let announcement;
    const ranges = [];
    const painted = new Set();
    const sizes = new WeakMap();
    let repaintFrame;
    const resize = !customHighlights && typeof ResizeObserver === "function"
      ? new ResizeObserver((entries) => {
        let changed = false;
        for (const { target, contentRect } of entries) {
          const size = `${contentRect.width}:${contentRect.height}`;
          if (sizes.get(target) !== size) changed = true;
          sizes.set(target, size);
        }
        if (changed) {
          cancelAnimationFrame(repaintFrame);
          repaintFrame = requestAnimationFrame(refresh);
        }
      }) : null;

    function clearHighlights() {
      for (const range of ranges) {
        allHighlights.delete(range);
        activeHighlights.delete(range);
      }
      ranges.length = 0;
      resize?.disconnect();
      for (const element of painted) {
        element.classList.remove("search-painted");
        for (const property of ["--search-backgrounds", "--search-sizes", "--search-positions"]) {
          element.style.removeProperty(property);
        }
      }
      painted.clear();
    }

    function refresh() {
      if (disposed) return;
      clearHighlights();
      const bySegment = new Map();
      for (const [position, match] of matches.entries()) {
        if (!bySegment.has(match.segment)) bySegment.set(match.segment, []);
        bySegment.get(match.segment).push({ ...match, active: position === active });
      }
      for (const [segment, hits] of bySegment) {
        const element = adapter.element(segment);
        if (!element || element.textContent !== adapter.texts[segment]) continue;
        if (customHighlights) {
          for (const hit of hits) {
            const range = rangeFor(element, hit.start, hit.end);
            if (!range) continue;
            ranges.push(range);
            allHighlights.add(range);
            if (hit.active) activeHighlights.add(range);
          }
        } else {
          // Splitting Arabic shaping runs with inline marks can hide glyphs in
          // WebKit. Paint behind the original text instead. The active hit has
          // a double underline; foreground and glyph shaping never change.
          const area = element.getBoundingClientRect();
          const backgrounds = [];
          const positions = [];
          const dimensions = [];
          for (const hit of hits.sort((a, b) => Number(b.active) - Number(a.active))) {
            const range = rangeFor(element, hit.start, hit.end);
            if (!range) continue;
            for (const box of range.getClientRects()) {
              if (!box.width || !box.height) continue;
              backgrounds.push(hit.active
                ? "linear-gradient(to top, var(--green) 0 1px, transparent 1px 3px, var(--green) 3px 4px, var(--warning-soft) 4px)"
                : "linear-gradient(var(--warning-soft), var(--warning-soft))");
              positions.push(`${box.left - area.left + element.scrollLeft}px ${box.top - area.top + element.scrollTop}px`);
              dimensions.push(`${box.width}px ${box.height}px`);
            }
          }
          element.style.setProperty("--search-backgrounds", backgrounds.join(","));
          element.style.setProperty("--search-positions", positions.join(","));
          element.style.setProperty("--search-sizes", dimensions.join(","));
          element.classList.add("search-painted");
          painted.add(element);
          resize?.observe(element);
        }
      }
    }

    function announce(message) {
      clearTimeout(announcement);
      // Only announcements wait for a pause; the visible count updates at once.
      announcement = setTimeout(() => {
        if (!disposed) status.textContent = message;
      }, 250);
    }

    // The field shows the position in short, like a browser's find bar ("1/11", one left-to-right
    // run in both languages); the full sentence and the match's cue or page are announced.
    function showPosition() {
      const match = matches[active];
      const position = (template, isolate) => template
        .replace("{current}", isolate(active + 1))
        .replace("{total}", isolate(matches.length));
      count.textContent = `\u2066${position(toolbar.dataset.matchShort, String)}\u2069`;
      label.textContent = adapter.label(match.segment);
      announce(`${position(toolbar.dataset.matchCount, (number) => `\u2068${number}\u2069`)} — ${label.textContent}`);
    }

    async function reveal() {
      const current = generation;
      const request = ++navigation;
      const match = matches[active];
      if (!match) return;
      const element = await adapter.show(match.segment);
      if (disposed || current !== generation || request !== navigation) return;
      refresh();
      // Reveal the exact hit in the nearest scrolling viewer. Only where the viewer has no scroll
      // region of its own (as on phones) does the page scroll, as little as it takes, below the
      // toolbar that stays in view while searching.
      const range = rangeFor(element, match.start, match.end);
      if (!range) return;
      const box = range.getBoundingClientRect();
      for (let region = element.parentElement; region && region !== document.body; region = region.parentElement) {
        if (!/auto|scroll/.test(getComputedStyle(region).overflowY)) continue;
        const area = region.getBoundingClientRect();
        if (box.top < area.top || box.bottom > area.bottom) region.scrollTop += box.top - area.top - 12;
        return;
      }
      const bar = toolbar.closest(".result-toolbar") ?? toolbar;
      const top = getComputedStyle(bar).position === "sticky" ? Math.max(0, bar.getBoundingClientRect().bottom) : 0;
      if (box.top < top) window.scrollBy(0, box.top - top - 12);
      else if (box.bottom > window.innerHeight) window.scrollBy(0, box.bottom - window.innerHeight + 12);
    }

    function move(step) {
      if (!matches.length) return;
      active = (active + step + matches.length) % matches.length;
      showPosition();
      void reveal();
    }

    async function search() {
      const current = ++generation;
      navigation++;
      clearTimeout(announcement);
      status.textContent = "";
      matches = [];
      active = 0;
      clearHighlights();
      previous.disabled = next.disabled = true;
      label.textContent = "";
      const options = { ignoreMarks: marks.checked, ignoreTatweel: tatweel.checked };
      const query = comparison(field.value, options).value;
      if (!query.trim()) {
        count.textContent = "";
        return;
      }
      count.textContent = toolbar.dataset.searching;
      const key = `${options.ignoreMarks}:${options.ignoreTatweel}`;
      if (!index || indexOptions !== key) {
        const prepared = [];
        let sliceStart = performance.now();
        for (const text of adapter.texts) {
          prepared.push(comparison(text, options));
          if (performance.now() - sliceStart > 8) {
            await new Promise((resolve) => setTimeout(resolve, 0));
            if (disposed || current !== generation) return;
            sliceStart = performance.now();
          }
        }
        index = prepared;
        indexOptions = key;
      }
      if (disposed || current !== generation) return;
      matches = index.flatMap((buffer, segment) => matchesIn(buffer, query, segment));
      if (!matches.length) {
        count.textContent = toolbar.dataset.noMatches;
        announce(count.textContent);
        return;
      }
      previous.disabled = next.disabled = false;
      showPosition();
      void reveal();
    }

    toolbar.hidden = false;
    if (!segmenter) {
      for (const control of toolbar.querySelectorAll("input, button")) control.disabled = true;
      count.textContent = toolbar.dataset.unavailable;
    } else {
      const on = (element, type, listener) => element.addEventListener(type, listener, { signal: events.signal });
      // The matching options show while the user searches: from focusing the find bar until
      // focus or a press lands elsewhere. CSS keeps them shown while a query is present.
      // Presses count because Safari does not focus a checkbox it toggles; they close the
      // options only after their click, so nothing moves under the pointer.
      let pressedOutside = false;
      const close = () => { delete toolbar.dataset.searchOpen; };
      on(toolbar, "focusin", () => { toolbar.dataset.searchOpen = ""; });
      on(document, "focusin", (event) => {
        if (!pressedOutside && !toolbar.contains(event.target)) close();
      });
      on(document, "pointerdown", (event) => { pressedOutside = !toolbar.contains(event.target); });
      on(document, "pointerup", () => {
        if (pressedOutside) setTimeout(close);
        pressedOutside = false;
      });
      on(document, "pointercancel", () => { pressedOutside = false; });
      on(field, "input", () => void search());
      on(marks, "change", () => void search());
      on(tatweel, "change", () => void search());
      on(field, "keydown", (event) => {
        if (event.key === "Enter" && !event.isComposing) {
          event.preventDefault();
          move(event.shiftKey ? -1 : 1);
        }
      });
      on(previous, "click", () => move(-1));
      on(next, "click", () => move(1));
      on(clear, "click", () => {
        field.focus({ preventScroll: true });
        field.value = "";
        void search();
      });
      if (!customHighlights) {
        on(window, "resize", refresh);
        if (document.fonts) on(document.fonts, "loadingdone", refresh);
      }
      void search();
    }

    return {
      refresh,
      dispose() {
        disposed = true;
        generation++;
        navigation++;
        events.abort();
        clearTimeout(announcement);
        cancelAnimationFrame(repaintFrame);
        clearHighlights();
        index = null;
        matches = [];
      },
    };
  }

  app.search = { find, attach };
})();
