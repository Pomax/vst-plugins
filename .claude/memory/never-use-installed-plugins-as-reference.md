---
name: never-use-installed-plugins-as-reference
description: "Hard rule from the user: never inspect plugins installed on their machine as examples or benchmarks — only VST3 docs, the SDK, and the project's crates count"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 6b463a17-af3f-45d1-8322-abe91500cf0d
  modified: 2026-08-02T06:52:41.974Z
---

For the Markdown Notes project (and any VST3 work for this user): **do not inspect,
load, diff against, or otherwise use any plugin installed on the machine as a
reference, example or benchmark.** Not Surge, not Vital, not iZotope. Do not
read the DAW's plugin database or config either.

The only valid sources are the official VST3 documentation, the VST3 SDK
(headers and its example plug-ins such as `again`), and the crates the project
depends on with their source and examples.

**Why:** debugging why the plugin was classified as an instrument, I repeatedly
loaded installed third-party plugins to compare factory metadata instead of
reading the spec. It produced three wrong diagnoses in a row — "the subcategory
is wrong" (it wasn't), "IPluginFactory3 is missing" (real gap, not the cause),
"the DAW cached it" (it hadn't) — and led to rebuilding the plugin as two
classes purely because other plugins are shaped that way, which is not a
reason. The user was explicit and furious: *"THAT IS NOT YOUR FUCKING CONCERN OR
FUCKING BENCHMARK, ONLY THE VST3 DOCUMENTATION, SDK, AND ASSOCIATED CRATES
ARE."* Hours were lost.

**How to apply:** when a VST3 question comes up, go to the documentation or the
SDK first and quote it. If the answer is not there, say so and ask — do not go
hunting for a binary to reverse-engineer. This rule is also written into the
project's CLAUDE.md. See [[blanket-permission-means-just-run-it]].
