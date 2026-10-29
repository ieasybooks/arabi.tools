# Deploy

The production stack runs the app and Cloudflare Tunnel with Docker Compose. The app has no published host ports; Cloudflare forwards the chosen public hostname to `http://app:3000`. Releases publish `ghcr.io/ieasybooks/arlang.tools:<version>` and `:latest`, then deploy the versioned image over SSH. The image currently targets Linux x86-64 servers.

## Set up once

The server only needs Docker. Everything else lives in the repository's **production** environment on GitHub, and every deploy writes the server's `.env` from it, so changing a setting means editing a secret and running the Deploy workflow again.

1. Install [Docker Engine and the Compose plugin](https://docs.docker.com/engine/install/) on a Linux x86-64 server, and keep at least 10 GB free for uploads in progress. Uploads live in the container's disk-backed `/tmp`. The deploy user needs Docker access, which gives it control of the host.
2. In the Cloudflare dashboard, create a named Cloudflare Tunnel with the Docker connector and copy its token. Its wizard waits for a connector: run the `docker run …` command it shows on any machine until the tunnel shows as healthy, then stop it. Add a published application route from your hostname to service URL `http://app:3000`. Cloudflare manages the hostname's DNS and HTTPS; no domain is stored in the repository.
3. Generate a key for deployments only, with no passphrase, and add its public half to the deploy user's `~/.ssh/authorized_keys`: `ssh-keygen -t ed25519 -N "" -C arlang-deploy -f ./arlang-deploy`. Collect the server's SSH host keys with `ssh-keyscan -p PORT HOST` and check them against a fingerprint you trust; keep the `[HOST]:PORT` form for a nonstandard port.
4. In the repository's **production** environment, limit deployments to the `main` branch and `v*` tags, and add:

   | Kind | Name | Value |
   | --- | --- | --- |
   | Secret | `DEPLOY_HOST` | Server hostname or IP address |
   | Secret | `DEPLOY_USER` | SSH user with Docker access |
   | Secret | `DEPLOY_PORT` | SSH port, usually `22` |
   | Secret | `DEPLOY_SSH_KEY` | The private key from step 3 |
   | Secret | `DEPLOY_KNOWN_HOSTS` | The checked host-key lines from step 3 |
   | Secret | `WIT_TOKENS_AR` | Comma-separated Wit tokens of Arabic apps |
   | Secret | `DRIVE_SERVICE_ACCOUNTS` | JSON array of Google Drive service-account key objects (`type`, `client_email`, `private_key`); each account needs Drive storage |
   | Secret | `TUNNEL_TOKEN` | The tunnel token from step 2 |
   | Variable | `DEPLOY_DIRECTORY` | Absolute directory for the deployment, such as `/opt/arlang.tools`; letters, numbers, dots, dashes, underscores and slashes only |

5. Enable **Settings → Actions → General → Allow GitHub Actions to create and approve pull requests** for release-please. The image job pushes to GHCR with the workflow's own token, and the server logs in with a short-lived token for each deploy, so no registry credentials are stored anywhere.

No inbound HTTP or HTTPS ports are needed. The server needs outbound DNS and HTTPS for image pulls, Wit, Google OAuth token exchange and Google Drive, and Cloudflare Tunnel's outbound port `7844` over UDP or TCP; see [Cloudflare's tunnel firewall requirements](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/tunnel-with-firewall/). The app reads the visitor's address from `CF-Connecting-IP`, which it trusts only because the tunnel is the only way in.

To change a token, update its secret and run **Actions → Deploy → Run workflow** with the current release tag. A new transcription language needs its own `WIT_TOKENS_<LANG>` secret and a line in `deploy.yml` that writes it, alongside the code change adding the language. To move servers, change the `DEPLOY_*` secrets and deploy again; the workflow creates the directory.

## Cut a release

Use Conventional Commits and matching squash-merge titles: `fix:` bumps the patch version, `feat:` bumps the minor version, and `feat!:` or a `BREAKING CHANGE:` footer marks incompatible changes. Before 1.0, breaking changes also bump the minor version. The release baseline is `0.1.0`; tags are `vX.Y.Z`.

After changes reach `main`, release-please opens or updates a release PR with the crate version, lockfile, changelog, and release notes. Review the diff and checks, then merge that release PR. If GitHub asks for approval of bot-created PR checks, approve them before merging. release-please creates the GitHub release and explicitly dispatches `deploy.yml` on its tag because releases created with `GITHUB_TOKEN` do not trigger release-event workflows. Nothing is published to crates.io.

A release published by a user also triggers `deploy.yml`. The workflow checks out the exact tag, verifies it matches `Cargo.toml`, builds and pushes both version and `latest` tags, copies `compose.yaml` and `deploy/deploy.sh` into the configured directory, pulls images, and waits for the app's `/health` check to pass. The tunnel starts after the app is healthy; check the chosen public hostname after the first deployment to verify the Cloudflare configuration too.

To retry a release, run **Actions → Deploy → Run workflow** with its `vX.Y.Z` tag, or use:

```sh
gh workflow run deploy.yml --ref main -f tag=v0.1.0
```

Release builds and deployments are serialized. The workflow rewrites `.env` from the environment's secrets, and the server script stores the selected version in `.env.image`. It pulls before stopping the running app, so a failed pull leaves the current service and selection unchanged. If startup or health fails after replacement, the workflow fails; inspect the server and choose an older version explicitly. There is no automatic rollback.

For server diagnostics, run these commands from the deployment directory. Avoid printing the rendered Compose configuration because it includes credentials.

```sh
docker compose --env-file .env --env-file .env.image ps
docker compose --env-file .env --env-file .env.image logs --tail 100 app cloudflared
```

## Roll back

Run the Deploy workflow with an older release tag as above. It rebuilds that tag, updates its versioned image and `latest`, and installs that version's Compose file and deploy script. The tag must exist and match the crate version at that tag.

For a faster rollback to an already published image, log in to the server, change to the deployment directory, and run `sh deploy/deploy.sh 0.1.0`. For a private GHCR package, first authenticate with a GitHub user/token that has package read access using `docker login ghcr.io --username YOUR_GITHUB_USER --password-stdin`, then `docker logout ghcr.io` after the rollback. This path keeps the current Compose file and script; use the workflow when the older release also needs its older deployment files. The script accepts a `v` prefix too and does not change the registry's `latest` tag.

## Jobs during a deploy

The old container receives SIGTERM. It stops accepting new uploads, cancels reservations and queued jobs, and gives running transcriptions up to 270 seconds to finish. OCR stops starting pages and waits for active Drive calls to finish and delete their documents. Compose allows five minutes before forcing the old container to stop. The new container then starts, and deployment waits for health before reporting success. Expect a short interruption while it is replaced; this is a single app instance.

Jobs and finished results are held in memory and do not survive replacement. Users should download finished results before a deploy and upload unfinished files again afterward. Temporary upload files disappear with the old container. The deploy script does not prune other images or remove host data, and it removes only its temporary registry credentials and temporary version file.

## OCR runtime and credentials

The browser OCR preview bundles [PDF.js and its rendering resources](../src/app/vendor/pdfjs/README.md) through Topcoat alongside the binary, including its same-origin worker; it needs no additional host, endpoint, or retained upload. Container checks cover JavaScript/CSS/vendor changes and verify the viewer module, worker MIME types and rendering resources.

The runtime image includes PDFium chromium/8111 beside the app binary, with its `LICENSE` and third-party `licenses/` under `/usr/share/doc/pdfium`. `deploy/pdfium.sh` verifies the upstream archive against tahweel's pinned SHA-256 before extracting it. The script supports Linux x86-64 and arm64; the release workflow currently builds x86-64.

Store `DRIVE_SERVICE_ACCOUNTS` as a JSON array, not a JSON string containing an array. The private key's newlines must be JSON `\n` escapes. The workflow's `deploy/write-env.py` validates and compacts the JSON, then writes it as a single-quoted Compose value. Single quotes preserve JSON backslashes and dollar signs through both `--env-file` and the service's `env_file`; apostrophes and dollar signs inside JSON strings become `\u0027` and `\u0024`. Key contents survive unchanged. Do not source `.env` as a shell script or print the resolved Compose configuration. `python3 tests/deploy_env.py` checks this path with generated dummy values and the installed Docker Compose parser.

The app initializes one shared Drive recognizer using all configured accounts, automatic language detection and tahweel's default eight page operations per account. Startup converts, exports and deletes one blank page to verify storage is usable. The site serves transcription while this check runs; OCR becomes available when it completes. Missing or invalid credentials, or no account with storage, leave OCR temporarily unavailable while transcription continues. For development, `TAHWEEL_PDFIUM` may point to the shared library explicitly.

The tools run four jobs at once in total, and each tool runs at most three of them, so a busy tool leaves a slot available to other tools. When a slot frees, the job that has waited longest starts, unless its tool already runs three. Each tool accepts 100 jobs, waiting and running, and one active job per IP address. OCR accepts one PDF, JPG/JPEG or PNG up to 99,000,000 bytes and 1,000 pages, checks the count before queueing and again during conversion, and retains text previews and TXT/DOCX/JSON downloads for 30 minutes. Upload directories are removed when jobs end. Pages are rendered on one blocking thread per running job. Closing the page cancels an abandoned job after one minute; active Drive calls finish and delete their files before the slot is released. During shutdown OCR stops starting pages immediately and waits for active calls to clean up. Docker's five-minute forced stop still applies if the upstream does not finish in time.

## Diacritization runtime and models

Diacritization uses the exact released [`tashkeel` 0.1.0](https://github.com/ieasybooks/tashkeel-rs/releases/tag/v0.1.0) Rust library and the official CPU ONNX Runtime 1.31.0 shared library (API 31). `deploy/onnxruntime.sh` pins the Linux x86-64 and ARM64 release archives by byte count and SHA-256. It installs `libonnxruntime.so`, `LICENSE`, and `ThirdPartyNotices.txt`. The release image still targets x86-64; an ARM64 image build can use the same helper with `arm64`.

`deploy/tashkeel-models.sh` downloads the official [Shakkelha sigmoid conversion from models-v1](https://github.com/ieasybooks/tashkeel-rs/releases/tag/models-v1) and the upstream [CATT v2 encoder-only archive](https://github.com/abjadai/catt/releases/tag/v2). Their byte counts and SHA-256 digests match the 0.1.0 crate's model catalog. It checks the CATT archive before extracting its two named files, then verifies every extracted model. Reusing a cache also verifies every file; a corrupt cache fails without replacing its files. Use a fresh cache directory to repair it. Completed model directories are published only after verification. Keep the cache on persistent disk and share it between both models and all jobs; it contains about 89 MB of weights.

The container build provisions everything before the image is published. It sets `TASHKEEL_MODEL_CACHE=/app/models` and `ORT_DYLIB_PATH=/app/libonnxruntime.so`, and keeps models readable by the unprivileged app user. The shared cache has this layout:

```text
/app/models/
  shakkelha/v1/model.onnx
  catt/v1/encoder.onnx
  catt/v1/decoder.onnx
```

No new production secrets or credentials are required. Docker image layers retain the models across container replacement; no writable model volume is needed. A custom model volume must already contain the verified layout above, because mounting an empty directory over `/app/models` hides the packaged files. Model and library notices are retained under `/usr/share/doc/tashkeel`, including Shakkelha's MIT license and CATT's Apache-2.0 license; the native runtime's MIT license and third-party notices are under `/usr/share/doc/onnxruntime`.

The app verifies these local files and initializes the models at startup. If the cache is unset, missing or corrupt, or the runtime fails to load, the site still starts and diacritization shows a clear temporarily unavailable message in Arabic and English. Other configured tools continue working. `ORT_DYLIB_PATH` is optional when the library is beside the executable, but an explicit absolute path is recommended for local development. Initialization and inference never download model files, and no Python inference process is used. After repairing missing files, restart the app to initialize them.

For local Linux development, install `curl`, `tar`, `unzip`, and the standard coreutils tools, then provision once outside the request path. Choose `arm64` instead of `amd64` on ARM64 hosts:

```sh
sh deploy/onnxruntime.sh amd64 "$PWD/target/onnxruntime"
sh deploy/tashkeel-models.sh "$PWD/target/tashkeel-models"
export ORT_DYLIB_PATH="$PWD/target/onnxruntime/libonnxruntime.so"
export TASHKEEL_MODEL_CACHE="$PWD/target/tashkeel-models"
topcoat dev
```

Run ordinary checks with `cargo test --locked`; they do not require a native runtime or weights. With the environment above, run all opt-in unit and integration tests against real downloaded files:

```sh
cargo test --locked -- --ignored --test-threads=1
```

CI runs the ordinary tests first, provisions and verifies both models and the runtime, then runs these ignored tests for deterministic references and text/document preservation. The browser job waits for those checks, verifies its restored model cache again, and runs the Arabic and English diacritization flow with the real models. Its separate unavailable server omits the model environment. Wit and OCR remain mocked in browser tests. The container workflow also submits a text job to each packaged model and checks its download through the built image's HTTP interface. No live credentials are needed for these checks.

Diacritization shares the existing four-job global queue and three-job per-tool maximum. File uploads keep the 99,000,000-byte limit and completed results expire after 30 minutes. The model cache contains immutable weights, not uploaded text or document results; those remain subject to the normal job cleanup and deployment lifecycle above.

Cancellation requires the signed cookie issued to the browser that reserved the job. The signing key is generated at startup and lives as long as the in-memory jobs; it requires no additional deployment secret. A result URL alone does not authorize cancellation.

The 99 MB upload limit is separate from DOCX processing limits. The pinned library bounds XML parts to 8 MiB, paragraphs to 1 MiB, individual decompressed ZIP entries to 32 MiB, and the decompressed package to 256 MiB; it also rejects Word structures it cannot preserve. These bounds apply during validation and after adding diacritics, so a valid upload can still exceed an output limit. The interface distinguishes unsupported Word content from size/complexity limits and suggests saving a simpler document or splitting it into smaller documents and paragraphs.
