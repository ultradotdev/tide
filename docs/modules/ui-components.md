# UI Components

The Rust UI is implemented in `crates/tide-ui/src`.

| Surface | State and behavior |
| --- | --- |
| App | Input precedence, background events, commands and focus |
| Workbench/panes | Recursive splits, heterogeneous tabs and tab movement |
| Editor | Shared document views, wrapping, folding, selection and minimap |
| Sidebar accordion | Explorer, Source Control, Outline and Timeline |
| Agents | Provider groups, workspace sessions and existing-tab focus |
| Overlay/dialog | Lists, prompts, forms, intent-styled actions and dismissal guards |
| SQL tab | Editor, results, schema actions and typed row mutations |
| Terminal | Native PTY snapshot, cursor, input and resize |

Render functions record mouse targets alongside cell geometry. Command and preference metadata are shared with the compiled reference exporter. Add behavior through the owning state/controller and verify both keyboard and mouse activation where relevant.

See [rendering](../architecture/rendering.md) and [workbench controls](../features/workbench.md).
