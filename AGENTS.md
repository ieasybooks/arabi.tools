# AGENTS.md

Read `README.md` for setup and project context. Use the toolchain pinned in `rust-toolchain.toml`. Install configured development tools with `mise install`.

## Workflow and communication

- Check relevant code and documentation before asking factual questions. Ask the user about intent and preferences when those remain unclear.
- Do not invent requirements or silently choose unclear behavior. Ask before deciding limits, user-facing text, defaults, fallback behavior, or compatibility changes when the user has not settled them. Pause work that depends on the answer while continuing independent, authorized work.
- Ask focused questions early. Give concise choices and explain their consequences. Clearly distinguish recommendations from approved decisions.
- Carry the user's answers forward as constraints. Do not reopen settled decisions or treat silence as approval.
- Use simple English in explanations, comments, and discussions. Keep progress updates focused on what you learned, what needs a decision, and what remains to verify. Avoid narrating every command.
- Before declaring completion, challenge the approach, review the final diff for bugs and unrelated changes, and verify the requested behavior in a real browser with realistic inputs. Report what changed, what was tested, and any remaining uncertainty.
- If a repeated mistake suggests a useful rule for `AGENTS.md`, propose the specific improvement and wait for user approval before adding it.

## Verification

Run focused tests while iterating. Before handing off code changes, run these checks from the repository root:

```sh
cargo fmt --all --check
topcoat fmt --check src
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Tests require `ffmpeg` and `ffprobe` on PATH. They use a local mock of Wit and need no real tokens. Check every user-facing change in a browser, in both Arabic and English. Report any checks you could not run.

## Working style

- Prefer simple, straightforward code that is easy to read and maintain. Use clear names and direct control flow. Add abstractions and dependencies only when they simplify current requirements.
- Reproduce bugs before fixing them, address the root cause, and add a regression test when practical. Test observable behavior through the relevant interface.
- Do not hard-wrap prose in documentation or Markdown files. Keep each paragraph on one source line; preserve structural newlines and code-block formatting.
- Keep `README.md` concise: what the site does, how to run it, and links. Put deployment details in `docs/`.
- When adding or upgrading dependencies and tools, verify and use the latest stable releases from official sources. Update the relevant pins and lockfiles.

## Project conventions

- The site hosts several tools. Shared pieces (layout, translations, the job queue) must not assume a single tool. Each tool lives in its own module under `src/tools/` and its own route module under `src/app/`.
- Every user-facing string lives in `src/i18n.rs`, in Arabic and English. Arabic is the default and the page direction follows the language.
- Routes use Topcoat's module router: a file under `src/app/` maps to the URL of its module path.
- Run `topcoat fmt` after editing `view!` markup.
- Use Conventional Commits for commits and PR titles. Release-please manages versions and the changelog; a published release builds the image and deploys it.
