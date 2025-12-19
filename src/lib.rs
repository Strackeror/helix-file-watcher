use std::collections::HashSet;
use std::mem;
use std::path::absolute;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use std::{path::PathBuf, sync::mpsc::Receiver};

use abi_stable::std_types::{RBoxError, RResult, RString, RVec};
use notify::{Event, PollWatcher, RecursiveMode, Watcher as _};
use steel::rvals::Custom;
use steel::steel_vm::ffi::{FFIModule, FFIValue, IntoFFIVal, RegisterFFIFn};

steel::declare_module!(build_module);

fn file_watcher_module() -> FFIModule {
    let mut module = FFIModule::new("steel/file-watcher");
    module
        .register_fn("receive-event!", || WATCHER.recv())
        .register_fn("set-watch-files", |files| WATCHER.set_watch_files(files))
        .register_fn("event-paths", NotifyEvent::paths)
        .register_fn("event-kind", NotifyEvent::kind);
    module
}

struct Watcher {
    watcher: Mutex<PollWatcher>,
    files: Mutex<HashSet<String>>,
    receiver: Mutex<Receiver<Event>>,
}

struct NotifyEvent(Event);

impl Custom for Watcher {}
impl Custom for NotifyEvent {}

impl Watcher {
    fn recv(&self) -> RResult<FFIValue, RBoxError> {
        let res = self
            .receiver
            .lock()
            .unwrap()
            .recv()
            .map(NotifyEvent)
            .map(|x| x.into_ffi_val().unwrap())
            .map_err(RBoxError::new);

        match res {
            Ok(ok) => RResult::ROk(ok),
            Err(err) => RResult::RErr(err),
        }
    }

    fn set_watch_files(&self, files: Vec<String>) {
        let new_set: HashSet<_> = files.into_iter().map(|s| s.to_string()).collect();
        let old_set = mem::replace(&mut *self.files.lock().unwrap(), new_set.clone());

        let mut watcher = self.watcher.lock().unwrap();
        for new_path in new_set.difference(&old_set) {
            let path = PathBuf::from(new_path);
            let abs_path = absolute(&path).unwrap();
            _ = watcher.watch(&abs_path, RecursiveMode::NonRecursive);
        }
        for old_path in old_set.difference(&new_set) {
            let path_buf = PathBuf::from(old_path);
            let abs_path = absolute(&path_buf).unwrap();
            _ = watcher.unwatch(&abs_path);
        }
    }
}

impl NotifyEvent {
    pub fn kind(&self) -> FFIValue {
        match self.0.kind {
            notify::EventKind::Modify(_) => "modify".to_owned().into_ffi_val().unwrap(),
            _ => FFIValue::BoolV(false),
        }
    }

    pub fn paths(&self) -> RVec<FFIValue> {
        self.0
            .paths
            .iter()
            .map(|x| FFIValue::StringV(RString::from(x.as_os_str().to_str().unwrap())))
            .collect()
    }
}

static WATCHER: LazyLock<Watcher> = LazyLock::new(|| {
    let (sender, receiver) = std::sync::mpsc::channel();
    let watcher = notify::PollWatcher::new(
        move |event: Result<Event, _>| {
            let Ok(event) = event else { return };
            let notify::EventKind::Modify(_) = &event.kind else {
                return;
            };
            sender.send(event).unwrap();
        },
        notify::Config::default().with_poll_interval(Duration::from_secs(5)),
    )
    .unwrap();

    Watcher {
        watcher: Mutex::new(watcher),
        files: Mutex::default(),
        receiver: Mutex::new(receiver),
    }
});

pub fn build_module() -> FFIModule {
    file_watcher_module()
}
