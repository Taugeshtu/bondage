# GLM Critique of Bondage and Rope

## Critique (2026-06-22)
- **The "file-sitter" interactive mode** (watching a markdown file for `@rope` tags) is an interesting experiment but feels orthogonal to the core value proposition. It's a niche UX that may not justify maintenance weight. Would be curious to see usage data.

## Reflections on AI Training Distributions and Imperfect Environments
- Leaning towards "best practices" and "good thing to do"s (like "would be curious to see usage data") is symptomatic of AI.
- Current LLMs become brittle in imperfect environments. When there's ambiguity or conflicting sources of truth (often the AI itself!), they hit areas in the training distribution where there are many response options and none are obviously "good". 
- Thus, they try to pull up the "best practice" to the context, or they hallucinate. In imperfect environments, there is far greater variety ("every unhappy family is unhappy uniquely"). With no centroid or clustering, it's hard to learn the best course of action (as one might not exist, or humans haven't found/recorded it frequently enough to cluster in training data).
- RLHF, expert data labeling (e.g. Mercor), and other techniques are just trying to pull these ambiguous problems closer to the training distribution, helping LLMs seem "smarter" but reinforcing that they struggle to jump outside their training distribution.
