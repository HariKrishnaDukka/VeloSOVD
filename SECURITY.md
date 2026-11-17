# Security Policy

## Supported Versions

VeloSOVD is an evolving demonstrator, not a production-qualified vehicle
software platform. Security fixes are currently considered only for the
latest project state on the `main` branch. No release branches or older
versions are currently designated as supported.

| Version / branch | Security support |
| --- | --- |
| Latest `main` | Supported for vulnerability reports |
| Other branches, tags, and snapshots | Not currently supported |

## Reporting a Vulnerability

Report suspected vulnerabilities privately using GitHub's private
vulnerability reporting feature:

1. Open the repository's **Security** tab.
2. Select **Report a vulnerability**.
3. Submit the report through the private advisory form.

Do not report security vulnerabilities through public GitHub issues,
discussions, or pull requests. This project does not publish a separate
security-reporting email address.

Please include, where possible:

- A concise description and the affected component or commit.
- The impact and realistic attack prerequisites.
- Reproduction steps or a proof of concept that does not access real vehicle
  systems or other people's data.
- Relevant logs, requests, or configuration, with secrets and personal data
  removed.
- Any suggested mitigation.

## Response Expectations

Response and remediation timelines have not yet been formally established.
The project maintainers will acknowledge and assess reports as capacity
permits. Do not assume a guaranteed response time, fix deadline, or service
level agreement.

## Security Scope and Demonstrator Limitations

This policy applies to the VeloSOVD repository and its maintained project
components. Reports concerning dependencies, build tooling, scripts, the
central server, virtual ECUs, network setup, and the web interface are in
scope when they create a security impact in this project.

VeloSOVD is a development demonstrator. In particular, the current Rust
central HTTP server does not provide authentication or TLS. Do not expose it,
its host proxy, or the demonstrator network to the public internet or other
untrusted networks. Use an isolated development environment and restrict
network access.

A successful report or fix does not imply that the demonstrator is suitable
for deployment in a real vehicle, production environment, or safety-critical
system. No production security or standards-conformance qualification is
claimed.
