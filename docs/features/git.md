<!-- Generated from crates/tide-ui/src/help/pages/git.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Git Workflows

Tide drives the Git command-line tool installed on your machine. Source Control covers the everyday workflow, diff tabs show and stage changes, Timeline shows history, and the rest of Git is in the command palette. Credentials and remote authentication stay with Git and your environment.

## Source Control

Open it with `Ctrl+Shift+G` or `Ctrl+Alt+G`. Below the branch line it groups conflicted, staged, changed and untracked files. Move with Up and Down (or `j` and `k`); Left and Right collapse or expand a group, and clicking a group header does the same. Double-click a file to open its diff.

| With Source Control focused | Press |
| --- | --- |
| Stage the selected file | `s` |
| Stage everything | `Shift+S` |
| Unstage the selected file | `u` |
| Discard the file's changes (asks first) | `d` |
| Open the file's diff | `Enter` |
| Open the file | `o` |
| Commit the staged changes | `c` |
| Refresh | `r` |

Staging works on whole files. The status bar shows the current branch and how far it is ahead of or behind its upstream.

## Diff tabs

A diff tab shows one file, unified by default; `v` switches to a side-by-side split. Space, Left, Right or Enter on a header collapses that file or hunk. The action bar, which you can also click, has `o` open file, `e` edit hunk, `s` stage, `u` unstage, `d` discard and `?` diagnostics. Edit hunk opens one unstaged hunk in a scratch editor; saving it applies the change as `tui.diffViewer.editMode` (`stage-modified`) says. Diffs of history are read-only.

**View Diff** shows the active file's changes and **View Staged Diff** picks a staged file to review.

## Timeline

Timeline lists commits for the active file, or for the whole repository after `Tab`. In file mode it follows whichever file you are editing.

| With Timeline focused | Press |
| --- | --- |
| Show the commit's diff | `Enter` |
| Open the file as it was at that commit | `o` |
| Copy the commit hash | `y` |
| First / last commit | `Home` or `g` / `End` or `Shift+G` |

`tui.timeline.commitCount` (`50`) sets how many commits it loads.

## Palette commands

| Command | Keys | ID |
| --- | --- | --- |
| Abort Merge | — | `git.abortMerge` |
| Abort Merge, Rebase, Cherry-Pick or Revert | — | `git.abort` |
| Abort Rebase | — | `git.abortRebase` |
| Apply Stash | — | `git.stashApply` |
| Cherry-Pick Commit... | — | `git.cherryPick` |
| Clean Untracked Files... | — | `git.clean` |
| Commit | `c` | `git.commit` |
| Commit (Amend)... | — | `git.commitAmend` |
| Continue Rebase | — | `git.continueRebase` |
| Continue Rebase, Cherry-Pick or Revert | — | `git.continue` |
| Create Branch... | — | `git.createBranch` |
| Create Tag... | — | `git.createTag` |
| Delete Branch... | — | `git.deleteBranch` |
| Discard All Changes | — | `git.discardAll` |
| Discard Changes in Active File | — | `git.discard` |
| Drop Stash | — | `git.stashDrop` |
| Fetch | — | `git.fetch` |
| Fetch All Remotes | — | `git.fetchAll` |
| Focus Git Panel | `Ctrl+Shift+G`, `Ctrl+Alt+G` | `git.focusPanel` |
| List Stashes | — | `git.stashList` |
| List Tags | — | `git.tags` |
| Manage Remotes | — | `git.remotes` |
| Merge Branch... | — | `git.merge` |
| Open Conflicted File... | — | `git.conflicts` |
| Pop Stash | — | `git.stashPop` |
| Pull | — | `git.pull` |
| Pull (Rebase) | — | `git.pullRebase` |
| Push | — | `git.push` |
| Push (Force with Lease) | — | `git.pushForce` |
| Rebase Branch... | — | `git.rebase` |
| Refresh | `r` | `git.refresh` |
| Rename Branch... | — | `git.renameBranch` |
| Reset to Commit... | — | `git.reset` |
| Revert Commit... | — | `git.revert` |
| Stage Active File | — | `git.stage` |
| Stage All Changes | `Shift+S` | `git.stageAll` |
| Stash | — | `git.stash` |
| Switch Branch... | — | `git.switchBranch` |
| Toggle Blame | — | `git.blame` |
| Unstage Active File | — | `git.unstage` |
| Unstage All | — | `git.unstageAll` |
| View Diff | — | `git.diff` |
| View File History | — | `git.logFile` |
| View Log | — | `git.log` |
| View Staged Diff | — | `git.diffStaged` |

**Switch Branch...** opens the branch picker; clicking the branch in the status bar opens it too. Remotes and tags open read-only output tabs, and blame opens the selected file's blame in a tab. Aborting a merge and discarding changes ask for confirmation first.

**Discard Changes in Active File** and **Discard All Changes** throw away unstaged changes after asking; staged changes and untracked files are kept. **Clean Untracked Files...** lists the untracked files it would delete before deleting them and never removes ignored files.

## Rewriting history

**Commit (Amend)...** opens the commit prompt with the last commit's subject; its body is kept. The confirmation warns when that commit is already on the upstream branch, which then needs **Push (Force with Lease)**. That uses `--force-with-lease`, so it refuses when the remote has commits you have not fetched.

**Rebase Branch...** picks a branch to rebase onto. **Cherry-Pick Commit...**, **Revert Commit...** and **Reset to Commit...** pick a commit from the log; a reset asks for soft, mixed or hard, and a hard reset asks again before it discards anything. Git never opens an editor for these: messages are used as they are.

## Conflicts

When a pull with rebase, a rebase, merge, cherry-pick or revert stops on conflicts, Tide leaves them in the working tree and the status bar says how to finish. **Open Conflicted File...** opens a conflicted file. Resolve and stage each one, then run **Continue Rebase, Cherry-Pick or Revert**, or **Abort Merge, Rebase, Cherry-Pick or Revert** to return to where you started; both act on whichever operation is paused. Finish a merge by committing.
