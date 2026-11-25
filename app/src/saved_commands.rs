use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Selection {
    None,
    #[default]
    One,
    Many,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCommand {
    pub id: u64,
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub selection: Selection,
}
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
}
impl SavedCommand {
    pub fn validate(&self) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        if self.program.as_os_str().encode_wide().any(|unit| unit == 0)
            || self
                .cwd
                .as_ref()
                .is_some_and(|path| path.as_os_str().encode_wide().any(|unit| unit == 0))
        {
            return Err(
                "Program and working-directory paths cannot contain NUL characters.".into(),
            );
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            return Err("Choose a command name of 1–80 characters.".into());
        }
        if !self.program.is_absolute()
            || !self
                .program
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            return Err("Choose the full path to an .exe program.".into());
        }
        if self.cwd.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err("Working directory must be an absolute folder path.".into());
        }
        if self.args.len() > 128
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 8192 || arg.contains('\0'))
        {
            return Err(
                "Use at most 128 arguments, each up to 8,192 bytes, without NUL characters.".into(),
            );
        }
        // Validate grammar without inventing a file or executing anything.
        for arg in &self.args {
            tokens(arg)?;
        }
        Ok(())
    }
    pub fn expand(&self, paths: &[PathBuf], scope: Option<&Path>) -> Result<Invocation, String> {
        self.validate()?;
        match self.selection {
            Selection::One if paths.len() != 1 => {
                return Err("Select exactly one item for this command.".into());
            }
            Selection::Many if paths.is_empty() => {
                return Err("Select one or more items for this command.".into());
            }
            _ => {}
        }
        let paths = if self.selection == Selection::None {
            &[][..]
        } else {
            paths
        };
        let folder = scope.or_else(|| {
            paths
                .first()
                .and_then(|first| first.parent())
                .filter(|parent| paths.iter().all(|path| path.parent() == Some(*parent)))
        });
        let file = || -> Result<&Path, String> {
            if paths.len() != 1 {
                return Err("Use {files} for multiple items, or select exactly one item.".into());
            }
            Ok(&paths[0])
        };
        let value = |key: &str| -> Result<OsString, String> {
            Ok(match key {
                "file" => file()?.as_os_str().to_owned(),
                "name" => file()?
                    .file_name()
                    .ok_or("The selected item has no filename.")?
                    .to_owned(),
                "folder" => folder
                    .ok_or("Open a folder or select items with one common parent.")?
                    .as_os_str()
                    .to_owned(),
                _ => return Err("Unknown command placeholder.".into()),
            })
        };
        let mut args = vec![];
        for arg in &self.args {
            if arg == "{files}" {
                if paths.is_empty() {
                    return Err("Select items for {files}.".into());
                }
                args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
            } else {
                let parts = tokens(arg)?;
                if let [Part::Key(key)] = parts.as_slice() {
                    args.push(value(key)?);
                    continue;
                }
                let mut expanded = String::new();
                for part in parts {
                    match part {
                        Part::Literal(text) => expanded.push_str(&text),
                        Part::Key(key) => expanded.push_str(value(&key)?.to_str().ok_or("This filename must be passed as a whole argument to preserve its characters.")?),
                    }
                }
                args.push(expanded.into());
            }
        }
        use std::os::windows::ffi::OsStrExt;
        let units = self.program.as_os_str().encode_wide().count()
            + args
                .iter()
                .map(|arg| 2 * arg.encode_wide().count() + 3)
                .sum::<usize>();
        if units > 30_000 {
            return Err("The expanded command is too long. Select fewer items.".into());
        }
        Ok(Invocation {
            program: self.program.clone(),
            args,
            cwd: self.cwd.clone().or_else(|| folder.map(Path::to_path_buf)),
        })
    }
}
enum Part {
    Literal(String),
    Key(String),
}
fn tokens(text: &str) -> Result<Vec<Part>, String> {
    if text == "{files}" {
        return Ok(vec![]);
    }
    let mut parts = vec![];
    let mut literal = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("{{") {
            literal.push('{');
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("}}") {
            literal.push('}');
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix('{') {
            let (key, tail) = tail
                .split_once('}')
                .ok_or("A command placeholder is missing its closing brace.")?;
            if !["file", "name", "folder"].contains(&key) {
                return Err(if key == "files" {
                    "Put {files} on its own argument line."
                } else {
                    "Use {file}, {files}, {name} or {folder}; double braces produce literal braces."
                }
                .into());
            }
            if !literal.is_empty() {
                parts.push(Part::Literal(std::mem::take(&mut literal)));
            }
            parts.push(Part::Key(key.into()));
            rest = tail;
        } else {
            let character = rest.chars().next().unwrap();
            if character == '}' {
                return Err("Double a literal closing brace: }}.".into());
            }
            literal.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(parts)
}
#[derive(Clone, Debug)]
pub struct RunOutput {
    pub code: Option<i32>,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}
fn capture(mut input: impl std::io::Read) -> std::io::Result<(String, bool)> {
    const LIMIT: usize = 32 * 1024;
    let mut saved = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let keep = read.min(LIMIT - saved.len());
        saved.extend_from_slice(&buffer[..keep]);
        truncated |= keep != read;
        // Drain excess bytes too, so a full pipe never blocks the program.
    }
    Ok((String::from_utf8_lossy(&saved).into_owned(), truncated))
}
pub fn run(invocation: Invocation, started: impl FnOnce(u32)) -> Result<RunOutput, String> {
    use std::os::windows::process::CommandExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    ACTIVE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
            (count < 4).then_some(count + 1)
        })
        .map_err(|_| {
            "Four saved programs are still running. Close one before starting another.".to_owned()
        })?;
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            ACTIVE.fetch_sub(1, Ordering::Relaxed);
        }
    }
    let _permit = Permit;
    let mut command = std::process::Command::new(invocation.program);
    command
        .args(invocation.args)
        .creation_flags(0x08000000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(folder) = invocation.cwd {
        command.current_dir(folder);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("The program could not be launched: {error}"))?;
    started(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or("Program output could not be captured")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Program messages could not be captured")?;
    std::thread::scope(|scope| {
        let stdout = std::thread::Builder::new()
            .name("seek-command-output".into())
            .spawn_scoped(scope, || capture(stdout))
            .map_err(|e| e.to_string())?;
        let stderr = std::thread::Builder::new()
            .name("seek-command-messages".into())
            .spawn_scoped(scope, || capture(stderr))
            .map_err(|e| e.to_string())?;
        let status = child.wait().map_err(|e| e.to_string())?;
        let (stdout, out_short) = stdout
            .join()
            .map_err(|_| "Output reader stopped")?
            .map_err(|e| e.to_string())?;
        let (stderr, err_short) = stderr
            .join()
            .map_err(|_| "Message reader stopped")?
            .map_err(|e| e.to_string())?;
        Ok(RunOutput {
            code: status.code(),
            success: status.success(),
            stdout,
            stderr,
            truncated: out_short || err_short,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_runs_return_real_output_and_bounded_readers_keep_draining() {
        let program =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/whoami.exe");
        let started = std::cell::Cell::new(false);
        let output = run(
            Invocation {
                program,
                args: vec![],
                cwd: None,
            },
            |_| started.set(true),
        )
        .unwrap();
        assert!(started.get() && output.success);
        assert_eq!(output.code, Some(0));
        assert!(!output.stdout.trim().is_empty());
        let (text, truncated) = capture(std::io::Cursor::new(vec![b'x'; 100_000])).unwrap();
        assert_eq!(text.len(), 32 * 1024);
        assert!(truncated);
    }
    fn command(args: &[&str], selection: Selection) -> SavedCommand {
        SavedCommand {
            id: 1,
            name: "Owned command".into(),
            program: PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/findstr.exe"),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            cwd: None,
            selection,
        }
    }
    #[test]
    fn expansions_preserve_argument_boundaries_and_selection_rules() {
        let files = vec![
            PathBuf::from(r"C:\Owned space\café & one.txt"),
            PathBuf::from(r"C:\Owned space\two.txt"),
        ];
        let result = command(
            &["--literal=$value & stop", "{files}", "{{file}}"],
            Selection::Many,
        )
        .expand(&files, None)
        .unwrap();
        assert_eq!(
            result.args,
            vec![
                OsString::from("--literal=$value & stop"),
                files[0].as_os_str().into(),
                files[1].as_os_str().into(),
                OsString::from("{file}")
            ]
        );
        assert!(
            command(&["{file}"], Selection::Many)
                .expand(&files, None)
                .is_err()
        );
        assert!(
            command(&["{files}"], Selection::None)
                .expand(&files, None)
                .is_err()
        );
        assert!(
            command(&["--input={files}"], Selection::Many)
                .validate()
                .is_err()
        );
        assert!(command(&["{unknown}"], Selection::One).validate().is_err());
        assert!(
            command(&["{file}"], Selection::One)
                .expand(&[], None)
                .is_err()
        );
        let mut shell = command(&[], Selection::None);
        shell.program.set_extension("cmd");
        assert!(shell.validate().is_err());
    }
    #[test]
    fn standalone_paths_are_lossless_and_configuration_round_trips() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let raw = OsString::from_wide(&[
            67, 58, 92, 79, 119, 110, 101, 100, 92, 0xd800, 46, 116, 120, 116,
        ]);
        let path = PathBuf::from(raw.clone());
        let saved = command(&["{file}"], Selection::One);
        let expanded = saved.expand(std::slice::from_ref(&path), None).unwrap();
        assert_eq!(
            expanded.args[0].encode_wide().collect::<Vec<_>>(),
            raw.encode_wide().collect::<Vec<_>>()
        );
        assert!(
            command(&["--path={file}"], Selection::One)
                .expand(&[path], None)
                .is_err()
        );
        let temp = tempfile::tempdir().unwrap();
        let settings = crate::settings::Settings {
            saved_commands: vec![saved.clone()],
            ..Default::default()
        };
        crate::settings::write(temp.path(), &settings).unwrap();
        assert_eq!(
            crate::settings::read(temp.path()).saved_commands,
            vec![saved]
        );
    }
    #[test]
    fn native_argv_probe() {
        let Some(report) = std::env::var_os("SEEK_OWNED_ARGV_REPORT") else {
            return;
        };
        let args = std::env::args_os()
            .skip_while(|arg| arg != "--")
            .skip(1)
            .collect::<Vec<_>>();
        assert_eq!(std::fs::read(&args[0]).unwrap(), b"literal & marker\n");
        let text = args
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect::<Vec<_>>();
        std::fs::write(report, serde_json::to_vec(&text).unwrap()).unwrap();
    }
    #[test]
    fn native_executable_reads_literal_metacharacters_and_unicode_paths() {
        use std::os::windows::process::CommandExt;
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("caf\u{e9} & spaces.txt");
        let report = temp.path().join("argv.json");
        std::fs::write(&file, "literal & marker\n").unwrap();
        let mut command = command(
            &[
                "--exact",
                "saved_commands::tests::native_argv_probe",
                "--nocapture",
                "--",
                "{file}",
                "literal & marker",
            ],
            Selection::One,
        );
        command.program = std::env::current_exe().unwrap();
        let plan = command
            .expand(std::slice::from_ref(&file), Some(temp.path()))
            .unwrap();
        let output = std::process::Command::new(plan.program)
            .args(plan.args)
            .current_dir(plan.cwd.unwrap())
            .env("SEEK_OWNED_ARGV_REPORT", &report)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let received: Vec<String> =
            serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
        assert_eq!(received, vec![file.to_str().unwrap(), "literal & marker"]);
    }
}
