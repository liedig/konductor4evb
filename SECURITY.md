# Security Policy

## Supported Versions

Konductor for Amazon EventBridge is pre-production and deployed per-organization from source — there are no released versions to patch. Only the `main` branch receives security fixes.

## Reporting a Vulnerability

Please report vulnerabilities privately. Do not open a public issue.

- **Preferred:** [GitHub private vulnerability reporting](https://github.com/liedig/konductor4evb/security/advisories/new) (Security → Advisories → Report a vulnerability).
- **Fallback:** email sliedig@gmail.com.

Include the affected component, reproduction steps, and the potential impact. Expect an acknowledgment within a few days and a triage decision (accepted, needs more information, or declined) shortly after. Accepted reports are fixed on `main`, with credit in the fix notes unless you prefer otherwise.

## Scope

Konductor is infrastructure you deploy into your own AWS organization. The most valuable reports concern defects in Konductor itself:

- Authorization bypasses in the grant engine, API admission paths, or the trusted-principal envelope
- Cross-domain event leakage (a consumer receiving events outside its entitlements)
- Privilege escalation through the trust role, queue policies, or provisioning workflows
- Injection or unsafe handling of publisher- or consumer-controlled inputs

Issues in your own deployment's configuration (for example, an overly permissive IAM policy you wrote) are out of scope — but if the documentation misled you into an insecure configuration, that documentation defect is very much in scope.
