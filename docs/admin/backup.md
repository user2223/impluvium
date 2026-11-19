# Backing up the instance key

The server keeps one secret, the **instance key**, in the file `instance.key`
inside its data directory. It is 32 random bytes and the root of every signing
key of the instance: the content signing key and the depot ticket signing keys
are derived from it. Paired devices pin the public content key. If the instance
key is lost, the server can no longer prove that it is the instance the devices
paired with, and every device has to be paired again.

## What to back up

Back up `instance.key` together with the database and the rest of the data
directory, and keep the copy at least as protected as the key itself: anyone who
holds the file can sign as your server. The key is not stored in the database.

## File rules

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

## Restoring

Restore `instance.key` with `chmod 600 instance.key` and the account that runs
the server as its owner, in the same data directory as before. Never copy a key
from another instance to "fix" a start failure: the key must match the database
it was created with.
