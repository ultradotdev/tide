# PostgreSQL Workspace

A SQL tab is an editor with its results below it. Connections, schema browsing, query history, favorites and row edits are all in the command palette under Database.

## Connections

Run **{{label:database.newConnection}}**. The form takes a name, host, port, database, user name, password, TLS mode (disable, prefer or require) with an optional CA certificate, read-only mode, timeouts, and a scope:

- **Global** connections are saved in {{path:connections}}.
- **Project** connections are saved in `.tide/connections.json` inside the workspace. They cannot store a password.

Passwords never go into the connection file. They are kept by the secret service: the macOS Keychain or Linux `secret-tool` when available, read-only `TIDE_SECRET_<KEY>` environment variables, or an encrypted file at {{path:secrets}} that you unlock with **{{label:secrets.manage}}** or `TIDE_MASTER_PASSWORD`.

**{{label:database.editConnection}}**, **{{label:database.testConnection}}**, **{{label:database.deleteConnection}}** and **{{label:database.disconnect}}** manage saved connections.

## Safety

New connections are **read-only**. Tide sets PostgreSQL's read-only transaction mode, runs each query in a read-only transaction, and allows one read statement per run. For a guarantee that holds in every client, also connect as a role without write privileges. On a writable connection, statements that look destructive, broad, mixed or unknown ask for confirmation first.

TLS `require` and `prefer` verify the server certificate against the system roots plus your CA certificate; `prefer` falls back to an unencrypted connection with a warning when the server has no TLS.

## Queries

{{key:database.newQuery}} opens a SQL tab using the current tab's connection, or the first saved one. In the SQL editor:

| To | Press |
| --- | --- |
| Run the whole query | {{key:database.executeQuery}} |
| Run only the selection | {{key:database.executeSelection}} |
| Choose the connection | {{key:database.connect}} |
| Complete table and column names | {{key:database.complete}} |

**{{label:database.browseSchema}}** walks schemas, tables, views and columns and can open a query for an object; **{{label:database.describeTable}}** shows a table's definition.

## Results

Results fill the lower part of the tab. Click a column header to sort: ascending, descending, then unsorted.

| With the results focused | Press |
| --- | --- |
| Move between rows and columns | Arrow keys or `h` `j` `k` `l` |
| Row details | {{key:queryResults.showDetails}} |
| Sort by the selected column | {{key:queryResults.sort}} |
| Switch the result view | {{key:queryResults.toggleViewMode}} |
| Run the query again | {{key:queryResults.refresh}} |
| Export as CSV | {{key:queryResults.export}} |

Export writes the whole result as CSV, in its original row order, to a path you choose (relative paths are inside the workspace); it asks before overwriting a file.

Updating, deleting and adding rows (**{{label:database.addRow}}**) need a writable connection and a result that maps to one table with a primary key. Field types and the key are checked before anything changes, and each change is confirmed.

## History and favorites

**{{label:database.viewHistory}}** lists earlier queries, including failed ones, with passwords redacted. Enter opens the entry in a new SQL tab on its connection without running it. In the history list, `Alt+F` toggles a favorite, `Alt+O` shows only favorites, `Alt+C` limits the list to the active connection, and `Alt+D` clears the listed entries after asking.

History is kept under {{path:history}}; turn it off with `session.databaseQueryHistory` ({{setting:session.databaseQueryHistory}}). Treat history and exported results as sensitive, and avoid putting passwords in SQL.

See {{help:settings}} and {{help:commands}}.
