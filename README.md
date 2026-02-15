# git-ai 

<img src="https://github.com/git-ai-project/git-ai/raw/main/assets/docs/git-ai.png" align="right"
     alt="Git AI Logo" width="140" height="140">

Git AI is an open source git extension that keeps track of the AI-generated code in your repositories. While you work each AI line is transparently linked to the agent, model plan/prompts; so that the intent, requirements and architecture decisions is preserved. 

* **Cross Agent AI Blame** - our [open standard](https://github.com/git-ai-project/git-ai/blob/main/specs/git_ai_standard_v3.0.0.md) for tracking AI-attribution is supported by every major coding agent. 
* **Save your prompts** - saving the context behind every line makes it possible to review, maintain and build on top of AI-generated code. Securely store your team's prompts on your own infrastructure. 
* **No workflow changes** - Just prompt, edit and commit. Git AI accuratly tracks AI-code without making your git history messy. Attributions live in Git Notes and survive squash, rebase, reset, stash/pop cherry-pick etc.




> Supported Agents:
> 
> <img src="assets/docs/badges/claude_code.svg" alt="Claude Code" height="25" /> <img src="assets/docs/badges/codex-black.svg" alt="Codex" height="25" /> <img src="assets/docs/badges/cursor.svg" alt="Cursor" height="25" /> <img src="assets/docs/badges/opencode.svg" alt="OpenCode" height="25" /> <img src="assets/docs/badges/gemini.svg" alt="Gemini" height="25" /> <img src="assets/docs/badges/copilot.svg" alt="GitHub Copilot" height="25" /> <img src="assets/docs/badges/continue.svg" alt="Continue" height="25" /> <img src="assets/docs/badges/droid.svg" alt="Droid" height="25" /> <img src="assets/docs/badges/junie_white.svg" alt="Junie" height="25" /> <img src="assets/docs/badges/rovodev.svg" alt="Rovo Dev" height="25" />
>
> [+ Add support for another agent](https://usegitai.com/docs/cli/add-your-agent)

## Install

#### Mac, Linux, Windows (WSL)

```bash
curl -sSL https://usegitai.com/install.sh | bash
```

#### Windows (non-WSL)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://usegitai.com/install.ps1 | iex"
```

🎊 That's it! **No per-repo setup.**

--- 

## AI-Blame 

Git AI blame is a drop-in replacement for git blame that reports the AI attribution for each line: 

```bash
git-ai blame /src/log_fmt/authorship_log.rs
```
<img width="1526" height="808" alt="image" src="https://github.com/user-attachments/assets/e1f2bcbe-d990-4932-92fc-55a7477a2416" />

### IDE Plugins 

In VSCode, Cursor, Windsurf and Antigravity the [Git AI extension](https://marketplace.visualstudio.com/items?itemName=git-ai.git-ai-vscode) shows see AI-blame decorations in the gutter color-coded by the session that generated those lines. 

Also availible in: 
- Emacs magit - https://github.com/jwiegley/magit-ai
- *...have you built support into another editor? Open a PR and we'll add it here*  

| Color-coded by Agent Session | Read the prompts / summaries |
|---|---|
| <img width="1192" height="890" alt="image" src="https://github.com/user-attachments/assets/94e332e7-5d96-4e5c-8757-63ac0e2f88e0" /> | <img width="1206" height="469" alt="image" src="https://github.com/user-attachments/assets/cc87f99d-208d-4007-b156-8ea9be4d6141" /> |

## Understand why with the `/ask` skill

See something you don't understand? The /ask skill lets you talk to the agent who wrote the code — its instructions, its decisions, the engineer's intent. Git AI gives you the context you need to maintain and build on top of the massive volume of AI-generated code flooding your codebases.

Just type 
```
/ask Why didn't we use the Sentry SDK here?
```

<details>

<summary>See the result </summary>

| With /ask | Just reading the code (all slop) |
|---|---|
| **Answer (from the original author's perspective):**<br><br>I wrote the log flushing this way because I needed a **background, non-blocking approach** to uploading telemetry. The pattern works in two stages:<br><br>1. **Write locally first** — during normal operation, log events (errors, metrics, messages) are appended to local envelope files via `append_envelope()`. This keeps the hot path fast and avoids blocking on network I/O.<br>2. **Flush in background** — `spawn_background_flush()` spawns a detached `flush-logs` subprocess that reads those local files and uploads them to Sentry and PostHog. This runs after key events (commits, checkpoints, login) without slowing down the user.<br><br>**Prompt ID:** `fff3b6ae44d4a9b2` | The thin wrapper in `src/commands/flush_logs.rs` is a **routing layer / module boundary convention**. Here's why it's structured this way:<br><br>**The pattern:** `src/commands/` modules act as the CLI command dispatch layer, while the actual logic lives in domain-specific modules (`src/observability/`).<br><br>— `src/commands/flush_logs.rs` — the command handler entry point (5 lines)<br>— `src/observability/flush.rs` — the real implementation (~700 lines of Sentry, PostHog, metrics upload, log processing, etc.)<br><br>**Why the indirection:**<br><br>1. **Separation of concerns** — `commands/` is responsible for routing CLI subcommands to their handlers. `observability/` owns the telemetry domain logic. The thin wrapper keeps the command registry clean without leaking domain internals into the command layer.<br>2. **Consistency** — looking at `src/commands/mod.rs`, there are parallel modules like `flush_cas`, `flush_logs`, `flush_metrics_db`. They all follow the same pattern of being thin dispatch points. This makes the command structure predictable.<br>3. **Future flexibility** — if you ever need to add CLI-level concerns to flush-logs (arg parsing, output formatting, error handling at the command boundary), you have a natural place to put it without touching the observability internals.<br><br>That said, it's arguably over-indirection for a 1-line passthrough. You could just call `crate::observability::flush::handle_flush_logs(args)` directly from wherever the CLI dispatches commands and skip the wrapper entirely. |

</details>





## Make your Agents Smarter




--- 

### Measure the actual impact of AI-code




#### How Does it work? 

Supported Coding Agents call Git AI and mark the lines they insert as AI-generated. 

On commit, Git AI saves the final AI-attributions into a Git Note. These notes power AI-Blame, AI contribution stats, and more. The CLI makes sure these notes are preserved through rebases, merges, squashes, cherry-picks, etc.

![Git Tree](https://github.com/user-attachments/assets/edd20990-ec0b-4a53-afa4-89fa33de9541)

The format of the notes is outlined here in the [Git AI Standard v3.0.0](https://github.com/git-ai-project/git-ai/blob/main/specs/git_ai_standard_v3.0.0.md)

## Goals of `git-ai` project

🤖 **Track AI code in a Multi-Agent** world. Because developers get to choose their tools, engineering teams need a **vendor agnostic** way to track AI impact in their repos.

🎯 **Accurate attribution** from Laptop → Pull Request → Merged. Claude Code, Cursor and Copilot cannot track code after generation—Git AI follows it through the entire workflow.

🔄 **Support real-world git workflows** by making sure AI-Authorship annotations survive a `merge --squash`, `rebase`, `reset`, `cherry-pick` etc.

🔗 **Maintain link between prompts and code** - there is valuable context and requirements in team prompts—preserve them alongside code.

🚀 **Git-native + Fast** - `git-ai` is built on git plumbing commands. Negligible impact even in large repos (&lt;100ms). Tested in [Chromium](https://github.com/chromium/chromium).







## Installing the Stats Bot (early access)

Aggregate `git-ai` data at the PR, developer, Repository and Organization levels:

- AI authorship breakdown for every Pull Request
- Measure % of code that is AI generated through the entire SDLC
- Compare accepted-rate for code written by each Agent + Model. 
- AI-Code Halflife (how durable is the AI code)
> [Get early access by chatting with the maintainers](https://calendly.com/acunniffe/meeting-with-git-ai-authors)

![alt](https://github.com/git-ai-project/git-ai/raw/main/assets/docs/dashboard.png)
