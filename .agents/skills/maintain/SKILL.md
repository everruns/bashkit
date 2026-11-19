---
name: maintain
description: Analyze, fix, validate, and ship Bashkit maintenance. Use for run maintenance, maintain, maintainace, maintaiance, or explicit analysis-only maintenance.
metadata:
  internal: true
---

# Bashkit maintenance

1. Read the [maintenance playbook](../../../knowledge/operations/maintenance.md).
   Its invocation contract defines the scope, including analysis-only requests.
2. Start from current `origin/main` and run the playbook's checklist, including
   the workflow credential audit. Follow its policy for external blockers.
3. For a full pass, use [ship](../ship/SKILL.md) to validate and merge the fixes.
4. Report what shipped, the verification evidence, and any remaining blockers
   or upstream constraints. Keep the maintenance record factual.
