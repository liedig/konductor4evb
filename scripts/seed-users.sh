#!/usr/bin/env bash
# Seed the two development personas into the Konductor user pool:
#
#   alice@example.com  platform-admin   (approvals inbox, enable/disable, full reads)
#   bob@example.com    team-payments    (domain owner/member; scoped reads)
#
# Creates the team group when absent and sets permanent passwords (no
# forced reset at first login). Re-running is safe: existing users get
# their password and group membership re-asserted.
#
# Usage:
#   ALICE_PASSWORD='...' BOB_PASSWORD='...' ./scripts/seed-users.sh
#
# USER_POOL_ID defaults to the KonductorUserPoolId output of the konductor-core
# stack (requires AWS_REGION and credentials with cloudformation:DescribeStacks).
set -euo pipefail

: "${ALICE_PASSWORD:?ALICE_PASSWORD is required (permanent password for alice@example.com)}"
: "${BOB_PASSWORD:?BOB_PASSWORD is required (permanent password for bob@example.com)}"

if [ -z "${USER_POOL_ID:-}" ]; then
  : "${AWS_REGION:?AWS_REGION is required when USER_POOL_ID is not set}"
  USER_POOL_ID=$(aws cloudformation describe-stacks \
    --stack-name konductor-core \
    --query "Stacks[0].Outputs[?OutputKey=='KonductorUserPoolId'].OutputValue" \
    --output text \
    --region "${AWS_REGION}")
fi
: "${USER_POOL_ID:?could not resolve the Konductor user pool id}"

ensure_group() {
  local group="$1"
  if aws cognito-idp get-group --user-pool-id "${USER_POOL_ID}" --group-name "${group}" >/dev/null 2>&1; then
    echo "  group exists:  ${group}"
  else
    aws cognito-idp create-group --user-pool-id "${USER_POOL_ID}" --group-name "${group}" >/dev/null
    echo "  group created: ${group}"
  fi
}

ensure_user() {
  local email="$1" password="$2" group="$3"

  if aws cognito-idp admin-get-user --user-pool-id "${USER_POOL_ID}" --username "${email}" >/dev/null 2>&1; then
    echo "  user exists:   ${email}"
  else
    aws cognito-idp admin-create-user \
      --user-pool-id "${USER_POOL_ID}" \
      --username "${email}" \
      --user-attributes Name=email,Value="${email}" Name=email_verified,Value=true \
      --message-action SUPPRESS >/dev/null
    echo "  user created:  ${email}"
  fi

  aws cognito-idp admin-set-user-password \
    --user-pool-id "${USER_POOL_ID}" \
    --username "${email}" \
    --password "${password}" \
    --permanent >/dev/null

  ensure_group "${group}"
  aws cognito-idp admin-add-user-to-group \
    --user-pool-id "${USER_POOL_ID}" \
    --username "${email}" \
    --group-name "${group}" >/dev/null
  echo "  membership:    ${email} -> ${group}"
}

echo "Seeding personas against user pool ${USER_POOL_ID}"
ensure_group "platform-admin" >/dev/null 2>&1 || true
ensure_user "alice@example.com" "${ALICE_PASSWORD}" "platform-admin"
ensure_user "bob@example.com" "${BOB_PASSWORD}" "team-payments"

echo "Done. Sign in with SRP against the KonductorUserPoolClientId stack output; see docs/development/oauth-authentication-guide.md."
