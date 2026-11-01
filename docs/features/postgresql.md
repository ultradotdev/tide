# PostgreSQL Workspace

Tide's Rust SQL tabs combine an editor with results beneath it. Connections, schema discovery, history, favorites and eligible row changes are available through the database commands.

## Connections and safety

Choose New Connection from the palette. The form supports host, port, database, username, password, TLS and read-only mode. Saved records refer to the secret service instead of embedding passwords. The service uses an available platform keychain, read-only environment values, or an unlocked encrypted-file provider.

New connections default to read-only. Tide adds client-side mutation checks and requests PostgreSQL read-only session behavior. Use a database role without write privileges when authorization must hold across clients. Writable connections still require confirmation for conservatively classified destructive, broad, mixed or unknown operations.

TLS modes preserve their configured verification policy. Validate a server's certificate authority and hostname for verified connections; encryption without identity verification is a different guarantee.

## Query and schema workflow

Open New Query and select a connection. `Ctrl+R` executes SQL in that editor. Database completion offers schema-aware items. Browse Schema opens schemas, tables/views and columns, with actions to inspect definitions or open a query for an object. Missing connections and service errors are shown explicitly.

## Results

Results occupy the lower portion of the SQL tab. Click a column header to sort it; selecting it again reverses the direction. Keyboard result commands remain available from the palette and generated shortcut reference. Row details expose the selected record, and export writes the displayed result data to an explicit destination.

Update, delete and add-row workflows require a supported table identity, suitable metadata and a writable connection. Typed fields and primary-key identity are checked before a mutation. The confirmation retains the connection and row authority; an arbitrary query result does not imply an editable table. Review the target and action before confirming.

## History and favorites

Query History can view or insert saved SQL without executing it, explicitly execute a chosen entry, toggle favorites, filter by connection and clear the selected scope. History is stored under Tide's data root and can be disabled with `session.databaseQueryHistory`.

Treat query history and exported data as sensitive. Avoid putting passwords in SQL or sharing unredacted logs. See [configuration](../reference/configuration.md) and [generated commands](../reference/commands.md).
