<!-- Generated from crates/tide-ui/src/help/pages/git.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Git Workflows

Tide drives the Git command-line tool installed on your machine. Source Control covers the everyday workflow, diff tabs show and stage changes, Timeline shows history, and the rest of Git is in the command palette. Credentials and remote authentication stay with Git and your environment.

## Source Control

Open it with `Ctrl+Shift+G` or `Ctrl+Alt+G`. The top line shows the branch, or `HEAD detached at` and the commit when no branch is checked out. While a rebase, merge, cherry-pick, revert, bisect or `git am` is paused, a second line says so, for example “Rebasing main onto 1a2b3c4d (3/7)”, and the status bar shows it too. Below that, Source Control groups conflicted, staged, changed and untracked files. Move with Up and Down (or `j` and `k`); Left and Right collapse or expand a group, and clicking a group header does the same. Double-click a file to open its diff.

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

## Commit messages

`c` opens the commit box. The first line is the summary; the rest is the body. Enter starts a new line, so you can write both.

| In the commit box | Press |
| --- | --- |
| Commit | Ctrl+S, or Alt+Enter |
| New line | Enter |
| Move to the Commit and Cancel buttons | Tab |
| Close, keeping what you typed | Esc |

Most terminals send Ctrl+Enter and Shift+Enter as a plain Enter, which is why Ctrl+S commits. When the summary and the body are not separated by a blank line, Tide adds one, because Git reads everything up to the first blank line as the summary. The counter in the corner shows the summary's length; past 72 characters the rest is highlighted.

A message you close the box on, or one whose commit fails (for example with nothing staged), is there the next time you open the box; it is cleared once the commit succeeds.

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

## Remotes, hooks and signing

Tide runs Git in the background, without a terminal, so Git cannot stop to ask you anything:

- **SSH:** keys need to be loaded in `ssh-agent` (`ssh-add`), or set `SSH_ASKPASS` to a graphical prompt. A server whose host key you have never accepted fails with “Host key verification failed”; connect once from a terminal to accept it.
- **HTTPS:** use a credential helper, such as the macOS keychain or Git Credential Manager.
- **Signed commits:** signing needs a graphical pinentry such as `pinentry-mac`. `pinentry-curses` cannot draw while Tide owns the screen.

When one of these stops a command, the status message says what to do. You can always run the same command in a Tide terminal instead.

Push, pull, fetch and the commands that run hooks (commit, merge, rebase, cherry-pick, revert, checkout) show “Git: running …” and may take up to ten minutes before Tide gives up; other Git commands get one minute.

## Rewriting history

**Commit (Amend)...** opens the commit box with the last commit's whole message, summary and body, so what you see is what the amended commit keeps. Leave it as it is to keep the message unchanged. The confirmation warns when that commit is already on the upstream branch, which then needs **Push (Force with Lease)**. That uses `--force-with-lease`, so it refuses when the remote has commits you have not fetched.

**Rebase Branch...** picks a branch to rebase onto. **Cherry-Pick Commit...**, **Revert Commit...** and **Reset to Commit...** pick a commit from the log; a reset asks for soft, mixed or hard, and a hard reset asks again before it discards anything. Git never opens an editor for these: messages are used as they are.

## Conflicts

When a pull with rebase, a rebase, merge, cherry-pick or revert stops on conflicts, Tide leaves them in the working tree and the status bar says how to finish. Source Control keeps showing the operation and its conflicted files until it is finished or aborted, including a rebase started from a terminal. **Open Conflicted File...** opens a conflicted file. Resolve and stage each one, then run **Continue Rebase, Cherry-Pick or Revert**, or **Abort Merge, Rebase, Cherry-Pick or Revert** to return to where you started; both act on whichever operation is paused. Finish a merge by committing.
