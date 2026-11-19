// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The pure decisions of the start guard: what the combination of database, data directory marker
//! and instance key means, and what the history of applied migrations means. Nothing here touches
//! a database, a file or the clock.

use std::fmt;

use super::PublicKeys;
use crate::InitialisationState;
use crate::instance::{DataMarker, InstanceId, LAYOUT_VERSION};

/// What the database says about the instance.
#[derive(Clone, Debug, Default)]
pub(super) struct DbFacts {
    /// Identifier and layout version of the instance row.
    pub instance: Option<(InstanceId, i32)>,
    pub initialisation: Option<InitialisationState>,
    /// Number of rows in `content_key_binding`, retired ones included.
    pub binding_rows: i64,
    /// Key id and public key of the active binding.
    pub active_binding: Option<(Vec<u8>, Vec<u8>)>,
    /// Number of rows in `ticket_key_generation`.
    pub ticket_rows: i64,
    /// Key id of generation 1.
    pub first_ticket_key_id: Option<Vec<u8>>,
    /// The instance tables do not exist but the database holds other relations (tables, views,
    /// sequences) outside the system schemas.
    pub foreign_relations: bool,
}

/// The step an interrupted first initialisation continues at.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum InitStep {
    /// The instance and initialisation rows are written.
    Rows,
    /// The data directory marker is written.
    Marker,
    /// The instance key is created.
    Key,
    /// The key binding and the first ticket key generation are written and the initialisation is
    /// marked completed.
    Bind,
}

impl fmt::Display for InitStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Rows => "the instance rows",
            Self::Marker => "the data directory marker",
            Self::Key => "the instance key",
            Self::Bind => "the key binding",
        })
    }
}

/// What the start has to do about the identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Plan {
    /// Everything exists and matches.
    Normal,
    /// Nothing exists: initialise from the start.
    FirstInit,
    /// An initialisation was interrupted; continue at this step.
    Resume(InitStep),
    /// Bind an empty database to the existing data directory.
    Reattach,
}

/// Why the identity check refuses to start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityRefusal {
    /// The database belongs to another instance than the data directory.
    ForeignDatabase,
    /// The database has an instance but the data directory has no marker.
    DatabaseWithoutMarker,
    /// The data directory belongs to an instance but there is no instance key.
    MarkerWithoutKey,
    /// The instance key is not the one bound in the database.
    KeyMismatch,
    /// The database is empty but the data directory has a marker and no reattach was requested
    /// for it.
    EmptyDatabaseWithData,
    /// The database has no instance tables but is not empty: it belongs to something else.
    NotEmpty,
    /// Reattaching was requested but the data directory has no marker.
    ReattachWithoutMarker,
    /// An instance key exists although the data directory has no marker and the database no
    /// instance.
    KeyWithoutMarker,
    /// The data layout is not the one this version uses.
    UnsupportedLayout,
    /// The database and the data directory name different layout versions.
    LayoutMismatch,
    /// An earlier first initialisation was interrupted and the files do not show that it was this
    /// installation that began it.
    InterruptedInitialisation,
    /// The rows of the database contradict each other.
    Inconsistent,
    /// After the migration the identity no longer matched a normal start.
    Unconfirmed,
}

impl fmt::Display for IdentityRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ForeignDatabase => "foreign database or foreign data directory",
            Self::DatabaseWithoutMarker => {
                "the database belongs to an instance but the data directory has no marker; \
                 this is a foreign or restored database"
            }
            Self::MarkerWithoutKey => {
                "the data directory belongs to an instance but its instance key is missing"
            }
            Self::KeyMismatch => "the instance key does not match the database",
            Self::EmptyDatabaseWithData => {
                "empty database with existing data directory; to use the data directory with \
                 this empty database, accept it explicitly with the instance identifier of the \
                 data directory"
            }
            Self::NotEmpty => "the database is not empty and is not an impluvium database",
            Self::ReattachWithoutMarker => {
                "accepting a new database needs a data directory that has an instance marker"
            }
            Self::KeyWithoutMarker => {
                "an instance key exists but the data directory has no marker and the database \
                 no instance"
            }
            Self::UnsupportedLayout => "the data layout is not supported by this version",
            Self::LayoutMismatch => {
                "the database and the data directory have different layout versions"
            }
            Self::InterruptedInitialisation => {
                "an earlier first start was interrupted and the data directory does not match it"
            }
            Self::Inconsistent => "the instance tables of the database contradict each other",
            Self::Unconfirmed => "the identity could not be confirmed after the migration",
        })
    }
}

/// Decides what the start does about the identity, or why it must not start.
///
/// `accept_new_database` is only looked at when the database has no instance. It must then name the
/// instance of the marker; without a marker it is refused rather than ignored.
pub(super) fn decide(
    db: &DbFacts,
    marker: Option<&DataMarker>,
    keys: Option<&PublicKeys>,
    accept_new_database: Option<&InstanceId>,
) -> Result<Plan, IdentityRefusal> {
    use IdentityRefusal as R;
    let Some((id, layout)) = &db.instance else {
        if db.foreign_relations {
            return Err(R::NotEmpty);
        }
        if db.initialisation.is_some() || db.binding_rows > 0 || db.ticket_rows > 0 {
            return Err(R::Inconsistent);
        }
        return match (marker, keys) {
            (None, None) if accept_new_database.is_some() => Err(R::ReattachWithoutMarker),
            (None, None) => Ok(Plan::FirstInit),
            (None, Some(_)) => Err(R::KeyWithoutMarker),
            (Some(marker), keys) => {
                if accept_new_database != Some(&marker.instance_id) {
                    return Err(R::EmptyDatabaseWithData);
                }
                if marker.layout_version != LAYOUT_VERSION {
                    return Err(R::UnsupportedLayout);
                }
                if keys.is_none() {
                    return Err(R::MarkerWithoutKey);
                }
                Ok(Plan::Reattach)
            }
        };
    };
    if *layout != LAYOUT_VERSION {
        return Err(R::UnsupportedLayout);
    }
    match db.initialisation {
        None => Err(R::Inconsistent),
        Some(InitialisationState::Started) => {
            if db.binding_rows > 0 || db.ticket_rows > 0 {
                return Err(R::Inconsistent);
            }
            match (marker, keys) {
                (None, None) => Ok(Plan::Resume(InitStep::Marker)),
                (Some(marker), keys) if marker.instance_id == *id => {
                    if marker.layout_version != LAYOUT_VERSION {
                        return Err(R::UnsupportedLayout);
                    }
                    Ok(Plan::Resume(if keys.is_some() {
                        InitStep::Bind
                    } else {
                        InitStep::Key
                    }))
                }
                _ => Err(R::InterruptedInitialisation),
            }
        }
        Some(InitialisationState::Completed) => {
            let marker = marker.ok_or(R::DatabaseWithoutMarker)?;
            if marker.instance_id != *id {
                return Err(R::ForeignDatabase);
            }
            if marker.layout_version != *layout {
                return Err(R::LayoutMismatch);
            }
            let keys = keys.ok_or(R::MarkerWithoutKey)?;
            let (binding_key_id, binding_public) =
                db.active_binding.as_ref().ok_or(R::Inconsistent)?;
            let first_ticket = db.first_ticket_key_id.as_ref().ok_or(R::Inconsistent)?;
            if binding_public != &keys.content_public_key
                || binding_key_id != &keys.content_key_id
                || first_ticket != &keys.ticket_key_id
            {
                return Err(R::KeyMismatch);
            }
            Ok(Plan::Normal)
        }
    }
}

/// An entry of the `_sqlx_migrations` table.
#[derive(Clone, Debug)]
pub(super) struct AppliedMigration {
    pub version: i64,
    pub checksum: Vec<u8>,
    pub success: bool,
}

/// A migration compiled into this build.
#[derive(Clone, Debug)]
pub(super) struct KnownMigration {
    pub version: i64,
    pub checksum: Vec<u8>,
}

/// Why the migration history stops the start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationRefusal {
    /// The database has a migration this build does not contain.
    Downgrade { version: i64 },
    /// An applied migration differs from the one in this build.
    Changed { version: i64 },
    /// A migration failed earlier.
    Failed { version: i64 },
}

impl fmt::Display for MigrationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Downgrade { version } => write!(
                f,
                "the database was migrated by a newer server (migration {version} is unknown \
                 here); downgrade refused"
            ),
            Self::Changed { version } => write!(f, "migration {version} was changed"),
            Self::Failed { version } => write!(f, "migration {version} failed earlier"),
        }
    }
}

/// What the history of applied migrations leaves to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MigrationState {
    /// Highest applied version.
    pub applied_max: Option<i64>,
    /// Versions of this build that are not applied, ascending.
    pub pending: Vec<i64>,
}

/// Checks the applied migrations against the ones of this build, lowest version first.
pub(super) fn classify_migrations(
    applied: &[AppliedMigration],
    known: &[KnownMigration],
) -> Result<MigrationState, MigrationRefusal> {
    let mut rows: Vec<&AppliedMigration> = applied.iter().collect();
    rows.sort_by_key(|row| row.version);
    for row in &rows {
        let Some(expected) = known.iter().find(|known| known.version == row.version) else {
            return Err(MigrationRefusal::Downgrade {
                version: row.version,
            });
        };
        if expected.checksum != row.checksum {
            return Err(MigrationRefusal::Changed {
                version: row.version,
            });
        }
        if !row.success {
            return Err(MigrationRefusal::Failed {
                version: row.version,
            });
        }
    }
    let mut pending: Vec<i64> = known
        .iter()
        .map(|known| known.version)
        .filter(|version| !rows.iter().any(|row| row.version == *version))
        .collect();
    pending.sort_unstable();
    Ok(MigrationState {
        applied_max: rows.last().map(|row| row.version),
        pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b3c1e9a-0000-4000-8000-000000000001";
    const OTHER: &str = "0b3c1e9a-0000-4000-8000-000000000002";

    fn id(text: &str) -> InstanceId {
        InstanceId::parse(text).unwrap()
    }

    fn marker(text: &str) -> DataMarker {
        DataMarker {
            instance_id: id(text),
            layout_version: LAYOUT_VERSION,
        }
    }

    fn keys() -> PublicKeys {
        PublicKeys {
            content_public_key: [1; 32],
            content_key_id: [2; 8],
            ticket_key_id: [3; 8],
        }
    }

    fn completed() -> DbFacts {
        DbFacts {
            instance: Some((id(ID), LAYOUT_VERSION)),
            initialisation: Some(InitialisationState::Completed),
            binding_rows: 1,
            active_binding: Some((vec![2; 8], vec![1; 32])),
            ticket_rows: 1,
            first_ticket_key_id: Some(vec![3; 8]),
            foreign_relations: false,
        }
    }

    fn started() -> DbFacts {
        DbFacts {
            instance: Some((id(ID), LAYOUT_VERSION)),
            initialisation: Some(InitialisationState::Started),
            ..DbFacts::default()
        }
    }

    use IdentityRefusal as R;

    #[test]
    fn an_empty_everything_is_a_first_initialisation() {
        let db = DbFacts::default();
        assert_eq!(decide(&db, None, None, None), Ok(Plan::FirstInit));
        // Consent to reattach without a marker is refused, not ignored.
        assert_eq!(
            decide(&db, None, None, Some(&id(ID))),
            Err(R::ReattachWithoutMarker)
        );
        assert_eq!(
            decide(&db, None, Some(&keys()), None),
            Err(R::KeyWithoutMarker)
        );
    }

    #[test]
    fn a_database_with_other_relations_is_never_adopted() {
        let db = DbFacts {
            foreign_relations: true,
            ..DbFacts::default()
        };
        assert_eq!(decide(&db, None, None, None), Err(R::NotEmpty));
        assert_eq!(
            decide(&db, Some(&marker(ID)), Some(&keys()), Some(&id(ID))),
            Err(R::NotEmpty)
        );
    }

    #[test]
    fn a_marker_with_an_unsupported_layout_stops_a_resume_before_any_write() {
        let mut newer = marker(ID);
        newer.layout_version = LAYOUT_VERSION + 1;
        assert_eq!(
            decide(&started(), Some(&newer), None, None),
            Err(R::UnsupportedLayout)
        );
        assert_eq!(
            decide(&started(), Some(&newer), Some(&keys()), None),
            Err(R::UnsupportedLayout)
        );
    }

    #[test]
    fn an_empty_database_with_a_marker_needs_the_exact_instance_identifier() {
        let db = DbFacts::default();
        let marker = marker(ID);
        let keys = keys();
        assert_eq!(
            decide(&db, Some(&marker), Some(&keys), None),
            Err(R::EmptyDatabaseWithData)
        );
        assert_eq!(
            decide(&db, Some(&marker), Some(&keys), Some(&id(OTHER))),
            Err(R::EmptyDatabaseWithData)
        );
        assert_eq!(
            decide(&db, Some(&marker), None, Some(&id(ID))),
            Err(R::MarkerWithoutKey)
        );
        assert_eq!(
            decide(&db, Some(&marker), Some(&keys), Some(&id(ID))),
            Ok(Plan::Reattach)
        );
        let newer = DataMarker {
            layout_version: LAYOUT_VERSION + 1,
            ..marker
        };
        assert_eq!(
            decide(&db, Some(&newer), Some(&keys), Some(&id(ID))),
            Err(R::UnsupportedLayout)
        );
    }

    #[test]
    fn a_database_that_contradicts_itself_is_refused() {
        let leftovers = [
            DbFacts {
                initialisation: Some(InitialisationState::Started),
                ..DbFacts::default()
            },
            DbFacts {
                binding_rows: 1,
                ..DbFacts::default()
            },
            DbFacts {
                ticket_rows: 1,
                ..DbFacts::default()
            },
        ];
        for db in &leftovers {
            assert_eq!(decide(db, None, None, None), Err(R::Inconsistent));
        }
        // An instance without an initialisation row, or without its key rows after completion.
        let mut db = completed();
        db.initialisation = None;
        assert_eq!(
            decide(&db, Some(&marker(ID)), Some(&keys()), None),
            Err(R::Inconsistent)
        );
        let mut db = completed();
        db.active_binding = None;
        assert_eq!(
            decide(&db, Some(&marker(ID)), Some(&keys()), None),
            Err(R::Inconsistent)
        );
        let mut db = completed();
        db.first_ticket_key_id = None;
        assert_eq!(
            decide(&db, Some(&marker(ID)), Some(&keys()), None),
            Err(R::Inconsistent)
        );
    }

    #[test]
    fn a_completed_instance_starts_only_with_its_own_marker_and_key() {
        let db = completed();
        let marker = marker(ID);
        let keys = keys();
        assert_eq!(
            decide(&db, Some(&marker), Some(&keys), None),
            Ok(Plan::Normal)
        );
        // The reattach option changes nothing here either.
        assert_eq!(
            decide(&db, Some(&marker), Some(&keys), Some(&id(OTHER))),
            Ok(Plan::Normal)
        );
        assert_eq!(
            decide(&db, None, Some(&keys), None),
            Err(R::DatabaseWithoutMarker)
        );
        assert_eq!(
            decide(&db, Some(&self::marker(OTHER)), Some(&keys), None),
            Err(R::ForeignDatabase)
        );
        assert_eq!(
            decide(&db, Some(&marker), None, None),
            Err(R::MarkerWithoutKey)
        );
        for change in [
            (|keys: &mut PublicKeys| keys.content_public_key[0] ^= 1) as fn(&mut PublicKeys),
            |keys: &mut PublicKeys| keys.content_key_id[0] ^= 1,
            |keys: &mut PublicKeys| keys.ticket_key_id[0] ^= 1,
        ] {
            let mut other = keys.clone();
            change(&mut other);
            assert_eq!(
                decide(&db, Some(&marker), Some(&other), None),
                Err(R::KeyMismatch)
            );
        }
        let mut layout = completed();
        layout.instance = Some((id(ID), LAYOUT_VERSION + 1));
        assert_eq!(
            decide(&layout, Some(&marker), Some(&keys), None),
            Err(R::UnsupportedLayout)
        );
        let mut marker_layout = self::marker(ID);
        marker_layout.layout_version = LAYOUT_VERSION + 1;
        assert_eq!(
            decide(&db, Some(&marker_layout), Some(&keys), None),
            Err(R::LayoutMismatch)
        );
    }

    #[test]
    fn an_interrupted_initialisation_resumes_only_what_it_began() {
        let db = started();
        let keys = keys();
        assert_eq!(
            decide(&db, None, None, None),
            Ok(Plan::Resume(InitStep::Marker))
        );
        assert_eq!(
            decide(&db, Some(&marker(ID)), None, None),
            Ok(Plan::Resume(InitStep::Key))
        );
        assert_eq!(
            decide(&db, Some(&marker(ID)), Some(&keys), None),
            Ok(Plan::Resume(InitStep::Bind))
        );
        // Every other combination is refused.
        assert_eq!(
            decide(&db, None, Some(&keys), None),
            Err(R::InterruptedInitialisation)
        );
        assert_eq!(
            decide(&db, Some(&marker(OTHER)), None, None),
            Err(R::InterruptedInitialisation)
        );
        assert_eq!(
            decide(&db, Some(&marker(OTHER)), Some(&keys), None),
            Err(R::InterruptedInitialisation)
        );
        // Key rows next to a started initialisation cannot come from the one transaction.
        let mut bound = started();
        bound.binding_rows = 1;
        assert_eq!(decide(&bound, None, None, None), Err(R::Inconsistent));
    }

    fn known() -> Vec<KnownMigration> {
        (1..=3)
            .map(|version| KnownMigration {
                version,
                checksum: vec![version as u8],
            })
            .collect()
    }

    fn applied(version: i64) -> AppliedMigration {
        AppliedMigration {
            version,
            checksum: vec![version as u8],
            success: true,
        }
    }

    #[test]
    fn the_migration_history_decides_what_is_pending() {
        let state = classify_migrations(&[], &known()).unwrap();
        assert_eq!((state.applied_max, state.pending), (None, vec![1, 2, 3]));
        let state = classify_migrations(&[applied(2), applied(1)], &known()).unwrap();
        assert_eq!((state.applied_max, state.pending), (Some(2), vec![3]));
        let state = classify_migrations(&[applied(1), applied(2), applied(3)], &known()).unwrap();
        assert_eq!((state.applied_max, state.pending), (Some(3), vec![]));
    }

    #[test]
    fn a_history_this_build_cannot_continue_is_refused() {
        assert_eq!(
            classify_migrations(&[applied(1), applied(9)], &known()).unwrap_err(),
            MigrationRefusal::Downgrade { version: 9 }
        );
        let mut changed = applied(2);
        changed.checksum = vec![42];
        assert_eq!(
            classify_migrations(&[applied(1), changed], &known()).unwrap_err(),
            MigrationRefusal::Changed { version: 2 }
        );
        let mut failed = applied(1);
        failed.success = false;
        assert_eq!(
            classify_migrations(&[failed], &known()).unwrap_err(),
            MigrationRefusal::Failed { version: 1 }
        );
        let text = MigrationRefusal::Downgrade { version: 9 }.to_string();
        assert!(text.contains("downgrade refused"), "{text}");
    }
}
