//! Parsing of `git fetch --porcelain` output (requires git >= 2.41).
//!
//! Each updated ref is reported as one `<flag> <old-oid> <new-oid> <local-ref>`
//! line; rejected updates may carry a `(reason)` suffix after the ref name.
use std::str::FromStr;

/// One ref update reported by `git fetch --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefUpdate {
    /// Fast-forward update (' ').
    FastForward {
        old: gix_hash::ObjectId,
        new: gix_hash::ObjectId,
        reference: String,
    },
    /// Forced (non-fast-forward) update ('+').
    Forced {
        old: gix_hash::ObjectId,
        new: gix_hash::ObjectId,
        reference: String,
    },
    /// Newly created ref ('*').
    New {
        new: gix_hash::ObjectId,
        reference: String,
    },
    /// Deleted ref ('-').
    Deleted {
        old: gix_hash::ObjectId,
        reference: String,
    },
    /// Update rejected by the remote ('!'), with an optional reason.
    Rejected {
        reference: String,
        reason: Option<String>,
    },
}

impl RefUpdate {
    /// The local ref this update applies to.
    pub fn reference(&self) -> &str {
        match self {
            RefUpdate::FastForward { reference, .. } => reference,
            RefUpdate::Forced { reference, .. } => reference,
            RefUpdate::New { reference, .. } => reference,
            RefUpdate::Deleted { reference, .. } => reference,
            RefUpdate::Rejected { reference, .. } => reference,
        }
    }
}

/// One ref update reported by `git push --porcelain`.
///
/// Unlike fetch porcelain, push porcelain reports abbreviated oids in the
/// summary field, so `old`/`new` are kept as strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushRefUpdate {
    /// Fast-forward update (' '), summary `old..new`.
    FastForward {
        old: String,
        new: String,
        reference: String,
    },
    /// Forced (non-fast-forward) update ('+'), summary `old...new`.
    Forced {
        old: String,
        new: String,
        reference: String,
    },
    /// Newly created ref ('*').
    New { reference: String },
    /// Deleted ref ('-').
    Deleted { reference: String },
    /// Ref already up to date ('=').
    UpToDate { reference: String },
    /// Update rejected by the remote ('!'), with an optional reason.
    Rejected {
        reference: String,
        reason: Option<String>,
    },
}

impl PushRefUpdate {
    /// The remote ref this update applies to.
    pub fn reference(&self) -> &str {
        match self {
            PushRefUpdate::FastForward { reference, .. } => reference,
            PushRefUpdate::Forced { reference, .. } => reference,
            PushRefUpdate::New { reference } => reference,
            PushRefUpdate::Deleted { reference } => reference,
            PushRefUpdate::UpToDate { reference } => reference,
            PushRefUpdate::Rejected { reference, .. } => reference,
        }
    }
}

/// Split a push porcelain summary (`old..new` or `old...new`) into its oids.
fn parse_summary_range(summary: &str) -> Option<(String, String)> {
    if let Some((old, new)) = summary.split_once("...") {
        Some((old.to_string(), new.to_string()))
    } else {
        summary
            .split_once("..")
            .map(|(old, new)| (old.to_string(), new.to_string()))
    }
}

/// Parse `git push --porcelain` output.
///
/// The output is framed by a `To <url>` line and a trailing `Done` line; in
/// between, each updated ref is one `<flag>\t<from>:<to>\t<summary> (<reason>)`
/// line.
pub fn parse_push_porcelain(output: &str) -> anyhow::Result<Vec<PushRefUpdate>> {
    let parse_error =
        |line: &str| anyhow::anyhow!("failed to parse git push --porcelain line: {:?}", line);

    let mut updates = Vec::new();
    for line in output.lines() {
        if line.is_empty() || line.starts_with("To ") || line == "Done" {
            continue;
        }

        let flag = line.chars().next().ok_or_else(|| parse_error(line))?;
        let rest = line.get(2..).ok_or_else(|| parse_error(line))?;
        let mut parts = rest.splitn(2, '\t');

        let refspec = parts.next().ok_or_else(|| parse_error(line))?;
        let summary = parts.next().ok_or_else(|| parse_error(line))?;

        // The pushed refspec source may be an oid or ref; the destination
        // (which cannot contain ':') is what we report on.
        let (_, reference) = refspec.split_once(':').ok_or_else(|| parse_error(line))?;
        let reference = reference.to_string();

        // The reason, when present, is a parenthesized suffix after the
        // summary, e.g. "[rejected] (non-fast-forward)". Bracketed summaries
        // like "[new branch]" contain spaces but no reason.
        let (summary, reason) = if summary.ends_with(')') {
            match summary.rsplit_once(" (") {
                Some((summary, reason)) => {
                    (summary, Some(reason.trim_end_matches(')').to_string()))
                }
                None => (summary, None),
            }
        } else {
            (summary, None)
        };

        let update = match flag {
            ' ' => {
                let (old, new) = parse_summary_range(summary).ok_or_else(|| parse_error(line))?;
                PushRefUpdate::FastForward {
                    old,
                    new,
                    reference,
                }
            }
            '+' => {
                let (old, new) = parse_summary_range(summary).ok_or_else(|| parse_error(line))?;
                PushRefUpdate::Forced {
                    old,
                    new,
                    reference,
                }
            }
            '*' => PushRefUpdate::New { reference },
            '-' => PushRefUpdate::Deleted { reference },
            '=' => PushRefUpdate::UpToDate { reference },
            '!' => PushRefUpdate::Rejected { reference, reason },
            _ => return Err(parse_error(line)),
        };

        updates.push(update);
    }

    Ok(updates)
}

/// Parse `git fetch --porcelain` output (one line per updated ref).
pub fn parse_fetch_porcelain(output: &str) -> anyhow::Result<Vec<RefUpdate>> {
    let parse_error = |line: &str| {
        anyhow::anyhow!(
            "failed to parse git fetch --porcelain line (git >= 2.41 required): {:?}",
            line
        )
    };

    let mut updates = Vec::new();
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        let flag = line.chars().next().ok_or_else(|| parse_error(line))?;
        let rest = line.get(2..).ok_or_else(|| parse_error(line))?;
        let mut parts = rest.splitn(3, ' ');

        let old = parts.next().ok_or_else(|| parse_error(line))?;
        let new = parts.next().ok_or_else(|| parse_error(line))?;
        let reference = parts.next().ok_or_else(|| parse_error(line))?;

        let old = gix_hash::ObjectId::from_str(old).map_err(|_| parse_error(line))?;
        let new = gix_hash::ObjectId::from_str(new).map_err(|_| parse_error(line))?;
        let reference = reference.to_string();

        let update = match flag {
            ' ' => RefUpdate::FastForward {
                old,
                new,
                reference,
            },
            '+' => RefUpdate::Forced {
                old,
                new,
                reference,
            },
            '*' => RefUpdate::New { new, reference },
            '-' => RefUpdate::Deleted { old, reference },
            '!' => {
                // Ref names cannot contain spaces; anything after the ref is
                // the rejection reason, e.g. " (non-fast-forward)".
                let (reference, reason) = match reference.split_once(' ') {
                    Some((reference, reason)) => (
                        reference.to_string(),
                        Some(
                            reason
                                .trim_start_matches('(')
                                .trim_end_matches(')')
                                .to_string(),
                        ),
                    ),
                    None => (reference, None),
                };
                RefUpdate::Rejected { reference, reason }
            }
            _ => return Err(parse_error(line)),
        };

        updates.push(update);
    }

    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO: &str = "0000000000000000000000000000000000000000";
    const OID_A: &str = "af180e6da554e60815593af48d419ac0e719c47a";
    const OID_B: &str = "2bf1cefd82c96e5d7478ff834c59194d40e539c4";

    fn oid(s: &str) -> gix_hash::ObjectId {
        gix_hash::ObjectId::from_str(s).unwrap()
    }

    #[test]
    fn parse_porcelain_fast_forward_and_new_ref() {
        let output = format!(
            "  {} {} refs/remotes/origin/main\n* {} {} refs/remotes/origin/feature\n",
            OID_A, OID_B, ZERO, OID_A
        );

        let updates = parse_fetch_porcelain(&output).unwrap();

        assert_eq!(
            updates,
            vec![
                RefUpdate::FastForward {
                    old: oid(OID_A),
                    new: oid(OID_B),
                    reference: "refs/remotes/origin/main".to_string(),
                },
                RefUpdate::New {
                    new: oid(OID_A),
                    reference: "refs/remotes/origin/feature".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_porcelain_forced_deleted_rejected() {
        let output = format!(
            "+ {} {} refs/remotes/origin/a\n- {} {} refs/remotes/origin/b\n! {} {} refs/remotes/origin/c (non-fast-forward)\n",
            OID_A, OID_B, OID_B, ZERO, OID_A, OID_B
        );

        let updates = parse_fetch_porcelain(&output).unwrap();

        assert_eq!(
            updates,
            vec![
                RefUpdate::Forced {
                    old: oid(OID_A),
                    new: oid(OID_B),
                    reference: "refs/remotes/origin/a".to_string(),
                },
                RefUpdate::Deleted {
                    old: oid(OID_B),
                    reference: "refs/remotes/origin/b".to_string(),
                },
                RefUpdate::Rejected {
                    reference: "refs/remotes/origin/c".to_string(),
                    reason: Some("non-fast-forward".to_string()),
                },
            ]
        );
    }

    #[test]
    fn parse_porcelain_empty_output() {
        assert_eq!(parse_fetch_porcelain("").unwrap(), vec![]);
        assert_eq!(parse_fetch_porcelain("\n").unwrap(), vec![]);
    }

    #[test]
    fn parse_porcelain_malformed_line_errors() {
        assert!(parse_fetch_porcelain("not a valid line").is_err());
        assert!(parse_fetch_porcelain(&format!("  {} {}\n", OID_A, OID_B)).is_err());
        assert!(
            parse_fetch_porcelain(&format!("? {} {} refs/remotes/origin/a\n", OID_A, OID_B))
                .is_err()
        );
    }

    #[test]
    fn parse_push_porcelain_all_flags() {
        let output = concat!(
            "To /tmp/remote\n",
            "*\tabcdef1:refs/heads/new\t[new branch]\n",
            " \tabcdef1:refs/heads/ff\t1234567..abcdef1\n",
            "+\tabcdef1:refs/heads/forced\t1234567...abcdef1 (forced update)\n",
            "-\t:refs/heads/gone\t[deleted]\n",
            "=\tabcdef1:refs/heads/same\t[up to date]\n",
            "!\tabcdef1:refs/heads/rejected\t[rejected] (non-fast-forward)\n",
            "Done\n",
        );

        let updates = parse_push_porcelain(output).unwrap();

        assert_eq!(
            updates,
            vec![
                PushRefUpdate::New {
                    reference: "refs/heads/new".to_string(),
                },
                PushRefUpdate::FastForward {
                    old: "1234567".to_string(),
                    new: "abcdef1".to_string(),
                    reference: "refs/heads/ff".to_string(),
                },
                PushRefUpdate::Forced {
                    old: "1234567".to_string(),
                    new: "abcdef1".to_string(),
                    reference: "refs/heads/forced".to_string(),
                },
                PushRefUpdate::Deleted {
                    reference: "refs/heads/gone".to_string(),
                },
                PushRefUpdate::UpToDate {
                    reference: "refs/heads/same".to_string(),
                },
                PushRefUpdate::Rejected {
                    reference: "refs/heads/rejected".to_string(),
                    reason: Some("non-fast-forward".to_string()),
                },
            ]
        );
    }

    #[test]
    fn parse_push_porcelain_empty_output() {
        assert_eq!(parse_push_porcelain("").unwrap(), vec![]);
        assert_eq!(
            parse_push_porcelain("To /tmp/remote\nDone\n").unwrap(),
            vec![]
        );
    }

    #[test]
    fn parse_push_porcelain_malformed_line_errors() {
        assert!(parse_push_porcelain("not a valid line").is_err());
        assert!(parse_push_porcelain("*\tno-colon-here\t[new branch]\n").is_err());
        assert!(parse_push_porcelain(" \tabcdef1:refs/heads/a\tnot-a-range\n").is_err());
        assert!(parse_push_porcelain("?\tabcdef1:refs/heads/a\t[unknown]\n").is_err());
    }
}
