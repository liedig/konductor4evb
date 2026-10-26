# Uninstall Konductor

[Documentation home](../README.md)

Companion to the [installation guide](installation.md). Destructive cleanup requires separate authorization and a verified resource inventory.

## Procedures

To remove a Konductor installation completely, run these steps with the platform account's credentials, as described in [credentials and Region](installation.md#credentials-and-region), and pass the installation's Region to each target.

### 1. Delete all domains

Before deleting the stacks, remove all domains to clean up provisioned resources (event buses, DLQs, dashboards) in both accounts:

```bash
# List all domains
make list-domains AWS_REGION=<region>

# Tear down each domain (drives the deprovision-env workflow per environment)
make teardown-domain AWS_REGION=<region> DOMAIN_NAME=<domainName>
```

Repeat for each domain. The teardown script drives the same deprovisioning
workflow used by the provisioning saga's rollback path, removing all per-env
resources in both the platform and domain accounts before deleting the domain
record. A domain that published schemas, has subscribers, or has rules on its
application bus reports `FAILED (resources left behind)` and keeps its record
until you clear those leftovers; follow
[schemas and event bus rules block deprovisioning](domain-recovery.md#schemas-and-event-bus-rules-block-deprovisioning),
then re-run the teardown.

### 2. Delete the core stack

Empty the core stack's two S3 buckets first. CloudFormation cannot delete a
bucket that still holds objects, so the stack deletion would fail:

- The web console bucket (`{org}-konductor-web-{account}-{region}`) holds the
  build `make deploy-web` published.
- The system-event failure bucket
  (`konductor-system-event-failures-{account}-{region}`) keeps failed
  projector batches for 90 days. Copy out any you want to keep;
  [system event recovery](system-events.md#targeted-failed-message-recovery-s3-failure-bucket)
  describes what they hold.

```bash
WEB_BUCKET=$(aws cloudformation describe-stacks --stack-name konductor-core --region <region> \
  --query "Stacks[0].Outputs[?OutputKey=='KonductorWebBucketName'].OutputValue" --output text)
aws s3 rm "s3://${WEB_BUCKET}" --recursive --region <region>

FAILURE_BUCKET=$(aws cloudformation describe-stack-resource --stack-name konductor-core \
  --logical-resource-id SystemEventProjectorFailureBucket --region <region> \
  --query "StackResourceDetail.PhysicalResourceId" --output text)
# Optional: keep a local copy of the failed batches
aws s3 sync "s3://${FAILURE_BUCKET}" ./system-event-failures --region <region>
aws s3 rm "s3://${FAILURE_BUCKET}" --recursive --region <region>

make teardown-core AWS_REGION=<region>
```

### 3. Delete trust stacks (in each domain account)

If you deployed the optional [organization guardrails](guardrails.md), delete
their stack first, with the organization-admin credentials that deployed it.
The guardrail SCPs deny deleting the trust role to every principal except the
recovery role, so the trust stacks cannot be removed while the SCPs are attached.
`delete-stack` returns before the deletion finishes, so wait for it to complete:

```bash
aws cloudformation delete-stack --stack-name konductor-guardrails --region <region>
aws cloudformation wait stack-delete-complete --stack-name konductor-guardrails --region <region>
```

Then run once per domain account, naming that account's AWS CLI profile:

```bash
make teardown-trust AWS_REGION=<region> APP_PROFILE=<domain-account-profile>
```

### 4. Delete the notifications stack

```bash
make teardown-notifications AWS_REGION=<region>
```

### 5. Clean up retained resources

The DynamoDB tables are deleted with the core stack. The following resources
survive it:

- **Cognito user pool** `{org}-konductor-users` (`DeletionPolicy: Retain`),
  with every user account. A reinstall creates a new pool and cannot adopt this
  one, so export anything you need first. Pool names are not unique, and a
  failed install can leave several pools with the same name; delete each match.
- **System-event schema registry** `{org}-konductor-system-schemas`
  (`DeletionPolicy: Retain`). Delete its schemas before the registry.
- **CloudWatch log groups** under `/aws/lambda/konductor-*` and
  `/aws/apigateway/konductor-*`, which Lambda and API Gateway can recreate after
  the stack removes them.

```bash
# User pools named <org>-konductor-users
aws cognito-idp list-user-pools --max-results 60 --region <region> \
  --query "UserPools[?Name=='<org>-konductor-users'].Id" --output text
aws cognito-idp delete-user-pool --user-pool-id <user-pool-id> --region <region>

# System-event schemas, then the registry
aws schemas list-schemas --registry-name <org>-konductor-system-schemas \
  --query "Schemas[*].SchemaName" --output text --region <region>
aws schemas delete-schema --registry-name <org>-konductor-system-schemas \
  --schema-name <schema-name> --region <region>
aws schemas delete-registry --registry-name <org>-konductor-system-schemas --region <region>

# List and delete log groups
aws logs describe-log-groups \
  --log-group-name-prefix "/aws/lambda/konductor-" \
  --region <region> \
  --query "logGroups[*].logGroupName" --output text

aws logs delete-log-group --log-group-name <log-group-name> --region <region>
```
