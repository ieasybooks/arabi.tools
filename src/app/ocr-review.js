// Page-level review of a browser-held source; recognition and exports stay on the server.
(() => {
  const app = (window.arabiTools ??= {});
  if (app.ocrReview) return;
  const adapters = (app.ocrReview = new WeakMap());
  const enhanced = new WeakSet();
  const active = new Map();
  const MAX_SIDE = 4096;
  const MAX_PIXELS = 8 * 1024 * 1024;

  function enhance(root) {
    const labels = JSON.parse(root.dataset.labels);
    const resources = JSON.parse(root.dataset.pdfResources);
    const find = (name) => root.querySelector(`[data-ocr-${name}]`);
    const texts = Array.from(root.querySelectorAll('[data-ocr-page-text]'), (node) => node.textContent);
    if (!texts.length) return;
    const pages = find('pages');
    const viewport = find('source-viewport');
    const textViewport = find('text-viewport');
    const pageField = find('page-number');
    const picker = find('source-picker');
    const fileField = find('source-file');
    const confirm = find('confirm');
    const sourceStatus = find('source-status');
    const sourceError = find('source-error');
    const copyStatus = root.querySelector('[data-copy-status]');
    const fallback = root.querySelector('[data-copy-fallback]');
    const events = new AbortController();
    const on = (node, event, handler) => node.addEventListener(event, handler, { signal: events.signal });
    const phrase = (key, values = {}) => labels[key].replace(/\{(\w+)\}/g, (_, name) => values[name]);
    const pageName = (page) => phrase('page_name', { current: page + 1 });
    let current = -1;
    let textElement;
    let source;
    let sourceVersion = 0;
    let renderVersion = 0;
    let renderTask;
    let renderQueue = Promise.resolve();
    let renderedPage;
    let canvas;
    let fit = 'width';
    let zoom = 1;
    let disposed = false;
    let search;
    let resizeFrame;

    function clearCanvas() {
      if (canvas) canvas.width = canvas.height = 0;
      canvas = null;
      viewport.replaceChildren();
      renderedPage?.cleanup();
      renderedPage = null;
      delete viewport.dataset.renderedPage;
    }

    function releaseSource() {
      sourceVersion++;
      renderVersion++;
      renderTask?.cancel();
      const old = source;
      source = null;
      delete root.dataset.sourceAttached;
      old?.abort.abort();
      if (old?.url) URL.revokeObjectURL(old.url);
      if (old?.image) old.image.removeAttribute('src');
      // The loading task owns its document and worker, including an unfinished load.
      if (old?.task) void old.task.destroy().catch(() => {});
      clearCanvas();
      viewport.hidden = true;
      find('zoom').hidden = true;
    }

    function previewFailure(message) {
      releaseSource();
      sourceStatus.textContent = labels.source_failed;
      sourceError.textContent = message;
      sourceError.hidden = false;
      picker.hidden = false;
      fileField.setAttribute('aria-invalid', 'true');
    }

    function updateZoom() {
      root.querySelectorAll('[data-ocr-fit]').forEach((button) => {
        button.setAttribute('aria-pressed', String(button.dataset.ocrFit === fit));
      });
      find('zoom-value').textContent = `${Math.round(zoom * 100)}%`;
      find('zoom-out').disabled = zoom <= 0.1;
      find('zoom-in').disabled = zoom >= 4;
    }

    function scale(width, height) {
      if (fit !== 'manual') {
        const style = getComputedStyle(viewport);
        const horizontalPadding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
        const verticalPadding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
        zoom = Math.max(0.001, (viewport.clientWidth - horizontalPadding) / width);
        if (fit === 'page') zoom = Math.min(zoom, Math.max(0.001, (viewport.clientHeight - verticalPadding) / height));
      }
      updateZoom();
      return zoom;
    }

    function render() {
      const version = ++renderVersion;
      const selected = current;
      const owner = source;
      renderTask?.cancel();
      // Hide the previous bitmap immediately: it must never be paired with another page's text.
      if (canvas) canvas.hidden = true;
      delete viewport.dataset.renderedPage;
      renderQueue = renderQueue.then(async () => {
        if (disposed || version !== renderVersion || !owner || !owner.ready) return;
        clearCanvas();
        // A hidden narrow-screen source pane is rendered when the user opens it.
        if (!viewport.clientWidth) return;
        try {
          if (owner.image) {
            const image = owner.image;
            const factor = scale(image.naturalWidth, image.naturalHeight);
            image.style.width = `${image.naturalWidth * factor}px`;
            image.style.height = `${image.naturalHeight * factor}px`;
            image.alt = pageName(selected);
            viewport.append(image);
          } else {
            const page = await owner.document.getPage(selected + 1);
            if (disposed || version !== renderVersion) { page.cleanup(); return; }
            renderedPage = page;
            const base = page.getViewport({ scale: 1 });
            const factor = scale(base.width, base.height);
            const pixels = Math.min(
              factor * (window.devicePixelRatio || 1),
              MAX_SIDE / base.width,
              MAX_SIDE / base.height,
              Math.sqrt(MAX_PIXELS / (base.width * base.height)),
            );
            const target = page.getViewport({ scale: pixels });
            canvas = document.createElement('canvas');
            canvas.width = Math.max(1, Math.floor(target.width));
            canvas.height = Math.max(1, Math.floor(target.height));
            canvas.style.width = `${base.width * factor}px`;
            canvas.style.height = `${base.height * factor}px`;
            canvas.setAttribute('role', 'img');
            canvas.setAttribute('aria-label', pageName(selected));
            viewport.append(canvas);
            renderTask = page.render({ canvasContext: canvas.getContext('2d'), viewport: target });
            await renderTask.promise;
            renderTask = null;
          }
          if (disposed || version !== renderVersion) return;
          viewport.dataset.renderedPage = String(selected + 1);
          sourceStatus.textContent = labels[owner.local ? 'source_local' : 'source_browser'];
        } catch (error) {
          renderTask = null;
          if (disposed || version !== renderVersion || error.name === 'RenderingCancelledException') return;
          previewFailure(labels.source_failed);
        }
      });
    }

    function pageError(message = '') {
      find('page-error').textContent = message;
      find('page-error').hidden = !message;
      pageField.setAttribute('aria-invalid', String(Boolean(message)));
    }

    function setPage(page, fromSearch = false) {
      if (!Number.isInteger(page) || page < 0 || page >= texts.length || disposed) return null;
      pageError();
      pageField.value = String(page + 1);
      if (current === page) return textElement;
      current = page;
      root.dataset.currentPage = String(page + 1);
      const item = document.createElement('li');
      item.className = 'ocr-page';
      const heading = document.createElement('h3');
      heading.textContent = pageName(page);
      textElement = document.createElement('p');
      textElement.className = 'ocr-page-text';
      textElement.dataset.ocrPageText = '';
      textElement.dir = 'auto';
      textElement.textContent = texts[page];
      item.append(heading, textElement);
      if (!texts[page].trim()) {
        const empty = document.createElement('p');
        empty.className = 'ocr-no-text';
        empty.textContent = root.dataset.noText;
        item.append(empty);
      }
      pages.replaceChildren(item);
      textViewport.scrollTop = 0;
      viewport.scrollTop = viewport.scrollLeft = 0;
      find('previous').disabled = page === 0;
      find('next').disabled = page === texts.length - 1;
      find('position').textContent = phrase('page_position', { current: page + 1, total: texts.length });
      render();
      if (!fromSearch) search?.refresh();
      return textElement;
    }

    function setView(view) {
      root.dataset.view = view;
      root.querySelector(`.ocr-view-choice input[value="${view}"]`).checked = true;
      if (view === 'source') render();
    }

    const adapter = {
      texts,
      show(page) {
        setView('text');
        const element = setPage(page, true);
        return element ? Promise.resolve(element) : Promise.reject(new RangeError('Invalid OCR page'));
      },
      element: (page) => page === current && !disposed ? textElement : null,
      label: pageName,
    };
    adapters.set(root, adapter);
    setView('text');
    setPage(0);
    const toolbar = find('search-slot')?.firstElementChild;
    if (app.search && toolbar) search = app.search.attach(toolbar, adapter);
    find('controls').hidden = false;
    picker.hidden = false;
    root.dataset.enhanced = 'true';

    function submitPage() {
      // Both Arabic digit sets are accepted, alongside the displayed ASCII numbers.
      const value = pageField.value.trim().replace(/[٠-٩۰-۹]/g, (digit) => String(digit.charCodeAt(0) - (digit <= '٩' ? 0x660 : 0x6f0)));
      const number = /^\d+$/.test(value) ? Number(value) : NaN;
      if (!Number.isInteger(number) || number < 1 || number > texts.length) {
        pageError(phrase('page_error', { total: texts.length }));
        return;
      }
      setPage(number - 1);
    }
    on(find('navigation'), 'submit', (event) => { event.preventDefault(); submitPage(); });
    on(pageField, 'change', submitPage);
    on(find('previous'), 'click', () => setPage(current - 1));
    on(find('next'), 'click', () => setPage(current + 1));
    root.querySelectorAll('.ocr-view-choice input').forEach((radio) => on(radio, 'change', () => setView(radio.value)));
    root.querySelectorAll('[data-ocr-fit]').forEach((button) => on(button, 'click', () => { fit = button.dataset.ocrFit; render(); }));
    for (const [name, factor] of [['zoom-out', 1 / 1.25], ['zoom-in', 1.25]]) {
      on(find(name), 'click', () => { fit = 'manual'; zoom = Math.min(4, Math.max(0.1, zoom * factor)); render(); });
    }

    async function attach(file, local) {
      if (source && source.file === file && source.local === local) return;
      releaseSource();
      sourceError.hidden = true;
      fileField.removeAttribute('aria-invalid');
      confirm.removeAttribute('aria-invalid');
      if (!file) {
        sourceStatus.textContent = labels.source_missing;
        picker.hidden = false;
        return;
      }
      const owner = source = { file, local, abort: new AbortController() };
      const version = sourceVersion;
      sourceStatus.textContent = labels.source_loading;
      const stale = () => disposed || version !== sourceVersion;
      try {
        const bytes = new Uint8Array(await file.slice(0, 8).arrayBuffer());
        if (stale()) return;
        const extension = file.name.split('.').pop().toLowerCase();
        const pdf = extension === 'pdf' && String.fromCharCode(...bytes.subarray(0, 5)) === '%PDF-';
        const png = extension === 'png' && bytes.join(',') === '137,80,78,71,13,10,26,10';
        const jpeg = ['jpg', 'jpeg'].includes(extension) && bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255;
        if (!pdf && !png && !jpeg) { previewFailure(root.dataset.wrongType); return; }
        let count = 1;
        if (pdf) {
          const library = await import(root.dataset.pdfModule);
          if (stale()) return;
          library.GlobalWorkerOptions.workerSrc = root.dataset.pdfWorker;
          const data = new Uint8Array(await file.arrayBuffer());
          if (stale()) return;
          owner.task = library.getDocument({
            data,
            useWorkerFetch: false,
            // PDF.js asks for image decoders by filename. Resolve those requests through
            // Topcoat's content-hashed local bundle. Fonts a PDF does not embed use system fonts.
            BinaryDataFactory: class {
              async fetch({ filename }) {
                const url = resources[filename];
                if (!url) throw new Error('Unknown PDF resource');
                const response = await fetch(url, { signal: owner.abort.signal });
                if (!response.ok) throw new Error('PDF resource unavailable');
                return new Uint8Array(await response.arrayBuffer());
              }
            },
            canvasMaxAreaInBytes: MAX_PIXELS * 4,
            stopAtErrors: true,
          });
          owner.document = await owner.task.promise;
          if (stale()) return;
          count = owner.document.numPages;
        } else {
          owner.url = URL.createObjectURL(file);
          owner.image = new Image();
          owner.image.src = owner.url;
          await owner.image.decode();
          if (stale()) return;
        }
        if (count !== texts.length) {
          previewFailure(phrase('source_mismatch', { actual: count, total: texts.length }));
          return;
        }
        owner.ready = true;
        root.dataset.sourceAttached = 'true';
        const restoreFocused = picker.contains(document.activeElement);
        picker.hidden = true;
        // The File is retained by this controller; clear the picker reference after attachment.
        fileField.value = '';
        confirm.checked = false;
        sourceStatus.textContent = labels[local ? 'source_local' : 'source_browser'];
        viewport.hidden = false;
        find('zoom').hidden = false;
        if (restoreFocused) viewport.focus({ preventScroll: true });
        render();
      } catch (_) {
        if (!stale()) previewFailure(labels.source_failed);
      }
    }
    on(picker, 'submit', (event) => {
      event.preventDefault();
      if (!fileField.files[0] || !confirm.checked) {
        sourceError.textContent = labels.source_required;
        sourceError.hidden = false;
        fileField.setAttribute('aria-invalid', String(!fileField.files[0]));
        confirm.setAttribute('aria-invalid', String(!confirm.checked));
        return;
      }
      void attach(fileField.files[0], true);
    });
    on(document, 'arabi:source', (event) => {
      if (event.detail?.jobId === root.dataset.jobId) void attach(app.sources?.get(root.dataset.jobId), false);
    });
    void attach(app.sources?.get(root.dataset.jobId), false);

    const copy = text => app.copyText(text, { status: copyStatus, fallback });
    on(find('copy-page'), 'click', () => void copy(texts[current]));
    on(find('copy-all'), 'click', () => void copy(texts.join('\n\n')));

    let lastSize = '';
    const resize = new ResizeObserver(() => {
      const size = `${viewport.clientWidth}:${viewport.clientHeight}`;
      if (size === lastSize) return;
      lastSize = size;
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(render);
    });
    resize.observe(viewport);
    active.set(root, () => {
      disposed = true;
      events.abort();
      resize.disconnect();
      cancelAnimationFrame(resizeFrame);
      search?.dispose();
      releaseSource();
      fallback.replaceChildren();
      fileField.value = '';
      adapters.delete(root);
    });
  }

  function scan() {
    for (const [root, dispose] of active) {
      if (!root.isConnected) { dispose(); active.delete(root); }
    }
    document.querySelectorAll('[data-result="ocr"]').forEach((root) => {
      if (enhanced.has(root)) return;
      enhanced.add(root);
      enhance(root);
    });
  }
  new MutationObserver(scan).observe(document.documentElement, { childList: true, subtree: true });
  scan();
})();
