# git-ai 

<img src="assets/docs/badges/claude_code.svg" alt="Claude Code" height="25" /> <img src="assets/docs/badges/codex-black.svg" alt="Codex" height="25" /> <img src="assets/docs/badges/continue.svg" alt="Continue" height="25" /> <img src="assets/docs/badges/copilot.svg" alt="GitHub Copilot" height="25" /> <img src="assets/docs/badges/cursor.svg" alt="Cursor" height="25" /> <img src="assets/docs/badges/droid.svg" alt="Droid" height="25" /> <img src="assets/docs/badges/gemini.svg" alt="Gemini" height="25" /> <img src="assets/docs/badges/junie_white.svg" alt="Junie" height="25" /> <img src="assets/docs/badges/opencode.svg" alt="OpenCode" height="25" /> <img src="assets/docs/badges/rovodev.svg" alt="Rovo Dev" height="25" />

<img src="https://github.com/git-ai-project/git-ai/raw/main/assets/docs/git-ai.png" align="right"
     alt="Git AI Logo" width="140" height="140">

Git AI is an open source git extension that keeps track of the AI-generated code in your repositories. While you work each AI line is transparently linked to the agent, model plan/prompts; so that the intent, requirements and architecture decisions is preserved. 

* **Cross Agent AI Blame** - our [open standard](https://github.com/git-ai-project/git-ai/blob/main/specs/git_ai_standard_v3.0.0.md) for tracking AI-attribution is supported by every major coding agent. 
* **Save your prompts** - saving the context behind every line makes it possible to review, maintain and build on top of AI-generated code. Securely store your team's prompts on your own infrastructure. 
* **No workflow changes** - Just prompt, edit and commit. Git AI accuratly tracks AI-code without making your git history messy. Attributions live in Git Notes and survive squash, rebase, reset, stash/pop cherry-pick etc.


```bash
$ git commit

you  ██████████████████████████░░░░░░░░░░░░░░ ai
     65%                                  35%
     84% AI code accepted

[fix: sqlite migration 34fb9ed9]
 23 files changed, 203 insertions(+)
```


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


















### Documentation https://usegitai.com/docs
- [AI Blame](https://usegitai.com/docs/cli/ai-blame)
- [Cross-Agent Prompt Saving](https://usegitai.com/docs/cli/prompt-storage)
- [CLI Reference](https://usegitai.com/docs/cli/reference)
- [Configuring Git AI for the enterprise](https://usegitai.com/docs/cli/configuration)

### Just Install and Commit

Build as usual. Just prompt, edit and commit. Git AI will track every line of AI-Code and record the Coding Agent, Model, and prompt that generated it. 

<img src="https://github.com/git-ai-project/git-ai/raw/main/assets/docs/graph.jpg" width="400" />

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
