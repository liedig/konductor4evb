#!/usr/bin/env bash
# Delete a domain and every resource it provisioned, for manual cleanup after
# smoke testing or ad-hoc experimentation. Not part of any deploy/CI path —
# invoke it explicitly.
#
# Drives the same konductor-deprovision-env workflow the provisioning saga's own
# rollback path uses, once per environment on the domain record, then deletes
# the domain's DynamoDB record. A domain still PENDING_APPROVAL has its approval
# workflow stopped first. Safe to re-run: deprovision-env is idempotent
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

# A pending domain's approval workflow is still waiting for a decision. Left
# running, its rejection or 7-day timeout writes the record back as REJECTED,
# or rejects a domain later re-registered under the same name. Stop it and
# confirm it ended before anything is deleted. ExecutionDoesNotExist means the
# registration's workflow never started, so there is nothing to stop.
if [ "$(echo "${ITEM}" | jq -r '.status.S // empty')" = "PENDING_APPROVAL" ]; then
  REQUEST_ID=$(echo "${ITEM}" | jq -r '.requestId.S // empty')
  if [ -z "${REQUEST_ID}" ]; then
    echo "  approval workflow: FAILED (no requestId on record) — domain record NOT deleted; stop the domain's konductor-domain-approval execution, then delete the record by hand" >&2
    exit 1
  fi
  APPROVAL_STATE_MACHINE_ARN=$(aws cloudformation describe-stack-resource \
    --stack-name konductor-core \
    --logical-resource-id ApprovalWorkflow \
    --query StackResourceDetail.PhysicalResourceId \
    --output text \
    --region "${AWS_REGION}")
  APPROVAL_EXECUTION_ARN="${APPROVAL_STATE_MACHINE_ARN/:stateMachine:/:execution:}:approval-${REQUEST_ID}"
  echo -n "  approval workflow: stopping..."
  if STOP_ERROR=$(aws stepfunctions stop-execution \
    --execution-arn "${APPROVAL_EXECUTION_ARN}" \
    --region "${AWS_REGION}" 2>&1 >/dev/null); then
    APPROVAL_STATUS="RUNNING"
    for _ in $(seq 1 15); do
      APPROVAL_STATUS=$(aws stepfunctions describe-execution \
        --execution-arn "${APPROVAL_EXECUTION_ARN}" \
        --region "${AWS_REGION}" \
        --query status --output text)
      if [ "${APPROVAL_STATUS}" != "RUNNING" ]; then
        break
      fi
      sleep 2
    done
    if [ "${APPROVAL_STATUS}" = "RUNNING" ]; then
      echo " FAILED (still RUNNING) — domain record NOT deleted; see ${APPROVAL_EXECUTION_ARN}" >&2
      exit 1
    fi
    echo " done (${APPROVAL_STATUS})"
  elif [[ "${STOP_ERROR}" == *ExecutionDoesNotExist* ]]; then
    echo " no execution to stop"
  else
    echo " FAILED — domain record NOT deleted; see ${APPROVAL_EXECUTION_ARN}" >&2
    echo "${STOP_ERROR}" >&2
    exit 1
  fi
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

# Domain-level system-events topic: deleted through the topic manager, which
# verifies the Konductor ownership tags before removing anything. A domain
# provisioned before system events has no recorded ARN — nothing to do.
TOPIC_ARN=$(echo "${ITEM}" | jq -r '.systemEventsTopicArn.S // empty')
if [ -n "${TOPIC_ARN}" ]; then
  echo -n "  system-events topic: deleting..."
  INVOKE_OUT=$(mktemp)
  aws lambda invoke \
    --function-name konductor-system-topic-manager \
    --cli-binary-format raw-in-base64-out \
    --payload "$(jq -nc --arg arn "${TOPIC_ARN}" --arg domain "${DOMAIN_NAME}" \
      '{action: "delete", topicArn: $arn, domainName: $domain}')" \
    --region "${AWS_REGION}" \
    "${INVOKE_OUT}" >/dev/null
  FN_ERROR=$(jq -r '.FunctionError // empty' "${INVOKE_OUT}" 2>/dev/null || true)
  if [ -n "${FN_ERROR}" ]; then
    echo " FAILED — see Lambda logs; domain record NOT deleted" >&2
    cat "${INVOKE_OUT}" >&2
    rm -f "${INVOKE_OUT}"
    exit 1
  fi
  rm -f "${INVOKE_OUT}"
  echo " done"
fi

# The namespace parameter belongs to the same footprint; the registration
# collision check blocks re-registration while it exists. SSM rejects deleting
# a missing parameter, and one is missing for never-provisioned domains,
# domains provisioned before the parameter step, and re-runs after a partial
# teardown, so ParameterNotFound counts as deleted. Any other error keeps the
# record so a re-run can finish.
if SSM_ERROR=$(aws ssm delete-parameter \
  --name "/namespace/${DOMAIN_NAME}" \
  --region "${AWS_REGION}" 2>&1 >/dev/null); then
  echo "  namespace parameter: deleted"
elif [[ "${SSM_ERROR}" == *ParameterNotFound* ]]; then
  echo "  namespace parameter: already absent"
else
  echo "  namespace parameter: FAILED — domain record NOT deleted" >&2
  echo "${SSM_ERROR}" >&2
  exit 1
fi

aws dynamodb delete-item \
  --table-name konductor-domains \
  --key "{\"domainName\":{\"S\":\"${DOMAIN_NAME}\"}}" \
  --region "${AWS_REGION}"

echo "Domain '${DOMAIN_NAME}' fully torn down."
