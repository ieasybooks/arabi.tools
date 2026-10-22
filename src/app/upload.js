// Uploads the chosen file with a progress bar, after reserving a place in the queue so a
// full queue is reported before any time is spent uploading. Messages come from the form's
// data attributes, already in the page's language.

const form = document.getElementById("upload-form");

if (form) {
  const input = form.elements.file;
  const button = form.querySelector("button[type=submit]");
  const progress = form.querySelector(".upload-progress");
  const bar = progress.querySelector("progress");
  const error = form.querySelector(".error");
  const extensions = form.dataset.extensions.split(",");
  const maxBytes = Number(form.dataset.maxBytes);

  const problemWith = (file) => {
    if (!file) return form.dataset.missing;
    const dot = file.name.lastIndexOf(".");
    const extension = dot > 0 ? file.name.slice(dot + 1).toLowerCase() : "";
    if (!extensions.includes(extension)) return form.dataset.wrongType;
    if (file.size > maxBytes) return form.dataset.tooLarge;
    return null;
  };

  const showError = (message) => {
    error.textContent = message;
    error.hidden = false;
    progress.hidden = true;
    button.disabled = false;
    input.disabled = false;
  };

  // Sends a POST and resolves with its JSON reply, or rejects with a message to show.
  const post = (url, body, onProgress) =>
    new Promise((resolve, reject) => {
      const request = new XMLHttpRequest();
      request.open("POST", url);
      request.responseType = "json";
      if (onProgress) {
        request.upload.onprogress = (event) => {
          if (event.lengthComputable) onProgress(event.loaded / event.total);
        };
      }
      request.onload = () => {
        const reply = request.response;
        if (reply && reply.error) reject(reply.error);
        else if (request.status === 413) reject(form.dataset.tooLarge);
        else if (request.status < 300 && reply) resolve(reply);
        else reject(form.dataset.failed);
      };
      request.onerror = () => reject(form.dataset.failed);
      request.send(body);
    });

  input.addEventListener("change", () => {
    const problem = problemWith(input.files[0]);
    if (problem) showError(problem);
    else error.hidden = true;
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const file = input.files[0];
    const problem = problemWith(file);
    if (problem) return showError(problem);

    error.hidden = true;
    button.disabled = true;
    bar.value = 0;
    progress.hidden = false;
    try {
      const reservation = await post(form.dataset.reserve, null);
      const data = new FormData();
      if (form.elements.language) data.append("language", form.elements.language.value);
      data.append("file", file);
      input.disabled = true;
      const result = await post(reservation.upload, data, (done) => {
        bar.value = Math.round(done * 100);
      });
      window.location.assign(result.next);
    } catch (message) {
      showError(typeof message === "string" ? message : form.dataset.failed);
    }
  });
}
