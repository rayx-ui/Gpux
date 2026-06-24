# CRITICAL: GENERAL

Before starting a new task, review `.agentx/memory/knowledge/INDEX.md`, then the relevant domain `INDEX.md` and `state.md`, then the relevant subdomain `INDEX.md`, `state.md`, `rules.md`, and `hypotheses.md`.
Apply rules by default. Check if any hypothesis can be tested with today's work.
Every `.agentx/**/*.md` memory file must use AgentX YAML frontmatter with `kind`, `scope`, `status`, `created`, `updated`, `topics`, `supersedes`, and `superseded_by`. Treat backlinks as candidate routes, not a checklist: follow only task-relevant links, default to relevance-gated depth 1, and use `status`, explicit supersession, then newest `updated` when memories conflict on the same topic.
If a memory tree does not exist yet, or the user asks to initialize it from scratch, use the available `@agentx` skill (global or repository-local), inspect the repository architecture, create the conservative project-specific domain/subdomain tree, and record the taxonomy decision immediately.
If a task does not map cleanly to an existing subdomain, read the domain-level `rules.md` and `hypotheses.md`, create the missing subdomain only when it fits the current repository taxonomy, and update the indexes as part of the task. Avoid adding or renaming top-level domains unless the repository architecture clearly requires it; when it does, update indexes and log a decision in the same change.

**Insights:**

- At the end of each completed task, autonomously extract durable insights that future agents should reuse. Write `.agentx` memories from completed evidence and durable reuse value.
- Do not extract insights for work that is still blocked, failed, abandoned, speculative, or incomplete.
- Store insights in the current repository taxonomy under `.agentx/memory/knowledge/`:
  - `.agentx/memory/knowledge/<domain>/state.md` for the current technical state of a domain
  - `.agentx/memory/knowledge/<domain>/<subdomain>/knowledge.md` for facts and patterns
  - `.agentx/memory/knowledge/<domain>/<subdomain>/hypotheses.md` for items that need more data
  - `.agentx/memory/knowledge/<domain>/<subdomain>/rules.md` for confirmed guidance to apply by default
  - `.agentx/memory/knowledge/<domain>/<subdomain>/state.md` for the current technical state of a subdomain
- Do not use a hardcoded domain list. Derive domains from the repository architecture during initialization and then follow the taxonomy in `.agentx/memory/knowledge/INDEX.md`.
- Use domain-root `knowledge.md`, `hypotheses.md`, and `rules.md` only for guidance that truly applies across more than one subdomain in that domain
- Every active domain and subdomain folder must contain a `state.md`
- `state.md` is mandatory reading before work in that domain/subdomain and mandatory to update in the same change whenever that domain/subdomain is modified

Maintain `.agentx/memory/knowledge/INDEX.md`, the relevant domain `INDEX.md`, and `.agentx/memory/decisions/INDEX.md` so they route to the active domains and subdomains. Create the structure if it does not exist yet.
When a hypothesis gets confirmed 3+ times, promote it to a rule.
When a rule gets contradicted by new data, demote it back to a hypothesis.

When about to make a decision that affects more than today's task, first review `.agentx/memory/decisions/INDEX.md` and grep the relevant decision domain/subdomain folder for prior decisions in that area. Follow them unless new information invalidates the reasoning.
If no prior decision exists, or you are replacing one, log it:
File: `.agentx/memory/decisions/<domain>/<subdomain>/YYYY-MM-DD-<domain>-<subdomain>-{topic}.md`
Format:

```
## Decision: {what you decided}

### Context: {why this came up}

### Alternatives considered: {what else was on the table}

### Reasoning: {why this option won}

### Trade-offs accepted: {what you gave up}

### Supersedes: {link to prior decision, if replacing}
```

Durable domain knowledge and apply-by-default rules live under `.agentx/memory/knowledge/`. Keep this file focused on workflow and repository guidelines.
When starting work in a new area and you need task kickoff plus memory sync, or when the task is to prune, reorganize, or audit `.agentx/`, use the available `@agentx` skill, whether it is installed globally or in the repository.
