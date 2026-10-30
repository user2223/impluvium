# impluvium

impluvium is a self-hosted platform for DRM-free games: a server that hosts your game library and a desktop client for
Linux and Windows that installs and launches the games.

> [!WARNING]
> **Pre-0.1, under reconstruction.** impluvium is not usable yet. There are no releases, and contributions are not
> accepted before 0.1 (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## Status

The code inherited from Drop is being cleaned up and restructured. Version 0.1 will be the first release: a clean,
working foundation that others can build on. Until then, anything in this repository may change or disappear without
notice, and nothing here is meant to be deployed.

## Origin

impluvium is derived from [Drop](https://github.com/Drop-OSS/drop), an open-source game distribution platform. Many
thanks to the Drop maintainers and contributors for the work this project builds on.

impluvium is an independent project. It is not affiliated with or endorsed by the Drop project. For anything about Drop
itself, please use the [Drop project's own channels](https://droposs.org).

impluvium is developed with AI assistance. Drop has a no-AI contributions policy
([contributing guide](https://developer.droposs.org/contributing#policy)), so changes made here are not submitted
upstream.

## License

impluvium is licensed under the GNU Affero General Public License, version 3 (see [LICENSE](LICENSE)). Code inherited
from Drop keeps the terms its authors gave it: some parts are marked AGPL-3.0-or-later, others AGPL-3.0-only. A few
inherited components carry other licenses; see the license file in their directory. New code is AGPL-3.0-or-later,
except in the planned `engine/` directory, which will be 0BSD.

## Security

Please report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).
