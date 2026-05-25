# Security Policy

Cortex is pre-1.0 alpha software. We take security reports seriously and
appreciate responsible disclosure.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

- Preferred: use GitHub's **private vulnerability reporting** ("Report a
  vulnerability" under the repository's *Security* tab).
- Or email the maintainer: **ammar.siddiqui847@gmail.com**.

Include repro steps, affected surface (library / core server / desktop / web),
and impact. We aim to acknowledge within a few days; please allow reasonable
time to investigate and fix before public disclosure.

## Supported versions

Only the latest `main` / most recent release is supported during alpha. There
are no backported security fixes for older `0.x` tags yet.

## Scope worth knowing

A few areas warrant extra care when evaluating or deploying Cortex:

- **Agent harness** can act on the user's behalf, up to file persistence.
  Default to the least-privileged level and treat escalation as sensitive.
- **API keys** (`ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`) live in your local
  `.env` for the LLM bridge. Never commit `.env`; only `.env.example` is
  tracked.
- **Vault/knowledge-base ingestion** reads a user-supplied folder path. Be
  mindful of what you point it at.
- **Data egress**: Cortex is local-first. The `local-embedding` encoder sends
  nothing to third parties; the Haiku / Gemini encoders send prompt text to
  Anthropic / Google respectively.
