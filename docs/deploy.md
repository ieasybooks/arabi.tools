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

The runtime image includes PDFium chromium/8111 beside the app binary, with its `LICENSE` and third-party `licenses/` under `/usr/share/doc/pdfium`. `deploy/pdfium.sh` verifies the upstream archive against tahweel's pinned SHA-256 before extracting it. The script supports Linux x86-64 and arm64; the release workflow currently builds x86-64.

Store `DRIVE_SERVICE_ACCOUNTS` as a JSON array, not a JSON string containing an array. The private key's newlines must be JSON `\n` escapes. The workflow's `deploy/write-env.py` validates and compacts the JSON, then writes it as a single-quoted Compose value. Single quotes preserve JSON backslashes and dollar signs through both `--env-file` and the service's `env_file`; apostrophes and dollar signs inside JSON strings become `\u0027` and `\u0024`. Key contents survive unchanged. Do not source `.env` as a shell script or print the resolved Compose configuration. `python3 tests/deploy_env.py` checks this path with generated dummy values and the installed Docker Compose parser.

The app initializes one shared Drive recognizer using all configured accounts, automatic language detection and tahweel's default eight page operations per account. Startup converts, exports and deletes one blank page to verify storage is usable. The site serves transcription while this check runs; OCR becomes available when it completes. Missing or invalid credentials, or no account with storage, leave OCR temporarily unavailable while transcription continues. For development, `TAHWEEL_PDFIUM` may point to the shared library explicitly.

The tools run four jobs at once in total, and each tool runs at most three of them, so a busy tool always leaves a slot for the other. When a slot frees, the job that has waited longest starts, unless its tool already runs three. Each tool accepts 100 jobs, waiting and running, and one active job per IP address. OCR accepts one PDF, JPG/JPEG or PNG up to 99,000,000 bytes and 1,000 pages, checks the count before queueing and again during conversion, and retains text previews and TXT/DOCX/JSON downloads for 30 minutes. Upload directories are removed when jobs end. Pages are rendered on one blocking thread per running job. Closing the page cancels an abandoned job after one minute; active Drive calls finish and delete their files before the slot is released. During shutdown OCR stops starting pages immediately and waits for active calls to clean up. Docker's five-minute forced stop still applies if the upstream does not finish in time.
