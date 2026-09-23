# Uninstall Konductor

[Documentation home](../README.md)

Companion to the [installation guide](installation.md). Destructive cleanup requires separate authorization and a verified resource inventory.

## Procedures

To remove a Konductor installation completely:

### 1. Delete all domains

Before deleting the stacks, remove all domains to clean up provisioned resources (event buses, DLQs, dashboards) in both accounts:

```bash
# List all domains
awscurl -X GET "${API_URL}/domains" --region "${REGION}"

# Tear down each domain (drives the deprovision-env workflow per environment)
make teardown-domain DOMAIN_NAME=<domainName>
```

Repeat for each domain. The teardown script drives the same deprovisioning
workflow used by the provisioning saga's rollback path, removing all per-env
resources in both the platform and domain accounts before deleting the domain
record.

### 2. Delete the core stack

```bash
aws cloudformation delete-stack \
  --stack-name konductor-core \
  --region <region>
```

### 3. Delete trust stacks (in each domain account)

Switch credentials to each domain account:

```bash
aws cloudformation delete-stack \
  --stack-name konductor-trust \
  --region <region>
```

### 4. Delete the notifications stack

```bash
aws cloudformation delete-stack \
  --stack-name konductor-notifications \
  --region <region>
```

### 5. Clean up retained resources

The following resources are not automatically deleted when their stacks are removed:

- **DynamoDB table**: `konductor-domains` (retained by `DeletionPolicy`)
- **CloudWatch log groups**: Under `/aws/lambda/konductor-*` and `/aws/apigateway/konductor-*`

```bash
# Delete the retained DynamoDB table
aws dynamodb delete-table --table-name konductor-domains --region <region>

# List and delete log groups
aws logs describe-log-groups \
  --log-group-name-prefix "/aws/lambda/konductor-" \
  --region <region> \
  --query "logGroups[*].logGroupName" --output text

aws logs delete-log-group --log-group-name <log-group-name> --region <region>
```
