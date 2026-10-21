#!/usr/bin/env bash
# Backfill system-events topics for domains that predate domain-system-events.
#
# Pages konductor-domains, selects ACTIVE/INACTIVE domains with no recorded
# systemEventsTopicArn, ensures the managed SNS topic through the
# konductor-system-topic-manager Lambda (the same ownership-checked helper the
# provisioning workflow uses), then records the ARN with a conditional write
# that preserves lifecycle status and every other attribute. Backfill emits no
# historical upgrade events.
#
# Safe to interrupt and re-run: topic ensure is idempotent, and a stop between
# ensure and the record write is completed by the next run's conditional write.
#
# Usage:
#   AWS_REGION=eu-west-1 ./scripts/backfill-system-event-topics.sh
#   AWS_REGION=eu-west-1 DOMAINS="checkout billing" ./scripts/backfill-system-event-topics.sh
#   AWS_REGION=eu-west-1 DRY_RUN=1 ./scripts/backfill-system-event-topics.sh
set -euo pipefail

: "${AWS_REGION:?AWS_REGION is required}"

TABLE_NAME="konductor-domains"
TOPIC_MANAGER="konductor-system-topic-manager"

ORGANIZATION=$(aws cloudformation describe-stacks \
  --stack-name konductor-core \
  --query "Stacks[0].Parameters[?ParameterKey=='Organization'].ParameterValue | [0]" \
  --output text \
  --region "${AWS_REGION}")
: "${ORGANIZATION:?could not resolve the Organization stack parameter}"

# Page through the table, yielding eligible domain items as compact JSON.
eligible_domains() {
  local starting_token="" page
  while :; do
    local args=(--table-name "${TABLE_NAME}" --region "${AWS_REGION}" --output json
      --projection-expression "domainName,#s,ownerAccountId,ownerEmail,environmentAccounts,costCenter,systemEventsTopicArn"
      --expression-attribute-names '{"#s":"status"}')
    [ -n "${starting_token}" ] && args+=(--starting-token "${starting_token}")
    page=$(aws dynamodb scan "${args[@]}")
    echo "${page}" | jq -c '.Items[]
      | select(.status.S == "ACTIVE" or .status.S == "INACTIVE")
      | select(has("systemEventsTopicArn") | not)'
    starting_token=$(echo "${page}" | jq -r '.NextToken // empty')
    [ -z "${starting_token}" ] && break
  done
}

CANDIDATES=()
while IFS= read -r item; do
  [ -n "${item}" ] && CANDIDATES+=("${item}")
done < <(eligible_domains)

if [ -n "${DOMAINS:-}" ]; then
  SELECTED=()
  for item in ${CANDIDATES[@]+"${CANDIDATES[@]}"}; do
    name=$(echo "${item}" | jq -r '.domainName.S')
    if echo " ${DOMAINS} " | grep -q " ${name} "; then
      SELECTED+=("${item}")
    fi
  done
  CANDIDATES=(${SELECTED[@]+"${SELECTED[@]}"})
fi

echo "Found ${#CANDIDATES[@]} eligible domain(s) without a recorded system-events topic."

FAILURES=0
SUCCEEDED=0
for item in ${CANDIDATES[@]+"${CANDIDATES[@]}"}; do
  name=$(echo "${item}" | jq -r '.domainName.S')
  topic_name="${ORGANIZATION}-${name}-system-events"

  if [ "${DRY_RUN:-0}" = "1" ]; then
    echo "  ${name}: DRY RUN — would ensure topic '${topic_name}', subscribe the owner email, and record its ARN"
    continue
  fi

  owner=$(echo "${item}" | jq -r '.ownerAccountId.S')
  owner_email=$(echo "${item}" | jq -r '.ownerEmail.S // empty')
  if [ -z "${owner_email}" ]; then
    echo "  ${name}: FAILED (record has no ownerEmail; cannot subscribe the default endpoint)" >&2
    FAILURES=$((FAILURES + 1))
    continue
  fi
  env_accounts=$(echo "${item}" | jq -c '.environmentAccounts.M // {} | with_entries(.value = .value.S)')
  cost_center=$(echo "${item}" | jq -r '.costCenter.S // empty')
  payload=$(jq -nc \
    --arg domain "${name}" \
    --arg owner "${owner}" \
    --arg email "${owner_email}" \
    --argjson accounts "${env_accounts}" \
    --arg cc "${cost_center}" \
    '{action: "ensure", domainName: $domain, ownerAccountId: $owner, ownerEmail: $email, environmentAccounts: $accounts}
     + (if $cc == "" then {} else {costCenter: $cc} end)')

  invoke_out=$(mktemp)
  if ! invoke_result=$(aws lambda invoke \
      --function-name "${TOPIC_MANAGER}" \
      --payload "${payload}" \
      --region "${AWS_REGION}" \
      "${invoke_out}" 2>&1); then
    echo "  ${name}: FAILED (invoke error) — ${invoke_result}" >&2
    rm -f "${invoke_out}"
    FAILURES=$((FAILURES + 1))
    continue
  fi
  if [ -n "$(echo "${invoke_result}" | jq -r '.FunctionError // empty')" ]; then
    echo "  ${name}: FAILED (topic ensure) — $(cat "${invoke_out}")" >&2
    rm -f "${invoke_out}"
    FAILURES=$((FAILURES + 1))
    continue
  fi
  topic_arn=$(jq -r '.topicArn // empty' "${invoke_out}")
  rm -f "${invoke_out}"
  if [ -z "${topic_arn}" ]; then
    echo "  ${name}: FAILED (ensure returned no topicArn)" >&2
    FAILURES=$((FAILURES + 1))
    continue
  fi

  if ! aws dynamodb update-item \
      --table-name "${TABLE_NAME}" \
      --key "{\"domainName\":{\"S\":\"${name}\"}}" \
      --update-expression "SET systemEventsTopicArn = :arn" \
      --condition-expression "attribute_not_exists(systemEventsTopicArn) OR systemEventsTopicArn = :arn" \
      --expression-attribute-values "{\":arn\":{\"S\":\"${topic_arn}\"}}" \
      --region "${AWS_REGION}" >/dev/null; then
    echo "  ${name}: FAILED (record write) — topic exists at ${topic_arn}; re-run converges" >&2
    FAILURES=$((FAILURES + 1))
    continue
  fi

  echo "  ${name}: topic ready at ${topic_arn}"
  SUCCEEDED=$((SUCCEEDED + 1))
done

if [ "${DRY_RUN:-0}" = "1" ]; then
  echo "DRY RUN complete — no writes performed."
  exit 0
fi

echo "Backfill complete: ${SUCCEEDED} configured, ${FAILURES} failed."
[ "${FAILURES}" -eq 0 ] || exit 1
