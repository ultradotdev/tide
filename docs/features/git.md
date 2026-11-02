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
| Apply Stash | — | `git.stashApply` |
| Checkout... | — | `git.checkout` |
| Commit | — | `git.commit` |
| Create Branch... | — | `git.createBranch` |
| Delete Branch... | — | `git.deleteBranch` |
| Drop Stash | — | `git.stashDrop` |
| Fetch | — | `git.fetch` |
| Focus Git Panel | `Ctrl+Shift+G`, `Ctrl+Alt+G` | `git.focusPanel` |
| List Branches | — | `git.branches` |
| List Stashes | — | `git.stashList` |
| List Tags | — | `git.tags` |
| Manage Remotes | — | `git.remotes` |
| Merge Branch... | — | `git.merge` |
| Pop Stash | — | `git.stashPop` |
| Pull | — | `git.pull` |
| Push | — | `git.push` |
| Refresh | — | `git.refresh` |
| Rename Branch... | — | `git.renameBranch` |
| Show Status | — | `git.status` |
| Stage All Changes | — | `git.stageAll` |
| Stage Changes | — | `git.stage` |
| Stash | — | `git.stash` |
| Switch Branch... | — | `git.switchBranch` |
| Toggle Blame | — | `git.blame` |
| Unstage All | — | `git.unstageAll` |
| Unstage Changes | — | `git.unstage` |
| View Active File Diff | — | `git.openFileDiff` |
| View Diff | — | `git.diff` |
| View File History | — | `git.logFile` |
| View Log | — | `git.log` |
| View Staged Changes | — | `git.viewStagedChanges` |
| View Staged Diff | — | `git.diffStaged` |

Branch switching, checkout and **List Branches** open the same branch picker. Remotes and tags open read-only output tabs, and blame opens the selected file's blame in a tab. Aborting a merge and discarding changes ask for confirmation first.
