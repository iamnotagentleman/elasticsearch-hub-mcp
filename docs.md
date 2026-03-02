## Environment Selection Rule

When a user requests an Elasticsearch query without specifying an environment, always **ask** which environment they want to query before running any searches.

If the user says "prod" or "production", use the production instance. If they say "qa", "test", or "dev", use the QA/test instance. If ambiguous, ask before proceeding.



## Post-Query Checklist

After completing a user's Elasticsearch query request, always check if there's anything worth saving to memory:

1. **New field discoveries** — Did you learn about new fields, mappings, or data structures? → `write_memory(type="info")`
2. **Query patterns that worked** — Did you find a useful query pattern for this index/instance? → `write_memory(type="info")`
3. **Gotchas or surprises** — Did something unexpected happen (wrong field type, missing data, misleading field names)? → `write_memory(type="lessons_learned")`
4. **Recurring error patterns** — Did you identify a known error pattern or root cause? → `write_memory(type="lessons_learned")`
5. **Instance-specific knowledge** — Did you learn which instance/index to use for a specific type of query? → `write_memory(type="info")`

Only write genuinely useful memories — not every query result.
