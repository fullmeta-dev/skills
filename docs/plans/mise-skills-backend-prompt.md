Implement the mise-managed Agent Skills feature through GitHub issues #1–#6 in
fullmeta-dev/skills. Read AGENTS.md,
docs/plans/mise-skills-backend-plan.md, and
docs/research/skill-discovery-and-catalog-design.md completely before work.

Treat the plan and settled design as the contract. Preserve existing plan and
research changes; do not redesign the architecture or widen scope.

 Work the dependency frontier:
 - Complete and merge #1 first.
 - Then implement #2 and #3 in parallel in isolated worktrees, assigning one
   owner per worktree and resolving any integration seams deliberately.
 - Then complete #4, #5, and #6 in order.

For every issue:
1. Read its full body and linked source material.
2. Implement the complete scope and acceptance criteria, with proportionate
   unit, fixture, and isolated integration coverage.
3. Use subagents for focused implementation, source verification, and two
   independent review passes: plan/spec compliance and code-quality/regression.
4. Fix substantiated review findings, run the relevant tests, and inspect the
   final diff.
5. When the issue is complete and verified, merge its branch into `main` in
   dependency order. Leave a concise issue comment with validation evidence,
   then close the issue. Do not create PRs unless they are needed for a real
   review or merge constraint.

Do not start a blocked issue early except for non-conflicting preparatory work.
At the end of #6, run a clean isolated end-to-end flow covering bootstrap,
source configuration, install, discovery, all supported agent activation,
status, upgrade, removal, and recovery. The final result must be a complete,
correct `mise skills` implementation, not partial scaffolding.
