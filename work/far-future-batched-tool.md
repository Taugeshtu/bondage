far future: batched tool calls. On one hand, we probably should; on the other - agents are usually very apprehensive about having multiple reads. I think probably better approach would be to afford an agent batch variants of tools, but that's cognitive load... Probably not worth it at the current tech level.

More context:
**B9. Parallel tool execution.**
`to_genai_messages` groups consecutive `ModelToolRequest`s into a single assistant message (parallel tool calls to the engine). But both the current loops and the `step_agent` execute tool calls sequentially in a `for` loop. Should `step_agent` parallelize execution? Not addressed.
> @rope-done: **Don't parallelize. Not now. Here's the reasoning:**

The model *requests* parallel tool calls (multiple `ModelToolRequest` messages), and `to_genai_messages` correctly groups them into one assistant turn for the *next* LLM call — that's a message-format concern, not an execution-ordering concern. Those are separate problems. The grouping just tells the engine "these calls were part of one turn," it says nothing about whether they ran simultaneously.

The tools we have (`lookup`, `write`, `bash`) have strong side-effect ordering concerns: `write` then `lookup` on the same file, `bash` that creates a directory then `write` into it, etc. Parallelizing these would introduce race conditions and non-deterministic failures. Worse, the approval flow (the main thing `ToolExecutor` does) is inherently interactive and sequential — you can't ask a user to approve 3 tool calls at once on a terminal. `RopeExecutor` prints status messages, previews, and prompts that would interleave chaotically if parallelized. The benefit (faster execution) is marginal for this tool set, and the cost (race conditions, interleaved console output, concurrent approval prompts) is high.

If we ever need parallelism (e.g., batch `lookup` calls with no side effects), the right place is *inside* the executor — the executor can choose to fan out if it knows the calls are independent. `step_agent` shouldn't force it. Leave the `for` loop as-is. This is a non-issue for the current architecture.

