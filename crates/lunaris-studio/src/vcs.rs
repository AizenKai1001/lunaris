//! Source control in the Project panel: what git says about the files of the workspace, so that a tile can carry the badge Unreal's source control gives an asset (changed, new, not tracked, in conflict) and the panel can say which branch
//! the project is on. Git runs on another thread every few seconds (a big workspace takes a while to scan), so the panel never waits for it; a workspace that is not in a repository, or a machine without git, simply shows nothing.
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Status {
    /// Not in the repository yet.
    Untracked,
    /// Staged as a new file.
    Added,
    /// Changed since the last commit (staged or not).
    Modified,
    /// Both sides changed it and the merge is not finished.
    Conflict,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Untracked => "not tracked by git",
            Status::Added => "added to git",
            Status::Modified => "changed since the last commit",
            Status::Conflict => "in conflict",
        }
    }
}

/// What a scan of the repository found, with the paths relative to the workspace folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub branch: String,
    pub files: HashMap<String, Status>,
}

impl Snapshot {
    /// The strongest status among the files in `folder` (a path in the workspace; empty: the whole workspace), for the badge of a folder.
    pub fn of_folder(&self, folder: &str) -> Option<Status> {
        let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        self.files.iter().filter(|(p, _)| p.starts_with(&prefix)).map(|(_, s)| *s).max()
    }

    /// "main, 3 changed" for the foot of the panel.
    pub fn summary(&self) -> String {
        let n = self.files.len();
        match (self.branch.is_empty(), n) {
            (true, 0) => String::new(),
            (true, _) => format!("{n} changed"),
            (false, 0) => self.branch.clone(),
            (false, _) => format!("{}, {n} changed", self.branch),
        }
    }
}

/// The files in the output of `git status --porcelain=v1 -z`, with `prefix` (the workspace folder's place in the repository, "" or "dir/sub/") taken off and what is outside the workspace left out.
pub fn parse_porcelain(text: &str, prefix: &str) -> HashMap<String, Status> {
    let mut out = HashMap::new();
    let mut entries = text.split('\0');
    while let Some(e) = entries.next() {
        let mut chars = e.chars();
        let (Some(x), Some(y), Some(' ')) = (chars.next(), chars.next(), chars.next()) else { continue };
        let path = &e[3..];
        if matches!(x, 'R' | 'C') {
            // a rename or a copy names where it came from in the entry after it
            entries.next();
        }
        let status = match (x, y) {
            ('?', '?') => Status::Untracked,
            ('!', '!') => continue,
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => Status::Conflict,
            ('A', _) => Status::Added,
            _ => Status::Modified,
        };
        if let Some(rel) = path.strip_prefix(prefix) {
            out.insert(rel.to_string(), status);
        }
    }
    out
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(root).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Ask git about the workspace at `root`: None when it is not inside a repository (or git cannot be run).
pub fn query(root: &Path) -> Option<Snapshot> {
    let prefix = git(root, &["rev-parse", "--show-prefix"])?.trim().to_string();
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| b.trim().to_string()).filter(|b| b != "HEAD").unwrap_or_default();
    let status = git(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    Some(Snapshot { branch, files: parse_porcelain(&status, &prefix) })
}

/// The latest scan, and the scan under way.
#[derive(Default)]
pub struct Vcs {
    pub snapshot: Option<Snapshot>,
    asked_at: Option<Instant>,
    pending: Option<Receiver<Option<Snapshot>>>,
    /// The last scan found no repository: look again only now and then.
    absent: bool,
}

impl Vcs {
    /// Take in a finished scan and start another when the last one is old enough (a few seconds; half a minute when there was no repository).
    pub fn poll(&mut self, root: &Path) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(s) => {
                    self.absent = s.is_none();
                    self.snapshot = s;
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => self.pending = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let every = Duration::from_secs(if self.absent { 30 } else { 4 });
        if self.pending.is_none() && self.asked_at.is_none_or(|t| t.elapsed() >= every) {
            self.asked_at = Some(Instant::now());
            let (tx, rx) = channel();
            let root = root.to_path_buf();
            std::thread::spawn(move || {
                let _ = tx.send(query(&root));
            });
            self.pending = Some(rx);
        }
    }

    /// Scan again at the next frame (a file was saved).
    pub fn invalidate(&mut self) {
        self.asked_at = None;
    }

    pub fn status_of(&self, path: &str) -> Option<Status> {
        self.snapshot.as_ref()?.files.get(path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_gives_a_status_for_each_file_inside_the_workspace_and_skips_the_origin_of_a_rename() {
        let text = "?? new.txt\0 M scenes/a.json\0A  scenes/b.json\0R  scenes/c.json\0scenes/old.json\0UU scenes/d.json\0!! junk\0MM other/e.txt\0";
        let all = parse_porcelain(text, "");
        assert_eq!(all["new.txt"], Status::Untracked);
        assert_eq!(all["scenes/a.json"], Status::Modified);
        assert_eq!(all["scenes/b.json"], Status::Added);
        assert_eq!(all["scenes/c.json"], Status::Modified, "a rename is a change of the new name");
        assert!(!all.contains_key("scenes/old.json") && !all.contains_key("junk"));
        assert_eq!(all["scenes/d.json"], Status::Conflict);
        // the workspace is a folder of the repository: its files lose the prefix and the rest is not its business
        let inside = parse_porcelain(text, "scenes/");
        assert_eq!(inside.len(), 4);
        assert_eq!(inside["a.json"], Status::Modified);
        let snap = Snapshot { branch: "main".to_string(), files: all };
        assert_eq!(snap.of_folder("scenes"), Some(Status::Conflict), "the strongest of what is inside");
        assert_eq!(snap.of_folder("other"), Some(Status::Modified));
        assert_eq!(snap.of_folder("nothing"), None);
        assert_eq!(snap.of_folder(""), Some(Status::Conflict));
        assert_eq!(snap.summary(), "main, 6 changed");
        assert_eq!(Snapshot::default().summary(), "");
    }

    #[test]
    fn a_real_repository_is_scanned_and_a_folder_that_is_not_one_is_not() {
        let dir = std::env::temp_dir().join(format!("lunaris-vcs-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        if git(&dir, &["--version"]).is_none() {
            return; // no git on this machine
        }
        assert!(query(&dir).is_none(), "a scratch folder is not in a repository");
        let run = |args: &[&str]| assert!(git(&dir, args).is_some(), "git {args:?}");
        run(&["init", "-q"]);
        std::fs::write(dir.join("sub/a.txt"), "one").unwrap();
        run(&["add", "."]);
        run(&["-c", "user.name=t", "-c", "user.email=t@users.noreply.github.com", "commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("sub/a.txt"), "two").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "new").unwrap();
        let snap = query(&dir).expect("a repository");
        assert_eq!(snap.files.get("sub/a.txt"), Some(&Status::Modified), "{snap:?}");
        assert_eq!(snap.files.get("sub/b.txt"), Some(&Status::Untracked));
        // the same repository seen from a folder inside it: the paths are relative to that folder
        let inner = query(&dir.join("sub")).expect("inside the repository");
        assert_eq!(inner.files.get("a.txt"), Some(&Status::Modified), "{inner:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
