# Security policy

pacod-benchmark holds a platform-admin token of every gateway it manages and the token of the
load agents it drives. Reports of weaknesses are welcome and are handled privately until a fix is
out.

## Reporting a vulnerability

Do not open a public issue. Use GitHub's private vulnerability reporting: the repository's
**Security** tab, then **Report a vulnerability**.

Please include:

- the version or commit, and which part is affected: the API (`pacod-bench serve`), the
  controller, the command line, the UI, or the deployment files;
- what an attacker needs (a benchmark role, network access to the API or the agents, nothing);
- the steps to reproduce, and what happens compared with what should happen;
- whether you have told anyone else.

Never send real gateway tokens, agent tokens or keys that are not yours. Placeholders are enough.

## What happens next

The maintainers confirm that the report arrived, agree on a disclosure date with you, and tell
you when the fix is released. You are credited in the release notes unless you prefer not to be.

There are no fixed response times; the maintainers answer as soon as they can.

## Supported versions

Until version 1.0, only the latest release receives security fixes.

## Scope

In scope: the code in this repository and the deployment files it ships, with their defaults.

A run acts on a managed gateway with the benchmark's admin token, so a holder of
`bench.runs.start` reaches what that token allows through a run's scene; the threat model lists
this trade-off and what limits it. Reports that only restate it are out of scope; a way past the
limits it lists is in.

Out of scope: pacod-gw and pacod-id themselves (report there), and settings an operator chose
against the documentation, such as the API published beyond pacod-admin-net or `BENCH_AUTH=dev`
outside development.

What the benchmark protects against, and how, is described in
[docs/threat-model.md](docs/threat-model.md).
