//! Parsing of `git fetch --porcelain` output (requires git >= 2.41).
//!
//! Each updated ref is reported as one `<flag> <old-oid> <new-oid> <local-ref>`
//! line; rejected updates may carry a `(reason)` suffix after the ref name.

/// One ref update reported by `git fetch --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefUpdate {
    /// Fast-forward update (' ').
    FastForward {
        old: git2::Oid,
        new: git2::Oid,
        reference: String,
    },
    /// Forced (non-fast-forward) update ('+').
    Forced {
        old: git2::Oid,
        new: git2::Oid,
        reference: String,
    },
    /// Newly created ref ('*').
    New { new: git2::Oid, reference: String },
    /// Deleted ref ('-').
    Deleted { old: git2::Oid, reference: String },
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

        let old = git2::Oid::from_str(old).map_err(|_| parse_error(line))?;
        let new = git2::Oid::from_str(new).map_err(|_| parse_error(line))?;
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

    fn oid(s: &str) -> git2::Oid {
        git2::Oid::from_str(s).unwrap()
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
}
