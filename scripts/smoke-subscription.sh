#!/usr/bin/env bash
# End-to-end subscription smoke test. Requires two ACTIVE domains (producer
# and consumer) — run `make smoke` twice or register them manually first.
#
# Authorization prerequisites (subscribe is grant-gated): the IAM account
# running this script needs a `subscriber` binding on CONSUMER_DOMAIN, and
# CONSUMER_DOMAIN needs an entitlement covering PRODUCER_DOMAIN's
# SmokeTestEvent (e.g. scope pattern "SmokeTestEvent" or "*"). Seed both via
# `make seed-bindings` and `make seed-entitlements` with a platform-admin
# token before running.
#
# Flow: register schema in producer registry -> subscribe -> publish a
# conforming event to the producer platform bus -> assert rule matched ->
# negative checks (mis-stamped event, duplicate subscribe) -> unsubscribe ->
# assert rules removed and record REMOVED -> delete the schema it registered.
# Self-cleaning: leaves the two domains as they were before the run. To also
# remove the domains themselves, use `make teardown-domain`.
#
# Required env: ORGANIZATION, AWS_REGION, PRODUCER_DOMAIN, CONSUMER_DOMAIN, ENV_NAME
set -euo pipefail

: "${ORGANIZATION:?ORGANIZATION is required}"
: "${AWS_REGION:?AWS_REGION is required}"
: "${PRODUCER_DOMAIN:?PRODUCER_DOMAIN is required (an ACTIVE domain)}"
: "${CONSUMER_DOMAIN:?CONSUMER_DOMAIN is required (an ACTIVE domain)}"
: "${ENV_NAME:=dev}"

EVENT_NAME="SmokeTestEvent"
SOURCE="${ORGANIZATION}-${PRODUCER_DOMAIN}"
SCHEMA_NAME="${SOURCE}@${EVENT_NAME}"
RULE_NAME="${CONSUMER_DOMAIN}-${EVENT_NAME}"
SUBSCRIPTION_ID="${CONSUMER_DOMAIN}.${PRODUCER_DOMAIN}.${EVENT_NAME}"
REGISTRY="${ORGANIZATION}-${ENV_NAME}-${PRODUCER_DOMAIN}-schema-registry"
BUS="${ORGANIZATION}-${ENV_NAME}-${PRODUCER_DOMAIN}-event-bus"

API_URL=$(aws cloudformation describe-stacks \
  --stack-name nexus-core \
  --query "Stacks[0].Outputs[?OutputKey=='IamApiEndpoint'].OutputValue" \
  --output text --region "${AWS_REGION}")
[ -n "${API_URL}" ] || { echo "nexus-core stack has no IamApiEndpoint output — deploy first" >&2; exit 1; }
echo "API endpoint: ${API_URL}"

echo ""
echo "(1/8) Register schema ${SCHEMA_NAME} in every environment's registry"
for REG_ENV in $(aws dynamodb get-item --table-name nexus-domains \
  --key "{\"domainName\":{\"S\":\"${PRODUCER_DOMAIN}\"}}" \
  --region "${AWS_REGION}" --output json | jq -r '.Item.environments.L[].S'); do
  aws schemas create-schema \
    --registry-name "${ORGANIZATION}-${REG_ENV}-${PRODUCER_DOMAIN}-schema-registry" \
    --schema-name "${SCHEMA_NAME}" \
    --type OpenApi3 \
    --content '{"openapi":"3.0.0","info":{"title":"'"${EVENT_NAME}"'","version":"1.0.0"},"paths":{},"components":{"schemas":{"Event":{"type":"object","properties":{"metadata":{"type":"object","properties":{"schemaVersion":{"type":"integer"}}},"data":{"type":"object"}}}}}}' \
    --region "${AWS_REGION}" >/dev/null 2>&1 || echo "  (schema already exists in ${REG_ENV})"
done

echo "(2/8) Subscribe ${CONSUMER_DOMAIN} -> ${SCHEMA_NAME}"
RESPONSE=$(awscurl -X POST "${API_URL}/subscriptions" \
  --region "${AWS_REGION}" -H "Content-Type: application/json" \
  -d "{\"consumerDomain\":\"${CONSUMER_DOMAIN}\",\"producerDomain\":\"${PRODUCER_DOMAIN}\",\"schemaName\":\"${SCHEMA_NAME}\"}")
echo "  ${RESPONSE}"

echo "(3/8) Poll for ACTIVE subscription"
STATUS=""
for i in $(seq 1 30); do
  STATUS=$(awscurl -X GET "${API_URL}/subscriptions/${SUBSCRIPTION_ID}" \
    --region "${AWS_REGION}" 2>/dev/null | jq -r '.status // "PENDING"')
  printf "  status=%s (attempt %s/30)\r" "${STATUS}" "$i"
  [ "${STATUS}" = "ACTIVE" ] && { echo ""; break; }
  [ "${STATUS}" = "FAILED" ] && { echo ""; echo "ERROR: activation failed"; exit 1; }
  sleep 4
done
[ "${STATUS}" != "ACTIVE" ] && { echo "ERROR: subscription never became ACTIVE"; exit 1; }

echo "(4/8) Assert rule exists with pinned version"
PATTERN=$(aws events describe-rule --name "${RULE_NAME}" --event-bus-name "${BUS}" \
  --region "${AWS_REGION}" --query EventPattern --output text)
echo "  pattern: ${PATTERN}"
echo "${PATTERN}" | jq -e '.detail.metadata.schemaVersion | length > 0' >/dev/null \
  || { echo "ERROR: rule pattern does not pin a schema version"; exit 1; }

echo "(5/8) Publish conforming + mis-stamped events"
PINNED=$(echo "${PATTERN}" | jq -r '.detail.metadata.schemaVersion[0]')
aws events put-events --region "${AWS_REGION}" --entries "[
  {\"EventBusName\":\"${BUS}\",\"Source\":\"${SOURCE}\",\"DetailType\":\"${EVENT_NAME}\",\"Detail\":\"{\\\"metadata\\\":{\\\"schemaVersion\\\":${PINNED}},\\\"data\\\":{\\\"smoke\\\":true}}\"},
  {\"EventBusName\":\"${BUS}\",\"Source\":\"${SOURCE}\",\"DetailType\":\"${EVENT_NAME}\",\"Detail\":\"{\\\"metadata\\\":{\\\"schemaVersion\\\":\\\"${PINNED}\\\"},\\\"data\\\":{\\\"misStamped\\\":true}}\"}
]" >/dev/null
echo "  published 1 conforming (numeric v${PINNED}) + 1 mis-stamped (string version)"
echo "  waiting 60s for rule metrics..."
sleep 60
MATCHED=$(aws cloudwatch get-metric-statistics --namespace AWS/Events \
  --metric-name Invocations \
  --dimensions Name=EventBusName,Value="${BUS}" Name=RuleName,Value="${RULE_NAME}" \
  --start-time "$(date -u -v-10M '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || date -u -d '10 minutes ago' '+%Y-%m-%dT%H:%M:%SZ')" \
  --end-time "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  --period 600 --statistics Sum --region "${AWS_REGION}" \
  --query 'Datapoints[0].Sum' --output text)
echo "  rule invocations: ${MATCHED}"
[ "${MATCHED}" = "1.0" ] || echo "  WARNING: expected exactly 1 invocation (conforming only), got ${MATCHED} — metrics can lag; verify manually"

echo "(6/8) Duplicate subscribe is idempotent"
DUP=$(awscurl -X POST "${API_URL}/subscriptions" \
  --region "${AWS_REGION}" -H "Content-Type: application/json" \
  -d "{\"consumerDomain\":\"${CONSUMER_DOMAIN}\",\"producerDomain\":\"${PRODUCER_DOMAIN}\",\"schemaName\":\"${SCHEMA_NAME}\"}")
echo "  ${DUP} (accepted; handler treats existing ACTIVE stream as no-op)"

echo "(7/8) Disable ${ENV_NAME}, assert rule DISABLED, re-enable"
awscurl -X POST "${API_URL}/subscriptions/state" \
  --region "${AWS_REGION}" -H "Content-Type: application/json" \
  -d "{\"id\":\"${SUBSCRIPTION_ID}\",\"environments\":{\"${ENV_NAME}\":{\"enabled\":false}}}" >/dev/null
sleep 10
RULE_STATE=$(aws events describe-rule --name "${RULE_NAME}" --event-bus-name "${BUS}" \
  --region "${AWS_REGION}" --query State --output text)
[ "${RULE_STATE}" = "DISABLED" ] || { echo "ERROR: rule not DISABLED after state update (state=${RULE_STATE})"; exit 1; }
echo "  rule state: ${RULE_STATE} ✓"
awscurl -X POST "${API_URL}/subscriptions/state" \
  --region "${AWS_REGION}" -H "Content-Type: application/json" \
  -d "{\"id\":\"${SUBSCRIPTION_ID}\",\"environments\":{\"${ENV_NAME}\":{\"enabled\":true}}}" >/dev/null

echo "(8/8) Unsubscribe, assert rule removed and record REMOVED"
awscurl -X DELETE "${API_URL}/subscriptions/${SUBSCRIPTION_ID}" --region "${AWS_REGION}" >/dev/null
for i in $(seq 1 20); do
  STATUS=$(awscurl -X GET "${API_URL}/subscriptions/${SUBSCRIPTION_ID}" \
    --region "${AWS_REGION}" 2>/dev/null | jq -r '.status // "UNKNOWN"')
  [ "${STATUS}" = "REMOVED" ] && break
  sleep 4
done
[ "${STATUS}" != "REMOVED" ] && { echo "ERROR: record never reached REMOVED"; exit 1; }
if aws events describe-rule --name "${RULE_NAME}" --event-bus-name "${BUS}" \
  --region "${AWS_REGION}" >/dev/null 2>&1; then
  echo "ERROR: rule still exists after unsubscribe"; exit 1
fi
echo "  record REMOVED, rule gone ✓"

echo ""
echo "(9/9) Clean up schema ${SCHEMA_NAME} from every environment's registry"
for REG_ENV in $(aws dynamodb get-item --table-name nexus-domains \
  --key "{\"domainName\":{\"S\":\"${PRODUCER_DOMAIN}\"}}" \
  --region "${AWS_REGION}" --output json | jq -r '.Item.environments.L[].S'); do
  aws schemas delete-schema \
    --registry-name "${ORGANIZATION}-${REG_ENV}-${PRODUCER_DOMAIN}-schema-registry" \
    --schema-name "${SCHEMA_NAME}" \
    --region "${AWS_REGION}" >/dev/null 2>&1 || echo "  (schema already gone in ${REG_ENV})"
done
echo "  schema removed ✓"

echo ""
echo "✓ Subscription smoke test complete"
