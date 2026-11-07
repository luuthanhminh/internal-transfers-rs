#!/usr/bin/env bash
# Deploys one signed release image on this host. Run it on the server, from
# a checkout of this repository, with deploy/.env in place.
#
#   deploy/deploy.sh ghcr.io/<owner>/<repo>@sha256:<digest>
#
# Steps, each one stops the deploy if it fails:
#   1. refuse anything that is not an image digest signed by this repository's
#      Release workflow on a version tag (cosign, keyless);
#   2. apply migrations as a release step (forward-only, ADR 0006), so the
#      running version keeps working on the new schema;
#   3. replace the app; in-flight requests drain on SIGTERM;
#   4. smoke test through the public hostname.
# Rollback = run it again with the previous digest (printed at the end).
set -euo pipefail

IMAGE_REF="${1:?usage: deploy/deploy.sh <image>@sha256:<digest>}"
REPO="${REPO:-luuthanhminh/internal-transfers-rs}"
cd "$(dirname "$0")"

case "$IMAGE_REF" in
  *@sha256:*) ;;
  *) echo "refusing a mutable tag; pass the image by digest" >&2; exit 2 ;;
esac
[ -f .env ] || { echo "deploy/.env is missing (start from deploy/.env.example)" >&2; exit 2; }

echo "==> verifying signature of $IMAGE_REF"
cosign verify "$IMAGE_REF" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp "^https://github.com/${REPO}/\.github/workflows/release\.yml@refs/tags/v" \
  > /dev/null

previous="$(sed -n 's/^IMAGE=//p' .env)"
if grep -q '^IMAGE=' .env; then
  sed -i "s|^IMAGE=.*|IMAGE=${IMAGE_REF}|" .env
else
  echo "IMAGE=${IMAGE_REF}" >> .env
fi
domain="$(sed -n 's/^DOMAIN=//p' .env)"

echo "==> pulling"
docker compose pull --quiet app

echo "==> migrations (release step)"
docker compose --profile release run --rm migrate

echo "==> starting"
docker compose up -d --remove-orphans db app caddy cloudflared

echo "==> smoke test https://${domain}"
for _ in $(seq 1 30); do
  curl -fsS -o /dev/null "https://${domain}/ready" && break
  sleep 2
done
curl -fsS "https://${domain}/ready"
echo
curl -fsS -o /dev/null -w "GET /docs -> %{http_code}\n" "https://${domain}/docs"

echo "==> deployed ${IMAGE_REF}"
echo "    rollback: deploy/deploy.sh ${previous}"
