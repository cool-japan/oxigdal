# Security Policy

## Supported Versions

We release patches for security vulnerabilities in the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

## Reporting a Vulnerability

We take the security of OxiGDAL seriously. If you believe you have found a security vulnerability, please report it to us as described below.

### Where to Report

**Please DO NOT report security vulnerabilities through public GitHub issues.**

Instead, please report them via one of the following methods:

1. **Email**: Send details to `security@cooljapan.ee`
2. **GitHub Security Advisory**: Use the [Security Advisories](https://github.com/cool-japan/oxigdal/security/advisories/new) feature

### What to Include

When reporting a vulnerability, please include the following information:

- **Type of vulnerability**: e.g., buffer overflow, SQL injection, cross-site scripting, etc.
- **Full paths of affected source files**: Include file paths and line numbers if possible
- **Location of the affected code**: Tag/branch/commit or direct URL
- **Step-by-step instructions to reproduce**: Include proof-of-concept or exploit code if available
- **Impact of the vulnerability**: What an attacker could achieve by exploiting this vulnerability
- **Any special configuration required**: Dependencies, environment setup, etc.
- **Affected versions**: Which versions of OxiGDAL are impacted

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

OxiGDAL uses automated security scanning:

- **cargo-audit**: Daily scans for known vulnerabilities in dependencies
- **cargo-deny**: License and security compliance checks
- **Dependabot**: Automated dependency updates
- **cargo-geiger**: Unsafe code analysis

See our [Security GitHub Action](.github/workflows/security.yml) for details.

## Security Best Practices for Users

### Dependency Management

- Keep OxiGDAL and all dependencies up to date
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

OxiGDAL follows the **Pure Rust Policy**. Some features may include C/Fortran dependencies:

- Default features are 100% Pure Rust
- Optional C/Fortran dependencies are feature-gated
- Review enabled features for security implications

### WASM Considerations

When using OxiGDAL in WebAssembly:

- Validate all input from JavaScript
- Be aware of browser security policies
- Use Content Security Policy (CSP) headers
- Limit memory usage in WASM modules

## Known Security Considerations

### Unsafe Code

OxiGDAL minimizes the use of `unsafe` code, but some is necessary for performance:

- All `unsafe` blocks are documented with safety comments
- Regular audits are performed using `cargo-geiger`
- Consider reviewing unsafe usage before deployment

### Memory Safety

OxiGDAL is written in Rust, which provides memory safety guarantees:

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

OxiGDAL primarily uses well-maintained dependencies from the Rust ecosystem:

- **Arrow/Parquet**: Apache Arrow ecosystem for data processing
- **tokio**: Async runtime from the Tokio project
- **serde**: Serialization framework

### COOLJAPAN Ecosystem

OxiGDAL may use COOLJAPAN ecosystem crates:

- **OxiBLAS**: Pure Rust BLAS implementation
- **Oxicode**: Pure Rust serialization (alternative to bincode)
- **SciRS2**: Scientific computing libraries

These are developed with the same security standards as OxiGDAL.

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
- **GitHub**: [Security Advisories](https://github.com/cool-japan/oxigdal/security/advisories)
- **Project Homepage**: https://github.com/cool-japan/oxigdal

## Acknowledgments

We thank the security researchers who have responsibly disclosed vulnerabilities to us. Contributors will be acknowledged in our security advisories unless they prefer to remain anonymous.

---

**Last Updated**: January 2026
**Author**: COOLJAPAN OU (Team Kitasan)
**License**: Apache-2.0
