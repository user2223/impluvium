# Backups

This page covers what the server backs up, how to make a backup, how to restore one and what a backup does
not contain. The instance key, the one secret outside the database, comes first.

## The instance key

The server keeps one secret, the **instance key**, in the file `instance.key`
inside its data directory. It is 32 random bytes and the root of every signing
key of the instance: the content signing key and the depot ticket signing keys
are derived from it. Paired devices pin the public content key. If the instance
key is lost, the server can no longer prove that it is the instance the devices
paired with, and every device has to be paired again.

### What to back up

Back up `instance.key` together with the database and the rest of the data
directory, and keep the copy at least as protected as the key itself: anyone who
holds the file can sign as your server. The key is not stored in the database.

### File rules

The server creates the file itself, once, when a new instance is initialised.

- Only the owner may access it: the mode must be exactly `0600` (or `0400` for a
  read-only key), and the file must belong to the user that runs the server.
  Any other mode or owner is rejected and the server does not start.
- It must be a regular file of exactly 32 bytes. Symbolic links, directories,
  truncated, oversized and all-zero files are rejected.
- An existing key is never replaced or repaired. If the file of an existing
  instance is missing, the server refuses to start instead of creating a new
  key. Restore it from the backup.
- Creation is atomic: the key is written to a temporary file next to the
  target, flushed to disk and then linked into place. A crash during creation
  can leave a stale file named `.instance.key.<random>.tmp` in the data
  directory. It is never used as a key and may be deleted.
- The directory must support hard links. The strict file checks are implemented
  for Unix platforms only; the server is supported on Linux.

### Restoring the key

Restore `instance.key` with `chmod 600 instance.key` and the account that runs
the server as its owner, in the same data directory as before. Never copy a key
from another instance to "fix" a start failure: the key must match the database
it was created with.

## Backup sets

A backup set is a directory inside `backups/` in the data directory. It is made by the `backup`
command (a manual set) and by the server before it applies pending migrations (a pre-migration set).

| Kind | Name | Made by |
|---|---|---|
| Manual | `manual-<time>` | `impluvium-server backup` |
| Pre-migration | `pre-<from>-<to>-<time>` | the server, before it migrates from version `<from>` to `<to>` |

`<time>` is the UTC time as `yyyymmddThhmmssZ`. A second set in the same second gets a `-2`, `-3`, ... suffix.
Characters other than letters, digits, `_`, `.` and `-` in a version name are replaced by `_`.

A set holds four files:

- `database.dump`: the PostgreSQL archive made by `pg_dump --format=custom`. Its table of contents is read back with
  `pg_restore --list` before the set counts as finished.
- `instance.key`: a byte-for-byte copy of the instance key. The backup directory holds a secret and is
  protected like the key itself.
- `.impluvium-instance`: a copy of the data directory marker, when the data directory has one.
- `manifest.json`: kind, creation time (UTC), the PostgreSQL server version number, the `pg_dump` version, the names
  and sizes of the files and, when the database has one, the instance identifier and the layout version.

The directories are readable by the owner only (mode `0700`) and the files too (`0600`).

A set is written to a hidden `.partial-*` directory first. Every file is flushed to disk, the manifest comes last,
and only then the directory gets its final name. A directory without `manifest.json`, and every `.partial-*`
directory, is not a backup. A failed run removes its own working directory and leaves earlier sets alone; the
next backup removes `.partial-*` directories that a crashed run left behind. A lock file, `backups/.lock`, makes a second backup fail with "another backup is running" while one is in progress.

## Making a backup

```text
impluvium-server backup
```

The command reads the same environment as the server, uses the built-in `pg_dump` backup whatever
`IMPLUVIUM_BACKUP_MODE` says, prints the name of the new set and exits with a non-zero status on failure. The error
messages never contain the database URL or the password. `IMPLUVIUM_BACKUP_TIMEOUT_SECS` (default 3600) bounds the
whole run; programs that are still running at the deadline are killed.

The PostgreSQL client tools `pg_dump` and `pg_restore` must be installed and found through `PATH`, in a major
version at least as new as the database server. The backup fails with a clear message if a tool is missing or too old.
The database URL may use the parameters `sslmode`, `sslrootcert`, `sslcert`, `sslkey`, `host` (a socket directory or a
host name), `application_name` and `connect_timeout`; other parameters make the backup fail. The URL is passed to
`pg_dump` through `PG*` environment variables of the child process, never as an argument. Put host, port, user and
database into the URL: if the URL leaves one of them out while the server's environment sets `PGHOST`, `PGHOSTADDR`,
`PGPORT`, `PGUSER` or `PGDATABASE`, the backup refuses to run, because the server would use that value and `pg_dump`
would not.

## Backup before a migration

`IMPLUVIUM_BACKUP_MODE` decides how the backup before pending migrations is made:

| Mode | What happens | Guarantee |
|---|---|---|
| `auto` (default) | The server writes a pre-migration set as described above. | A migration runs only after a complete, checked set exists. If the backup fails, the migration does not run. |
| `command` | The server starts the program in `IMPLUVIUM_BACKUP_COMMAND` (a JSON array, no shell) and waits for it, with the same time limit. | A migration runs only if the program exits with status 0. A non-zero status, a signal, a missing program or a timeout stops the migration. The server writes no set in this mode: the operator's program owns the backup. |
| `off` | No backup. The server logs this visibly. | None. Use it only when something else guarantees a restorable copy. |

After a new pre-migration set succeeded, the server deletes older pre-migration sets so that the three newest remain.
Manual sets are never deleted by the server, and nothing is deleted when a backup fails.

## Restoring

1. Stop the server.
2. Create an empty database and restore the dump into it, giving the connection settings through the environment
   rather than on the command line:

   ```text
   createdb impluvium
   pg_restore --no-owner --dbname=impluvium backups/<set>/database.dump
   ```

   Use `--no-owner` unless the original database users exist; set `PGHOST`, `PGUSER` and `PGPASSWORD` as for any
   other PostgreSQL client. Use a `pg_restore` of at least the version of the database server.
   Restoring with a `pg_restore` of version 17 or newer into a server older than 17 reports one harmless error about
   the setting `transaction_timeout`; the data is restored.
3. Copy `instance.key` and `.impluvium-instance` from the set into the data directory, with the account that runs the
   server as owner and mode `0600`:

   ```text
   cp -p backups/<set>/instance.key backups/<set>/.impluvium-instance /srv/impluvium/data/
   chmod 600 /srv/impluvium/data/instance.key
   ```

   The key must belong to the database it was backed up with. The manifest names the instance identifier of the set.
4. Point `DATABASE_URL` at the restored database and start the server.

## What a set does not contain

A set holds the database and the instance key. The game library and, later, object storage are **not** part of
a set. Back them up separately: they live in the library directory (`IMPLUVIUM_LIBRARY_DIR`, by default `library`
inside the data directory) and are not restored by `pg_restore`. A restored database refers to library files that
must still exist.

