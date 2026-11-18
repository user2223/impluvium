// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Versioned library merge and isolated rebuilds (events.md §8). Inputs to this
//! decoded-model layer have already passed the API/event validators. Request and
//! whole-build deadlines belong to caller policy: expiry invokes fail_rebuild.

use super::{Epoch, Position};
use crate::proto::impluvium::v2::{GameSummary, LibraryChanged, ListLibraryResponse};

#[derive(Clone, Copy, Debug)]
pub struct Budgets {
    pub pages: usize,
    pub buffered_events: usize,
    pub reorder_events: usize,
    /// Includes tombstones. Bounds both the live model and its candidate.
    pub entries: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetExceeded {
    Pages,
    BufferedEvents,
    ReorderEvents,
    Entries,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub game_id: String,
    pub version: u64,
    /// None retains a deletion marker; a later version may recreate the entry.
    pub game: Option<GameSummary>,
}
impl Entry {
    pub fn game(game: GameSummary) -> Self {
        Self {
            game_id: game.game_id.clone(),
            version: game.entry_version,
            game: Some(game),
        }
    }
}
#[derive(Clone, Debug)]
struct Data {
    seq: u64,
    prev: u64,
    entry: Option<Entry>,
}
#[derive(Clone, Debug)]
struct Candidate {
    epoch: Epoch,
    entries: Vec<Entry>,
    positioned: bool,
    finished_pages: bool,
    pages: usize,
}
#[derive(Clone, Debug)]
pub struct Library {
    epoch: Option<Epoch>,
    model_epoch: Option<Epoch>,
    applied: u64,
    baseline: bool,
    entries: Vec<Entry>,
    buffer: Vec<Data>,
    candidate: Option<Candidate>,
    candidate_applied: u64,
    highest_heartbeat: u64,
    rebuild: bool,
    stale: bool,
    budgets: Budgets,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Applied,
    Duplicate,
    HeldGap,
    BufferedBuilding,
    PageMerged,
    HeartbeatCurrent,
    HeartbeatRebuild,
    NewEpochRebuild,
    SameEpoch,
    RebuildStarted,
    RebuildReplaced,
    RebuildFailed,
    RebuildGap,
    PageEpochRebuild,
    Budget(BudgetExceeded),
}
fn merge(entries: &mut Vec<Entry>, entry: Entry, capacity: usize) -> Result<(), BudgetExceeded> {
    if let Some(known) = entries.iter_mut().find(|e| e.game_id == entry.game_id) {
        if entry.version > known.version {
            *known = entry;
        }
    } else {
        if entries.len() >= capacity {
            return Err(BudgetExceeded::Entries);
        }
        entries.push(entry);
    }
    Ok(())
}
fn drain(
    entries: &mut Vec<Entry>,
    applied: &mut u64,
    buffer: &mut Vec<Data>,
    capacity: usize,
) -> Result<(), BudgetExceeded> {
    buffer.sort_unstable_by_key(|e| e.seq);
    while let Some(event) = buffer.first() {
        if event.seq <= *applied {
            buffer.remove(0);
            continue;
        }
        if event.prev > *applied {
            break;
        }
        // Do not allocate or advance the contiguous position if merge fails.
        if let Some(entry) = &event.entry {
            merge(entries, entry.clone(), capacity)?;
        }
        *applied = event.seq;
        buffer.remove(0);
    }
    Ok(())
}
impl Library {
    pub fn empty(budgets: Budgets) -> Self {
        Self {
            epoch: None,
            model_epoch: None,
            applied: 0,
            baseline: false,
            entries: Vec::new(),
            buffer: Vec::new(),
            candidate: None,
            candidate_applied: 0,
            highest_heartbeat: 0,
            rebuild: true,
            stale: false,
            budgets,
        }
    }
    /// Restore an already validated, contiguously applied baseline. Unchanged
    /// entries may have version zero. Tombstones count toward the entry budget.
    pub fn from_baseline(
        position: Position,
        entries: &[Entry],
        budgets: Budgets,
    ) -> Result<Self, BudgetExceeded> {
        if entries.len() > budgets.entries {
            return Err(BudgetExceeded::Entries);
        }
        let mut state = Self::empty(budgets);
        for e in entries {
            merge(&mut state.entries, e.clone(), budgets.entries)?;
        }
        state.epoch = Some(position.epoch);
        state.model_epoch = Some(position.epoch);
        state.applied = position.seq;
        state.baseline = true;
        state.rebuild = false;
        Ok(state)
    }
    pub fn epoch(&self) -> Option<Epoch> {
        self.epoch
    }
    pub fn model_epoch(&self) -> Option<Epoch> {
        self.model_epoch
    }
    pub fn applied(&self) -> u64 {
        self.applied
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
    pub fn rebuilding(&self) -> bool {
        self.rebuild
    }
    pub fn stale(&self) -> bool {
        self.stale
    }
    pub fn candidate_epoch(&self) -> Option<Epoch> {
        self.candidate.as_ref().map(|c| c.epoch)
    }
    pub fn candidate_entries(&self) -> &[Entry] {
        self.candidate
            .as_ref()
            .map_or(&[], |c| c.entries.as_slice())
    }
    /// Retained after disposal for model observations; never a resume point.
    pub fn candidate_applied(&self) -> u64 {
        self.candidate_applied
    }
    pub fn resume_point(&self) -> Option<Position> {
        if !self.baseline {
            return None;
        }
        self.epoch.map(|epoch| Position {
            epoch,
            seq: self.applied,
        })
    }
    pub fn connected(&self, epoch: Epoch) -> (Self, Decision) {
        let mut next = self.clone();
        if self.epoch == Some(epoch) {
            return (next, Decision::SameEpoch);
        }
        next.epoch = Some(epoch);
        next.applied = 0;
        next.baseline = false;
        next.buffer.clear();
        next.candidate = None;
        next.candidate_applied = 0;
        next.highest_heartbeat = 0;
        next.rebuild = true;
        (next, Decision::NewEpochRebuild)
    }
    pub fn begin_rebuild(&self) -> (Self, Decision) {
        let mut next = self.clone();
        next.rebuild = true;
        next.candidate_applied = 0;
        next.candidate = next.epoch.map(|epoch| Candidate {
            epoch,
            entries: Vec::new(),
            positioned: false,
            finished_pages: false,
            pages: 0,
        });
        (next, Decision::RebuildStarted)
    }
    fn failed(mut self, decision: Decision) -> (Self, Decision) {
        self.candidate = None;
        self.rebuild = true;
        self.stale = true;
        (self, decision)
    }
    pub fn fail_rebuild(&self) -> (Self, Decision) {
        self.clone().failed(Decision::RebuildFailed)
    }
    /// The caller invokes this after its bounded wait for a gap expires.
    pub fn gap_expired(&self) -> (Self, Decision) {
        self.begin_rebuild()
    }
    pub fn heartbeat(&self, seq: u64) -> (Self, Decision) {
        let mut next = self.clone();
        next.highest_heartbeat = next.highest_heartbeat.max(seq);
        // FORMAT.md's label convention uses the candidate once its first page
        // exists. Completion still checks the highest heartbeat in the epoch.
        let position = if next.candidate.as_ref().is_some_and(|c| c.positioned) {
            next.candidate_applied
        } else {
            next.applied
        };
        if seq > position {
            next.rebuild = true;
            (next, Decision::HeartbeatRebuild)
        } else {
            (next, Decision::HeartbeatCurrent)
        }
    }
    pub fn data(&self, seq: u64, prev: u64, change: Option<LibraryChanged>) -> (Self, Decision) {
        let mut next = self.clone();
        if self.baseline && seq <= self.applied {
            return (next, Decision::Duplicate);
        }
        let building = self.rebuild;
        let capacity = if building {
            self.budgets.buffered_events
        } else {
            self.budgets.reorder_events
        };
        // Duplicate held events need neither an allocation nor another slot.
        if self.buffer.iter().any(|e| e.seq == seq) {
            return (next, Decision::Duplicate);
        }
        if !building && prev <= self.applied {
            if let Some(change) = change {
                let entry = Entry {
                    game_id: change.game_id,
                    version: seq,
                    game: if change.deleted {
                        None
                    } else {
                        Some((*change.game).clone())
                    },
                };
                if let Err(e) = merge(&mut next.entries, entry, self.budgets.entries) {
                    return self.clone().failed(Decision::Budget(e));
                }
            }
            next.applied = seq;
            if let Err(e) = drain(
                &mut next.entries,
                &mut next.applied,
                &mut next.buffer,
                self.budgets.entries,
            ) {
                return self.clone().failed(Decision::Budget(e));
            }
            return (next, Decision::Applied);
        }
        if next.buffer.len() >= capacity {
            let e = if building {
                BudgetExceeded::BufferedEvents
            } else {
                BudgetExceeded::ReorderEvents
            };
            // A dropped event requires a fresh snapshot. An older applied
            // baseline remains a valid resume point but is explicitly stale.
            return next.failed(Decision::Budget(e));
        }
        let entry = change.map(|change| Entry {
            game_id: change.game_id,
            version: seq,
            game: if change.deleted {
                None
            } else {
                Some((*change.game).clone())
            },
        });
        next.buffer.push(Data { seq, prev, entry });
        (
            next,
            if building {
                Decision::BufferedBuilding
            } else {
                Decision::HeldGap
            },
        )
    }
    pub fn page(&self, page: &ListLibraryResponse) -> (Self, Decision) {
        let Ok(epoch) = <Epoch>::try_from(page.epoch.as_slice()) else {
            // This layer requires validated pages; a mismatched epoch never
            // mutates the live model, even if an adapter violates that contract.
            return self.clone().failed(Decision::PageEpochRebuild);
        };
        if Some(epoch) != self.epoch {
            let (mut next, _) = self.begin_rebuild();
            next.buffer.clear();
            return (next, Decision::PageEpochRebuild);
        }
        if page.games.len() > 200 {
            return self
                .clone()
                .failed(Decision::Budget(BudgetExceeded::Entries));
        }
        let mut next = self.clone();
        if let Some(candidate) = &mut next.candidate {
            if candidate.pages >= self.budgets.pages {
                return self.clone().failed(Decision::Budget(BudgetExceeded::Pages));
            }
            candidate.pages += 1;
            candidate.finished_pages = page.next_page_token.is_empty();
            if !candidate.positioned {
                next.candidate_applied = page.snapshot_seq;
                candidate.positioned = true;
            }
            for game in &page.games {
                if let Err(e) = merge(
                    &mut candidate.entries,
                    Entry::game(game.clone()),
                    self.budgets.entries,
                ) {
                    return self.clone().failed(Decision::Budget(e));
                }
            }
        } else if self.rebuild {
            // A failed/restarted candidate requires an explicit new build;
            // delayed pages cannot populate the retained live model.
            return (next, Decision::PageEpochRebuild);
        } else {
            // Standalone versioned-merge substage (FORMAT.md), not a reload.
            for game in &page.games {
                if let Err(e) = merge(
                    &mut next.entries,
                    Entry::game(game.clone()),
                    self.budgets.entries,
                ) {
                    return self.clone().failed(Decision::Budget(e));
                }
            }
        }
        (next, Decision::PageMerged)
    }
    /// Called only after all pages finished successfully. Events merge into the
    /// isolated candidate; unresolved chain/heartbeat gaps retain the old model.
    pub fn complete_rebuild(&self) -> (Self, Decision) {
        let mut next = self.clone();
        let Some(mut candidate) = next.candidate.take() else {
            return next.failed(Decision::RebuildGap);
        };
        if !candidate.positioned || !candidate.finished_pages {
            return next.failed(Decision::RebuildGap);
        }
        if let Err(e) = drain(
            &mut candidate.entries,
            &mut next.candidate_applied,
            &mut next.buffer,
            self.budgets.entries,
        ) {
            return next.failed(Decision::Budget(e));
        }
        if !next.buffer.is_empty() || next.candidate_applied < next.highest_heartbeat {
            return next.failed(Decision::RebuildGap);
        }
        next.entries = candidate.entries;
        next.model_epoch = Some(candidate.epoch);
        next.applied = next.candidate_applied;
        next.baseline = true;
        next.rebuild = false;
        next.stale = false;
        (next, Decision::RebuildReplaced)
    }
    pub fn discard(&self) -> Self {
        let mut next = Self::empty(self.budgets);
        next.epoch = self.epoch;
        next
    }
}
