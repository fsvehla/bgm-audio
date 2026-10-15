//! Automatic updates with WinSparkle.
//!
//! WinSparkle.dll ships next to Ambience.exe in the release zip and is loaded at runtime,
//! so dev builds without it simply have no updater. WinSparkle reads
//! appcast-windows.xml (one item per release, an enclosure per architecture), checks
//! the EdDSA signature (the macOS Sparkle key) and downloads the zip. We then unpack it
//! next to the running exe, swap the files and restart; no installer.

use std::ffi::{CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use futures::channel::mpsc::UnboundedSender;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{HSTRING, PCSTR};

use crate::Event;

const APPCAST_URL: &str =
    "https://github.com/fsvehla/bgm-audio/releases/latest/download/appcast-windows.xml";
/// Same key as SUPublicEDKey in packaging/Info.plist.
const ED_PUBLIC_KEY: &str = "bAS+Hld3ES4qPgSeef0qEpzvZY8+auSIBdoEMGoLHJY=";
/// From winsparkle.h.
const WINSPARKLE_RETURN_ERROR: i32 = -1;

/// The updater's callbacks run on WinSparkle's threads.
static TX: Mutex<Option<UnboundedSender<Event>>> = Mutex::new(None);

pub struct Updater {
    library: HMODULE,
}

impl gpui::Global for Updater {}

impl Updater {
    /// Loads WinSparkle.dll from the exe's folder and starts scheduled update checks.
    /// `AMBIENCE_APPCAST_URL` and `AMBIENCE_VERSION` override the feed and the version
    /// (for testing an update against a local feed).
    pub fn start(tx: UnboundedSender<Event>) -> Option<Self> {
        let dir = exe_dir()?;
        remove_old_files(&dir);
        let dll = dir.join("WinSparkle.dll");
        if !dll.is_file() {
            return None;
        }
        let library = unsafe { LoadLibraryW(&HSTRING::from(dll.as_os_str())) }.ok()?;
        *TX.lock().unwrap() = Some(tx);
        let updater = Self { library };

        let url = std::env::var("AMBIENCE_APPCAST_URL").unwrap_or_else(|_| APPCAST_URL.into());
        let version =
            std::env::var("AMBIENCE_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into());
        let url = CString::new(url).ok()?;
        let key = CString::new(ED_PUBLIC_KEY).ok()?;
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (company, app, version) = (wide("fsvehla"), wide("Ambience"), wide(&version));
        unsafe {
            let set_url: extern "C" fn(*const c_char) =
                updater.function(c"win_sparkle_set_appcast_url")?;
            let set_key: extern "C" fn(*const c_char) -> i32 =
                updater.function(c"win_sparkle_set_eddsa_public_key")?;
            let set_details: extern "C" fn(*const u16, *const u16, *const u16) =
                updater.function(c"win_sparkle_set_app_details")?;
            let set_build: extern "C" fn(*const u16) =
                updater.function(c"win_sparkle_set_app_build_version")?;
            let set_installer: extern "C" fn(extern "C" fn(*const u16) -> i32) =
                updater.function(c"win_sparkle_set_user_run_installer_callback")?;
            let set_shutdown: extern "C" fn(extern "C" fn()) =
                updater.function(c"win_sparkle_set_shutdown_request_callback")?;
            let init: extern "C" fn() = updater.function(c"win_sparkle_init")?;

            set_url(url.as_ptr());
            if set_key(key.as_ptr()) != 1 {
                eprintln!("Ambience: WinSparkle rejected the public key");
                return None;
            }
            set_details(company.as_ptr(), app.as_ptr(), version.as_ptr());
            set_build(version.as_ptr());
            set_installer(run_installer);
            set_shutdown(shutdown_requested);
            init();
        }
        Some(updater)
    }

    /// "Check for Updates…": WinSparkle shows its own progress and result windows.
    pub fn check_for_updates(&self) {
        unsafe {
            if let Some(check) =
                self.function::<extern "C" fn()>(c"win_sparkle_check_update_with_ui")
            {
                check();
            }
        }
    }

    /// The exported function `name` as `F`, which must be its `extern "C"` signature.
    unsafe fn function<F: Copy>(&self, name: &std::ffi::CStr) -> Option<F> {
        let f = unsafe { GetProcAddress(self.library, PCSTR(name.as_ptr().cast()))? };
        Some(unsafe { std::mem::transmute_copy::<unsafe extern "system" fn() -> isize, F>(&f) })
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        unsafe {
            if let Some(cleanup) = self.function::<extern "C" fn()>(c"win_sparkle_cleanup") {
                cleanup();
            }
        }
    }
}

fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

/// Files replaced by the last update (`*.old`); they were in use when it ran.
fn remove_old_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().extension().is_some_and(|e| e == "old") {
            std::fs::remove_file(entry.path()).ok();
        }
    }
}

fn send(event: Event) {
    if let Some(tx) = TX.lock().unwrap().as_ref() {
        tx.unbounded_send(event).ok();
    }
}

extern "C" fn shutdown_requested() {
    send(Event::Quit);
}

/// WinSparkle downloaded and verified the update zip at `path`: install it ourselves.
extern "C" fn run_installer(path: *const u16) -> i32 {
    let len = (0..).take_while(|&i| unsafe { *path.add(i) } != 0).count();
    let path = PathBuf::from(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(path, len)
    }));
    match install(&path) {
        Ok(()) => {
            send(Event::Quit);
            1
        }
        Err(e) => {
            eprintln!("Ambience: update failed: {e}");
            WINSPARKLE_RETURN_ERROR
        }
    }
}

/// Unpacks the zip next to the exe, renames the running files to `*.old` (Windows
/// allows renaming files that are in use, not replacing them), moves the new ones in,
/// and starts the new exe once this process has exited.
fn install(zip_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let dir = exe_dir().ok_or("no exe folder")?;
    let staging = dir.join("update-staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    zip::ZipArchive::new(std::fs::File::open(zip_path)?)?.extract(&staging)?;
    // The zip holds a single folder (Ambience-<version>-windows-<arch>) or the files.
    let mut source = staging.clone();
    let entries: Vec<_> = std::fs::read_dir(&staging)?.flatten().collect();
    if let [only] = entries.as_slice()
        && only.path().is_dir()
    {
        source = only.path();
    }
    if !source.join("Ambience.exe").is_file() {
        return Err("Ambience.exe missing from the update".into());
    }
    for entry in std::fs::read_dir(&source)?.flatten() {
        let target = dir.join(entry.file_name());
        if target.exists() {
            let old = dir.join(format!("{}.old", entry.file_name().to_string_lossy()));
            std::fs::remove_file(&old).ok();
            std::fs::rename(&target, &old)?;
        }
        std::fs::rename(entry.path(), &target)?;
    }
    std::fs::remove_dir_all(&staging).ok();

    relaunch(&dir.join("Ambience.exe"))?;
    Ok(())
}

/// Starts `exe` after this process exits, via a hidden PowerShell.
fn relaunch(exe: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = format!(
        "Wait-Process -Id {} -ErrorAction SilentlyContinue; Start-Process -FilePath '{}'",
        std::process::id(),
        exe.display().to_string().replace('\'', "''")
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}
