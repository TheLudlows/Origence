# T0 exact-head job selection

Run the isolated tests with:

    python3 -m unittest discover -s tools/t0-ci-selection -p 'test_*.py' -v

The selector compares the endpoint trees from the runner-provided
`pull_request` / `synchronize` event's `before` and `after` values. Both must
be nonzero lowercase 40-hex commit IDs; `after` must equal the event PR head
and the actual checkout. Before any skip, `GITHUB_WORKFLOW_SHA` must resolve
and its workflow bytes must equal the head workflow. This prevents a changed
merge-ref/base-branch workflow from bypassing the execution-context comparison.
The previous commit is fetched by its exact ID from
checkout's existing `origin` only if it is missing. No branch-name resolution,
API token, hardcoded known-good head, cached test result or whole-PR diff is used.

Only changes confined to the standalone Turso or ANN probe select that probe.
Explicit T0 report/contract/validation Markdown and dated T0 evidence JSON
paths listed in the selector are non-executed documentation. They do not
select a native probe by themselves; edits to their semantics are not validated
by historical ANN results. The executable ANN policy contract and source
baseline stay inside the ANN input directory and always select ANN.

Shared/unknown paths, incomplete event context, forks, unavailable commits,
fetch/diff failures, empty changes, and unrecognized workflow edits run both.
A selector-job failure or missing output also runs both via downstream guards.

For the initial selector introduction and later Turso-only workflow edits, the
workflow comparison accepts only the known top-level layout. Global settings
must remain byte-identical except the added helper-path trigger. The ANN job
must remain byte-identical after stripping only the two exact `needs` / `if`
control lines introduced here. The new selector/report jobs must exactly match
the bounded templates in `select_jobs.py`. ANN steps, runner, toolchain, env,
permissions, caches and timeouts cannot change under this exception. Unknown
jobs or selector wiring changes run both. Changes under this helper directory
can use that same proof; they are not native ANN build inputs.

The summary records the exact before/after IDs and selected probes. An
unselected probe is **SKIPPED / NOT_RUN on the current head**, never a pass.
A prior failing, incomplete or absent test result is not converted into a
passing current-head result. This selection is an incremental development
optimization, not full-head acceptance or permission to merge. Historical
failure gates remain in selected jobs; ordinary application CI is separate.

## Event references

- [GitHub Actions event reference](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request)
  documents `synchronize` among the default PR activities and requires
  `github.event.pull_request.head.sha` to check out the actual PR head rather
  than the synthetic merge commit.
- [GitHub webhook reference](https://docs.github.com/en/webhooks/webhook-events-and-payloads#pull_request)
  describes PR activity payloads. The rendered page defaults to another action;
  do not assume its visible fields establish a usable synchronize baseline.
- [GitHub's Octokit synchronize payload schema](https://github.com/octokit/webhooks/blob/main/payload-schemas/api.github.com/pull_request/synchronize.schema.json)
  explicitly declares `action: synchronize`, `before` and `after`. The selector
  still treats missing or inconsistent fields as unknown and runs both.

- [GitHub runner variables](https://docs.github.com/en/actions/reference/workflows-and-actions/variables)
  defines `GITHUB_WORKFLOW_SHA` as the commit SHA for the executed workflow file.
