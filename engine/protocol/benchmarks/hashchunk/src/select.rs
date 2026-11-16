// SPDX-License-Identifier: 0BSD
//! The chunk-parameter selection rule of the benchmark protocol, executed mechanically.
//!
//! Input: per corpus class the mean transfer share T (percent of the new version's size) of every candidate, and the
//! set of candidates that violate the format limits. T-bar is the class-equal-weight mean; the gain of A over B is
//! T-bar(B) - T-bar(A) in percentage points.
//!
//! 1. Candidates that violate the limits are dropped.
//! 2. CDC from large to small: start at the largest remaining; a smaller candidate replaces the *current* choice only
//!    with a gain of at least 5 points over the current choice -> C*.
//! 3. Fixed blocks likewise, starting at 16 MiB -> F*.
//! 4. C* wins only if it beats F* and the fixed block of the same nominal size by at least 5 points; else F*.
//! 5. A class that deviates by more than 10 points in the other direction is listed (the protocol must justify the
//!    choice). Interpretation used here (to be reviewed): for every decisive comparison of steps 2-4 (replacements
//!    within a family, kept candidates, C* against F* and against the same-size fixed block) a class deviates when it
//!    would be better off by more than 10 points with the comparison's loser.
//!
//! A result is a decision (`chosen`) only if all preconditions hold: complete grid, step 1 executed, all classes
//! present, a valid same-size fixed block for C*. Otherwise only `diagnostic_choice` is set. A missing comparison
//! value never counts as passed.

use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};

pub const STEP_PP: f64 = 5.0;
pub const DEVIATION_PP: f64 = 10.0;
const EPS: f64 = 1e-9;

#[derive(Clone, Debug)]
pub struct ClassT {
    pub name: String,
    /// candidate id -> mean T in percent of the new version's size
    pub t: BTreeMap<String, f64>,
}

#[derive(Serialize, Debug, Clone)]
pub struct Deviation {
    pub class: String,
    /// The comparison of the rule this deviation belongs to, e.g. "step 2: cdc-1m replaces cdc-4m".
    pub comparison: String,
    pub preferred_by_rule: String,
    pub preferred_by_class: String,
    pub preferred_by_rule_t: f64,
    pub preferred_by_class_t: f64,
    /// T_class(rule's pick) - T_class(other): how much better the class would be off with the other candidate.
    pub points: f64,
}

/// Exact measurements and branch outcomes survive until presentation.
#[derive(Debug, Clone)]
pub enum SelectionStep {
    Start {
        family: &'static str,
        current: String,
        t: f64,
    },
    Compare {
        family: &'static str,
        candidate: String,
        current: String,
        gain: f64,
        replaces: bool,
    },
    Choice {
        family: &'static str,
        current: String,
    },
    Decision {
        cdc: String,
        fixed: String,
        gain: f64,
        same: Option<String>,
        gain_same: Option<f64>,
    },
}

impl SelectionStep {
    /// With display means, all gains are differences of the exported means; exact branch outcomes stay intact.
    pub fn render(
        &self,
        means: Option<&BTreeMap<String, f64>>,
        number: impl Fn(f64) -> String,
    ) -> String {
        let gain = |a: &str, b: &str, exact: f64| means.map(|m| m[a] - m[b]).unwrap_or(exact);
        match self {
            Self::Start { family, current, t } => {
                let t = means.map(|m| m[current]).unwrap_or(*t);
                format!("{family}: start {current} (T-bar {})", number(t))
            }
            Self::Compare {
                family,
                candidate,
                current,
                gain: exact,
                replaces,
            } => {
                let gain = number(gain(current, candidate, *exact));
                if *replaces {
                    format!(
                        "{family}: {candidate} replaces {current}: gain {gain} points >= {STEP_PP}"
                    )
                } else {
                    format!("{family}: {candidate} stays out: gain {gain} points over current {current} < {STEP_PP}")
                }
            }
            Self::Choice { family, current } => format!("{family}*: {current}"),
            Self::Decision {
                cdc,
                fixed,
                gain: exact,
                same,
                gain_same,
            } => {
                let gain_f = number(gain(fixed, cdc, *exact));
                let gain_same = gain_same
                    .map(|g| number(gain(same.as_deref().expect("same-size comparison"), cdc, g)))
                    .unwrap_or_else(|| "n/a".into());
                format!("decision: C* {cdc} vs F* {fixed}: gain {gain_f}; vs same-size fixed {}: gain {gain_same}", same.as_deref().unwrap_or("(none)"))
            }
        }
    }
}

impl Serialize for SelectionStep {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.render(None, |n| format!("{n:.2}")))
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct Selection {
    /// Only set when the inputs satisfy every precondition of the rule (`decidable`).
    pub chosen: Option<String>,
    /// What the rule would pick on the given inputs, for diagnosis. Never a decision.
    pub diagnostic_choice: Option<String>,
    pub decidable: bool,
    pub not_decidable_because: Vec<String>,
    pub c_star: Option<String>,
    pub f_star: Option<String>,
    pub t_bar: BTreeMap<String, f64>,
    pub classes_used: Vec<String>,
    pub steps: Vec<SelectionStep>,
    pub dropped_by_limits: Vec<String>,
    /// Step 5: classes that prefer the loser of a decisive comparison by more than 10 points (all comparisons of
    /// steps 2-4, within a family and across families).
    pub deviations_over_10_points: Vec<Deviation>,
    pub reason: String,
}

fn t_bar(classes: &[ClassT], id: &str) -> Option<f64> {
    let mut sum = 0.0;
    for c in classes {
        sum += *c.t.get(id)?;
    }
    if classes.is_empty() {
        None
    } else {
        Some(sum / classes.len() as f64)
    }
}

/// `cdc_desc` / `fixed_desc` list the candidate ids from large to small; `fixed_for_avg` maps a CDC id to the fixed
/// candidate with the same nominal size. `preconditions` are reasons why the inputs are not a valid rule input
/// (incomplete grid, skipped step 1, missing classes); any of them makes the result diagnostic only.
pub fn select(
    classes: &[ClassT],
    cdc_desc: &[&str],
    fixed_desc: &[&str],
    fixed_for_avg: &dyn Fn(&str) -> Option<String>,
    dropped: &BTreeSet<String>,
    preconditions: &[String],
) -> Selection {
    let mut steps = Vec::new();
    let mut comps: Vec<(String, String, String)> = Vec::new(); // (winner, loser, label)
    let mut missing: Vec<String> = preconditions.to_vec();
    let tb = |id: &str| t_bar(classes, id);
    let mut tb_map = BTreeMap::new();
    for id in cdc_desc.iter().chain(fixed_desc.iter()) {
        match tb(id) {
            Some(v) => {
                tb_map.insert(id.to_string(), v);
            }
            None => {
                if !dropped.contains(*id) {
                    missing.push(format!("no T value for candidate {id}"));
                }
            }
        }
    }
    let pick = |family: &'static str,
                step: u8,
                list: &[&str],
                steps: &mut Vec<SelectionStep>,
                comps: &mut Vec<(String, String, String)>|
     -> Option<String> {
        let valid: Vec<&str> = list
            .iter()
            .copied()
            .filter(|id| !dropped.contains(*id) && tb_map.contains_key(*id))
            .collect();
        let mut cur = *valid.first()?;
        steps.push(SelectionStep::Start {
            family,
            current: cur.into(),
            t: tb_map[cur],
        });
        for cand in valid.iter().skip(1) {
            let gain = tb_map[cur] - tb_map[*cand];
            if gain >= STEP_PP - EPS {
                steps.push(SelectionStep::Compare {
                    family,
                    candidate: cand.to_string(),
                    current: cur.into(),
                    gain,
                    replaces: true,
                });
                comps.push((
                    cand.to_string(),
                    cur.to_string(),
                    format!("step {step}: {cand} replaces {cur}"),
                ));
                cur = cand;
            } else {
                steps.push(SelectionStep::Compare {
                    family,
                    candidate: cand.to_string(),
                    current: cur.into(),
                    gain,
                    replaces: false,
                });
                comps.push((
                    cur.to_string(),
                    cand.to_string(),
                    format!("step {step}: {cur} kept against {cand}"),
                ));
            }
        }
        steps.push(SelectionStep::Choice {
            family,
            current: cur.into(),
        });
        Some(cur.to_string())
    };
    let c_star = pick("CDC", 2, cdc_desc, &mut steps, &mut comps);
    let f_star = pick("fixed", 3, fixed_desc, &mut steps, &mut comps);
    let (diag, reason) = match (&c_star, &f_star) {
        (Some(c), Some(f)) => {
            let gain_f = tb_map[f] - tb_map[c];
            let same = fixed_for_avg(c).filter(|s| tb_map.contains_key(s) && !dropped.contains(s));
            if same.is_none() {
                missing.push(format!(
                    "step 4: no valid fixed block of the same nominal size as {c}"
                ));
            }
            // A missing comparison value never counts as passed.
            let gain_same = same.as_ref().map(|s| tb_map[s] - tb_map[c]);
            let ok = gain_f >= STEP_PP - EPS && gain_same.is_some_and(|g| g >= STEP_PP - EPS);
            steps.push(SelectionStep::Decision {
                cdc: c.clone(),
                fixed: f.clone(),
                gain: gain_f,
                same: same.clone(),
                gain_same,
            });
            let (w, _l, why) = if ok {
                (c.clone(), f.clone(), format!("C* {c} beats F* {f} and the fixed block of the same nominal size by >= {STEP_PP} points"))
            } else {
                (f.clone(), c.clone(), format!("C* {c} does not beat F* {f} and the same-size fixed block by >= {STEP_PP} points; F* is chosen"))
            };
            // Both end comparisons are decisive and registered independently of the overall outcome.
            let (wa, la) = if gain_f >= STEP_PP - EPS {
                (c.clone(), f.clone())
            } else {
                (f.clone(), c.clone())
            };
            comps.push((
                wa.clone(),
                la.clone(),
                format!("step 4: {wa} against {la} (C* vs F*)"),
            ));
            if let (Some(sm), Some(g)) = (&same, gain_same) {
                let (wb, lb) = if g >= STEP_PP - EPS {
                    (c.clone(), sm.clone())
                } else {
                    (sm.clone(), c.clone())
                };
                comps.push((
                    wb.clone(),
                    lb.clone(),
                    format!("step 4: {wb} against {lb} (C* vs same-size fixed)"),
                ));
            }
            (Some(w), why)
        }
        (None, Some(f)) => (Some(f.clone()), "no valid CDC candidate".into()),
        (Some(c), None) => (Some(c.clone()), "no valid fixed candidate".into()),
        (None, None) => (None, "no valid candidate".into()),
    };
    let mut devs = Vec::new();
    for (w, l, label) in &comps {
        for cl in classes {
            if let (Some(tw), Some(tl)) = (cl.t.get(w), cl.t.get(l)) {
                let pts = tw - tl;
                if pts > DEVIATION_PP + EPS {
                    devs.push(Deviation {
                        class: cl.name.clone(),
                        comparison: label.clone(),
                        preferred_by_rule: w.clone(),
                        preferred_by_class: l.clone(),
                        preferred_by_rule_t: *tw,
                        preferred_by_class_t: *tl,
                        points: pts,
                    });
                }
            }
        }
    }
    let decidable = missing.is_empty();
    Selection {
        chosen: if decidable { diag.clone() } else { None },
        diagnostic_choice: diag,
        decidable,
        not_decidable_because: missing,
        c_star,
        f_star,
        t_bar: tb_map,
        classes_used: classes.iter().map(|c| c.name.clone()).collect(),
        steps,
        dropped_by_limits: dropped.iter().cloned().collect(),
        deviations_over_10_points: devs,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cls(name: &str, v: &[(&str, f64)]) -> ClassT {
        ClassT {
            name: name.into(),
            t: v.iter().map(|(k, x)| (k.to_string(), *x)).collect(),
        }
    }
    const CDC: [&str; 3] = ["cdc-4m", "cdc-1m", "cdc-256k"];
    const FIX: [&str; 4] = ["fixed-16m", "fixed-4m", "fixed-1m", "fixed-256k"];
    fn same(c: &str) -> Option<String> {
        Some(c.replace("cdc", "fixed"))
    }

    #[test]
    fn legacy_trace_serialization_matches_golden_bytes() {
        let c = cls(
            "a",
            &[
                ("cdc-4m", 50.1234),
                ("cdc-1m", 46.0123),
                ("cdc-256k", 44.0987),
                ("fixed-16m", 80.9876),
                ("fixed-4m", 80.8765),
                ("fixed-1m", 80.7654),
                ("fixed-256k", 80.6543),
            ],
        );
        let selection = select(&[c], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert!(
            matches!(&selection.steps[0], SelectionStep::Start { t, .. } if *t == 50.1234),
            "trace retains exact structured value"
        );
        let expected = r#"[
  "CDC: start cdc-4m (T-bar 50.12)",
  "CDC: cdc-1m stays out: gain 4.11 points over current cdc-4m < 5",
  "CDC: cdc-256k replaces cdc-4m: gain 6.02 points >= 5",
  "CDC*: cdc-256k",
  "fixed: start fixed-16m (T-bar 80.99)",
  "fixed: fixed-4m stays out: gain 0.11 points over current fixed-16m < 5",
  "fixed: fixed-1m stays out: gain 0.22 points over current fixed-16m < 5",
  "fixed: fixed-256k stays out: gain 0.33 points over current fixed-16m < 5",
  "fixed*: fixed-16m",
  "decision: C* cdc-256k vs F* fixed-16m: gain 36.89; vs same-size fixed fixed-256k: gain 36.56"
]"#;
        assert_eq!(
            serde_json::to_string_pretty(&selection.steps)
                .unwrap()
                .as_bytes(),
            expected.as_bytes(),
            "legacy trace bytes"
        );
        let missing = select(
            &[cls("a", &[("cdc-1m", 0.012345), ("fixed-16m", 4.99999)])],
            &["cdc-1m"],
            &["fixed-16m"],
            &|_| None,
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(
            serde_json::to_string(&missing.steps[0]).unwrap(),
            "\"CDC: start cdc-1m (T-bar 0.01)\""
        );
        assert_eq!(serde_json::to_string(missing.steps.last().unwrap()).unwrap(),
            "\"decision: C* cdc-1m vs F* fixed-16m: gain 4.99; vs same-size fixed (none): gain n/a\"");
    }

    #[test]
    fn compares_with_current_not_neighbour() {
        // 1m gains 4 over 4m (stays out), 256k gains 6 over 4m (the current choice) -> 256k.
        let c = cls(
            "a",
            &[
                ("cdc-4m", 50.0),
                ("cdc-1m", 46.0),
                ("cdc-256k", 44.0),
                ("fixed-16m", 80.0),
                ("fixed-4m", 80.0),
                ("fixed-1m", 80.0),
                ("fixed-256k", 80.0),
            ],
        );
        let s = select(&[c], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert_eq!(s.c_star.as_deref(), Some("cdc-256k"));
        assert_eq!(s.chosen.as_deref(), Some("cdc-256k"));
    }

    #[test]
    fn exactly_five_points_counts() {
        let c = cls(
            "a",
            &[
                ("cdc-4m", 50.0),
                ("cdc-1m", 45.0),
                ("cdc-256k", 45.0),
                ("fixed-16m", 60.0),
                ("fixed-4m", 60.0),
                ("fixed-1m", 60.0),
                ("fixed-256k", 60.0),
            ],
        );
        let s = select(&[c], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert_eq!(s.c_star.as_deref(), Some("cdc-1m"));
    }

    #[test]
    fn fixed_wins_without_cdc_gain() {
        let c = cls(
            "a",
            &[
                ("cdc-4m", 30.0),
                ("cdc-1m", 30.0),
                ("cdc-256k", 30.0),
                ("fixed-16m", 33.0),
                ("fixed-4m", 33.0),
                ("fixed-1m", 33.0),
                ("fixed-256k", 33.0),
            ],
        );
        let s = select(&[c], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert_eq!(s.chosen.as_deref(), Some("fixed-16m"));
    }

    #[test]
    fn cdc_must_beat_same_size_fixed_too() {
        // C* = cdc-1m beats F* (fixed-16m, 70) by 20 but not fixed-1m (T 35 vs cdc 30: gain 5 -> ok) ; make it 34 -> fails.
        let c = cls(
            "a",
            &[
                ("cdc-4m", 40.0),
                ("cdc-1m", 30.0),
                ("cdc-256k", 30.0),
                ("fixed-16m", 70.0),
                ("fixed-4m", 60.0),
                ("fixed-1m", 34.0),
                ("fixed-256k", 34.0),
            ],
        );
        let s = select(&[c], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert_eq!(s.c_star.as_deref(), Some("cdc-1m"));
        assert_eq!(
            s.f_star.as_deref(),
            Some("fixed-1m"),
            "fixed-4m 60->34 gains 26 over 16m: 4m replaces; then 1m vs 4m... "
        );
        assert_eq!(s.chosen.as_deref(), Some("fixed-1m"));
    }

    #[test]
    fn limit_violation_drops_candidate_and_start_moves() {
        let c = cls(
            "a",
            &[
                ("cdc-4m", 50.0),
                ("cdc-1m", 40.0),
                ("cdc-256k", 40.0),
                ("fixed-16m", 90.0),
                ("fixed-4m", 90.0),
                ("fixed-1m", 90.0),
                ("fixed-256k", 90.0),
            ],
        );
        let mut d = BTreeSet::new();
        d.insert("cdc-4m".to_string());
        let s = select(&[c], &CDC, &FIX, &same, &d, &[]);
        assert_eq!(s.c_star.as_deref(), Some("cdc-1m"));
        assert_eq!(s.dropped_by_limits, vec!["cdc-4m".to_string()]);
    }

    #[test]
    fn different_final_opponents_second_condition_decides() {
        // C* = cdc-1m beats F* = fixed-4m by 6 points but not the same-size fixed-1m (gain 4): F* must win.
        let c = cls(
            "a",
            &[
                ("cdc-4m", 50.0),
                ("cdc-1m", 30.0),
                ("cdc-256k", 30.0),
                ("fixed-16m", 70.0),
                ("fixed-4m", 36.0),
                ("fixed-1m", 34.0),
                ("fixed-256k", 34.0),
            ],
        );
        let s = select(&[c], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert_eq!(s.c_star.as_deref(), Some("cdc-1m"));
        assert_eq!(s.f_star.as_deref(), Some("fixed-4m"));
        assert_eq!(s.chosen.as_deref(), Some("fixed-4m"), "{:?}", s.steps);
    }

    #[test]
    fn missing_same_size_comparison_is_not_a_pass() {
        let c = cls(
            "a",
            &[
                ("cdc-4m", 50.0),
                ("cdc-1m", 30.0),
                ("cdc-256k", 30.0),
                ("fixed-16m", 70.0),
                ("fixed-4m", 36.0),
                ("fixed-1m", 34.0),
                ("fixed-256k", 34.0),
            ],
        );
        let mut d = BTreeSet::new();
        d.insert("fixed-1m".to_string());
        let s = select(&[c], &CDC, &FIX, &same, &d, &[]);
        assert!(!s.decidable);
        assert!(s.chosen.is_none());
        assert_eq!(
            s.diagnostic_choice.as_deref(),
            Some("fixed-4m"),
            "missing same-size value must not count as passed"
        );
        assert!(s
            .not_decidable_because
            .iter()
            .any(|r| r.contains("same nominal size")));
    }

    #[test]
    fn preconditions_make_result_diagnostic() {
        let c = cls(
            "a",
            &[
                ("cdc-4m", 20.0),
                ("cdc-1m", 20.0),
                ("cdc-256k", 20.0),
                ("fixed-16m", 60.0),
                ("fixed-4m", 60.0),
                ("fixed-1m", 60.0),
                ("fixed-256k", 60.0),
            ],
        );
        let s = select(
            &[c],
            &CDC,
            &FIX,
            &same,
            &BTreeSet::new(),
            &["step 1 not executed".to_string()],
        );
        assert!(!s.decidable && s.chosen.is_none());
        assert_eq!(s.diagnostic_choice.as_deref(), Some("cdc-4m"));
    }

    #[test]
    fn both_end_comparisons_are_registered_when_both_fail() {
        // Means C*=30, F*=32, same-size fixed=34: both end conditions fail. Class a
        // prefers CDC over both opponents by 25 points.
        let mk = |n: &str, c: f64, f16: f64, f4: f64| {
            cls(
                n,
                &[
                    ("cdc-4m", c),
                    ("cdc-1m", c),
                    ("cdc-256k", c),
                    ("fixed-16m", f16),
                    ("fixed-4m", f4),
                    ("fixed-1m", f4),
                    ("fixed-256k", f4),
                ],
            )
        };
        let s = select(
            &[
                mk("a", 20.0, 45.0, 45.0),
                mk("b", 35.0, 25.5, 28.5),
                mk("c", 35.0, 25.5, 28.5),
            ],
            &CDC,
            &FIX,
            &same,
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(s.c_star.as_deref(), Some("cdc-4m"));
        assert_eq!(s.f_star.as_deref(), Some("fixed-16m"));
        assert_eq!(s.chosen.as_deref(), Some("fixed-16m"));
        let d: Vec<_> = s
            .deviations_over_10_points
            .iter()
            .filter(|d| d.class == "a" && d.comparison.contains("same-size"))
            .collect();
        assert_eq!(d.len(), 1, "{:?}", s.deviations_over_10_points);
        assert!((d[0].points - 25.0).abs() < 1e-9);
        assert_eq!(d[0].preferred_by_class, "cdc-4m");
        assert!(s
            .deviations_over_10_points
            .iter()
            .any(|d| d.class == "a" && d.comparison.contains("C* vs F*")));
    }

    #[test]
    fn same_family_deviation_is_listed() {
        // overall: cdc-1m replaces cdc-4m; class b would be 20 points better off with cdc-4m.
        let a = cls(
            "a",
            &[
                ("cdc-4m", 80.0),
                ("cdc-1m", 40.0),
                ("cdc-256k", 40.0),
                ("fixed-16m", 90.0),
                ("fixed-4m", 90.0),
                ("fixed-1m", 90.0),
                ("fixed-256k", 90.0),
            ],
        );
        let b = cls(
            "b",
            &[
                ("cdc-4m", 30.0),
                ("cdc-1m", 50.0),
                ("cdc-256k", 50.0),
                ("fixed-16m", 90.0),
                ("fixed-4m", 90.0),
                ("fixed-1m", 90.0),
                ("fixed-256k", 90.0),
            ],
        );
        let s = select(&[a, b], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert_eq!(s.c_star.as_deref(), Some("cdc-1m"));
        let d: Vec<_> = s
            .deviations_over_10_points
            .iter()
            .filter(|d| d.class == "b" && d.comparison.contains("cdc-1m replaces cdc-4m"))
            .collect();
        assert_eq!(d.len(), 1, "{:?}", s.deviations_over_10_points);
        assert_eq!(d[0].preferred_by_class, "cdc-4m");
        assert!((d[0].points - 20.0).abs() < 1e-9);
    }

    #[test]
    fn class_deviation_over_ten_points_is_listed() {
        let a = cls(
            "a",
            &[
                ("cdc-4m", 20.0),
                ("cdc-1m", 20.0),
                ("cdc-256k", 20.0),
                ("fixed-16m", 60.0),
                ("fixed-4m", 60.0),
                ("fixed-1m", 60.0),
                ("fixed-256k", 60.0),
            ],
        );
        let b = cls(
            "b",
            &[
                ("cdc-4m", 70.0),
                ("cdc-1m", 70.0),
                ("cdc-256k", 70.0),
                ("fixed-16m", 50.0),
                ("fixed-4m", 50.0),
                ("fixed-1m", 50.0),
                ("fixed-256k", 50.0),
            ],
        );
        let s = select(&[a, b], &CDC, &FIX, &same, &BTreeSet::new(), &[]);
        assert_eq!(s.chosen.as_deref(), Some("cdc-4m"));
        assert!(!s.deviations_over_10_points.is_empty());
        assert!(s.deviations_over_10_points.iter().all(|d| d.class == "b"));
        assert!(s
            .deviations_over_10_points
            .iter()
            .any(|d| d.preferred_by_class.starts_with("fixed")));
    }
}

#[cfg(test)]
mod threshold_tests {
    use super::*;
    #[test]
    fn exactly_ten_is_not_a_deviation_within_or_across_families() {
        let mk = |name: &str, c: f64, smaller: f64, fixed: f64| ClassT {
            name: name.into(),
            t: [
                ("cdc-4m".into(), c),
                ("cdc-1m".into(), smaller),
                ("fixed-4m".into(), fixed),
                ("fixed-1m".into(), fixed),
            ]
            .into(),
        };
        let s = select(
            &[mk("a", 80.0, 20.0, 90.0), mk("b", 20.0, 30.0, 20.0)],
            &["cdc-4m", "cdc-1m"],
            &["fixed-4m", "fixed-1m"],
            &|c| Some(c.replace("cdc", "fixed")),
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(s.chosen.as_deref(), Some("cdc-1m"));
        assert!(s.deviations_over_10_points.is_empty());
    }
    #[test]
    fn below_five_never_replaces_current() {
        let c = ClassT {
            name: "a".into(),
            t: [
                ("cdc-4m".into(), 50.0),
                ("cdc-1m".into(), 45.5),
                ("fixed-4m".into(), 80.0),
                ("fixed-1m".into(), 80.0),
            ]
            .into(),
        };
        let s = select(
            &[c],
            &["cdc-4m", "cdc-1m"],
            &["fixed-4m", "fixed-1m"],
            &|c| Some(c.replace("cdc", "fixed")),
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(s.c_star.as_deref(), Some("cdc-4m"));
    }
}

#[cfg(test)]
mod final_threshold_tests {
    use super::*;
    #[test]
    fn both_final_gains_accept_exactly_five_and_reject_either_smaller_gain() {
        for (fixed_large, fixed_same, winner) in [
            (35.0, 35.0, "cdc-1m"),
            (34.5, 35.0, "fixed-4m"),
            (36.0, 34.5, "fixed-4m"),
        ] {
            let class = ClassT {
                name: "a".into(),
                t: [
                    ("cdc-1m".into(), 30.0),
                    ("fixed-4m".into(), fixed_large),
                    ("fixed-1m".into(), fixed_same),
                ]
                .into(),
            };
            let s = select(
                &[class],
                &["cdc-1m"],
                &["fixed-4m", "fixed-1m"],
                &|_| Some("fixed-1m".into()),
                &BTreeSet::new(),
                &[],
            );
            assert_eq!(s.chosen.as_deref(), Some(winner));
        }
    }
}
