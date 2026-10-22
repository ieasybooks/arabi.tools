# Deploy

The production stack runs the app and Cloudflare Tunnel with Docker Compose. The app has no published host ports; Cloudflare forwards the chosen public hostname to `http://app:3000`. Releases publish `ghcr.io/ieasybooks/arlang.tools:<version>` and `:latest`, then deploy the versioned image over SSH. The image currently targets Linux x86-64 servers.

## Set up the server once

1. Install [Docker Engine and the Compose plugin](https://docs.docker.com/engine/install/) on a Linux x86-64 server. Give the deploy user access to Docker and verify `docker version` and `docker compose version` as that user. Docker access gives the user control of the host.
2. Choose an absolute deployment directory owned by that user, for example `/home/deploy/arlang.tools`. Create it with `install -d -m 700 /home/deploy/arlang.tools`. Keep at least 10 GB of disk space available for temporary uploads, plus room for images and audio processing. Uploads live in the container's disk-backed `/tmp`, not a RAM-backed tmpfs or a persistent host volume.
3. In the Cloudflare dashboard, create a named Cloudflare Tunnel using the Docker connector. Copy its tunnel token. Add a public hostname of your choice with service type **HTTP** and URL **app:3000** (`http://app:3000`). Cloudflare manages the hostname's DNS and HTTPS; no domain is embedded in the repository.
4. Create `/home/deploy/arlang.tools/.env` with mode `600`, owned by the deploy user. The app checks that each Wit token belongs to an app in the corresponding language at startup. Keep the real values on the server; the deploy workflow preserves this file.

   ```dotenv
   WIT_TOKENS_AR=first-arabic-wit-token,second-arabic-wit-token
   TUNNEL_TOKEN=cloudflare-tunnel-token
   RUST_LOG=info
   ```

   Add another `WIT_TOKENS_<LANG>` variable for each additional transcription language, for example `WIT_TOKENS_EN`. Compose fixes `HOST=0.0.0.0`, `PORT=3000`, `TMPDIR=/tmp`, and `CLIENT_IP_HEADER=CF-Connecting-IP`. The app trusts the nearest proxy for that visitor-IP header, so keep the app reachable only through the tunnel.

5. Allow SSH on the server's chosen port and restrict it as appropriate for your GitHub Actions runners and administrators. No inbound HTTP or HTTPS ports are needed. Allow outbound DNS and HTTPS for image pulls and Wit, and Cloudflare Tunnel's outbound port `7844` over UDP (QUIC) or TCP (HTTP/2). See [Cloudflare's tunnel firewall requirements](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/tunnel-with-firewall/).
6. On a trusted administrator machine, generate a new key specifically for deployments: `ssh-keygen -t ed25519 -f ./arlang-deploy -C arlang-deploy`. For unattended Actions access, leave the passphrase empty. Add the public key to the deploy user's `~/.ssh/authorized_keys` with `~/.ssh` mode `700` and `authorized_keys` mode `600`. The private key goes only into the GitHub environment secret below.
7. Obtain the server's SSH host-key fingerprint from its console or administrator. Use `ssh-keyscan -p PORT HOST` to collect the known-hosts entry, verify its fingerprint against that trusted value, and save the verified entry as `DEPLOY_KNOWN_HOSTS`. For a nonstandard SSH port, retain the `[HOST]:PORT` form. The workflow requires strict host-key checking.
8. Create the repository's **production** environment in GitHub and configure the following secrets and variable. Apply any desired deployment approval rules there. Moving servers only requires updating these settings and copying the server's `.env` securely.

   | Kind | Name | Value |
   | --- | --- | --- |
   | Secret | `DEPLOY_HOST` | Server hostname or IP address |
   | Secret | `DEPLOY_USER` | SSH user with Docker and directory access |
   | Secret | `DEPLOY_PORT` | SSH port, usually `22` |
   | Secret | `DEPLOY_SSH_KEY` | Complete private key generated above |
   | Secret | `DEPLOY_KNOWN_HOSTS` | Verified SSH known-hosts entry |
   | Variable | `DEPLOY_DIRECTORY` | Absolute directory, such as `/home/deploy/arlang.tools`; use only letters, numbers, dots, dashes, underscores, and slashes |

9. Enable **Settings → Actions → General → Allow GitHub Actions to create and approve pull requests** for release-please. The image workflow uses `GITHUB_TOKEN` to push to GHCR, and the deploy job logs in on the server using its own short-lived token with package read permission. No permanent registry token is needed for automated deployments. If the GHCR package already exists, grant this repository Actions access to it in the package settings.

## Cut a release

Use Conventional Commits and matching squash-merge titles: `fix:` bumps the patch version, `feat:` bumps the minor version, and `feat!:` or a `BREAKING CHANGE:` footer marks incompatible changes. Before 1.0, breaking changes also bump the minor version. The release baseline is `0.1.0`; tags are `vX.Y.Z`.

After changes reach `main`, release-please opens or updates a release PR with the crate version, lockfile, changelog, and release notes. Review the diff and checks, then merge that release PR. If GitHub asks for approval of bot-created PR checks, approve them before merging. release-please creates the GitHub release and explicitly dispatches `deploy.yml` on its tag because releases created with `GITHUB_TOKEN` do not trigger release-event workflows. Nothing is published to crates.io.

A release published by a user also triggers `deploy.yml`. The workflow checks out the exact tag, verifies it matches `Cargo.toml`, builds and pushes both version and `latest` tags, copies `compose.yaml` and `deploy/deploy.sh` into the configured directory, pulls images, and waits for the app's `/health` check to pass. The tunnel starts after the app is healthy; check the chosen public hostname after the first deployment to verify the Cloudflare configuration too.

To retry a release, run **Actions → Deploy → Run workflow** with its `vX.Y.Z` tag, or use:

```sh
gh workflow run deploy.yml --ref main -f tag=v0.1.0
```

Release builds and deployments are serialized. The server script stores the selected version in `.env.image`, leaving `.env` intact. It pulls before stopping the running app, so a failed pull leaves the current service and selection unchanged. If startup or health fails after replacement, the workflow fails; inspect the server and choose an older version explicitly. There is no automatic rollback.

For server diagnostics, run these commands from the deployment directory. Avoid printing the rendered Compose configuration because it includes credentials.

```sh
docker compose --env-file .env --env-file .env.image ps
docker compose --env-file .env --env-file .env.image logs --tail 100 app cloudflared
```

## Roll back

Run the Deploy workflow with an older release tag as above. It rebuilds that tag, updates its versioned image and `latest`, and installs that version's Compose file and deploy script. The tag must exist and match the crate version at that tag.

For a faster rollback to an already published image, log in to the server, change to the deployment directory, and run `sh deploy/deploy.sh 0.1.0`. For a private GHCR package, first authenticate with a GitHub user/token that has package read access using `docker login ghcr.io --username YOUR_GITHUB_USER --password-stdin`, then `docker logout ghcr.io` after the rollback. This path keeps the current Compose file and script; use the workflow when the older release also needs its older deployment files. The script accepts a `v` prefix too and does not change the registry's `latest` tag.

## Jobs during a deploy

The old container receives SIGTERM. It stops accepting new uploads, cancels reservations and queued jobs, and gives running jobs up to 270 seconds to finish. Compose allows five minutes before forcing the old container to stop. The new container then starts, and deployment waits for health before reporting success. Expect a short interruption while it is replaced; this is a single app instance.

Jobs and finished transcripts are held in memory and do not survive replacement. Users should download finished transcripts before a deploy and upload unfinished files again afterward. Temporary upload files disappear with the old container. The deploy script does not prune other images or remove host data, and it removes only its temporary registry credentials and temporary version file.
