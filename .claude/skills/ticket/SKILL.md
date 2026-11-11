---
name: ticket
description: Runs one Sagiri ticket end to end - the implementer agent builds it on its own branch, the reviewer agent checks it, findings go back for up to two fix rounds, and the result is offered for merging into master. Use this whenever the user asks to run, start, implement, build, do or work on a ticket, names a ticket id such as 0001 or ticket/0003, or asks for the next ticket, even if they mention neither the agents nor this skill.
argument-hint: "<ticket id, e.g. 0001, or 'next'>"
---

# Run a ticket

Requested ticket: `$ARGUMENTS`

You are the coordinator. Two project agents do the work:

- `implementer` builds one ticket in its own git worktree, on a branch `ticket/<id>`.
- `reviewer` checks that branch without changing it and ends with a verdict line.

Your job is the handoffs between them and knowing when to stop. Do not write or fix ticket
code yourself: the review is only worth something if the reviewer checks work it had no
part in, and the code lives in the implementer's worktree, not in your checkout.

The rules the agents follow are in `.claude/agents/` and `tickets/README.md`. Keep your
prompts to them short; do not restate those rules.

## 1. Pick the ticket

- Normalize the argument to a four-digit id: `1`, `0001`, `ticket/0001` and `ticket 1` all
  mean `0001`.
- If the argument is empty or `next`: list the tickets on `master` with status and
  dependencies, choose the lowest-numbered one that is `todo` and whose dependencies are
  all `done`, and say which one you chose. If none qualifies, say so and stop.
- The ticket file is the single match of `tickets/<id>-*.md` on `master`.

## 2. Preflight

Read everything from `master`, not from your working tree. The implementer branches from
`master`, so a ticket that only exists in your checkout is invisible to it.

```bash
git ls-tree --name-only master tickets/                  # the ticket file and README exist
git show master:tickets/<file> | sed -n '1,8p'           # front matter: status, depends_on
git rev-parse --verify --quiet refs/heads/ticket/<id>    # must print nothing
```

Then read the front matter of each ticket in `depends_on` the same way.

Stop and tell the user, without working around it, if:

- the ticket or `tickets/README.md` is not on `master`;
- its `status` is not `todo`;
- a dependency is not `done` on `master` (name it);
- the branch `ticket/<id>` already exists. An earlier run was left unfinished. Show where it
  is checked out (`git worktree list`) and ask whether to review that branch as it is or
  leave it. Never delete it on your own.

## 3. Implement

Start the implementer with the Agent tool, `subagent_type: "implementer"`, and this prompt:

```
Implement ticket <id>: tickets/<file>
```

Do not pass a model or effort override unless the user asked for one.

The agent runs in the background. Say in one line that it is running, end your turn, and
continue when its completion notice arrives. Do not poll and do not start the reviewer
early.

Keep the agent's id: fix rounds need it.

When it returns, check the result before trusting the report. A report is a claim.

```bash
git log --oneline master..ticket/<id>       # at least one commit
git worktree list --porcelain               # find the worktree on refs/heads/ticket/<id>
git -C <worktree> status --porcelain        # must print nothing
```

Take the worktree path from `git worktree list`. The completion notice also carries a
path and a branch, but that branch is the placeholder `worktree-agent-...` the worktree
was created with, not `ticket/<id>`. Note the placeholder's name for cleanup.

If the implementer stopped on a contradiction or a blocker, or there is no branch or no
commit, relay its report and stop. Do not review unfinished work.

## 4. Review

Start a reviewer with the Agent tool, `subagent_type: "reviewer"`:

```
Review ticket <id>: tickets/<file>
Branch: ticket/<id>
Base: master
Worktree: <absolute path>
Round: 1
```

Wait for its completion notice the same way. The last line of its report is either
`VERDICT: approve` or `VERDICT: changes-requested`. If there is no verdict line, the review
did not happen: run a reviewer once more, and if that also fails, report it and stop.

## 5. Fix rounds

At most two.

On `changes-requested`, send the findings to the **same** implementer with SendMessage,
addressed to the agent id from step 3:

```
Review findings for ticket <id>, round <n>. Fix them on ticket/<id> as described in your
"Review round" instructions.

<the reviewer's blocker and should-fix findings, verbatim>
```

Resuming matters. The resumed agent is still in its worktree with the branch checked out
and remembers what it built. A newly started implementer would get a second worktree in
which `ticket/<id>` cannot be created.

When it returns, repeat the checks from step 3, then start a **new** reviewer so the fix is
judged with fresh eyes. Use the prompt from step 4 with the next round number and append:

```
Findings from the previous round. Verify each one first:
<the findings you sent>
```

If the verdict is still `changes-requested` after the second fix round, stop. Do not
merge. Give the user the remaining findings and let them decide. Two agents that disagree
can go around indefinitely; a person should break the tie.

## 6. Finish

Tell the user:

- the ticket, the branch and commit, the verdict, and how many rounds it took;
- what the reviewer ran, and its result;
- should-fix findings and nits that remain;
- anything nobody verified, such as manual checks that need a real terminal.

Then ask whether to merge. Merging is the user's decision every time, also after a clean
approval.

If they agree:

```bash
git worktree list --porcelain                           # find the checkout that has master
git -C <master checkout> merge --ff-only ticket/<id>
```

- If no checkout has `master`: `git fetch . ticket/<id>:master` (fast-forward only).
- If the fast-forward is refused because `master` moved, stop and say so. Do not rebase,
  and do not force anything.

After a successful merge, remove what this run created, in this order:

```bash
git worktree remove <ticket worktree>     # refuses if it has uncommitted changes: then leave it and say so
git branch -d ticket/<id>                 # -d, never -D
git branch -d <placeholder branch>        # the worktree-agent-... branch, if it exists
```

If they decline, leave everything as it is and say where the branch and the worktree are.

Never push. Run one ticket per invocation; independent tickets can run in separate
sessions, but their merges still happen one at a time.
