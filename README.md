> I'll write my own harness, with blackjack and hookers!

Bondage is an embeddable, stateless library that binds an LLM engine to execution context and tools. You bring the engine and history; Bondage provides the run-loop translation and local action capabilities.

### Just NO:

- Not a harness. Minimal CLI app sidecared as a welcome bonus
- No hidden state. Manage the genai client and messages how you want, Bondage only adds 1 (one) extra handle for ergonomics
- Not a policy engine. Your code owns tool allow/deny policy. Minimal policy kit offered as courtesy

### Afforded Tools

Bondage provides minimal set of high-leverage tools:

1. **`lookup`:** Examine a local file (whole or anchor-matched with context), list/grep directories recursively, or fetch remote webpage text.
2. **`write`:** Overwrite a file completely or apply a patch.
3. **`bash`:** Execute arbitrary commands in a shell.

---

### Integration Example

// here we'll just send the reader to our minimal CLI app's core

---

For the CLI companion integration details, see [Rope](docs/Rope.md).
