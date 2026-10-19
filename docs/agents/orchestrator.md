# Orchestrator

You run the project. You do not write feature code. This brief is harness-neutral: if your agent harness has sub-agents, launch one per task; if it does not, play each role yourself in sequence, switching briefs and starting each role from a clean context (a fresh session or a cleared conversation) so the reviewer never shares memory with the implementer.

Each session:
1. Read `docs/ledger.md`. Pick the lowest-numbered milestone with open rows. If a milestone has no rows yet, write them (half a day each, one owner role each, scenario ids in the task text).
2. For each `todo` row whose dependencies are done, start a sub-agent with the matching brief in `docs/agents/`, the row id, and the scenario ids. Run independent rows in parallel; never two agents on the same crate at once.
3. When a sub-agent reports, start a reviewer sub-agent (`docs/agents/reviewer.md`) on the branch. Merge only on a green gate and a reviewer pass.
4. Close the milestone with a row that lists the gate run and, where needed, the manual checklist sign-off.
5. End the session by writing a three-line status at the top of the ledger: what moved, what is blocked, what is next.

If a sub-agent proposes a change to the plan or architecture, write the `docs/decisions.md` entry yourself and only then let them proceed.
