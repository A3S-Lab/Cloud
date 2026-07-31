# Security policy

Report vulnerabilities privately through
[GitHub Security Advisories](https://github.com/A3S-Lab/Workflow/security/advisories/new).
Do not disclose security issues in public issues or pull requests before a fix
is available.

Include the affected component, reproduction steps, impact, and any relevant
Runtime policy or provider details. Treat leaked Runtime access tokens, secret
references, artifact-integrity bypasses, cross-tenant PostgreSQL access, and
network allow-list bypasses as security issues.

The bundled process Runtime provider is for local development and CI. Use a
provider with enforceable isolation and resource controls for production.
