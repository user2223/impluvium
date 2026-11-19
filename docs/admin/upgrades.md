# Upgrades

## There is no way down by migration

Migrations only go forward. The server has no down migrations, and it never edits a migration after it was
applied: a database whose applied history differs from the migrations of the installed version is refused.

The way back to an older version is to restore the backup that was made before the upgrade:

1. Stop the new server.
2. Restore the pre-migration set of the upgrade into an empty database, and restore `instance.key` and the data
   directory marker from the same set (see [Backups](backup.md#restoring)).
3. Start the older version against the restored database.

Anything written to the database after the upgrade is lost by this. Game files in the library are not part of a
backup set; they stay as they are.

## What happens during an upgrade

`impluvium-server` does not start an instance yet. Once it does, the behaviour below applies: when the server
starts and finds pending migrations, it makes a backup first, according to `IMPLUVIUM_BACKUP_MODE`:

- `auto`: the server writes a pre-migration set (`pre-<from>-<to>-<time>`) with `pg_dump`. The three newest
  pre-migration sets are kept.
- `command`: the server runs your backup program (`IMPLUVIUM_BACKUP_COMMAND`) and waits for it.
- `off`: no backup is made and the server logs that visibly.

## What blocks a migration

In modes `auto` and `command` a failed backup stops the migration and the server does not start. The causes
include a missing or too old `pg_dump` or `pg_restore`, a database URL that the tools cannot use, a data directory
that cannot be written, a full disk, a dump that does not pass `pg_restore --list`, a program that exits with a
non-zero status or is killed, and a run that exceeds `IMPLUVIUM_BACKUP_TIMEOUT_SECS`. Nothing is migrated and the
database stays as it was. Fix the cause and start again.

Make the same kind of backup yourself at any time with `impluvium-server backup`, for example before you upgrade.
