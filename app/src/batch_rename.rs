//! Batch name planning and guarded, recoverable same-directory renames.
use crate::operation_journal::{Context, Fingerprint, MovePlan};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub enum Mode {
    Replace {
        find: String,
        replacement: String,
    },
    Number {
        base: String,
        start: u64,
    },
    /// One explicit name per path, in order (Suggest Names).
    Names(Vec<String>),
}
#[derive(Clone, Debug)]
pub struct Proposal {
    pub source: PathBuf,
    pub name: String,
    pub error: Option<String>,
    pub changed: bool,
}
fn same(a: &Path, b: &Path) -> bool {
    seek_engine::paths::cmp_ci(
        &seek_engine::paths::internal(a),
        &seek_engine::paths::internal(b),
    )
    .is_eq()
}
pub fn preview(paths: &[PathBuf], mode: &Mode) -> Vec<Proposal> {
    let mut rows = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let original = path.file_name().and_then(|name| name.to_str());
            let name = match (original, mode) {
                (Some(original), Mode::Replace { find, replacement }) if !find.is_empty() => {
                    original.replace(find, replacement)
                }
                (Some(original), Mode::Names(names)) => names
                    .get(index)
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| original.into()),
                (Some(_), Mode::Number { base, start }) => start
                    .checked_add(index as u64)
                    .map(|number| {
                        let extension = if path.is_dir() {
                            None
                        } else {
                            path.extension().and_then(|ext| ext.to_str())
                        };
                        format!(
                            "{base} {number:03}{}",
                            extension.map(|ext| format!(".{ext}")).unwrap_or_default()
                        )
                    })
                    .unwrap_or_default(),
                _ => original.unwrap_or_default().into(),
            };
            let error = if original.is_none() {
                Some("This name cannot be edited as text".into())
            } else if !crate::file_ops::valid_name(&name) {
                Some("Invalid Windows name or number overflow".into())
            } else {
                None
            };
            Proposal {
                source: path.clone(),
                changed: original != Some(name.as_str()),
                name,
                error,
            }
        })
        .collect::<Vec<_>>();
    for index in 0..rows.len() {
        let target = rows[index].source.with_file_name(&rows[index].name);
        if rows.iter().enumerate().any(|(other, row)| {
            other != index && same(&row.source.with_file_name(&row.name), &target)
        }) {
            rows[index].error = Some("More than one item would have this name".into());
        }
    }
    // A conflicted/skipped source cannot vacate a destination for another row.
    // Propagate that dependency until all remaining rows can move as a set.
    loop {
        let mut blocked = vec![];
        for (index, row) in rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.error.is_none() && row.changed)
        {
            let target = row.source.with_file_name(&row.name);
            match std::fs::symlink_metadata(&target) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_)
                    if rows.iter().any(|source| {
                        source.error.is_none() && source.changed && same(&source.source, &target)
                    }) => {}
                _ => blocked.push(index),
            }
        }
        if blocked.is_empty() {
            break;
        }
        for index in blocked {
            rows[index].error = Some("The destination exists or cannot be checked".into());
        }
    }
    rows
}
fn relocate(
    location: &mut PathBuf,
    target: &Path,
    context: &Context,
    expected: &Fingerprint,
) -> windows::core::Result<()> {
    let result = crate::safe_rename::move_guarded(location, target, context, expected);
    // Mutation can succeed before recording its outcome fails. Track the live
    // object so recovery never claims that an unrecorded rename did not occur.
    if result.is_ok()
        || Fingerprint::at(target)
            .is_some_and(|current| current.volume == expected.volume && current.id == expected.id)
    {
        *location = target.into();
    }
    result
}
pub fn execute(plans: Vec<MovePlan>, context: &Context) -> windows::core::Result<()> {
    execute_with(plans, context, |_| crate::operation_control::check())
}
fn execute_with(
    mut plans: Vec<MovePlan>,
    context: &Context,
    mut checkpoint: impl FnMut(usize) -> Result<(), String>,
) -> windows::core::Result<()> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    let error =
        |message: String| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, message);
    if plans.is_empty() {
        return Err(error("No names would change".into()));
    }
    plans.retain(|plan| plan.path != plan.target);
    let mut held = vec![];
    let mut keys = std::collections::HashSet::new();
    for (index, plan) in plans.iter().enumerate() {
        checkpoint(index).map_err(error)?;
        if plan.path.parent() != plan.target.parent()
            || !plan
                .target
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(crate::file_ops::valid_name)
        {
            return Err(error(
                "Batch rename requires valid names in each item's existing folder".into(),
            ));
        }
        if plans.iter().enumerate().any(|(other, row)| {
            other != index && (same(&row.path, &plan.path) || same(&row.target, &plan.target))
        }) {
            return Err(error(
                "Duplicate sources or destinations; no items were renamed".into(),
            ));
        }
        held.extend(crate::safe_rename::stable_parent(
            plan.path
                .parent()
                .ok_or_else(|| error("An item has no parent".into()))?,
        )?);
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1 | 4)
            .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
            .open(&plan.path)
            .map_err(|e| error(e.to_string()))?;
        let current = Fingerprint::read(&file).map_err(error)?;
        if !current.compatible(&plan.expected) || !keys.insert((current.volume, current.id)) {
            return Err(error(
                "An item changed or refers to the same file object twice; no items were renamed"
                    .into(),
            ));
        }
        match std::fs::symlink_metadata(&plan.target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) if plans.iter().any(|source| same(&source.path, &plan.target)) => {}
            _ => {
                return Err(error(
                    "A destination exists or cannot be checked; no items were renamed".into(),
                ));
            }
        }
        held.push(file);
    }
    let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }?;
    let stages = plans
        .iter()
        .enumerate()
        .map(|(index, plan)| {
            plan.path
                .with_file_name(format!(".seek-rename-{id:?}-{index}"))
        })
        .collect::<Vec<_>>();
    let mut locations = plans
        .iter()
        .map(|plan| plan.path.clone())
        .collect::<Vec<_>>();
    let result: windows::core::Result<()> = (|| {
        for (index, plan) in plans.iter().enumerate() {
            checkpoint(index).map_err(error)?;
            relocate(
                &mut locations[index],
                &stages[index],
                context,
                &plan.expected,
            )?;
        }
        for (index, plan) in plans.iter().enumerate() {
            checkpoint(plans.len() + index).map_err(error)?;
            relocate(&mut locations[index], &plan.target, context, &plan.expected)?;
        }
        Ok(())
    })();
    if let Err(failure) = result {
        // Cancellation stops forward work, not the best-effort restoration of
        // our own stages. Every rollback still validates identity/no-replace.
        let _rollback_scope = crate::operation_control::enter(Default::default());
        let mut failures = vec![];
        for (index, plan) in plans.iter().enumerate() {
            if locations[index] != stages[index] && locations[index] != plan.path {
                match relocate(
                    &mut locations[index],
                    &stages[index],
                    context,
                    &plan.expected,
                ) {
                    Ok(()) => locations[index] = stages[index].clone(),
                    Err(e) => failures.push(format!("{}: {e}", locations[index].display())),
                }
            }
        }
        for (index, plan) in plans.iter().enumerate() {
            if locations[index] == stages[index] {
                match relocate(&mut locations[index], &plan.path, context, &plan.expected) {
                    Ok(()) => locations[index] = plan.path.clone(),
                    Err(e) => failures.push(format!("{}: {e}", locations[index].display())),
                }
            }
        }
        return Err(error(if failures.is_empty() {
            format!("{failure}. Completed renames were returned to their original names.")
        } else {
            format!(
                "{failure}. Some names could not be restored. Review history and these paths: {}",
                failures.join("; ")
            )
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation_journal::{Event, Journal};
    #[test]
    fn explicit_names_are_checked_like_any_batch_rename() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for name in ["scan0001.pdf", "scan0002.pdf", "untitled.md", "taken.md"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let paths: Vec<PathBuf> = ["scan0001.pdf", "scan0002.pdf", "untitled.md"]
            .iter()
            .map(|name| root.join(name))
            .collect();
        let rows = preview(
            &paths,
            &Mode::Names(vec![
                "Invoice.pdf".into(),
                "invoice.PDF".into(),
                "taken.md".into(),
            ]),
        );
        // Two suggestions for one name conflict, case-insensitively.
        assert!(rows[0].error.is_some() && rows[1].error.is_some());
        // An existing file is never a destination.
        assert!(rows[2].error.is_some());
        // A blank field keeps the old name and changes nothing.
        let rows = preview(&paths[..1], &Mode::Names(vec!["  ".into()]));
        assert!(!rows[0].changed && rows[0].error.is_none());
        let rows = preview(&paths[..1], &Mode::Names(vec!["bad:name.pdf".into()]));
        assert!(rows[0].error.is_some());
    }
    fn context(path: &Path) -> Context {
        let mut journal = Journal::open(path).unwrap();
        journal
            .append(Event::Begin {
                job: 1,
                label: "Batch rename".into(),
            })
            .unwrap();
        Context {
            journal: std::sync::Arc::new(std::sync::Mutex::new(journal)),
            job: 1,
        }
    }
    fn plan(path: &Path, target: &Path) -> MovePlan {
        MovePlan {
            path: path.into(),
            target: target.into(),
            expected: Fingerprint::at(path).unwrap(),
        }
    }
    #[test]
    fn preview_preserves_extensions_and_propagates_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let aa = temp.path().join("aa.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&aa, b"aa").unwrap();
        // a -> aa is blocked when aa -> aaaa collides with an unselected file.
        std::fs::write(temp.path().join("aaaa.txt"), b"occupied").unwrap();
        let rows = preview(
            &[a.clone(), aa.clone()],
            &Mode::Replace {
                find: "a".into(),
                replacement: "aa".into(),
            },
        );
        assert!(rows.iter().all(|row| row.error.is_some()));
        let rows = preview(
            &[a.clone(), aa],
            &Mode::Number {
                base: "Photo".into(),
                start: 9,
            },
        );
        assert_eq!(rows[0].name, "Photo 009.txt");
        assert_eq!(rows[1].name, "Photo 010.txt");
        assert!(rows.iter().all(|row| row.error.is_none()));
        std::fs::write(temp.path().join("Photo 009.txt"), b"existing").unwrap();
        assert!(
            preview(
                &[a],
                &Mode::Number {
                    base: "Photo".into(),
                    start: 9
                }
            )[0]
            .error
            .is_some()
        );
    }
    #[test]
    fn cycles_and_case_only_changes_are_no_replace_and_reversible() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let context = context(&temp.path().join("history.jsonl"));
        execute(vec![plan(&a, &b), plan(&b, &a)], &context).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"b");
        assert_eq!(std::fs::read(&b).unwrap(), b"a");
        context
            .append(Event::End {
                job: 1,
                error: None,
            })
            .unwrap();
        let inverse = context
            .journal
            .lock()
            .unwrap()
            .snapshot
            .batch_rename_plan(false, None)
            .unwrap();
        assert_eq!(inverse.len(), 2);
        let next = context.journal.lock().unwrap().next_job().unwrap();
        context
            .journal
            .lock()
            .unwrap()
            .append(Event::Begin {
                job: next,
                label: "Undo batch rename".into(),
            })
            .unwrap();
        let inverse_context = Context {
            journal: context.journal.clone(),
            job: next,
        };
        execute(inverse, &inverse_context).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        inverse_context
            .append(Event::End {
                job: next,
                error: None,
            })
            .unwrap();
        assert!(
            context
                .journal
                .lock()
                .unwrap()
                .snapshot
                .batch_rename_plan(true, None)
                .is_some()
        );
        let context = self::context(&temp.path().join("case-history.jsonl"));
        execute(vec![plan(&a, &temp.path().join("A.txt"))], &context).unwrap();
        assert!(
            std::fs::read_dir(temp.path())
                .unwrap()
                .any(|entry| entry.unwrap().file_name() == "A.txt")
        );
    }
    #[test]
    fn partial_finalization_failure_rolls_back_without_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let context = context(&temp.path().join("history.jsonl"));
        let mut calls = 0;
        let result = execute_with(vec![plan(&a, &b), plan(&b, &a)], &context, |_| {
            calls += 1;
            if calls == 6 {
                Err("injected finalization failure".into())
            } else {
                Ok(())
            }
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("returned to their original names")
        );
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert!(!std::fs::read_dir(temp.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".seek-rename-")
        }));
    }
    #[test]
    fn cancellation_after_staging_restores_original_names() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let context = context(&temp.path().join("history.jsonl"));
        let token: crate::operation_control::Token = Default::default();
        let _scope = crate::operation_control::enter(token.clone());
        let mut calls = 0;
        let result = execute_with(vec![plan(&a, &b), plan(&b, &a)], &context, |_| {
            calls += 1;
            if calls == 5 {
                token.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            crate::operation_control::check()
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert!(crate::operation_control::cancelled());
    }
    #[test]
    fn rollback_collision_preserves_external_file_and_reports_remaining_stage() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let context = context(&temp.path().join("history.jsonl"));
        let mut calls = 0;
        let result = execute_with(vec![plan(&a, &b), plan(&b, &a)], &context, |_| {
            calls += 1;
            if calls == 6 {
                std::fs::write(&a, b"external").unwrap();
                Err("injected collision".into())
            } else {
                Ok(())
            }
        });
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Some names could not be restored"));
        assert!(error.contains(".seek-rename-"));
        assert_eq!(std::fs::read(&a).unwrap(), b"external");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        let stages = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".seek-rename-")
            })
            .collect::<Vec<_>>();
        assert_eq!(stages.len(), 1);
        assert_eq!(std::fs::read(stages[0].path()).unwrap(), b"a");
    }
}
