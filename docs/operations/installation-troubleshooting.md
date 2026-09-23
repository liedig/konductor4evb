# Installation troubleshooting

[Documentation home](../README.md)

Companion to the [installation guide](installation.md). Destructive cleanup requires separate authorization and a verified resource inventory.

## Procedures

### SES sandbox: verification emails not received

In SES sandbox mode, both the sender and recipient email addresses must be verified. After deploying the notifications stack, AWS sends a verification email to each address. If you don't receive it:

- Check spam/junk folders.
- Confirm the email addresses are correct in the stack parameters.
- Verify status with `aws ses get-identity-verification-attributes`.
- SES sandbox only allows sending to verified addresses. You cannot send to arbitrary recipients until you request production access.
- The `ownerEmail` and `secondaryOwnerEmails` values submitted during domain registration are notification recipients. In SES sandbox mode, each must be a verified SES identity before any approval, rejection, registration-failure, or provisioning-failure notification can be delivered to them. Either verify each owner email as an SES identity, or request SES production access to lift the sandbox restriction.

### RAM share failed: not in an AWS organization

The `AwsOrganizationArn` parameter is required. Konductor uses AWS RAM to share domain namespace information across accounts. If you see errors during provisioning related to RAM:

- Confirm the platform account belongs to an AWS organization.
- Verify the Organization ARN is correct (`arn:aws:organizations::...`).
- Ensure the domain account is in the same Organization.

### Trust role AssumeRole denied

If domain provisioning fails with `AccessDenied` when assuming the trust role:

- Verify the trust stack is deployed in the target domain account.
- Check the trust role's trust policy lists the correct platform account role ARNs.
- Confirm the role ARNs match the `KonductorFunctionRoleArn`, `ProvisioningWorkflowRoleArn`, and `RetagWorkflowRoleArn` outputs from the core stack.

```bash
aws iam get-role \
  --role-name <org>-konductor-trust-role \
  --region <region>
```

### zig 0.16+ `ar` regression breaks SAM build

If `make build` fails with errors like:

```text
ar: error: unable to open '...libring_core_0_17_14_.a': No such file or directory
```

or

```text
ar: error: unable to open '...libaws_lc_0_39_1_crypto.a': No such file or directory
```

This is a regression in zig 0.16.0 where the `ar` subcommand (LLVM Archiver) cannot create new archive files. The `c` (create) modifier is broken—it always attempts to open an existing file first, failing when the output `.a` file does not yet exist. This affects any Rust crate using `cc-rs` for native compilation during cross-builds (including `ring` and `aws-lc-sys`, both used by the AWS SDK).

**Fix:** Downgrade zig to 0.14.x:

```bash
brew unlink zig
brew install zig@0.14
brew link zig@0.14
```

Then clear caches and rebuild:

```bash
rm -rf ~/Library/Caches/cargo-zigbuild
make clean
make build
```

If builds also hang during the `aws-lc-sys` compilation step, set the following environment variable (already included in the Makefile):

```bash
export AWS_LC_SYS_NO_JITTER_ENTROPY=1
```

### SAM build cache is stale

If builds fail or produce unexpected results after changing code:

```bash
make clean    # removes cargo build artifacts and .aws-sam
make build    # fresh build
```

The `sam build -c` command uses a cache (`-c` flag). If you encounter build issues, clearing the cache with `make clean` is the first step.

### DynamoDB table retained after stack delete

The `DomainsTable` has `DeletionPolicy: Retain`. When you delete the core stack, the DynamoDB table is preserved. To fully remove it:

```bash
aws dynamodb delete-table \
  --table-name konductor-domains \
  --region <region>
```

## Related review items

[Trust-role parameters](../reference/documentation-discrepancies.md#d03) and [retained-resource inventories](../reference/documentation-discrepancies.md#d08) need separate technical review.
