# Security policy

## Reporting a vulnerability

Please report security issues privately through GitHub:
[Report a vulnerability](https://github.com/emre6943/Kesit/security/advisories/new)
(Security tab → Advisories → Report a vulnerability). Do not open a public issue.

Include the Kesit version (`kesit --version`), your operating system and, if you can, steps to
reproduce. You should get a first reply within a week. Fixes are released as a new version and
credited in the advisory unless you prefer otherwise.

## Supported versions

Only the latest release receives security fixes.

## Scope

Kesit runs locally and makes no network connections. Relevant issues include, for example,
handling of untrusted image files (`kesit edit`), the config file, files Kesit writes (saved
images, the login item, lock files) and the release and install scripts.
