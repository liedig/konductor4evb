.PHONY: help build test check smoke smoke-subscription deploy deploy-core deploy-notifications deploy-trust teardown-core teardown-notifications teardown-trust teardown-domain list-domains list-schemas list-subscriptions web-dev deploy-web clean

export AWS_LC_SYS_NO_JITTER_ENTROPY := 1

AWS_REGION ?= $(shell aws configure get region 2>/dev/null || echo us-east-1)
ORGANIZATION ?= acme
DOMAIN_NAME ?= smoke-test

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
	@printf "\n  $(BOLD)$(CYAN)EventBridge Konductor$(RESET) — Makefile targets\n\n"
	@printf "  $(BOLD)Build & Test$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(build|test|check|clean):.*## / {printf "    $(GREEN)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Deploy$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(deploy|deploy-core|deploy-notifications|deploy-trust):.*## / {printf "    $(GREEN)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Teardown$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(teardown-.+):.*## / {printf "    $(YELLOW)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Smoke Test$(RESET) $(DIM)(manual — never run by deploy or CI)$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(smoke|smoke-subscription):.*## / {printf "    $(MAGENTA)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Inspect$(RESET)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^(list-domains|list-schemas|list-subscriptions):.*## / {printf "    $(CYAN)%-22s$(RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n  $(BOLD)Environment variables$(RESET)\n"
	@printf "    $(DIM)AWS_REGION$(RESET)                   current: $(CYAN)$(AWS_REGION)$(RESET)\n"
	@printf "    $(DIM)DOMAIN_NAME$(RESET)                  current: $(CYAN)$(DOMAIN_NAME)$(RESET) — used by: smoke, teardown-domain\n"
	@printf "    $(DIM)APP_ACCOUNT_ID$(RESET)               required for: smoke\n"
	@printf "    $(DIM)OWNER_EMAIL$(RESET)                  required for: smoke\n"
	@printf "    $(DIM)SECONDARY_OWNER_EMAIL$(RESET)          required for: smoke\n"
	@printf "    $(DIM)PRODUCER_DOMAIN$(RESET)              required for: smoke-subscription\n"
	@printf "    $(DIM)CONSUMER_DOMAIN$(RESET)              required for: smoke-subscription\n"
	@printf "    $(DIM)NOTIFICATION_SENDER_EMAIL$(RESET)    required for: deploy-notifications\n"
	@printf "    $(DIM)PLATFORM_OPERATOR_EMAIL$(RESET)      required for: deploy-notifications\n"
	@printf "    $(DIM)APP_PROFILE$(RESET)                  required for: deploy-trust, teardown-trust\n"
	@printf "    $(DIM)ORGANIZATION$(RESET)                 optional for: deploy-trust (default: $(CYAN)$(ORGANIZATION)$(RESET))\n"
	@printf "    $(DIM)DOMAIN_STATUS$(RESET)               optional for: list-domains (filter by status)\n"
	@printf "    $(DIM)OWNER_ACCOUNT_ID$(RESET)            optional for: list-domains (filter by owner)\n"
	@printf "    $(DIM)CONSUMER_DOMAIN$(RESET)             required for: list-schemas (the catalog is entitlement-scoped)\n"
	@printf "    $(DIM)DOMAIN$(RESET)                      optional for: list-schemas (producer filter); required for: list-subscriptions\n"
	@printf "    $(DIM)DIRECTION$(RESET)                   required for: list-subscriptions (inbound|outbound)\n"
	@printf "    $(DIM)SCHEMA_NAME$(RESET)                 optional for: list-subscriptions (filter by schema)\n"
	@printf "    $(DIM)SUBSCRIPTION_STATUS$(RESET)         optional for: list-subscriptions (filter by status)\n\n"


build-ci: lambda-binaries-ci ## Build all Lambda binaries once, then package with SAM
	sam build --config-file samconfig-core.toml -c

lambda-binaries-ci: ## Cross-compile every function to arm64 (per-package; a single workspace build collapses same-named bootstrap binaries into one output dir)
	@ulimit -Sn 10240 2>/dev/null || true; \
	for dir in domain-management/functions/* event-streams/functions/* control-plane/functions/*; do \
		pkg=$$(basename $$dir); \
		cargo lambda build --release --arm64 --package $$pkg || exit 1; \
		cp target/lambda/bootstrap/bootstrap $$dir/bootstrap; \
	done
	
build: lambda-binaries ## Build all Lambda binaries once, then package with SAM
	sam build --config-file samconfig-core.toml -c

lambda-binaries: ## Cross-compile every function to arm64 (per-package to avoid bootstrap name collisions)
	@ulimit -Sn 10240 2>/dev/null || true; \
	for dir in domain-management/functions/* event-streams/functions/* control-plane/functions/*; do \
		pkg=$$(basename $$dir); \
		cargo lambda build --release --arm64 --package $$pkg || exit 1; \
		cp target/lambda/bootstrap/bootstrap $$dir/bootstrap; \
	done

test: ## cargo test --workspace
	cargo test --workspace

seed-bindings: ## Seed developer role bindings via the admin API (requires TOKEN and BINDINGS; API_URL optional)
	@./scripts/seed-bindings.sh

seed-users: ## Create dev personas alice (platform-admin) and bob (team-payments) in the user pool (requires ALICE_PASSWORD and BOB_PASSWORD)
	@./scripts/seed-users.sh

backfill-owner-bindings: ## Backfill owner bindings for pre-existing domains (requires TOKEN and DOMAIN_TEAMS; DRY_RUN=1 to preview)
	@./scripts/backfill-owner-bindings.sh

seed-entitlements: ## Seed subscriber entitlements via the admin API (requires TOKEN and ENTITLEMENTS; API_URL optional)
	@./scripts/seed-entitlements.sh

SAMPLES ?= 200

benchmark-grants: ## Deploy and run the in-region grant-engine latency benchmark (add-grant-engine 6.1/6.2)
	@ulimit -Sn 10240 2>/dev/null || true; cargo lambda build --release --arm64 --package grant-benchmark
	sam build --config-file samconfig-benchmark.toml -c
	sam deploy --config-file samconfig-benchmark.toml
	@echo "Invoking konductor-grant-benchmark ($(SAMPLES) samples/probe; allow ~2 min)..."
	@aws lambda invoke --function-name konductor-grant-benchmark --log-type Tail \
		--payload '{"samples": $(SAMPLES)}' --cli-binary-format raw-in-base64-out \
		--output json \
		/tmp/grant-benchmark-report.json > /tmp/grant-benchmark-invoke-meta.json
	@echo "Cold start (first invoke after deploy only):"; \
	(grep '^LogResult:' /tmp/grant-benchmark-invoke-meta.json | cut -d' ' -f2 | base64 -d 2>/dev/null || jq -r '.LogResult // empty' /tmp/grant-benchmark-invoke-meta.json | base64 -d 2>/dev/null) | grep -oE "Init Duration: [0-9.]+ ms" || echo "  warm invocation — no init segment"; \
	echo ""; \
	jq . /tmp/grant-benchmark-report.json

teardown-benchmark: ## Delete the grant benchmark stack (fixtures are removed by each run)
	sam delete --config-file samconfig-benchmark.toml --no-prompts

check: ## cargo fmt, check, clippy (CI-equivalent local gate)
	cargo fmt --check
	cargo check --workspace
	cargo clippy --workspace -- -D warnings

web-dev: ## Run the Konductor web console dev server (http://localhost:3000), proxying /dev to the deployed OAuth API
	@set -euo pipefail; \
	output() { aws cloudformation describe-stacks --stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='$$1'].OutputValue" --output text; }; \
	API_URL=$$(output OAuthApiEndpoint); \
	POOL_ID=$$(output KonductorUserPoolId); \
	CLIENT_ID=$$(output KonductorUserPoolClientId); \
	test -n "$$POOL_ID" || { echo "$(YELLOW)KonductorUserPoolId output missing — deploy the core stack first (make deploy-core)$(RESET)"; exit 1; }; \
	printf '{"apiBaseUrl":"/dev","userPoolId":"%s","userPoolClientId":"%s","region":"%s"}\n' \
		"$$POOL_ID" "$$CLIENT_ID" "$(AWS_REGION)" > web/public/config.json; \
	cd web && KONDUKTOR_DEV_API_URL="$$API_URL" pnpm dev

deploy-web: ## Build the web console, publish to the web bucket, and write config.json from stack outputs
	@set -euo pipefail; \
	output() { aws cloudformation describe-stacks --stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='$$1'].OutputValue" --output text; }; \
	BUCKET=$$(output KonductorWebBucketName); \
	DISTRIBUTION=$$(output KonductorWebDistributionId); \
	WEB_URL=$$(output KonductorWebUrl); \
	POOL_ID=$$(output KonductorUserPoolId); \
	CLIENT_ID=$$(output KonductorUserPoolClientId); \
	test -n "$$BUCKET" || { echo "$(YELLOW)KonductorWebBucketName output missing — deploy the core stack first (make deploy-core)$(RESET)"; exit 1; }; \
	echo "$(CYAN)Building web console$(RESET)"; \
	cd web && pnpm install --frozen-lockfile && pnpm build && cd ..; \
	printf '{"apiBaseUrl":"%s/dev","userPoolId":"%s","userPoolClientId":"%s","region":"%s"}\n' \
		"$$WEB_URL" "$$POOL_ID" "$$CLIENT_ID" "$(AWS_REGION)" > web/.output/public/config.json; \
	echo "$(CYAN)Syncing to s3://$$BUCKET$(RESET)"; \
	aws s3 sync web/.output/public "s3://$$BUCKET" --delete; \
	echo "$(CYAN)Invalidating distribution $$DISTRIBUTION$(RESET)"; \
	aws cloudfront create-invalidation --distribution-id "$$DISTRIBUTION" --paths "/*" > /dev/null; \
	echo "$(GREEN)Web console deployed: $$(output KonductorWebUrl)$(RESET)"

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
ifndef TOKEN
	$(error TOKEN is required — a platform-admin access token for the approval step, which is OAuth-only; see docs/development/oauth-authentication-guide.md)
endif
	@set -euo pipefail; \
	DOMAIN_NAME="$(DOMAIN_NAME)"; \
	echo "$(BOLD)$(CYAN)=== Smoke test: $${DOMAIN_NAME} ===$(RESET)"; \
	API_URL=$$(aws cloudformation describe-stacks \
		--stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='IamApiEndpoint'].OutputValue" \
		--output text \
		--region $(AWS_REGION)); \
	[ -n "$${API_URL}" ] && [ "$${API_URL}" != "None" ] || { echo "konductor-core stack not found or has no IamApiEndpoint output — deploy first with 'make deploy-core'"; exit 1; }; \
	OAUTH_URL=$$(aws cloudformation describe-stacks \
		--stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='OAuthApiEndpoint'].OutputValue" \
		--output text \
		--region $(AWS_REGION)); \
	[ -n "$${OAUTH_URL}" ] && [ "$${OAUTH_URL}" != "None" ] || { echo "konductor-core stack not found or has no OAuthApiEndpoint output — deploy first with 'make deploy-core'"; exit 1; }; \
	echo "  API endpoint: $${API_URL}"; \
	echo ""; \
	echo "$(BOLD)(1/4) Register domain$(RESET)"; \
	RESPONSE=$$(awscurl -X POST "$${API_URL}/domains" \
		--region $(AWS_REGION) \
		-H "Content-Type: application/json" \
		-d "{\"domainName\":\"$${DOMAIN_NAME}\",\"ownerAccountId\":\"$(APP_ACCOUNT_ID)\",\"ownerEmail\":\"$(OWNER_EMAIL)\",\"secondaryOwnerEmails\":[\"$(SECONDARY_OWNER_EMAIL)\"],\"description\":\"Smoke test domain ($${DOMAIN_NAME})\"}"); \
	echo "  $${RESPONSE}"; \
	REQUEST_ID=$$(echo "$${RESPONSE}" | jq -r '.requestId'); \
	[ "$${REQUEST_ID}" = "null" ] && { echo "  $(BOLD)ERROR: no requestId in response$(RESET)"; exit 1; } || true; \
	echo ""; \
	echo "$(BOLD)(2/4) Wait for domain record with taskToken$(RESET)"; \
	TASK_TOKEN=""; \
	for i in $$(seq 1 20); do \
		ITEM=$$(aws dynamodb get-item --table-name konductor-domains \
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
	echo "$(BOLD)(3/4) Approve (OAuth API — approvals are interactive-only)$(RESET)"; \
	DECIDE_RESPONSE=$$(curl -s -X POST "$${OAUTH_URL}/domains/registrations/decide" \
		-H "Authorization: Bearer $(TOKEN)" \
		-H "Content-Type: application/json" \
		-d "{\"taskToken\":\"$${TASK_TOKEN}\",\"decision\":\"ACCEPT\"}"); \
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
	echo "    make teardown-domain DOMAIN_NAME=$${DOMAIN_NAME}"

list-domains: ## List registered domains (optional: DOMAIN_STATUS=, OWNER_ACCOUNT_ID=)
	@set -euo pipefail; \
	API_URL=$$(aws cloudformation describe-stacks \
		--stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='IamApiEndpoint'].OutputValue" \
		--output text \
		--region $(AWS_REGION) 2>/dev/null) || true; \
	[ -n "$${API_URL}" ] && [ "$${API_URL}" != "None" ] || { echo "konductor-core stack not found or has no IamApiEndpoint output — deploy first with 'make deploy-core'"; exit 1; }; \
	QUERY=""; \
	[ -n "$(DOMAIN_STATUS)" ] && QUERY="$${QUERY}&status=$(DOMAIN_STATUS)"; \
	[ -n "$(OWNER_ACCOUNT_ID)" ] && QUERY="$${QUERY}&ownerAccountId=$(OWNER_ACCOUNT_ID)"; \
	QUERY=$${QUERY#&}; \
	URL="$${API_URL}/domains$${QUERY:+?$$QUERY}"; \
	awscurl -X GET "$$URL" --region $(AWS_REGION) 2>&1 | jq . 2>/dev/null || awscurl -X GET "$$URL" --region $(AWS_REGION)

list-schemas: ## List a consumer domain's schema catalog (required: CONSUMER_DOMAIN=, optional: DOMAIN= producer filter, ENV=)
ifndef CONSUMER_DOMAIN
	$(error CONSUMER_DOMAIN is required — the catalog shows only what this domain's entitlements cover; your account needs a subscriber binding on it)
endif
	@set -euo pipefail; \
	API_URL=$$(aws cloudformation describe-stacks \
		--stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='IamApiEndpoint'].OutputValue" \
		--output text \
		--region $(AWS_REGION) 2>/dev/null) || true; \
	[ -n "$${API_URL}" ] && [ "$${API_URL}" != "None" ] || { echo "konductor-core stack not found or has no IamApiEndpoint output — deploy first with 'make deploy-core'"; exit 1; }; \
	QUERY="consumerDomain=$(CONSUMER_DOMAIN)"; \
	[ -n "$(DOMAIN)" ] && QUERY="$${QUERY}&domain=$(DOMAIN)"; \
	[ -n "$(ENV)" ] && QUERY="$${QUERY}&environment=$(ENV)"; \
	awscurl -X GET "$${API_URL}/schemas?$${QUERY}" \
		--region $(AWS_REGION) 2>&1 | jq . 2>/dev/null || awscurl -X GET "$${API_URL}/schemas?$${QUERY}" --region $(AWS_REGION)

list-subscriptions: ## List a domain's subscriptions (required: DOMAIN=, DIRECTION=inbound|outbound, optional: SCHEMA_NAME=, SUBSCRIPTION_STATUS=)
ifndef DOMAIN
	$(error DOMAIN is required — the domain whose subscriptions to list)
endif
ifndef DIRECTION
	$(error DIRECTION is required — inbound (subscriber view) or outbound (publisher view))
endif
	@set -euo pipefail; \
	API_URL=$$(aws cloudformation describe-stacks \
		--stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='IamApiEndpoint'].OutputValue" \
		--output text \
		--region $(AWS_REGION) 2>/dev/null) || true; \
	[ -n "$${API_URL}" ] && [ "$${API_URL}" != "None" ] || { echo "konductor-core stack not found or has no IamApiEndpoint output — deploy first with 'make deploy-core'"; exit 1; }; \
	QUERY="domain=$(DOMAIN)&direction=$(DIRECTION)"; \
	[ -n "$(SCHEMA_NAME)" ] && QUERY="$${QUERY}&schema=$(SCHEMA_NAME)"; \
	[ -n "$(SUBSCRIPTION_STATUS)" ] && QUERY="$${QUERY}&status=$(SUBSCRIPTION_STATUS)"; \
	awscurl -X GET "$${API_URL}/subscriptions?$${QUERY}" \
		--region $(AWS_REGION) 2>&1 | jq . 2>/dev/null || awscurl -X GET "$${API_URL}/subscriptions?$${QUERY}" --region $(AWS_REGION)

deploy: build deploy-core ## Build then deploy the core stack

deploy-core: build ## Deploy the Konductor core stack (builds first)
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

smoke-subscription: ## End-to-end subscription smoke test (requires PRODUCER_DOMAIN + CONSUMER_DOMAIN, both ACTIVE)
ifndef PRODUCER_DOMAIN
	$(error PRODUCER_DOMAIN is required — an ACTIVE domain that will offer the schema)
endif
ifndef CONSUMER_DOMAIN
	$(error CONSUMER_DOMAIN is required — an ACTIVE domain that will subscribe)
endif
	ORGANIZATION=$(ORGANIZATION) AWS_REGION=$(AWS_REGION) PRODUCER_DOMAIN=$(PRODUCER_DOMAIN) CONSUMER_DOMAIN=$(CONSUMER_DOMAIN) ./scripts/smoke-subscription.sh

teardown-domain: ## Delete a domain and every resource it provisioned (default DOMAIN_NAME=smoke-test)
	AWS_REGION=$(AWS_REGION) DOMAIN_NAME=$(DOMAIN_NAME) ./scripts/teardown-domain.sh

teardown-core: ## Delete the Konductor core stack
	aws cloudformation delete-stack --stack-name konductor-core --region $(AWS_REGION)

teardown-notifications: ## Delete the notifications stack
	aws cloudformation delete-stack --stack-name konductor-notifications --region $(AWS_REGION)

deploy-trust: ## Deploy the trust stack to a domain account (requires APP_PROFILE)
ifndef APP_PROFILE
	$(error APP_PROFILE is required — the AWS CLI profile for the target domain account)
endif
	@set -euo pipefail; \
	KONDUKTOR_ROLE_ARN=$$(aws cloudformation describe-stacks \
		--stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='KonductorFunctionRoleArn'].OutputValue" \
		--output text \
		--region $(AWS_REGION)); \
	PROVISIONING_ROLE_ARN=$$(aws cloudformation describe-stacks \
		--stack-name konductor-core \
		--query "Stacks[0].Outputs[?OutputKey=='ProvisioningWorkflowRoleArn'].OutputValue" \
		--output text \
		--region $(AWS_REGION)); \
	echo "$(BOLD)$(CYAN)Deploying trust stack to profile: $(APP_PROFILE)$(RESET)"; \
	echo "  KonductorRoleArn:           $${KONDUKTOR_ROLE_ARN}"; \
	echo "  ProvisioningRoleArn:    $${PROVISIONING_ROLE_ARN}"; \
	echo "  Organization:           $(ORGANIZATION)"; \
	aws cloudformation deploy \
		--template-file trust-stack.yaml \
		--stack-name konductor-trust \
		--parameter-overrides \
			KonductorRoleArn=$${KONDUKTOR_ROLE_ARN} \
			ProvisioningWorkflowRoleArn=$${PROVISIONING_ROLE_ARN} \
			Organization=$(ORGANIZATION) \
		--tags Application=konductor Component=trust auto-delete=no \
		--capabilities CAPABILITY_NAMED_IAM \
		--region $(AWS_REGION) \
		--profile $(APP_PROFILE)

teardown-trust: ## Delete the trust stack from a domain account (requires APP_PROFILE)
ifndef APP_PROFILE
	$(error APP_PROFILE is required — the AWS CLI profile for the target domain account)
endif
	aws cloudformation delete-stack --stack-name konductor-trust --region $(AWS_REGION) --profile $(APP_PROFILE)

clean: ## Remove cargo target/ and .aws-sam build artifacts
	cargo clean
	rm -rf .aws-sam