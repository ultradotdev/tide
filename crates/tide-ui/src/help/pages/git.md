# Git Workflows

Tide drives the Git command-line tool installed on your machine. Source Control covers the everyday workflow, diff tabs show and stage changes, Timeline shows history, and the rest of Git is in the command palette. Credentials and remote authentication stay with Git and your environment.

## Source Control

Open it with {{key:git.focusPanel}}. Below the branch line it groups conflicted, staged, changed and untracked files. Move with Up and Down (or `j` and `k`); Left and Right collapse or expand a group, and clicking a group header does the same. Double-click a file to open its diff.

| With Source Control focused | Press |
| --- | --- |
| Stage the selected file | {{key:gitPanel.stage}} |
| Stage everything | {{key:gitPanel.stageAll}} |
| Unstage the selected file | {{key:gitPanel.unstage}} |
| Discard the file's changes (asks first) | {{key:gitPanel.discard}} |
| Open the file's diff | {{key:gitPanel.openDiff}} |
| Open the file | {{key:gitPanel.openFile}} |
| Commit the staged changes | {{key:gitPanel.commit}} |
| Refresh | {{key:gitPanel.refresh}} |

Staging works on whole files. The status bar shows the current branch and how far it is ahead of or behind its upstream.

## Diff tabs

A diff tab shows one file, unified by default; `v` switches to a side-by-side split. Space, Left, Right or Enter on a header collapses that file or hunk. The action bar, which you can also click, has `o` open file, `e` edit hunk, `s` stage, `u` unstage, `d` discard and `?` diagnostics. Edit hunk opens one unstaged hunk in a scratch editor; saving it applies the change as `tui.diffViewer.editMode` ({{setting:tui.diffViewer.editMode}}) says. Diffs of history are read-only.

**{{label:git.diff}}** shows the active file's changes and **{{label:git.diffStaged}}** picks a staged file to review.

## Timeline

Timeline lists commits for the active file, or for the whole repository after {{key:timelinePanel.toggleMode}}. In file mode it follows whichever file you are editing.

| With Timeline focused | Press |
| --- | --- |
| Show the commit's diff | {{key:timelinePanel.viewDiff}} |
| Open the file as it was at that commit | {{key:timelinePanel.openFileAtCommit}} |
| Copy the commit hash | {{key:timelinePanel.copyHash}} |
| First / last commit | {{key:timelinePanel.goToFirst}} / {{key:timelinePanel.goToLast}} |

`tui.timeline.commitCount` ({{setting:tui.timeline.commitCount}}) sets how many commits it loads.

## Palette commands

{{commands:Git}}

**{{label:git.switchBranch}}** opens the branch picker; clicking the branch in the status bar opens it too. Remotes and tags open read-only output tabs, and blame opens the selected file's blame in a tab. Aborting a merge and discarding changes ask for confirmation first.

**{{label:git.discard}}** and **{{label:git.discardAll}}** throw away unstaged changes after asking; staged changes and untracked files are kept. **{{label:git.clean}}** lists the untracked files it would delete before deleting them and never removes ignored files.

## Rewriting history

**{{label:git.commitAmend}}** opens the commit prompt with the last commit's subject; its body is kept. The confirmation warns when that commit is already on the upstream branch, which then needs **{{label:git.pushForce}}**. That uses `--force-with-lease`, so it refuses when the remote has commits you have not fetched.

**{{label:git.rebase}}** picks a branch to rebase onto. **{{label:git.cherryPick}}**, **{{label:git.revert}}** and **{{label:git.reset}}** pick a commit from the log; a reset asks for soft, mixed or hard, and a hard reset asks again before it discards anything. Git never opens an editor for these: messages are used as they are.

## Conflicts

When a pull with rebase, a rebase, merge, cherry-pick or revert stops on conflicts, Tide leaves them in the working tree and the status bar says how to finish. **{{label:git.conflicts}}** opens a conflicted file. Resolve and stage each one, then run **{{label:git.continue}}**, or **{{label:git.abort}}** to return to where you started; both act on whichever operation is paused. Finish a merge by committing.
