# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub:
**[Report a vulnerability](https://github.com/intergrado/diacraft/security/advisories/new)**
(Security tab › Advisories › Report a vulnerability).

Don't open a public issue or pull request for a vulnerability until a fix is released.

Useful to include:
- the DiaCraft version or commit, and the platform;
- what an attacker can do, and what they need (a crafted document, MCP or control-channel access, a local account);
- steps or a small file that reproduce it.

## Supported versions

Fixes go into `main` and ship in the next release. Older releases don't get backported fixes.

## Scope

In scope: anything that lets a document, file, command, MCP or control-channel request read or write files it shouldn't,
run code, leak data, or crash and lose work.
