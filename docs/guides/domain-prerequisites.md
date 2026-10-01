# Domain prerequisites

[Documentation home](../README.md)

Check these items with your platform administrator before you
[register a domain](domains.md). Most failed or stalled registrations trace
back to one of them.

## Checklist

| You need | Why | Who sets it up |
|---|---|---|
| A Konductor user in your team's group | Console and OAuth registrations name an owning team, which must be one of your team groups | Platform administrator: [set up users](../operations/access-administration.md#setting-up-users) |
| API credentials | Every API call is authenticated | You: [authenticate API requests](authentication.md) |
| An owner AWS account with the trust role | Konductor creates your application-side resources in this account | Account administrator: [deploy domain-account trust](../operations/trust-stack-deployment.md) |
| Any per-environment accounts, with the trust role | Optional: some environments can live in other accounts | Account administrator: the same trust deployment in each account |
| A domain name | It is your domain's permanent identity | You |
| Owner contacts | Konductor emails approval and provisioning outcomes | You |

## Team group and user

Interactive registrations, through the console or the OAuth API, set an
owning team. The owning team must be one of the Cognito team groups on your
access token, such as `team-payments`. Platform groups such as
`platform-admin` are not team groups, so platform teams cannot own domains.
See the OAuth example in [calling the API](authentication.md#calling-the-api).

When your domain is approved, your team receives an `owner` binding on it.
That binding lets your team read and update the domain.

## Owner account and trust role

Your registration names an owner account: the 12-digit AWS account ID where
your application-side event bus and integration queue are created. The trust
role `<org>-konductor-trust-role` must already exist in that account. Without
it, provisioning fails with an assume-role denial.

One trust role serves every domain and environment in an account. If your
account already hosts another domain, the role is already there.

## Per-environment accounts (optional)

By default, every environment of your domain uses the owner account. To place
an environment in a different account, for example production in a dedicated
account, list it in `environmentAccounts` when you register. Keys must be
environments the platform is configured with.

Deploy the trust role in every distinct account you name. The mapping is fixed
at registration and cannot be updated later. See the
[registration fields](domains.md#fields).

## Domain name

Domain names are globally unique within the installation and cannot be
renamed. Choose a name for the business capability the domain represents,
such as `billing` or `orders`. It must be 2 to 64 lowercase letters, digits,
or hyphens, and cannot start or end with a hyphen. The name `konductor` is
reserved.

The name appears in every resource Konductor creates for you. See
[resources and naming](../reference/resources.md) for the resulting names.

## Owner contacts

Registration requires a primary owner email and at least one secondary owner
email. The primary owner receives approval, rejection, and provisioning-issue
emails when notifications are enabled.

## Platform capacity

The platform account has a limited number of domain slots until its
administrator raises service quotas. With default quotas and three
environments, only three domains fit. If your registration is approved but
provisioning fails, ask your administrator to check
[service quotas](../operations/service-quotas.md).

## Next step

[Register your domain](domains.md#registering-a-domain).
