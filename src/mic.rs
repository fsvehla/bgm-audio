//! Detects when another app records from a microphone.
//!
//! Uses CoreAudio process objects (macOS 14.2+): every process that does audio
//! I/O has one, with a flag for "is running input". We listen for changes to
//! the process list and to each process's flag, and re-check on every change.

use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{NonNull, null};

use futures::channel::mpsc::UnboundedSender;
use objc2::rc::Retained;
use objc2_core_audio::{
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, kAudioHardwarePropertyProcessObjectList,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioProcessPropertyBundleID, kAudioProcessPropertyIsRunningInput, kAudioProcessPropertyPID,
};
use objc2_foundation::NSString;

use crate::Event;

/// Processes whose microphone use should not pause playback.
const IGNORED: &[&str] = &[
    // Holds the mic open for the whole time Remote Control / screen sharing is connected.
    "com.apple.avconferenced",
];

pub struct MicWatcher {
    /// Leaked once; CoreAudio holds a pointer to it for the app's lifetime.
    tx: &'static UnboundedSender<Event>,
    watched: HashSet<AudioObjectID>,
}

impl MicWatcher {
    pub fn new(tx: UnboundedSender<Event>) -> Self {
        let mut watcher = Self {
            tx: Box::leak(Box::new(tx)),
            watched: HashSet::new(),
        };
        watcher.listen(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyProcessObjectList,
        );
        watcher.sync();
        watcher
    }

    /// Starts listening on processes that appeared since the last call.
    pub fn sync(&mut self) {
        for process in processes() {
            if self.watched.insert(process) {
                self.listen(process, kAudioProcessPropertyIsRunningInput);
            }
        }
    }

    /// Bundle ID (or "pid N") of another process recording right now, if any.
    pub fn recording_app(&self) -> Option<String> {
        let me = std::process::id() as i32;
        processes().into_iter().find_map(|p| {
            let running: u32 = get(p, kAudioProcessPropertyIsRunningInput)?;
            let pid: i32 = get(p, kAudioProcessPropertyPID)?;
            if running == 0 || pid == me {
                return None;
            }
            let name = bundle_id(p).unwrap_or_else(|| format!("pid {pid}"));
            (!IGNORED.contains(&name.as_str())).then_some(name)
        })
    }

    fn listen(&self, object: AudioObjectID, selector: u32) {
        let address = address(selector);
        unsafe {
            AudioObjectAddPropertyListener(
                object,
                NonNull::from(&address),
                Some(on_change),
                self.tx as *const UnboundedSender<Event> as *mut c_void,
            );
        }
    }
}

unsafe extern "C-unwind" fn on_change(
    _: AudioObjectID,
    _: u32,
    _: NonNull<AudioObjectPropertyAddress>,
    data: *mut c_void,
) -> i32 {
    let tx = unsafe { &*(data as *const UnboundedSender<Event>) };
    let _ = tx.unbounded_send(Event::MicChanged);
    0
}

fn address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn processes() -> Vec<AudioObjectID> {
    let system = kAudioObjectSystemObject as AudioObjectID;
    let address = address(kAudioHardwarePropertyProcessObjectList);
    let mut size = 0u32;
    unsafe {
        if AudioObjectGetPropertyDataSize(
            system,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
        ) != 0
        {
            return Vec::new();
        }
        let mut ids = vec![0 as AudioObjectID; size as usize / size_of::<AudioObjectID>()];
        if ids.is_empty()
            || AudioObjectGetPropertyData(
                system,
                NonNull::from(&address),
                0,
                null(),
                NonNull::from(&mut size),
                NonNull::new_unchecked(ids.as_mut_ptr().cast()),
            ) != 0
        {
            return Vec::new();
        }
        ids.truncate(size as usize / size_of::<AudioObjectID>());
        ids
    }
}

fn get<T: Copy + Default>(object: AudioObjectID, selector: u32) -> Option<T> {
    let address = address(selector);
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    (status == 0).then_some(value)
}

fn bundle_id(process: AudioObjectID) -> Option<String> {
    // The property is a CFStringRef that we own (toll-free bridged to NSString).
    let ptr: usize = get(process, kAudioProcessPropertyBundleID)?;
    let s = unsafe { Retained::from_raw(ptr as *mut NSString) }?;
    Some(s.to_string()).filter(|s| !s.is_empty())
}
