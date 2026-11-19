# Starting the server

`impluvium-server start` runs the server. It reads its settings from the environment (see
[Configuration](configuration.md)) and checks everything it depends on before it listens. Any failed check
ends the process with a message that names the cause and holds no secret. Nothing listens until all checks
have passed.

## The order of the start

1. **Configuration.** The same checks as `impluvium-server config check`.
2. **Web application.** When `IMPLUVIUM_SPA_DIR` is set, the directory must hold the built web application and
   its manifest `csp-hashes.json`. Every file must be a plain file (no links) and must match the manifest in
   size and SHA-256, no file may be missing or extra, and the inline scripts of the HTML files must match the
   hashes in the manifest. The hashes of `index.html` go into the content security policy of the page.
3. **Data directory.** It must exist, belong to the user that runs the server and must not be writable by its
   group or by others. The server takes an exclusive lock on the file `.impluvium-lock` inside it and keeps
   it as long as it runs.
4. **PostgreSQL version.** PostgreSQL 16, 17 and 18 are supported. An older major version is refused; a newer
   one starts with a warning.
5. **Database start guard.** The server takes the database lock, checks that database, data directory and
   instance key belong together, makes the backup before pending migrations, applies them and, on the very
   first start, initialises the instance. See [Upgrades](upgrades.md) and [Backups](backup.md).
6. **Connection pool,** sized by `IMPLUVIUM_DB_POOL_MAX` and `IMPLUVIUM_DB_TIMEOUT_SECS`.
7. **Listener.** The server binds `IMPLUVIUM_LISTEN` (default `127.0.0.1:8080`; a container needs an address
   other than loopback) and answers requests. `GET /api/health/ready` returns `200` from this point on.

## What a refusal means

| Message | Meaning and remedy |
|---|---|
| `another server uses this data directory` | Another process holds `.impluvium-lock` in this data directory. Stop it or use another directory. |
| `another server owns this database` | Another server holds the lock of this database. Each database has at most one server. |
| `the data directory is not owned by the user running the server` | Change the owner or run the server as the owner. |
| `the data directory can be written by its group or by other users` | Remove those rights, for example `chmod go-w`. |
| `PostgreSQL N is not supported` | Upgrade PostgreSQL to version 16 or newer. |
| `foreign database or foreign data directory` and the other identity messages | Database, data directory and instance key do not belong together, for example after restoring only one of them. Nothing was changed. Restore the matching parts. |
| `empty database with existing data directory` | The database is empty but the data directory has data. If you restored the data directory and want to use a new database with it, see below. |
| `IMPLUVIUM_SPA_DIR: ...` | The web application directory does not match its manifest. Rebuild or redeploy it. |
| `cannot listen on the configured address` | The address in `IMPLUVIUM_LISTEN` is in use or not available on this machine. |

## A new database for an existing data directory

If you restore the data directory (with its instance key) but start with an empty database, the server refuses
by default. To bind the empty database to the existing data directory, set `IMPLUVIUM_ACCEPT_NEW_DATABASE` to
the instance identifier of the data directory. The identifier is the line `id ...` in the file
`.impluvium-instance` in the data directory. The server then initialises the database for that instance,
keeps the instance key, and logs what this means at warn level. Remove the setting after the start: it is
ignored in every other situation, and a different identifier is refused.

## Reattaching after a restart

A restart needs no setting. When data directory, database and instance key match, the server starts as a
normal start, applying pending migrations after the backup. An interrupted first start continues where it
stopped.

## Stopping

On `SIGTERM` or `SIGINT` the server turns the readiness probe to `503`, stops accepting connections and gives
requests in progress up to 30 seconds. It then closes the pool and releases the database lock.

## Losing the database lock

The server checks every five seconds that its database session still holds the lock. If the session is lost,
for example because PostgreSQL restarted or an administrator terminated the connection, another server could
take over. The server then turns the readiness probe to `503` at once, stops accepting connections, gives requests
in progress the usual 30 seconds and exits with code 3. It does not try to release the lock, which is gone.
Start it again, or let your supervisor restart it.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Stopped by a signal; requests in progress finished in time. |
| 1 | A check failed, the start was refused, or the server failed. |
| 3 | The database lock was lost while serving. |
| 4 | Stopped by a signal, but a request was still running when the 30 seconds ran out. |
