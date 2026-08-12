#!/usr/bin/env bash
# Delete a domain and every resource it provisioned, for manual cleanup after
# smoke testing or ad-hoc experimentation. Not part of any deploy/CI path —
# invoke it explicitly.
#
# Drives the same konductor-deprovision-env workflow the provisioning saga's own
# rollback path uses, once per environment on the domain record, then deletes
# the domain's DynamoDB record. Safe to re-run: deprovision-env is idempotent
# ("already gone" is a no-op), and a missing domain record is treated as
# already torn down.
#
# Usage: AWS_REGION=us-east-1 DOMAIN_NAME=smoke-test ./scripts/teardown-domain.sh
set -euo pipefail

: "${AWS_REGION:?AWS_REGION is required}"
: "${DOMAIN_NAME:?DOMAIN_NAME is required (the domain to tear down)}"

ITEM=$(aws dynamodb get-item \
  --table-name konductor-domains \
  --key "{\"domainName\":{\"S\":\"${DOMAIN_NAME}\"}}" \
  --region "${AWS_REGION}" \
  --output json | jq -r '.Item // empty')

if [ -z "${ITEM}" ]; then
  echo "Domain '${DOMAIN_NAME}' not found — nothing to tear down."
  exit 0
fi

OWNER_ACCOUNT=$(echo "${ITEM}" | jq -r '.ownerAccountId.S')
ENVS=$(echo "${ITEM}" | jq -r '.environments.L[]?.S // empty')
STATE_MACHINE_ARN=$(aws cloudformation describe-stacks \
  --stack-name konductor-core \
  --query "Stacks[0].Outputs[?OutputKey=='DeprovisionEnvStateMachineArn'].OutputValue" \
  --output text \
  --region "${AWS_REGION}")

if [ -z "${ENVS}" ]; then
  echo "Domain '${DOMAIN_NAME}' has no environments on record — deleting record only."
else
  echo "Tearing down '${DOMAIN_NAME}' (owner ${OWNER_ACCOUNT}) across: $(echo "${ENVS}" | tr '\n' ' ')"
  FAILURES=0
  for ENV in ${ENVS}; do
    RESOURCE_ARNS=$(echo "${ITEM}" | jq -c ".resourceArns.M.${ENV}.M // {}" | \
      jq -c 'with_entries(.value = (.value.S // .value))')
    if [ "${RESOURCE_ARNS}" = "{}" ]; then
      echo "  ${ENV}: no provisioned resources on record (never provisioned) — skipping"
      continue
    fi
    EXECUTION_NAME="teardown-${DOMAIN_NAME}-${ENV}-$(date +%s 2>/dev/null || echo manual)"
    INPUT=$(jq -nc \
      --arg env "${ENV}" \
      --arg domain "${DOMAIN_NAME}" \
      --arg owner "${OWNER_ACCOUNT}" \
      --argjson arns "${RESOURCE_ARNS}" \
      '{environment: $env, domainName: $domain, ownerAccountId: $owner, resourceArns: $arns}')

    EXECUTION_ARN=$(aws stepfunctions start-execution \
      --state-machine-arn "${STATE_MACHINE_ARN}" \
      --name "${EXECUTION_NAME}" \
      --input "${INPUT}" \
      --region "${AWS_REGION}" \
      --query executionArn --output text)

    echo -n "  ${ENV}: deprovisioning..."
    STATUS="RUNNING"
    for i in $(seq 1 30); do
      STATUS=$(aws stepfunctions describe-execution \
        --execution-arn "${EXECUTION_ARN}" \
        --region "${AWS_REGION}" \
        --query status --output text)
      if [ "${STATUS}" = "SUCCEEDED" ]; then
        # The workflow never hard-fails; per-step delete errors are collected
        # in compensationErrors, so SUCCEEDED alone does not mean clean.
        EXEC_OUTPUT=$(aws stepfunctions describe-execution \
          --execution-arn "${EXECUTION_ARN}" \
          --region "${AWS_REGION}" \
          --query output --output text) || EXEC_OUTPUT=""
        COMP_ERRORS=$(echo "${EXEC_OUTPUT}" | jq -r '.compensationErrors[]?' 2>/dev/null) || COMP_ERRORS=""
        if [ -z "${EXEC_OUTPUT}" ] || [ "${EXEC_OUTPUT}" = "None" ]; then
          echo " FAILED (could not read execution output) — see ${EXECUTION_ARN}" >&2
          FAILURES=$((FAILURES + 1))
        elif [ -n "${COMP_ERRORS}" ]; then
          echo " FAILED (resources left behind) — see ${EXECUTION_ARN}" >&2
          echo "${COMP_ERRORS}" | sed 's/^/    /' >&2
          FAILURES=$((FAILURES + 1))
        else
          echo " done"
        fi
        break
      fi
      if [ "${STATUS}" = "FAILED" ] || [ "${STATUS}" = "TIMED_OUT" ] || [ "${STATUS}" = "ABORTED" ]; then
        echo " FAILED (${STATUS}) — see ${EXECUTION_ARN}" >&2
        FAILURES=$((FAILURES + 1))
        break
      fi
      sleep 2
    done
    if [ "${STATUS}" = "RUNNING" ]; then
      echo " TIMED OUT waiting for execution — see ${EXECUTION_ARN}" >&2
      FAILURES=$((FAILURES + 1))
    fi
  done

  if [ "${FAILURES}" -gt 0 ]; then
    echo "Teardown finished with ${FAILURES} environment failure(s) — domain record NOT deleted; investigate and re-run." >&2
    exit 1
  fi
fi

aws dynamodb delete-item \
  --table-name konductor-domains \
  --key "{\"domainName\":{\"S\":\"${DOMAIN_NAME}\"}}" \
  --region "${AWS_REGION}"

echo "Domain '${DOMAIN_NAME}' fully torn down."
