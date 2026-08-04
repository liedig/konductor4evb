#!/usr/bin/env bash
# Seed developer role bindings through the binding administration API, so no
# one clicks through curl calls by hand during development.
#
# Goes through the real admin API (not direct table writes), so seeds exercise
# the same validation, conditional writes, and audit records as production
# administration. Re-running is safe: identical creates are idempotent (HTTP
# 200) and report the existing binding.
#
# Platform-admin resolves from Cognito groups, never from the grant table, so
# this script works against a completely empty bindings table — there is no
# bootstrap deadlock.
#
# Usage:
#   TOKEN="<platform-admin access token>" \
#   BINDINGS='[
#     {"domain":"orders","role":"publisher","principal":{"type":"aws-account","accountId":"111122223333"}},
#     {"domain":"orders","role":"publisher","principal":{"type":"cognito-group","issuer":"https://cognito-idp.<region>.amazonaws.com/<pool-id>","group":"orders-publishers"}}
#   ]' \
#   ./scripts/seed-bindings.sh
#
# API_URL defaults to the OAuthApiEndpoint output of the nexus-core stack
# (requires AWS_REGION and credentials with cloudformation:DescribeStacks).
set -euo pipefail

: "${TOKEN:?TOKEN is required (a platform-admin access token; see docs/development/oauth-authentication-guide.md)}"
: "${BINDINGS:?BINDINGS is required (a JSON array of {domain, role, principal})}"

if [ -z "${API_URL:-}" ]; then
  : "${AWS_REGION:?AWS_REGION is required when API_URL is not set}"
  API_URL=$(aws cloudformation describe-stacks \
    --stack-name nexus-core \
    --query "Stacks[0].Outputs[?OutputKey=='OAuthApiEndpoint'].OutputValue" \
    --output text \
    --region "${AWS_REGION}")
fi

echo "${BINDINGS}" | jq -e 'type == "array" and length > 0' >/dev/null || {
  echo "BINDINGS must be a non-empty JSON array" >&2
  exit 1
}

COUNT=$(echo "${BINDINGS}" | jq length)
echo "Seeding ${COUNT} binding(s) against ${API_URL}"

for i in $(seq 0 $((COUNT - 1))); do
  BODY=$(echo "${BINDINGS}" | jq -c ".[$i]")
  DESC=$(echo "${BODY}" | jq -r '"\(.principal.type)=\(.principal.group // .principal.accountId) \(.role) on \(.domain)"')

  RESPONSE=$(curl -s -w "\n%{http_code}" -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${BODY}" \
    "${API_URL}/bindings")
  STATUS=$(echo "${RESPONSE}" | tail -1)
  PAYLOAD=$(echo "${RESPONSE}" | sed '$d')

  case "${STATUS}" in
    200)
      echo "  exists:  ${DESC} (bindingId $(echo "${PAYLOAD}" | jq -r '.bindingId'))"
      ;;
    201)
      echo "  created: ${DESC} (bindingId $(echo "${PAYLOAD}" | jq -r '.bindingId'))"
      ;;
    *)
      echo "  FAILED:  ${DESC} — HTTP ${STATUS}: ${PAYLOAD}" >&2
      exit 1
      ;;
  esac
done

echo "Done. Verify with: curl -s -H \"Authorization: Bearer \${TOKEN}\" \"${API_URL}/bindings?domain=<domain>\" | jq ."
