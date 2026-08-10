#!/usr/bin/env bash
# Seed subscriber entitlements through the entitlement administration API —
# the migration path for consumer domains that must retain access before
# subscription re-evaluation begins.
#
# Goes through the real admin API (not direct table writes), so seeds exercise
# the same validation, deterministic idempotency, and audit records as
# production administration. Re-running is safe: an identical create answers
# the existing entitlement (HTTP 200).
#
# Order matters in migration: seed the subscriber binding first
# (scripts/seed-bindings.sh), then the entitlements here, then let the
# revocation worker and sweep re-evaluate existing subscriptions.
#
# Scope patterns use the single additive grammar: "*" covers the whole
# producer domain (including future schemas), an exact detail-type name covers
# all versions of one schema (including future versions), and a prefix with a
# trailing "*" covers a family. Prefix patterns share by naming convention —
# an unintended match is one entitlement delete away.
#
# Usage:
#   TOKEN="<platform-admin access token>" \
#   ENTITLEMENTS='[
#     {"consumerDomain":"checkout","producerDomain":"orders","scopePattern":"*"},
#     {"consumerDomain":"billing","producerDomain":"orders","scopePattern":"Order*"}
#   ]' \
#   ./scripts/seed-entitlements.sh
#
# API_URL defaults to the OAuthApiEndpoint output of the nexus-core stack
# (requires AWS_REGION and credentials with cloudformation:DescribeStacks).
set -euo pipefail

: "${TOKEN:?TOKEN is required (a platform-admin access token; see docs/development/oauth-authentication-guide.md)}"
: "${ENTITLEMENTS:?ENTITLEMENTS is required (a JSON array of {consumerDomain, producerDomain, scopePattern})}"

if [ -z "${API_URL:-}" ]; then
  : "${AWS_REGION:?AWS_REGION is required when API_URL is not set}"
  API_URL=$(aws cloudformation describe-stacks \
    --stack-name nexus-core \
    --query "Stacks[0].Outputs[?OutputKey=='OAuthApiEndpoint'].OutputValue" \
    --output text \
    --region "${AWS_REGION}")
fi

echo "${ENTITLEMENTS}" | jq -e 'type == "array" and length > 0' >/dev/null || {
  echo "ENTITLEMENTS must be a non-empty JSON array" >&2
  exit 1
}

COUNT=$(echo "${ENTITLEMENTS}" | jq length)
echo "Seeding ${COUNT} entitlement(s) against ${API_URL}"

for i in $(seq 0 $((COUNT - 1))); do
  BODY=$(echo "${ENTITLEMENTS}" | jq -c ".[$i]")
  DESC=$(echo "${BODY}" | jq -r '"\(.scopePattern) from \(.producerDomain) to \(.consumerDomain)"')

  RESPONSE=$(curl -s -w "\n%{http_code}" -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${BODY}" \
    "${API_URL}/entitlements")
  STATUS=$(echo "${RESPONSE}" | tail -1)
  PAYLOAD=$(echo "${RESPONSE}" | sed '$d')

  case "${STATUS}" in
    200)
      echo "  exists:  ${DESC} (entitlementId $(echo "${PAYLOAD}" | jq -r '.entitlementId'))"
      ;;
    201)
      echo "  created: ${DESC} (entitlementId $(echo "${PAYLOAD}" | jq -r '.entitlementId'))"
      ;;
    *)
      echo "  FAILED:  ${DESC} — HTTP ${STATUS}: ${PAYLOAD}" >&2
      exit 1
      ;;
  esac
done

echo "Done. Verify with: curl -s -H \"Authorization: Bearer \${TOKEN}\" \"\${API_URL}/entitlements?consumerDomain=<domain>\" | jq ."
