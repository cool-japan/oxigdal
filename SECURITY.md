# Security Policy

> **Note:** **OxiGeo** is the new name of **OxiGDAL**. v0.1.7 was the final release under
> the OxiGDAL name; development (including security fixes) continues under the **OxiGeo**
> name from v0.2.0 onward. This policy also covers the 0.1.x line published under the old
> OxiGDAL name — see below.

## Supported Versions

We release patches for security vulnerabilities in the following versions:

| Version      | Supported                                             |
| ------------ | ------------------------------------------------------ |
| OxiGeo >= 0.2.0 | :white_check_mark: active development line          |
| OxiGDAL 0.1.7 (final release under the old name) | :warning: critical fixes evaluated case-by-case |
| OxiGDAL 0.1.x (< 0.1.7) | :x: please upgrade to 0.1.7                |
| < 0.1        | :x:                                                     |

0.1.7 was the last release published under the OxiGDAL name. We do not commit to an
indefinite maintenance window for the 0.1.x line: we evaluate critical/high-severity
reports against 0.1.7 case-by-case and, where a fix is straightforward to backport,
publish a patch release. All new feature work and the primary security-maintenance line
moved to OxiGeo starting at 0.2.0.

## Reporting a Vulnerability

We take the security of OxiGeo seriously. If you believe you have found a security vulnerability, please report it to us as described below.

### Where to Report

**Please DO NOT report security vulnerabilities through public GitHub issues.**

Instead, please report them via one of the following methods:

1. **Email**: Send details to `security@cooljapan.ee`
2. **GitHub Security Advisory**: Use the [Security Advisories](https://github.com/cool-japan/oxigeo/security/advisories/new) feature

### What to Include

When reporting a vulnerability, please include the following information:

- **Type of vulnerability**: e.g., buffer overflow, SQL injection, cross-site scripting, etc.
- **Full paths of affected source files**: Include file paths and line numbers if possible
- **Location of the affected code**: Tag/branch/commit or direct URL
- **Step-by-step instructions to reproduce**: Include proof-of-concept or exploit code if available
- **Impact of the vulnerability**: What an attacker could achieve by exploiting this vulnerability
- **Any special configuration required**: Dependencies, environment setup, etc.
- **Affected versions**: Which versions of OxiGeo are impacted

### What to Expect

- **Initial response**: We will acknowledge your report within 48 hours
- **Regular updates**: We will keep you informed about our progress every 5-7 days
- **Fix timeline**: We aim to release patches for critical vulnerabilities within 30 days
- **Disclosure**: We will coordinate with you on responsible disclosure timing
- **Credit**: We will credit you in the security advisory unless you prefer to remain anonymous

## Security Update Process

1. **Vulnerability reported**: Security issue is reported privately
2. **Triage**: Security team evaluates severity and impact
3. **Fix development**: Patch is developed and tested in private
4. **Security advisory**: Draft advisory is prepared
5. **Coordinated disclosure**: Patch is released with public advisory
6. **CVE assignment**: CVE is requested if applicable

## Security Scanning

OxiGeo does not currently run scheduled CI security scans (house policy restricts
`.github/workflows/*.yml` to the `pypi-publish.yml` and `npm-publish.yml` publish
pipelines only — there is no `security.yml` workflow). Instead, security scanning is run
locally by maintainers and as part of release preparation:

- **cargo-audit**: run locally / at release time against `.cargo/audit.toml` (see below)
- **cargo-deny**: license and security compliance checks, run on demand
- **cargo-geiger**: unsafe code analysis, run on demand

If you rely on OxiGeo in production, we recommend running `cargo audit` against your own
lockfile on your own schedule rather than assuming upstream CI coverage.

### Allowlisted advisories

`.cargo/audit.toml` maintains an explicit, commented allowlist of advisories that
`cargo audit` would otherwise flag. As of the 0.1.7 release this allowlist covers 21
advisories, all transitive (pulled in by a dependency several levels removed from OxiGeo
code, with no upstream fix available yet or no fixed version published), grouped roughly
as:

- **TLS/certificate-validation edge cases** in `aws-lc-sys` / `rustls-webpki` /
  `rustls-pemfile`, reached only when the optional `cloud`/`security`/`tls` feature set
  pulls in the AWS/Azure SDKs or the TLS stack
- **Cloud/DB client crates** (`azure_core`/`http-types`, `tokio-postgres`/
  `postgres-protocol` via the optional `postgis` feature) — panics or DoS vectors in
  malformed-response parsing, not memory-unsafety
- **`quick-xml` DoS-class advisories** (unbounded namespace-declaration allocation;
  quadratic-runtime duplicate-attribute-name checking) — CPU/memory exhaustion, not
  memory-unsafety — reached via `pprof`/`inferno` (default dependency of
  `oxigeo-dev-tools`/`oxigeo-bench`) and via `azure_core` (non-default `azure`/
  `azure-blob` features of `oxigeo-cloud`/`oxigeo-cloud-enhanced`)
- **Unmaintained-but-unpatched** crates reached transitively (`fxhash`, `instant`, `json`,
  `paste`, `proc-macro-error2`, `atomic-polyfill`, `rand` 0.7.3) via `sled`, `heapless`/
  `proj`, `evcxr`/Jupyter, `nalgebra`/`scirs2`, `mysql_async`, and `azure_core`
  respectively
- **`rsa` timing side-channel** (RUSTSEC-2023-0071, the Marvin Attack) — no fixed version
  exists upstream yet

None of these are reachable through OxiGeo's default (Pure-Rust, no-cloud) feature set.
Each entry in `.cargo/audit.toml` carries a one-line justification; consult that file for
the authoritative, currently-ignored advisory IDs, and re-run `cargo audit` yourself before
enabling `cloud`, `security`/`tls`, or `postgis` in a security-sensitive deployment.

## Security Best Practices for Users

### Dependency Management

- Keep OxiGeo and all dependencies up to date
- Review security advisories regularly
- Use `cargo audit` to check for vulnerabilities
- Pin critical dependencies in production

### Safe Usage

- **Input validation**: Always validate user-provided data before processing
- **Resource limits**: Set appropriate limits for memory and processing
- **Error handling**: Never expose internal errors to end users
- **Unsafe code**: Review all uses of `unsafe` blocks carefully
- **Credentials**: Never hardcode credentials or secrets

### Feature Flags

OxiGeo follows the **Pure Rust Policy**. Some features may include C/Fortran dependencies:

- Default features are 100% Pure Rust
- Optional C/Fortran dependencies are feature-gated
- Review enabled features for security implications

### WASM Considerations

When using OxiGeo in WebAssembly:

- Validate all input from JavaScript
- Be aware of browser security policies
- Use Content Security Policy (CSP) headers
- Limit memory usage in WASM modules

## Known Security Considerations

### Unsafe Code

OxiGeo minimizes the use of `unsafe` code, but some is necessary for performance:

- All `unsafe` blocks are documented with safety comments
- Regular audits are performed using `cargo-geiger`
- Consider reviewing unsafe usage before deployment

### Memory Safety

OxiGeo is written in Rust, which provides memory safety guarantees:

- No buffer overflows or use-after-free bugs in safe code
- Thread safety enforced by the type system
- All unsafe code is carefully reviewed

### Denial of Service (DoS)

Be aware of potential DoS vectors:

- **Large files**: Processing extremely large geospatial files may consume significant memory
- **Malformed data**: Corrupted or malicious files may cause excessive processing
- **Recursive structures**: Deeply nested structures may cause stack overflow

Mitigations:

- Implement resource limits in your application
- Validate file sizes before processing
- Set timeouts for operations
- Use streaming APIs for large datasets

## Dependency Security

### Trusted Dependencies

OxiGeo primarily uses well-maintained dependencies from the Rust ecosystem:

- **Arrow/Parquet**: Apache Arrow ecosystem for data processing
- **tokio**: Async runtime from the Tokio project
- **serde**: Serialization framework

### COOLJAPAN Ecosystem

OxiGeo may use COOLJAPAN ecosystem crates:

- **OxiBLAS**: Pure Rust BLAS implementation
- **Oxicode**: Pure Rust serialization (alternative to bincode)
- **SciRS2**: Scientific computing libraries

These are developed with the same security standards as OxiGeo.

### Supply Chain Security

We protect against supply chain attacks:

- All dependencies are from crates.io or trusted sources
- `cargo-deny` enforces allowed registries
- Checksum verification for all dependencies
- Regular security audits

## Vulnerability Disclosure Policy

### Our Commitment

- We will investigate all legitimate reports
- We will not pursue legal action against researchers who:
  - Report vulnerabilities responsibly
  - Avoid privacy violations and service disruption
  - Follow coordinated disclosure guidelines

### Timeline

- **Critical vulnerabilities**: Patched within 7-30 days
- **High severity**: Patched within 30-60 days
- **Medium/Low severity**: Patched in next regular release

### Public Disclosure

- Security advisories are published on GitHub Security Advisories
- CVEs are requested for significant vulnerabilities
- Fixes are backported to supported versions when possible

## Contact

For security-related questions or concerns:

- **Email**: security@cooljapan.ee
- **GitHub**: [Security Advisories](https://github.com/cool-japan/oxigeo/security/advisories)
- **Project Homepage**: https://github.com/cool-japan/oxigeo

## Acknowledgments

We thank the security researchers who have responsibly disclosed vulnerabilities to us. Contributors will be acknowledged in our security advisories unless they prefer to remain anonymous.

---

**Last Updated**: July 2026
**Author**: COOLJAPAN OU (Team Kitasan)
**License**: Apache-2.0
