.PHONY: build test check smoke deploy deploy-core deploy-notifications teardown-core teardown-notifications clean

build:
	sam build --beta-features -c

test:
	cargo test --workspace

check:
	cargo fmt --check
	cargo check --workspace
	cargo clippy --workspace -- -D warnings

smoke: ## Run end-to-end smoke test (register, approve, verify, cleanup)
	@API_URL=$$(aws cloudformation describe-stacks \
		--stack-name acme-nexus \
		--query "Stacks[0].Outputs[?OutputKey=='ApiEndpoint'].OutputValue" \
		--output text \
		--region $(AWS_REGION)) && \
	echo "=== Registering smoke-test domain ===" && \
	RESPONSE=$$(awscurl -X POST "$${API_URL}/domains" \
		--region $(AWS_REGION) \
		-H "Content-Type: application/json" \
		-d "{\"domainName\":\"smoke-test\",\"ownerAccountId\":\"$(APP_ACCOUNT_ID)\",\"ownerEmail\":\"$(OWNER_EMAIL)\",\"description\":\"Smoke test domain\"}") && \
	echo "$$RESPONSE" && \
	DOMAIN_ID=$$(echo "$$RESPONSE" | jq -r '.domainId') && \
	echo "domainId=$${DOMAIN_ID}" && \
	echo "=== Approving domain ===" && \
	awscurl -X POST "$${API_URL}/domains/registrations/decide" \
		--region $(AWS_REGION) \
		-H "Content-Type: application/json" \
		-d "{\"domainName\":\"smoke-test\",\"decision\":\"ACCEPT\"}" && \
	echo "=== Polling for ACTIVE status ===" && \
	ATTEMPTS=0 && \
	while [ $$ATTEMPTS -lt 30 ]; do \
		STATUS=$$(awscurl -X GET "$${API_URL}/domains/$${DOMAIN_ID}" \
			--region $(AWS_REGION) | jq -r '.status') && \
		echo "  status=$$STATUS (attempt $$((ATTEMPTS+1))/30)" && \
		if [ "$$STATUS" = "ACTIVE" ]; then break; fi && \
		sleep 5 && \
		ATTEMPTS=$$((ATTEMPTS+1)); \
	done && \
	if [ "$$STATUS" != "ACTIVE" ]; then echo "ERROR: domain did not reach ACTIVE" && exit 1; fi && \
	echo "=== Verifying platform-account resources ===" && \
	aws events describe-event-bus --name acme-dev-smoke-test-domain-bus --region $(AWS_REGION) > /dev/null && echo "  event-bus: OK" && \
	aws schemas describe-registry --registry-name acme-dev-smoke-test-schema-registry --region $(AWS_REGION) > /dev/null && echo "  schema-registry: OK" && \
	aws sqs get-queue-url --queue-name acme-dev-smoke-test-subscription-dlq --region $(AWS_REGION) > /dev/null && echo "  subscription-dlq: OK" && \
	echo "=== Deleting smoke-test domain ===" && \
	awscurl -X DELETE "$${API_URL}/domains/$${DOMAIN_ID}" --region $(AWS_REGION) && \
	echo "=== Polling for DELETED status ===" && \
	ATTEMPTS=0 && \
	while [ $$ATTEMPTS -lt 30 ]; do \
		STATUS=$$(awscurl -X GET "$${API_URL}/domains/$${DOMAIN_ID}" \
			--region $(AWS_REGION) | jq -r '.status') && \
		echo "  status=$$STATUS (attempt $$((ATTEMPTS+1))/30)" && \
		if [ "$$STATUS" = "DELETED" ]; then break; fi && \
		sleep 5 && \
		ATTEMPTS=$$((ATTEMPTS+1)); \
	done && \
	if [ "$$STATUS" != "DELETED" ]; then echo "WARNING: domain did not reach DELETED within timeout"; fi && \
	echo "=== Smoke test complete ==="

deploy: build deploy-core

deploy-core: build
	sam deploy --config-env dev

deploy-notifications:
	aws cloudformation deploy \
		--template-file notifications-stack.yaml \
		--stack-name acme-nexus-notifications \
		--parameter-overrides FromEmail=$(FROM_EMAIL) ToEmail=$(TO_EMAIL) \
		--capabilities CAPABILITY_NAMED_IAM \
		--region $(AWS_REGION)

teardown-core:
	aws cloudformation delete-stack --stack-name acme-nexus --region $(AWS_REGION)

teardown-notifications:
	aws cloudformation delete-stack --stack-name acme-nexus-notifications --region $(AWS_REGION)

clean:
	cargo clean
	rm -rf .aws-sam
