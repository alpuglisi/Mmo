# Playtest 10 brief (shared by all six players)

You are an AI playtester for "Evolution MMO", the game being built in /home/user/Mmo, testing it for its owner. Five other AI players are on the same server, and each of you is trying to WIN while recording every problem you hit. Nine earlier playtests led to many fixes: read docs/playtest-9.md (and docs/changelog-playtests.md), which lists what was found, and judge whether those problems are fixed and what is still wrong.

Server: http://127.0.0.1:8101. It is already running and serves the web client. A game day is about 5 s and a year about 10 min.

Rules:
- Do NOT start or stop the server.
- Do NOT edit or commit anything in the repo.
- Do NOT run cargo builds.
- Don't create any account beyond your own.
- No floods and no load testing.
- You may read the repo for reference: README, docs/game-design.md, client/src, crates/server/src/http.rs and protocol, and client/e2e/*.mjs, which show how to drive the client with Playwright. Playwright is preinstalled: `require('playwright')` works and `chromium.launch()` needs no options.

Machine limits (4 shared cores, 6 players):
- Keep at most ONE headless browser open.
- Cap its frame rate with client/e2e/frames.mjs (5 fps).
- Use deviceScaleFactor 1.
- Between check-ins, prefer the HTTP API and small node scripts. Close the browser when you are not using the UI.
- Give any background script a name unique to you (e.g. `<YourName>-watch.mjs`), and kill only by PID. Never use pkill by pattern.

How to play: use the real web client as a human would (register, design, found, map, tabs, orders, market, ranks), plus the API like a power user. Play for about 70 minutes of wall-clock time, reacting to events. If your lineage fails, retire it and re-found.

Notes are the most important output.
- Keep a running log at `/tmp/claude-0/-home-user-Mmo/a9a7624c-2561-5b05-9349-61edc349657f/scratchpad/playtest10/<YourName>.md`.
- About every 5 minutes, overwrite a one-line status in `.../playtest10/<YourName>.status`, e.g. "day 63 · alive 212 · share 18% · rank 2/7 · doing: X".
- Record, with timestamps (wall clock and game day):
  - every bug, error, broken or confusing piece of UI, and wrong number: exact steps, what you expected and what you saw, console errors, failed requests;
  - balance, fairness and exploit concerns;
  - UX friction;
  - for each playtest-9 problem relevant to you: fixed, still present, or partly fixed.
- Rate the severity of each issue High, Medium or Low. High means it breaks the game, is unfair, is an exploit, or loses players' progress. Medium means a significant annoyance or imbalance. Low means cosmetic or wording.
- Screenshots go in `.../playtest10/shots/<YourName>-*.png`.

If the server stops responding for more than about 2 minutes, note it, retry for up to 10 minutes, then wrap up.

At the end, return a concise report:
- your final standing;
- a verification list for the playtest-9 issues;
- new problems, each with severity and steps to reproduce.
