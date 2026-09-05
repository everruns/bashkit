Analyze, fix, and ship Bashkit maintenance. "Run maintenance" (including
"maintainace" and "maintaiance") requests the complete shipped outcome.

Read `.agents/skills/maintain/SKILL.md`, execute the checklist in
`knowledge/operations/maintenance.md`, then run `.agents/skills/ship/SKILL.md`
through green CI and squash-merge. `$ARGUMENTS` may scope the maintenance area;
an explicit analysis-only request is the only default exception to fixing/shipping.

Use parallel agents for independent sections when useful. Do not replace fixes,
audits, or validation with deferral issues or stop at a local commit. Preserve
security contracts, use an appropriate validation environment, fix CI failures,
and never merge with red checks.
