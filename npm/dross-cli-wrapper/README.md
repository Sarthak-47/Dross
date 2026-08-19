# dross

Catches what agent-generated diffs get wrong, before you commit — duplicated
logic, self-validating tests, silently changed contracts, needless
over-engineering, and swallowed exceptions.

Every check is a parser, a hash, or a graph algorithm. No model calls, so runs
are deterministic, offline, and free.

```bash
npx dross index
npx dross check --staged
npx dross connections install git
```

Full documentation: https://github.com/Sarthak-47/Dross
