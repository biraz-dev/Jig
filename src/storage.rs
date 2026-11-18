use jig::model::State;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// A per-file lock prevents lost updates between CLI and dashboard sessions.
/// An interrupted process can leave a lock: remove it only after checking no Jig is running.
pub struct Store {
    pub path: PathBuf,
    lock: PathBuf,
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

impl Store {
    pub fn open(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let path = parent
            .canonicalize()?
            .join(path.file_name().ok_or("State path needs a filename")?);
        // Existing aliases must share a lock and keep atomic saves on the target.
        // A dangling symlink fails canonicalization rather than being replaced.
        let path = match fs::symlink_metadata(&path) {
            Ok(_) => path.canonicalize()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => path,
            Err(error) => return Err(error.into()),
        };
        let lock = append_suffix(&path, ".lock");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&lock).map_err(|e| format!("Cannot lock {}: {e}. If a previous process crashed, check it has stopped before removing the lock.", lock.display()))?;
        if let Err(e) = writeln!(file, "{}", std::process::id()) {
            let _ = fs::remove_file(&lock);
            return Err(e.into());
        }
        Ok(Self { path, lock })
    }
    pub fn load(&self) -> Result<State, Box<dyn std::error::Error>> {
        let state: State = match fs::read_to_string(&self.path) {
            Ok(text) => State::from_json(&text)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(state)
    }
    pub fn save(&self, state: &State) -> Result<(), Box<dyn std::error::Error>> {
        state.validate()?;
        let temporary = append_suffix(&self.path, ".tmp");
        // Never remove an existing temporary file: it may contain recoverable data.
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        serde_json::to_writer_pretty(&mut file, state)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, &self.path)?;
        File::open(self.path.parent().ok_or("Missing parent")?)?.sync_all()?;
        Ok(())
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_reload_lock_and_malformed_file() {
        let dir = std::env::temp_dir().join(format!("jig-storage-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("jig.json");
        {
            let store = Store::open(path.clone()).unwrap();
            assert!(Store::open(path.clone()).is_err());
            let mut state = store.load().unwrap();
            state.add_task("Original").unwrap();
            store.save(&state).unwrap();
            state.add_task("Second").unwrap();
            store.save(&state).unwrap();
            assert_eq!(store.load().unwrap().tasks.len(), 2);
            // A failed temporary write cannot truncate the saved state.
            fs::write(append_suffix(&path, ".tmp"), "recover me").unwrap();
            assert!(store.save(&State::default()).is_err());
            assert_eq!(store.load().unwrap().tasks.len(), 2);
            fs::remove_file(append_suffix(&path, ".tmp")).unwrap();
        }
        let store = Store::open(path.clone()).unwrap();
        assert_eq!(store.load().unwrap().tasks.len(), 2);
        fs::write(&path, "broken state").unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken state");
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn same_stem_state_files_have_independent_locks_and_temporary_files() {
        let dir = std::env::temp_dir().join(format!("jig-sidecars-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let first = Store::open(dir.join("work.json")).unwrap();
        let second = Store::open(dir.join("work.toml")).unwrap();
        let third = Store::open(dir.join("work")).unwrap();
        let mut state = State::default();
        state.add_task("Independent file").unwrap();
        // A recoverable temporary file for one state must not block another.
        fs::write(append_suffix(&first.path, ".tmp"), "recover me").unwrap();
        assert!(first.save(&state).is_err());
        second.save(&state).unwrap();
        third.save(&state).unwrap();
        assert_eq!(second.load().unwrap().tasks[0].text, "Independent file");
        assert_eq!(third.load().unwrap().tasks.len(), 1);
        assert!(first.load().unwrap().tasks.is_empty());
        assert!(Store::open(first.path.clone()).is_err());
        fs::remove_file(append_suffix(&first.path, ".tmp")).unwrap();
        first.save(&state).unwrap();
        drop((first, second, third));
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_state_uses_the_target_lock_and_preserves_the_alias_on_save() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!("jig-symlink-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("state.json");
        let alias = dir.join("alias.json");
        {
            let store = Store::open(target.clone()).unwrap();
            let mut state = State::default();
            state.add_task("Original").unwrap();
            store.save(&state).unwrap();
            symlink("state.json", &alias).unwrap();
            assert!(Store::open(alias.clone()).is_err());
        }
        {
            let store = Store::open(alias.clone()).unwrap();
            assert!(Store::open(target.clone()).is_err());
            let mut state = store.load().unwrap();
            state.add_task("Through the alias").unwrap();
            store.save(&state).unwrap();
            assert_eq!(fs::read_link(&alias).unwrap(), Path::new("state.json"));
            let saved = State::from_json(&fs::read_to_string(&target).unwrap()).unwrap();
            assert_eq!(saved.tasks.len(), 2);
            assert_eq!(saved.tasks[1].text, "Through the alias");
        }
        let store = Store::open(target).unwrap();
        assert_eq!(store.load().unwrap().tasks.len(), 2);
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dangling_state_symlink_is_rejected_without_creating_sidecars() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!("jig-dangling-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let alias = dir.join("alias.json");
        symlink("missing.json", &alias).unwrap();
        assert!(Store::open(alias.clone()).is_err());
        assert_eq!(fs::read_link(&alias).unwrap(), Path::new("missing.json"));
        assert!(!dir.join("missing.json").exists());
        assert!(!append_suffix(&alias, ".lock").exists());
        assert!(!append_suffix(&alias, ".tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
