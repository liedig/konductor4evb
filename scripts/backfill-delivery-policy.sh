#!/usr/bin/env bash
# Backfill the platform-delivery resource policy onto app-side domain buses
# for domains provisioned before the SetAppBusDeliveryPolicy step existed.
#
# For each ACTIVE domain x environment: assume the domain account's trust
# role and PutPermission (platform account may PutEvents) on the app-side
# bus. Idempotent — re-attaching the same statement id is a no-op overwrite,
# so the script is safe to re-run.
#
# Usage: ORGANIZATION=acme AWS_REGION=us-east-1 ./scripts/backfill-delivery-policy.sh
set -euo pipefail

: "${ORGANIZATION:?ORGANIZATION is required (org identifier used in resource naming)}"
: "${AWS_REGION:?AWS_REGION is required}"

PLATFORM_ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
echo "Platform account: ${PLATFORM_ACCOUNT_ID}"

DOMAINS=$(aws dynamodb query \
  --table-name nexus-domains \
  --index-name by-status \
  --key-condition-expression "#s = :active" \
  --expression-attribute-names '{"#s":"status"}' \
  --expression-attribute-values '{":active":{"S":"ACTIVE"}}' \
  --region "${AWS_REGION}" \
  --output json | jq -c '.Items[]')

if [ -z "${DOMAINS}" ]; then
  echo "No ACTIVE domains found — nothing to backfill."
  exit 0
fi

FAILURES=0
while read -r item; do
  DOMAIN=$(echo "${item}" | jq -r '.domainName.S')
  OWNER_ACCOUNT=$(echo "${item}" | jq -r '.ownerAccountId.S')
  ENVS=$(echo "${item}" | jq -r '.environments.L[].S')

  echo "Domain '${DOMAIN}' (owner ${OWNER_ACCOUNT})"

  CREDS=$(aws sts assume-role \
    --role-arn "arn:aws:iam::${OWNER_ACCOUNT}:role/${ORGANIZATION}-nexus-trust-role" \
    --role-session-name nexus-delivery-policy-backfill \
    --query 'Credentials.[AccessKeyId,SecretAccessKey,SessionToken]' \
    --output text)
  read -r KEY SECRET TOKEN <<<"${CREDS}"

  for ENV in ${ENVS}; do
    BUS="${ORGANIZATION}-${ENV}-${DOMAIN}-event-bus"
    if AWS_ACCESS_KEY_ID="${KEY}" AWS_SECRET_ACCESS_KEY="${SECRET}" AWS_SESSION_TOKEN="${TOKEN}" \
      aws events put-permission \
        --event-bus-name "${BUS}" \
        --action events:PutEvents \
        --principal "${PLATFORM_ACCOUNT_ID}" \
        --statement-id allow-platform-subscription-delivery \
        --region "${AWS_REGION}"; then
      echo "  ${ENV}: policy attached to ${BUS}"
    else
      echo "  ${ENV}: FAILED to attach policy to ${BUS}" >&2
      FAILURES=$((FAILURES + 1))
    fi
  done
done <<<"${DOMAINS}"

if [ "${FAILURES}" -gt 0 ]; then
  echo "Backfill finished with ${FAILURES} failure(s) — re-run after fixing." >&2
  exit 1
fi
echo "Backfill complete."
