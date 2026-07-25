---
type: Generated Inventory
title: Builtin Inventory
description: Generated inventory of Bashkit builtin commands and their flags.
resource: builtins.json
tags:
  - bashkit
  - builtins
  - generated
generated:
  by: process:just-regen-builtins
---

# Builtin Inventory

[`builtins.json`](builtins.json) is a generated view of implementation state: the
list of builtin commands the interpreter registers, with their supported flags.

Do not edit it by hand. Regenerate and commit it with the behavior change that
caused it:

```console
$ just regen-builtins
```

`builtins-drift.yml` fails CI when the committed file no longer matches what the
code registers. Design for the builtins themselves lives in
[builtins](../builtins.md); known gaps in [limitations](../limitations.md).
