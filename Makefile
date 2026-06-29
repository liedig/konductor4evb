.PHONY: help build test check smoke deploy deploy-core deploy-notifications deploy-trust teardown-core teardown-notifications teardown-trust clean

export AWS_LC_SYS_NO_JITTER_ENTROPY := 1

AWS_REGION ?= $(shell aws configure get region 2>/dev/null || echo us-east-1)
ORGANIZATION ?= acme

# Colour codes for help output (tput falls back to empty strings on dumb terminals)
BOLD := $(shell tput bold 2>/dev/null)
DIM := $(shell tput dim 2>/dev/null)
CYAN := $(shell tput setaf 6 2>/dev/null)
GREEN := $(shell tput setaf 2 2>/dev/null)
YELLOW := $(shell tput setaf 3 2>/dev/null)
MAGENTA := $(shell tput setaf 5 2>/dev/null)
RESET := $(shell tput sgr0 2>/dev/null)

.DEFAULT_GOAL := help

help: ## Show this help message
	@printf "\n  $(BOLD)$(CYAN)EventBridge Nexus$(RESET) — Makefile targets\n\n"
	@printf "  $(BOLD)Build & Test$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(build|test|check|clean):.*## / {printf "    $(GREEN)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Deploy$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(deploy|deploy-core|deploy-notifications|deploy-trust):.*## / {printf "    $(GREEN)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Teardown$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(teardown-.+):.*## / {printf "    $(YELLOW)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Smoke Test$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^smoke:.*## / {printf "    $(MAGENTA)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Environment variables$(RESET)\n"
	@printf "    $(DIM)AWS_REGION$(RESET)                   current: $(CYAN)$(AWS_REGION)$(RESET)\n"
	@printf "    $(DIM)APP_ACCOUNT_ID$(RESET)               required for: smoke\n"
	@printf "    $(DIM)OWNER_EMAIL$(RESET)                  required for: smoke\n"
	@printf "    $(DIM)SECONDARY_OWNER_EMAIL$(RESET)          required for: smoke\n"
	@printf "    $(DIM)NOTIFICATION_SENDER_EMAIL$(RESET)    required for: deploy-notifications\n"
	@printf "    $(DIM)PLATFORM_OPERATOR_EMAIL$(RESET)      required for: deploy-notifications\n"
	@printf "    $(DIM)APP_PROFILE$(RESET)                  required for: deploy-trust, teardown-trust\n"
	@printf "    $(DIM)ORGANIZATION$(RESET)                 optional for: deploy-trust (default: $(CYAN)$(ORGANIZATION)$(RESET))\n\n"

build: ## sam build (Rust cross-compilation for arm64, cached)
	sam build --config-file samconfig-core.toml --beta-features -c

test: ## cargo test --workspace
	cargo test --workspace

check: ## cargo fmt, check, clippy (CI-equivalent local gate)
	cargo fmt --check
	cargo check --workspace
	cargo clippy --workspace -- -D warnings

smoke: ## End-to-end smoke test (register, approve, verify ACTIVE across envs)
ifndef APP_ACCOUNT_ID
	$(error APP_ACCOUNT_ID is required — the AWS account ID of the domain owner)
endif
ifndef OWNER_EMAIL
	$(error OWNER_EMAIL is required — the verified SES recipient for the domain owner)
endif
ifndef SECONDARY_OWNER_EMAIL
	$(error SECONDARY_OWNER_EMAIL is required — at least one secondary owner email for the domain)
endif
	@set -euo pipefail; \
	DOMAIN_NAME="smoke-test"; \
	echo "$(BOLD)$(CYAN)=== Smoke test: $${DOMAIN_NAME} ===$(RESET)"; \
	API_URL=$$(aws cloudformation describe-stacks \
		--stack-name nexus-core \
		--query "Stacks[0].Outputs[?OutputKey=='ApiEndpoint'].OutputValue" \
		--output text \
		--region $(AWS_REGION)); \
	echo "  API endpoint: $${API_URL}"; \
	echo ""; \
	echo "$(BOLD)(1/4) Register domain$(RESET)"; \
	RESPONSE=$$(awscurl -X POST "$${API_URL}/domains" \
		--region $(AWS_REGION) \
		-H "Content-Type: application/json" \
		-d "{\"domainName\":\"$${DOMAIN_NAME}\",\"ownerAccountId\":\"$(APP_ACCOUNT_ID)\",\"ownerEmail\":\"$(OWNER_EMAIL)\",\"secondaryOwnerEmails\":[\"$(SECONDARY_OWNER_EMAIL)\"],\"description\":\"Smoke test domain\"}"); \
	echo "  $${RESPONSE}"; \
	REQUEST_ID=$$(echo "$${RESPONSE}" | jq -r '.requestId'); \
	[ "$${REQUEST_ID}" = "null" ] && { echo "  $(BOLD)ERROR: no requestId in response$(RESET)"; exit 1; } || true; \
	echo ""; \
	echo "$(BOLD)(2/4) Wait for domain record with taskToken$(RESET)"; \
	TASK_TOKEN=""; \
	for i in $$(seq 1 20); do \
		ITEM=$$(aws dynamodb get-item --table-name nexus-domains \
			--key "{\"domainName\":{\"S\":\"$${DOMAIN_NAME}\"}}" \
			--region $(AWS_REGION) \
			--output json 2>/dev/null | jq -r '.Item // empty'); \
		if [ -n "$${ITEM}" ]; then \
			TASK_TOKEN=$$(echo "$${ITEM}" | jq -r '.taskToken.S // empty'); \
			[ -n "$${TASK_TOKEN}" ] && break; \
		fi; \
		printf "  waiting for approval workflow to publish taskToken (attempt %s/20)\r" "$$i"; \
		sleep 3; \
	done; \
	echo ""; \
	[ -z "$${TASK_TOKEN}" ] && { echo "  $(BOLD)ERROR: taskToken never populated — check SendApprovalRequestNotificationFunction logs$(RESET)"; exit 1; } || true; \
	echo "  domainName=$${DOMAIN_NAME}"; \
	echo "  taskToken=$${TASK_TOKEN:0:24}... (truncated)"; \
	echo ""; \
	echo "$(BOLD)(3/4) Approve$(RESET)"; \
	DECIDE_RESPONSE=$$(awscurl -X POST "$${API_URL}/domains/registrations/decide" \
		--region $(AWS_REGION) \
		-H "Content-Type: application/json" \
		-d "{\"taskToken\":\"$${TASK_TOKEN}\",\"decision\":\"ACCEPT\",\"decidedBy\":\"smoke-test\"}"); \
	echo "  $${DECIDE_RESPONSE}"; \
	echo ""; \
	echo "$(BOLD)(4/4) Poll for ACTIVE status$(RESET)"; \
	STATUS=""; \
	for i in $$(seq 1 40); do \
		STATUS=$$(awscurl -X GET "$${API_URL}/domains/$${DOMAIN_NAME}" \
			--region $(AWS_REGION) 2>/dev/null | jq -r '.status // "UNKNOWN"'); \
		printf "  status=$(CYAN)%s$(RESET) (attempt %s/40)\r" "$${STATUS}" "$$i"; \
		case "$${STATUS}" in \
			ACTIVE) echo ""; break;; \
			PROVISIONING_FAILED) echo ""; echo "  $(BOLD)ERROR: provisioning failed — check Step Functions executions$(RESET)"; exit 1;; \
		esac; \
		sleep 5; \
	done; \
	[ "$${STATUS}" != "ACTIVE" ] && { echo "  $(BOLD)ERROR: domain did not reach ACTIVE$(RESET)"; exit 1; } || true; \
	echo ""; \
	echo "$(BOLD)$(GREEN)✓ Smoke test complete — domain '$${DOMAIN_NAME}' is ACTIVE$(RESET)"; \
	echo ""; \
	echo "  To clean up, run:"; \
	echo "    aws dynamodb delete-item --table-name nexus-domains --key '{\"domainName\":{\"S\":\"'$${DOMAIN_NAME}'\"}}' --region $(AWS_REGION)"; \
	echo "    (domain-level resources in your domain account remain until a DELETE flow is built)"

deploy: build deploy-core ## Build then deploy the core stack

deploy-core: build ## Deploy the Nexus core stack (builds first)
	sam deploy --config-file samconfig-core.toml

deploy-notifications: ## Deploy the notifications stack (SES identities)
ifndef NOTIFICATION_SENDER_EMAIL
	$(error NOTIFICATION_SENDER_EMAIL is required — verified SES sender)
endif
ifndef PLATFORM_OPERATOR_EMAIL
	$(error PLATFORM_OPERATOR_EMAIL is required — verified SES recipient (platform team inbox))
endif
	sam deploy --config-file samconfig-notifications.toml \
		--parameter-overrides NotificationSenderEmail=$(NOTIFICATION_SENDER_EMAIL) PlatformOperatorEmail=$(PLATFORM_OPERATOR_EMAIL)

teardown-core: ## Delete the Nexus core stack
	aws cloudformation delete-stack --stack-name nexus-core --region $(AWS_REGION)

teardown-notifications: ## Delete the notifications stack
	aws cloudformation delete-stack --stack-name nexus-notifications --region $(AWS_REGION)

deploy-trust: ## Deploy the trust stack to a domain account (requires APP_PROFILE)
ifndef APP_PROFILE
	$(error APP_PROFILE is required — the AWS CLI profile for the target domain account)
endif
	@set -euo pipefail; \
	NEXUS_ROLE_ARN=$$(aws cloudformation describe-stacks \
		--stack-name nexus-core \
		--query "Stacks[0].Outputs[?OutputKey=='NexusFunctionRoleArn'].OutputValue" \
		--output text \
		--region $(AWS_REGION)); \
	PROVISIONING_ROLE_ARN=$$(aws cloudformation describe-stacks \
		--stack-name nexus-core \
		--query "Stacks[0].Outputs[?OutputKey=='ProvisioningWorkflowRoleArn'].OutputValue" \
		--output text \
		--region $(AWS_REGION)); \
	echo "$(BOLD)$(CYAN)Deploying trust stack to profile: $(APP_PROFILE)$(RESET)"; \
	echo "  NexusRoleArn:           $${NEXUS_ROLE_ARN}"; \
	echo "  ProvisioningRoleArn:    $${PROVISIONING_ROLE_ARN}"; \
	echo "  Organization:           $(ORGANIZATION)"; \
	aws cloudformation deploy \
		--template-file trust-stack.yaml \
		--stack-name nexus-trust \
		--parameter-overrides \
			NexusRoleArn=$${NEXUS_ROLE_ARN} \
			ProvisioningWorkflowRoleArn=$${PROVISIONING_ROLE_ARN} \
			Organization=$(ORGANIZATION) \
		--tags Application=nexus Component=trust \
		--capabilities CAPABILITY_NAMED_IAM \
		--region $(AWS_REGION) \
		--profile $(APP_PROFILE)

teardown-trust: ## Delete the trust stack from a domain account (requires APP_PROFILE)
ifndef APP_PROFILE
	$(error APP_PROFILE is required — the AWS CLI profile for the target domain account)
endif
	aws cloudformation delete-stack --stack-name nexus-trust --region $(AWS_REGION) --profile $(APP_PROFILE)

clean: ## Remove cargo target/ and .aws-sam build artifacts
	cargo clean
	rm -rf .aws-sam
