// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Paged lists.
//!
//! The server pages lists with `skip` and `take` and caps `take`; [`PageQuery`]
//! carries these two and [`PageWindow`] is their checked form. A response is
//! either a [`Page`] (items plus the total number of matches, for lists that
//! can count) or a [`CursorPage`] (items plus an opaque cursor for the next
//! page, for lists that cannot). Both use the snake_case names of the error
//! body.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Items per page when the query names none.
pub const DEFAULT_TAKE: u32 = 24;
/// Largest page; a larger `take` is cut to this value.
pub const MAX_TAKE: u32 = 50;
/// Largest `skip`; a larger value is refused.
pub const MAX_SKIP: u32 = 1_000_000;

/// The paging part of a list query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct PageQuery {
    /// Number of items to skip; defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub skip: Option<u32>,
    /// Number of items to return; defaults to [`DEFAULT_TAKE`], at most [`MAX_TAKE`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub take: Option<u32>,
}

/// The checked window of a [`PageQuery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageWindow {
    /// Items to skip, at most [`MAX_SKIP`].
    pub skip: u32,
    /// Items to return, from 1 to [`MAX_TAKE`].
    pub take: u32,
}

/// Why a [`PageQuery`] is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageQueryError {
    /// `skip` is above [`MAX_SKIP`].
    SkipTooLarge,
    /// `take` is 0.
    TakeIsZero,
}

impl fmt::Display for PageQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SkipTooLarge => "skip is too large",
            Self::TakeIsZero => "take must be at least 1",
        })
    }
}

impl std::error::Error for PageQueryError {}

impl PageQuery {
    /// Applies the defaults and bounds. A `take` above [`MAX_TAKE`] is cut to it.
    pub fn window(&self) -> Result<PageWindow, PageQueryError> {
        let skip = self.skip.unwrap_or(0);
        if skip > MAX_SKIP {
            return Err(PageQueryError::SkipTooLarge);
        }
        let take = self.take.unwrap_or(DEFAULT_TAKE);
        if take == 0 {
            return Err(PageQueryError::TakeIsZero);
        }
        Ok(PageWindow {
            skip,
            take: take.min(MAX_TAKE),
        })
    }
}

/// A page of a list that knows its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Page<T> {
    /// The items of this page.
    pub items: Vec<T>,
    /// The number of items of the whole list.
    pub total: u32,
}

/// A page of a list that is walked with a cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CursorPage<T> {
    /// The items of this page.
    pub items: Vec<T>,
    /// Opaque cursor of the next page; `null` on the last page.
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_bounds_of_the_window() {
        let window = |skip, take| PageQuery { skip, take }.window();
        assert_eq!(
            window(None, None),
            Ok(PageWindow {
                skip: 0,
                take: DEFAULT_TAKE
            })
        );
        assert_eq!(
            window(Some(10), Some(1)),
            Ok(PageWindow { skip: 10, take: 1 })
        );
        assert_eq!(
            window(None, Some(MAX_TAKE)),
            Ok(PageWindow {
                skip: 0,
                take: MAX_TAKE
            })
        );
        assert_eq!(
            window(None, Some(u32::MAX)),
            Ok(PageWindow {
                skip: 0,
                take: MAX_TAKE
            })
        );
        assert_eq!(window(Some(MAX_SKIP), None).map(|w| w.skip), Ok(MAX_SKIP));
        assert_eq!(
            window(Some(MAX_SKIP + 1), None),
            Err(PageQueryError::SkipTooLarge)
        );
        assert_eq!(window(None, Some(0)), Err(PageQueryError::TakeIsZero));
    }

    #[test]
    fn query_reads_the_three_shapes_and_refuses_unknown_names() {
        let none: PageQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(none, PageQuery::default());
        let both: PageQuery = serde_json::from_str(r#"{"skip":5,"take":7}"#).unwrap();
        assert_eq!((both.skip, both.take), (Some(5), Some(7)));
        assert_eq!(
            serde_json::to_string(&both).unwrap(),
            r#"{"skip":5,"take":7}"#
        );
        assert_eq!(serde_json::to_string(&none).unwrap(), "{}");
        assert!(serde_json::from_str::<PageQuery>(r#"{"limit":5}"#).is_err());
        assert!(serde_json::from_str::<PageQuery>(r#"{"take":-1}"#).is_err());
    }

    #[test]
    fn pages_have_the_documented_wire_shape() {
        let page = Page {
            items: vec!["a", "b"],
            total: 7,
        };
        assert_eq!(
            serde_json::to_string(&page).unwrap(),
            r#"{"items":["a","b"],"total":7}"#
        );
        let cursor = CursorPage {
            items: vec![1],
            next_cursor: None,
        };
        assert_eq!(
            serde_json::to_string(&cursor).unwrap(),
            r#"{"items":[1],"next_cursor":null}"#
        );
        let back: CursorPage<u8> =
            serde_json::from_str(r#"{"items":[],"next_cursor":"c1"}"#).unwrap();
        assert_eq!(back.next_cursor.as_deref(), Some("c1"));
    }
}
