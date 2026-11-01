# Git Workflows

Tide uses the local Git CLI through a service interface. Source Control provides the everyday workflow; broader repository commands are available through the command palette.

## Source Control

The Source Control accordion groups staged, changed, and untracked files. Its active hint row reflects current keybindings.

| Key | Action |
| --- | --- |
| `s` | Stage selected file |
| `S` | Stage all files |
| `u` | Unstage selected file |
| `d` | Discard selected changes |
| Enter | Open diff |
| `o` | Open file |
| `c` | Commit |
| `r` | Refresh |

Diff tabs support unified and split layouts, file navigation, staging actions, and a shared action bar. Review the selected file and operation before discarding changes.

## History and repository commands

Timeline can show file or repository history, open a historical diff, open a file at a commit, and copy a commit hash. The command palette includes status, staging, commit, push, pull, fetch, stashes, log, blame, branches, tags, merge, rebase, conflicts, remotes, checkout, cherry-pick, revert, reset, and clean commands.

Commands are exposed only when an active handler exists. Git credentials and remote authentication remain owned by Git and the user's environment.
