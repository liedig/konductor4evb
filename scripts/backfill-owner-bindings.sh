#!/usr/bin/env bash
# Backfill owner bindings for domains that predate ownership-at-approval.
#
# Builds the owner-role BINDINGS array from a domain -> team-group mapping
# and hands it to scripts/seed-bindings.sh, so backfills go through the real
# admin API (validation, conditional writes, audit records) like any other
# administration. Re-running is idempotent.
#
# Usage:
#   TOKEN="<platform-admin access token>" \
#   DOMAIN_TEAMS='{"orders": "team-orders", "payments": "team-payments"}' \
#   ./scripts/backfill-owner-bindings.sh
#
# DRY_RUN=1 prints the BINDINGS payload without calling the API:
#   DRY_RUN=1 DOMAIN_TEAMS='{"orders":"team-orders"}' ./scripts/backfill-owner-bindings.sh
#
# COGNITO_ISSUER defaults to https://cognito-idp.<region>.amazonaws.com/<NexusUserPoolId>.
set -euo pipefail

: "${DOMAIN_TEAMS:?DOMAIN_TEAMS is required (a JSON object mapping domain name to team group)}"

echo "${DOMAIN_TEAMS}" | jq -e 'type == "object" and length > 0' >/dev/null || {
  echo "DOMAIN_TEAMS must be a non-empty JSON object" >&2
  exit 1
}

if [ -z "${COGNITO_ISSUER:-}" ]; then
  : "${AWS_REGION:?AWS_REGION is required when COGNITO_ISSUER is not set}"
  USER_POOL_ID=$(aws cloudformation describe-stacks \
    --stack-name nexus-core \
    --query "Stacks[0].Outputs[?OutputKey=='NexusUserPoolId'].OutputValue" \
    --output text \
    --region "${AWS_REGION}")
  : "${USER_POOL_ID:?could not resolve the Nexus user pool id}"
  COGNITO_ISSUER="https://cognito-idp.${AWS_REGION}.amazonaws.com/${USER_POOL_ID}"
fi

BINDINGS=$(echo "${DOMAIN_TEAMS}" | jq -c --arg issuer "${COGNITO_ISSUER}" '
  [to_entries[] | {
    domain: .key,
    role: "owner",
    principal: {type: "cognito-group", issuer: $issuer, group: .value}
  }]')

if [ "${DRY_RUN:-0}" = "1" ]; then
  echo "DRY RUN — would seed:"
  echo "${BINDINGS}" | jq .
  exit 0
fi

echo "Backfilling $(echo "${BINDINGS}" | jq length) owner binding(s)"
TOKEN="${TOKEN:?TOKEN is required (a platform-admin access token)}" \
BINDINGS="${BINDINGS}" \
  ./scripts/seed-bindings.sh
