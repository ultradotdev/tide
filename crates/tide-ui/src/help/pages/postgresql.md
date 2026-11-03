# PostgreSQL Workspace

A SQL tab is an editor with its results below it. Connections, schema browsing, query history, favorites and row edits are all in the command palette under Database.

## Connections

Run **{{label:database.newConnection}}**. The form takes a name, host, port, database, user name, password, TLS mode with an optional CA certificate, read-only mode, timeouts, and a scope:

- **Global** connections are saved in {{path:connections}}.
- **Project** connections are saved in `.tide/connections.json` inside the workspace, so you can share them with the repository. Their passwords are not.

In the form, Ctrl+S saves and Ctrl+T tests the connection.

## Passwords

Passwords never go into a connection file. They are kept by the secret service: the macOS Keychain or Linux `secret-tool` when available, read-only `TIDE_SECRET_<KEY>` environment variables, or an encrypted file at {{path:secrets}} that you unlock with **{{label:secrets.manage}}** or `TIDE_MASTER_PASSWORD`.

A password you type for a **project** connection is kept the same way, and {{path:connections}} records which destination (host, port, database, user name and TLS settings) it belongs to. The project file stays free of secrets, each person who opens the project enters their own password once, and the password is only ever sent to that exact destination. Change the host, port, database, user or TLS settings and Tide asks for the password again.

When Tide has no password stored for a connection, it uses your PostgreSQL password file, the same one `psql` reads: the file named by `PGPASSFILE`, or `~/.pgpass`. Each line is `hostname:port:database:username:password`, `*` matches any value, and `\` escapes a `:` or `\` inside a field. As with `psql`, the file is ignored unless only you can read it (`chmod 600 ~/.pgpass`). For a project connection, a line with `*` as the host is not used: the line has to name the host, so a shared project file cannot send your password to a server it picks.

**{{label:database.editConnection}}**, **{{label:database.testConnection}}**, **{{label:database.deleteConnection}}** and **{{label:database.disconnect}}** manage saved connections.

## Safety

New connections are **read-only**. Tide sets PostgreSQL's read-only transaction mode, runs each query in a read-only transaction, and allows one read statement per run. For a guarantee that holds in every client, also connect as a role without write privileges. On a writable connection, statements that look destructive, broad, mixed or unknown ask for confirmation first.

## TLS

TLS modes mean what they mean in `psql` and other libpq clients:

| Mode | Encrypts | Checks the server's certificate |
| --- | --- | --- |
| `disable` | Never | No |
| `prefer` | When the server supports it | No |
| `require` | Always | No; with a CA certificate set, checks it was issued by that CA, as `verify-ca` |
| `verify-ca` | Always | Issued by a trusted authority: the system's, or your CA certificate |
| `verify-full` | Always | As `verify-ca`, and it must name the host you connect to |

`require` works with servers that use a self-signed certificate. It protects against someone reading the traffic, not against someone pretending to be the server; use `verify-full` with the server's CA certificate when that matters. `prefer` falls back to an unencrypted connection, with a warning, when the server has no TLS.

When you leave the mode unset, Tide uses `disable` for `localhost` and Unix sockets and `verify-full` for every other host. Connections saved by earlier versions of Tide were always fully checked and still show as `verify-full`. A connection URL can set `sslmode` and `sslrootcert` (a CA certificate file), as with `psql`.

When a check fails, the error says why, for example `UnknownIssuer` for a certificate from an authority Tide does not trust, and suggests a mode or CA certificate that would work.

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
