# Work Item: Gemini Stream Parse Errors During Tool Calls

## Problem Statement
When streaming from Gemini models (`gemini-2.5-flash-lite`, `gemini-3.1-flash`, etc.) and a tool call is generated, the stream crashes with a JSON deserialization error:
```
Gemini Adapter Stream Error: StreamParse { 
    model_iden: ModelIden { adapter_kind: Gemini, model_name: ModelName("gemini-2.5-flash-lite") }, 
    serde_error: Error("EOF while parsing a string", line: 36, column: 35) 
}
```

---

## Root Cause
The crash originates within the `genai` crate (specifically `v0.1.23` in `/src/webc/web_stream.rs`):
1. Gemini streams back responses enclosed in a JSON array format (using `StreamMode::PrettyJsonArray`).
2. Inside `new_with_pretty_json_array(buff_string, _partial_message)`, the parser **ignores** the `_partial_message` buffer argument (hence the underscore prefix).
3. If a tool call chunk (which contains complex metadata like `id`, `name`, and serialized JSON arguments) is large enough to get split across multiple TCP/HTTP packets, the adapter attempts to parse the truncated/incomplete chunk (`serde_json::from_str::<Value>(block_string)`) immediately.
4. Because the array-parser does not accumulate partial messages across stream events, it fails with `EOF while parsing a string`.

---

## Proposed Options for Resolution

### Option A: Fall back to non-streaming `step` for tool execution
* **Description**: Switch the binary runner `rope` (or the library wrapper `step_stream`) to call `bondage::step` (non-streaming) when tool use is allowed/expected. 
* **Pros**: Clean, simple, and completely avoids the SSE fragmentation bug because the engine returns a single fully completed payload.
* **Cons**: Regresses streaming UX (text won't print token-by-token).

### Option B: Accumulate streaming chunks in our translation layer
* **Description**: Capture raw streaming chunks ourselves, or intercept stream errors in `step_stream` and fallback to a single non-streaming call when `StreamParse` occurs.
* **Pros**: Keeps streaming active for pure text outputs.
* **Cons**: Messy to detect/intercept since `genai::Error` types are returned as boxed stream results.

### Option C: Contribute / Update dependency upstream
* **Description**: Submit a fix to `genai` to make `new_with_pretty_json_array` buffer partial messages across chunks, or check if a newer version of `genai` fixes this.
* **Pros**: Best long-term solution.
* **Cons**: Blocks local development until upstream dependencies are updated/released.

---

## New Issue: Gemini 3.1+ `thought_signature` 400 Invalid Argument
When using Gemini 3.1 and later (e.g. `gemini-3.1-flash-lite`), multi-turn tool calling fails with the following API error after a tool execution result is returned to the model:
```
Function call is missing a thought_signature in functionCall parts. 
This is required for tools to work correctly, and missing thought_signature may lead to degraded model performance. 
Additional data, function call `default_api:lookup` , position 2.
```

### Root Cause
1. **Stateless Save State**: Since Gemini is stateless, it returns a cryptographic Reasoning/Chain of Thought blob named `thought_signature` inside the `functionCall` part of the response when calling a tool.
2. **Mandatory Reply**: When the client replies to the model with the tool output, it **must** repeat the exact `functionCall` part (including the immutable `thoughtSignature` string) in the model history. If the signature is missing or mutated, the API returns a `400 INVALID_ARGUMENT` error.
3. **GenAI Mapping Limitation**: In `genai-0.1.23` (specifically inside `src/adapter/adapters/gemini/adapter_impl.rs`), the adapter completely ignores/drops `thought_signature` during deserialization into `ToolCall`. Furthermore, when serializing the assistant message history back to the Gemini API, it reconstructs `functionCall` manually using only `"name"` and `"args"`, discarding any signature metadata:
   ```rust
   // From genai-0.1.23: adapter_impl.rs
   "functionCall": {
       "name": tool_call.fn_name,
       "args": tool_call.fn_arguments,
   }
   ```

### Proposed Solutions
1. **Upstream Fix**: Upgrade `genai` to a newer version that supports and preserves `thought_signature`, or patch `genai` to extract and preserve `thoughtSignature` from model outputs and include it in outbound function calls.
2. **Payload Interception**: Write a custom wrapper or override the Gemini adapter logic in `util.rs` to intercept model-tool requests and preserve their raw fields, bypassing `genai`'s serialization for multi-turn histories.
3. **OpenAI Protocol Adaptation (Recommended)**: Use Google Gemini's native OpenAI-compatibility layer (`https://generativelanguage.googleapis.com/v1beta/openai/`) by configuring the CLI `adapter` format to `openai`. Because this protocol routes via the standard OpenAI format inside `genai`, it completely bypasses Gemini-specific features like `thought_signature` and `PrettyJsonArray` chunk fragmentation, permitting fully streamed multi-turn tool calling without errors.


