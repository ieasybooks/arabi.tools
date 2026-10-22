#!/bin/sh
# Usage: deploy/deploy.sh VERSION [REGISTRY_USER]
# With REGISTRY_USER, read a GHCR token from stdin. Otherwise use the server's login.
set -eu
umask 077
cd "$(dirname "$0")/.."

version=${1:?Pass a released version, for example 0.1.0}
version=${version#v}
if ! printf '%s\n' "$version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
    echo 'Choose a released version such as 0.1.0.' >&2
    exit 1
fi
if [ ! -f .env ]; then
    echo 'Create .env with WIT_TOKENS_AR and TUNNEL_TOKEN first.' >&2
    exit 1
fi

registry_user=${2:-}
registry_config=
cleanup() {
    rm -f .env.image.next
    if [ -n "$registry_config" ]; then
        docker logout ghcr.io > /dev/null 2>&1 || true
        rm -rf "$registry_config"
    fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if [ -n "$registry_user" ]; then
    registry_config=$(mktemp -d "$PWD/.docker-login.XXXXXX")
    export DOCKER_CONFIG="$registry_config"
    docker login ghcr.io --username "$registry_user" --password-stdin > /dev/null
fi

# Keep credentials in .env; only the image selection changes between releases.
unset ARLANG_TAG
printf 'ARLANG_TAG=%s\n' "$version" > .env.image.next
# A failed pull leaves the current containers and selected version untouched.
docker compose --env-file .env --env-file .env.image.next pull --quiet
mv .env.image.next .env.image

# Five minutes for the old app to drain, plus two minutes for startup and health.
docker compose --env-file .env --env-file .env.image up --detach --wait --wait-timeout 420
docker compose --env-file .env --env-file .env.image ps
printf 'Deployed arlang.tools %s; app health check passed.\n' "$version"
