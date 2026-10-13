# Security Policy

## Supported versions

Security fixes are made for the latest release only.

## Reporting a vulnerability

**Please don't open a public issue for security problems.**

Report them privately through GitHub: go to the repository's **Security** tab and choose **Report a vulnerability**. Include:

- what the problem is and what an attacker could do with it
- steps or a file that reproduces it
- the LiteSheet version and your macOS version

You should get a reply within a week. We'll keep you updated while we work on a fix and credit you in the release notes, unless you'd rather stay anonymous.

## What counts as a vulnerability

LiteSheet opens files from anywhere and stores an API key, so these areas matter most:

- **Malicious files:** a crafted `.xlsx`, `.xls`, `.ods` or `.csv` that crashes the app, hangs it, or runs code
- **The API key:** anything that leaks it, for example into logs, files, settings or requests to the wrong host
- **Data sent to AI providers:** requests that include more spreadsheet data than the panel says, or go to a host other than the configured base URL
- **Unexpected network access:** any request other than the AI requests the user starts

LiteSheet has no server component, telemetry or accounts.
