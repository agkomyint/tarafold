# Security Policy

## Supported versions

Before 1.0, only the latest released minor version receives security fixes.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private vulnerability reporting
("Security" tab → "Report a vulnerability") on this repository.

Include affected version, platform, a reproducer (for example a crafted CSV), and impact.
We aim to acknowledge reports within 3 business days and to publish a fix or mitigation
within 30 days for confirmed issues, coordinating disclosure with the reporter.

## Scope

In scope:

- Crashes, hangs, or memory exhaustion from crafted input files beyond documented limits.
- Code execution or file writes outside `--out` from input data, configs, or constraint expressions.
- The local server (`tarafold serve`): authentication bypass, cross-origin or DNS-rebinding access, binding beyond the configured host.
- CSV/HTML export injection (formula injection, script injection in `report.html`).
- Supply-chain issues in release artifacts.

Out of scope: results that are mathematically wrong but not a security issue (please open a
normal issue), and running `serve --host 0.0.0.0` on an untrusted network by choice.

## Verifying releases

Each release includes `SHA256SUMS`, a CycloneDX SBOM, and GitHub build-provenance attestations:

```sh
gh attestation verify tarafold-<version>-<target>.tar.gz --repo <owner>/tarafold
```
