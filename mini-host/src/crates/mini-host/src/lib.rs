//! # mini-host
//!
//! A minimal VST3 host: enough of one to load a plugin binary, instantiate it,
//! open its editor, deliver keystrokes and read its state back.
//!
//! It talks to the plugin exactly the way a DAW does — `GetPluginFactory`,
//! `IPluginFactory::createInstance`, `IEditController::createView`,
//! `IPlugView::onKeyDown`, `IComponent::get/setState` — so a test driven
//! through this host exercises the real plugin boundary, not a shortcut around
//! it. Nothing in the plugin knows it is being tested.
//!
//! It is not specific to any plugin: it loads whatever VST3 binary it is
//! pointed at.

pub mod keys;
pub mod presets;
pub mod stream;

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use vst3::{ComPtr, ComWrapper, Interface, Steinberg::Vst::*, Steinberg::*};

pub use keys::{Key, Mods};
pub use stream::MemoryStream;

// VST3 virtual key codes and modifier mask (Steinberg::VirtualKeyCodes /
// KeyModifier). A host has to speak these natively, so they are defined here
// rather than borrowed from the plugin.
pub const KEY_BACK: i16 = 1;
pub const KEY_TAB: i16 = 2;
pub const KEY_RETURN: i16 = 4;
pub const KEY_ESCAPE: i16 = 6;
pub const KEY_END: i16 = 9;
pub const KEY_HOME: i16 = 10;
pub const KEY_LEFT: i16 = 11;
pub const KEY_UP: i16 = 12;
pub const KEY_RIGHT: i16 = 13;
pub const KEY_DOWN: i16 = 14;
pub const KEY_PAGEUP: i16 = 15;
pub const KEY_PAGEDOWN: i16 = 16;
pub const KEY_DELETE: i16 = 22;

pub const MOD_SHIFT: i16 = 1 << 0;
pub const MOD_ALT: i16 = 1 << 1;
pub const MOD_COMMAND: i16 = 1 << 2;
pub const MOD_CONTROL: i16 = 1 << 3;

/// What `IPluginFactory::getFactoryInfo` reports.
#[derive(Debug, Clone, Default)]
pub struct FactoryInfo {
    pub vendor: String,
    pub url: String,
    pub email: String,
    pub flags: i32,
}

impl ClassInfo2 {
    /// The class ID as `moduleinfo.json` spells it.
    ///
    /// A `TUID` is stored in Windows COM byte order, and `FUID::toString` in
    /// the SDK prints the first three fields numerically rather than byte by
    /// byte, so the string is not simply the bytes in order.
    pub fn cid_string(&self) -> String {
        let b = &self.cid;
        let data1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let data2 = u16::from_le_bytes([b[4], b[5]]);
        let data3 = u16::from_le_bytes([b[6], b[7]]);
        let rest: String = b[8..].iter().map(|byte| format!("{byte:02X}")).collect();
        format!("{data1:08X}{data2:04X}{data3:04X}{rest}")
    }
}

/// What `IPluginFactory2::getClassInfo2` reports — the metadata a DAW uses to
/// decide whether a plugin is an effect or an instrument.
#[derive(Debug, Clone)]
pub struct ClassInfo2 {
    pub cid: [u8; 16],
    pub name: String,
    pub category: String,
    pub sub_categories: String,
    pub vendor: String,
    pub version: String,
    pub sdk_version: String,
    pub class_flags: u32,
    pub cardinality: i32,
}

#[derive(Debug)]
pub enum HostError {
    Load(String),
    MissingEntryPoint(String),
    NoBinary(String),
    NoFactory,
    NoClass,
    NoSuchClass(i32),
    CreateFailed(i32),
    MissingInterface(&'static str),
    NoView,
    Call(&'static str, i32),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Load(e) => write!(f, "could not load plugin binary: {e}"),
            HostError::MissingEntryPoint(e) => write!(f, "missing VST3 entry point: {e}"),
            HostError::NoBinary(p) => write!(f, "no plugin binary found inside {p}"),
            HostError::NoFactory => write!(f, "GetPluginFactory returned null"),
            HostError::NoClass => write!(f, "the factory exposes no classes"),
            HostError::NoSuchClass(i) => write!(f, "no class at index {i}"),
            HostError::CreateFailed(c) => write!(f, "createInstance failed (tresult {c})"),
            HostError::MissingInterface(i) => write!(f, "plugin does not implement {i}"),
            HostError::NoView => write!(f, "createView returned null"),
            HostError::Call(m, c) => write!(f, "{m} failed (tresult {c})"),
        }
    }
}

impl std::error::Error for HostError {}

type FactoryFn = unsafe extern "system" fn() -> *mut IPluginFactory;
type BoolFn = unsafe extern "system" fn() -> bool;

/// A loaded plugin binary.
pub struct Module {
    // Declared before `library` so the COM pointer is released while the
    // library is still mapped.
    factory: ComPtr<IPluginFactory>,
    library: libloading::Library,
}

/// Find the loadable binary for a `.vst3` path.
///
/// A VST3 plugin may be shipped either as a bare shared library named `.vst3`
/// or as a bundle *directory* with the binary under
/// `Contents/<arch>-<os>/`. Both are in the wild — Vital ships the former,
/// Surge the latter — so a host has to cope with each.
pub fn resolve_binary(path: &Path) -> Result<PathBuf, HostError> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if !path.is_dir() {
        return Err(HostError::NoBinary(path.display().to_string()));
    }

    let contents = path.join("Contents");

    // macOS keeps the binary here, with no extension.
    let macos = contents.join("MacOS");
    if macos.is_dir() {
        if let Some(found) = first_file(&macos) {
            return Ok(found);
        }
    }

    // Windows and Linux use an architecture directory. Prefer this machine's,
    // then accept any as a fallback so a bundle built elsewhere still reports.
    let preferred = [
        arch_dir_name(),
        "x86_64-win".into(),
        "x86_64-linux".into(),
        "arm64-win".into(),
    ];
    for name in preferred {
        let dir = contents.join(&name);
        if dir.is_dir() {
            if let Some(found) = first_file(&dir) {
                return Ok(found);
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(&contents) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(found) = first_file(&entry.path()) {
                    return Ok(found);
                }
            }
        }
    }

    Err(HostError::NoBinary(path.display().to_string()))
}

/// First loadable file in a directory, ignoring resources like icons.
fn first_file(dir: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            matches!(ext.as_str(), "vst3" | "dll" | "so" | "dylib" | "")
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

fn arch_dir_name() -> String {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86"
    };
    let os = if cfg!(target_os = "windows") {
        "win"
    } else {
        "linux"
    };
    format!("{arch}-{os}")
}

impl Module {
    /// Load a plugin from disk and fetch its factory.
    ///
    /// `path` may be either the binary itself or the `.vst3` bundle directory.
    pub fn load(path: &Path) -> Result<Module, HostError> {
        let path = &resolve_binary(path)?;
        unsafe {
            let library = libloading::Library::new(path)
                .map_err(|e| HostError::Load(format!("{}: {e}", path.display())))?;

            // Hosts must run the module initialiser before anything else.
            #[cfg(target_os = "windows")]
            let init_name = b"InitDll\0".as_slice();
            #[cfg(target_os = "macos")]
            let init_name = b"BundleEntry\0".as_slice();
            #[cfg(target_os = "linux")]
            let init_name = b"ModuleEntry\0".as_slice();

            if let Ok(init) = library.get::<BoolFn>(init_name) {
                init();
            }

            let get_factory = library
                .get::<FactoryFn>(b"GetPluginFactory\0")
                .map_err(|e| HostError::MissingEntryPoint(e.to_string()))?;

            let factory = ComPtr::from_raw(get_factory()).ok_or(HostError::NoFactory)?;

            Ok(Module { factory, library })
        }
    }

    /// Everything `IPluginFactory::getFactoryInfo` reports.
    pub fn factory_info(&self) -> FactoryInfo {
        unsafe {
            let mut info: PFactoryInfo = std::mem::zeroed();
            if self.factory.getFactoryInfo(&mut info) != kResultOk {
                return FactoryInfo::default();
            }
            FactoryInfo {
                vendor: cstr_to_string(info.vendor.as_ptr()),
                url: cstr_to_string(info.url.as_ptr()),
                email: cstr_to_string(info.email.as_ptr()),
                flags: info.flags,
            }
        }
    }

    /// Vendor string from the factory, as a DAW would show it.
    pub fn vendor(&self) -> String {
        unsafe {
            let mut info: PFactoryInfo = std::mem::zeroed();
            if self.factory.getFactoryInfo(&mut info) != kResultOk {
                return String::new();
            }
            cstr_to_string(info.vendor.as_ptr())
        }
    }

    /// Which factory interfaces the plugin exposes.
    ///
    /// Hosts prefer the newest one they know. A plugin that only offers the
    /// v1 factory gives the host no `subCategories` field at all, so it cannot
    /// tell an effect from an instrument.
    pub fn factory_versions(&self) -> (bool, bool, bool) {
        (
            true,
            self.factory.cast::<IPluginFactory2>().is_some(),
            self.factory.cast::<IPluginFactory3>().is_some(),
        )
    }

    pub fn class_count(&self) -> i32 {
        unsafe { self.factory.countClasses() }
    }

    /// Name and category of a class, as listed in a DAW's plugin manager.
    pub fn class_info(&self, index: i32) -> Option<(String, String)> {
        unsafe {
            let mut info: PClassInfo = std::mem::zeroed();
            if self.factory.getClassInfo(index, &mut info) != kResultOk {
                return None;
            }
            Some((
                cstr_to_string(info.name.as_ptr()),
                cstr_to_string(info.category.as_ptr()),
            ))
        }
    }

    /// Index of the first class a DAW would load, i.e. the audio module.
    pub fn first_audio_class(&self) -> Option<i32> {
        (0..self.class_count())
            .find(|i| matches!(self.class_info(*i), Some((_, cat)) if cat == "Audio Module Class"))
    }

    /// Everything `IPluginFactory2::getClassInfo2` reports about a class.
    ///
    /// This is the metadata a DAW reads to decide *what kind of plugin* this
    /// is — effect or instrument, and which SDK it claims to speak. Getting it
    /// wrong is how a plugin ends up loaded as the wrong type.
    pub fn class_info2(&self, index: i32) -> Option<ClassInfo2> {
        unsafe {
            let factory2 = self.factory.cast::<IPluginFactory2>()?;
            let mut info: PClassInfo2 = std::mem::zeroed();
            if factory2.getClassInfo2(index, &mut info) != kResultOk {
                return None;
            }
            Some(ClassInfo2 {
                cid: std::mem::transmute::<TUID, [u8; 16]>(info.cid),
                name: cstr_to_string(info.name.as_ptr()),
                category: cstr_to_string(info.category.as_ptr()),
                sub_categories: cstr_to_string(info.subCategories.as_ptr()),
                vendor: cstr_to_string(info.vendor.as_ptr()),
                version: cstr_to_string(info.version.as_ptr()),
                sdk_version: cstr_to_string(info.sdkVersion.as_ptr()),
                class_flags: info.classFlags,
                cardinality: info.cardinality,
            })
        }
    }

    /// The same metadata as [`Module::class_info2`], but via
    /// `IPluginFactory3::getClassInfoUnicode` — the newest form, and the one a
    /// modern host prefers. If the two disagree, hosts disagree about the
    /// plugin.
    pub fn class_info_unicode(&self, index: i32) -> Option<ClassInfo2> {
        unsafe {
            let factory3 = self.factory.cast::<IPluginFactory3>()?;
            let mut info: PClassInfoW = std::mem::zeroed();
            if factory3.getClassInfoUnicode(index, &mut info) != kResultOk {
                return None;
            }
            Some(ClassInfo2 {
                cid: std::mem::transmute::<TUID, [u8; 16]>(info.cid),
                name: utf16_to_string(&info.name),
                category: cstr_to_string(info.category.as_ptr()),
                sub_categories: cstr_to_string(info.subCategories.as_ptr()),
                vendor: utf16_to_string(&info.vendor),
                version: utf16_to_string(&info.version),
                sdk_version: utf16_to_string(&info.sdkVersion),
                class_flags: info.classFlags,
                cardinality: info.cardinality,
            })
        }
    }

    /// Instantiate the first audio-module class and initialise it.
    pub fn create_plugin(&self) -> Result<Plugin, HostError> {
        let index = self.first_audio_class().unwrap_or(0);
        self.create_plugin_at(index)
    }

    /// Instantiate the class at `index`.
    ///
    /// Handles both plugin shapes. Most VST3 plugins are *two* classes — a
    /// processor implementing `IComponent` and a separate controller
    /// implementing `IEditController` — and the host is responsible for
    /// creating the second, initialising it, handing it the processor's state
    /// and connecting the pair. A minority (including this project's own
    /// plugin) are "single-component effects" where one object implements
    /// both, which is detected first because no second instance is needed.
    pub fn create_plugin_at(&self, index: i32) -> Result<Plugin, HostError> {
        unsafe {
            let mut info: PClassInfo = std::mem::zeroed();
            if self.factory.getClassInfo(index, &mut info) != kResultOk {
                return Err(HostError::NoSuchClass(index));
            }

            let mut obj: *mut c_void = std::ptr::null_mut();
            let res = self.factory.createInstance(
                info.cid.as_ptr() as FIDString,
                IComponent::IID.as_ptr() as FIDString,
                &mut obj,
            );
            if res != kResultOk || obj.is_null() {
                return Err(HostError::CreateFailed(res));
            }

            let component = ComPtr::from_raw(obj as *mut IComponent)
                .ok_or(HostError::MissingInterface("IComponent"))?;

            let res = component.initialize(std::ptr::null_mut());
            if res != kResultOk {
                return Err(HostError::Call("initialize", res));
            }

            // Shape 1: one object implementing both interfaces.
            if let Some(controller) = component.cast::<IEditController>() {
                return Ok(Plugin {
                    view: None,
                    controller: Some(controller),
                    separate_controller: false,
                    component,
                });
            }

            // Shape 2: ask the processor which controller class to create.
            let mut controller_cid: TUID = std::mem::zeroed();
            let controller = if component.getControllerClassId(&mut controller_cid) == kResultOk {
                let mut obj: *mut c_void = std::ptr::null_mut();
                let res = self.factory.createInstance(
                    controller_cid.as_ptr() as FIDString,
                    IEditController::IID.as_ptr() as FIDString,
                    &mut obj,
                );
                if res == kResultOk && !obj.is_null() {
                    ComPtr::from_raw(obj as *mut IEditController)
                } else {
                    None
                }
            } else {
                None
            };

            let Some(controller) = controller else {
                // A processor with no controller at all is legal, if unusual.
                return Ok(Plugin {
                    view: None,
                    controller: None,
                    separate_controller: false,
                    component,
                });
            };

            controller.initialize(std::ptr::null_mut());

            // The controller starts blank; a host gives it the processor's
            // state so the UI shows the right values.
            let stream = ComWrapper::new(MemoryStream::new());
            if let Some(ptr) = stream.to_com_ptr::<IBStream>() {
                if component.getState(ptr.as_ptr()) == kResultOk {
                    stream.rewind();
                    controller.setComponentState(ptr.as_ptr());
                }
            }

            // Connect the pair directly. A full DAW inserts a proxy so it can
            // marshal between threads; connecting them to each other is the
            // standard simple-host approach and is what plugins expect.
            if let (Some(a), Some(b)) = (
                component.cast::<IConnectionPoint>(),
                controller.cast::<IConnectionPoint>(),
            ) {
                a.connect(b.as_ptr());
                b.connect(a.as_ptr());
            }

            Ok(Plugin {
                view: None,
                controller: Some(controller),
                separate_controller: true,
                component,
            })
        }
    }

    /// Keep the library reachable for the lifetime of the module.
    pub fn library(&self) -> &libloading::Library {
        &self.library
    }
}

/// An instantiated plugin.
pub struct Plugin {
    // Dropped in declaration order: view, then controller, then component.
    view: Option<ComPtr<IPlugView>>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    component: ComPtr<IComponent>,
}

impl Plugin {
    /// True when the controller is a second object, as in most plugins.
    pub fn has_separate_controller(&self) -> bool {
        self.separate_controller
    }

    pub fn has_controller(&self) -> bool {
        self.controller.is_some()
    }

    /// Number of automatable parameters the controller exposes.
    pub fn parameter_count(&self) -> i32 {
        self.controller
            .as_ref()
            .map(|c| unsafe { c.getParameterCount() })
            .unwrap_or(0)
    }

    /// Audio or event bus count, e.g. `bus_count(true, true)` for audio inputs.
    pub fn bus_count(&self, audio: bool, input: bool) -> i32 {
        let media = if audio {
            MediaTypes_::kAudio
        } else {
            MediaTypes_::kEvent
        } as MediaType;
        let dir = if input {
            BusDirections_::kInput
        } else {
            BusDirections_::kOutput
        } as BusDirection;
        unsafe { self.component.getBusCount(media, dir) }
    }

    /// Title of parameter `index`, as a DAW shows it in an automation lane.
    pub fn parameter_name(&self, index: i32) -> Option<String> {
        let controller = self.controller.as_ref()?;
        unsafe {
            let mut info: ParameterInfo = std::mem::zeroed();
            if controller.getParameterInfo(index, &mut info) != kResultOk {
                return None;
            }
            Some(wstring_to_string(&info.title))
        }
    }

    /// Ask the plugin for its editor, as a DAW does when the window is opened.
    pub fn open_editor(&mut self) -> Result<(), HostError> {
        let controller = self
            .controller
            .as_ref()
            .ok_or(HostError::MissingInterface("IEditController"))?;
        unsafe {
            let view = controller.createView(ViewType::kEditor);
            let view = ComPtr::from_raw(view).ok_or(HostError::NoView)?;
            self.view = Some(view);
            Ok(())
        }
    }

    fn view(&self) -> Result<&ComPtr<IPlugView>, HostError> {
        self.view.as_ref().ok_or(HostError::NoView)
    }

    /// Give the editor a window to draw in.
    ///
    /// `parent` is the platform's native handle — an `HWND` on Windows, an
    /// `NSView` on macOS — and must outlive the attachment.
    ///
    /// # Safety
    /// The pointer must be a live window handle of that type.
    pub unsafe fn attach(&self, parent: *mut c_void) -> Result<(), HostError> {
        let view = self.view()?;
        #[cfg(target_os = "windows")]
        let platform = kPlatformTypeHWND;
        #[cfg(target_os = "macos")]
        let platform = kPlatformTypeNSView;
        #[cfg(target_os = "linux")]
        let platform = kPlatformTypeX11EmbedWindowID;

        let res = view.attached(parent, platform);
        if res == kResultOk || res == kResultTrue {
            Ok(())
        } else {
            Err(HostError::Call("IPlugView::attached", res))
        }
    }

    /// Take the editor back off its window.
    pub fn detach(&self) -> Result<(), HostError> {
        let view = self.view()?;
        let res = unsafe { view.removed() };
        if res == kResultOk || res == kResultTrue {
            Ok(())
        } else {
            Err(HostError::Call("IPlugView::removed", res))
        }
    }

    /// Deliver one key press through `IPlugView::onKeyDown`.
    pub fn send_key(&self, key: Key, mods: Mods) -> Result<bool, HostError> {
        let (unit, code) = encode_key(key);
        let modifiers = encode_mods(mods);
        let view = self.view()?;
        let res = unsafe { view.onKeyDown(unit, code, modifiers) };
        Ok(res == kResultTrue || res == kResultOk)
    }

    /// Type a string, one key event per character.
    ///
    /// Newlines are sent as the Return virtual key, so typing multi-line text
    /// goes through exactly the path a real keyboard would.
    pub fn type_text(&self, text: &str) -> Result<(), HostError> {
        for c in text.chars() {
            let key = if c == '\n' { Key::Enter } else { Key::Char(c) };
            self.send_key(key, Mods::NONE)?;
        }
        Ok(())
    }

    /// Speaker arrangements, as bitmasks (one bit per speaker).
    pub const MONO: u64 = SpeakerArr::kMono;
    pub const STEREO: u64 = SpeakerArr::kStereo;

    /// Negotiate a speaker arrangement, as a host does when the plugin is
    /// placed on a track of a particular width. Returns whether it was accepted.
    pub fn set_bus_arrangement(&self, arrangement: u64) -> Result<bool, HostError> {
        self.negotiate(Some(arrangement), Some(arrangement))
    }

    /// Ask the plugin to run with the given input/output arrangements, where
    /// `None` means "no bus on that side".
    ///
    /// Asking for no audio input is how a host works out whether a plugin can
    /// act as a generator. An effect must refuse.
    pub fn negotiate(
        &self,
        input: Option<u64>,
        output: Option<u64>,
    ) -> Result<bool, HostError> {
        let processor = self
            .component
            .cast::<IAudioProcessor>()
            .ok_or(HostError::MissingInterface("IAudioProcessor"))?;
        let mut ins = input.unwrap_or(0);
        let mut outs = output.unwrap_or(0);
        let res = unsafe {
            processor.setBusArrangements(
                &mut ins,
                input.is_some() as i32,
                &mut outs,
                output.is_some() as i32,
            )
        };
        Ok(res == kResultTrue)
    }

    /// What the plugin says it is running at now.
    pub fn bus_arrangement(&self, input: bool) -> Result<u64, HostError> {
        let processor = self
            .component
            .cast::<IAudioProcessor>()
            .ok_or(HostError::MissingInterface("IAudioProcessor"))?;
        let dir = if input {
            BusDirections_::kInput
        } else {
            BusDirections_::kOutput
        } as BusDirection;
        unsafe {
            let mut arrangement: SpeakerArrangement = 0;
            let res = processor.getBusArrangement(dir, 0, &mut arrangement);
            if res != kResultOk {
                return Err(HostError::Call("getBusArrangement", res));
            }
            Ok(arrangement)
        }
    }

    /// Channel count the plugin advertises for a bus.
    pub fn bus_channels(&self, input: bool) -> Result<i32, HostError> {
        let dir = if input {
            BusDirections_::kInput
        } else {
            BusDirections_::kOutput
        } as BusDirection;
        unsafe {
            let mut info: BusInfo = std::mem::zeroed();
            let res = self
                .component
                .getBusInfo(MediaTypes_::kAudio as MediaType, dir, 0, &mut info);
            if res != kResultOk {
                return Err(HostError::Call("getBusInfo", res));
            }
            Ok(info.channelCount)
        }
    }

    /// Run one block of audio through the plugin, as a DAW does on every
    /// buffer, and return what came out.
    ///
    /// Input and output are given *separate* buffers, which is the case that
    /// catches a plugin that forgets to write its output: a plugin that leaves
    /// the output untouched silences the track it is inserted on.
    ///
    /// Performs the full activation dance a host must do — `setupProcessing`,
    /// `activateBus`, `setActive`, `setProcessing` — and unwinds it afterwards.
    pub fn process_audio(&self, input: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, HostError> {
        let frames = input.first().map(|c| c.len()).unwrap_or(0);
        self.process_audio_inner(Some(input), input.len(), frames, 0.0)
    }

    /// Process with **no input bus at all**, handing the plugin an output
    /// buffer pre-filled with `prefill`.
    ///
    /// A host does this when the input bus is deactivated. A plugin that skips
    /// writing its output in that case leaves `prefill` in place, which reaches
    /// the track as noise — so this is how you catch it.
    pub fn process_audio_without_input(
        &self,
        channels: usize,
        frames: usize,
        prefill: f32,
    ) -> Result<Vec<Vec<f32>>, HostError> {
        self.process_audio_inner(None, channels, frames, prefill)
    }

    fn process_audio_inner(
        &self,
        input: Option<&[Vec<f32>]>,
        channels: usize,
        frames: usize,
        prefill: f32,
    ) -> Result<Vec<Vec<f32>>, HostError> {
        if channels == 0 || frames == 0 {
            return Ok(Vec::new());
        }

        unsafe {
            let processor = self
                .component
                .cast::<IAudioProcessor>()
                .ok_or(HostError::MissingInterface("IAudioProcessor"))?;

            let mut setup = ProcessSetup {
                processMode: ProcessModes_::kRealtime as int32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
                maxSamplesPerBlock: frames as int32,
                sampleRate: 48_000.0,
            };
            let res = processor.setupProcessing(&mut setup);
            if res != kResultOk {
                return Err(HostError::Call("setupProcessing", res));
            }

            let audio = MediaTypes_::kAudio as MediaType;
            self.component
                .activateBus(audio, BusDirections_::kInput as BusDirection, 0, 1);
            self.component
                .activateBus(audio, BusDirections_::kOutput as BusDirection, 0, 1);
            self.component.setActive(1);
            processor.setProcessing(1);

            // Owned buffers, kept alive for the duration of the call.
            let mut ins: Vec<Vec<f32>> = input.map(|i| i.to_vec()).unwrap_or_default();
            // Pre-filled so an unwritten output is visible rather than silently
            // looking like a correct pass-through of silence.
            let mut outs: Vec<Vec<f32>> = vec![vec![prefill; frames]; channels];
            let mut in_ptrs: Vec<*mut f32> = ins.iter_mut().map(|c| c.as_mut_ptr()).collect();
            let mut out_ptrs: Vec<*mut f32> = outs.iter_mut().map(|c| c.as_mut_ptr()).collect();

            let mut in_bus: AudioBusBuffers = std::mem::zeroed();
            in_bus.numChannels = in_ptrs.len() as int32;
            in_bus.__field0.channelBuffers32 = in_ptrs.as_mut_ptr();

            let mut out_bus: AudioBusBuffers = std::mem::zeroed();
            out_bus.numChannels = channels as int32;
            out_bus.__field0.channelBuffers32 = out_ptrs.as_mut_ptr();

            let mut data: ProcessData = std::mem::zeroed();
            data.processMode = ProcessModes_::kRealtime as int32;
            data.symbolicSampleSize = SymbolicSampleSizes_::kSample32 as int32;
            data.numSamples = frames as int32;
            data.numInputs = if input.is_some() { 1 } else { 0 };
            data.numOutputs = 1;
            data.inputs = if input.is_some() {
                &mut in_bus
            } else {
                std::ptr::null_mut()
            };
            data.outputs = &mut out_bus;

            let res = processor.process(&mut data);

            processor.setProcessing(0);
            self.component.setActive(0);

            if res != kResultOk {
                return Err(HostError::Call("process", res));
            }
            Ok(outs)
        }
    }

    /// Read the plugin's persisted state, as a DAW does when saving a project.
    pub fn get_state(&self) -> Result<Vec<u8>, HostError> {
        let stream = ComWrapper::new(MemoryStream::new());
        let ptr = stream
            .to_com_ptr::<IBStream>()
            .ok_or(HostError::MissingInterface("IBStream"))?;
        let res = unsafe { self.component.getState(ptr.as_ptr()) };
        if res != kResultOk {
            return Err(HostError::Call("getState", res));
        }
        Ok(stream.data())
    }

    /// Restore state, as a DAW does when reopening a project.
    pub fn set_state(&self, bytes: &[u8]) -> Result<(), HostError> {
        let stream = ComWrapper::new(MemoryStream::with_data(bytes.to_vec()));
        let ptr = stream
            .to_com_ptr::<IBStream>()
            .ok_or(HostError::MissingInterface("IBStream"))?;
        stream.rewind();
        let res = unsafe { self.component.setState(ptr.as_ptr()) };
        if res != kResultOk {
            return Err(HostError::Call("setState", res));
        }
        Ok(())
    }

    /// The editor's current size.
    pub fn view_size(&self) -> Result<(i32, i32), HostError> {
        let view = self.view()?;
        unsafe {
            let mut rect: ViewRect = std::mem::zeroed();
            let res = view.getSize(&mut rect);
            if res != kResultOk {
                return Err(HostError::Call("getSize", res));
            }
            Ok((rect.right - rect.left, rect.bottom - rect.top))
        }
    }

    /// Resize the editor, as a DAW does when the user drags the window edge.
    pub fn resize(&self, width: i32, height: i32) -> Result<(), HostError> {
        let view = self.view()?;
        unsafe {
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            let res = view.onSize(&mut rect);
            if res != kResultOk {
                return Err(HostError::Call("onSize", res));
            }
            Ok(())
        }
    }

    /// Ask the plugin what size it would accept for a proposed one.
    pub fn check_size(&self, width: i32, height: i32) -> Result<(i32, i32), HostError> {
        let view = self.view()?;
        unsafe {
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            view.checkSizeConstraint(&mut rect);
            Ok((rect.right - rect.left, rect.bottom - rect.top))
        }
    }

    pub fn can_resize(&self) -> Result<bool, HostError> {
        let view = self.view()?;
        Ok(unsafe { view.canResize() } == kResultTrue)
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        unsafe {
            self.view = None;
            // Tear down in the order a host does: disconnect the pair, then
            // terminate the controller, then the processor.
            if self.separate_controller {
                if let Some(controller) = &self.controller {
                    if let (Some(a), Some(b)) = (
                        self.component.cast::<IConnectionPoint>(),
                        controller.cast::<IConnectionPoint>(),
                    ) {
                        a.disconnect(b.as_ptr());
                        b.disconnect(a.as_ptr());
                    }
                    controller.terminate();
                }
            }
            self.controller = None;
            self.component.terminate();
        }
    }
}

/// Map an editor key onto the `(char, virtual key code)` pair a host sends.
pub fn encode_key(key: Key) -> (u16, i16) {
    match key {
        Key::Char(c) => {
            let mut buf = [0u16; 2];
            let units = c.encode_utf16(&mut buf);
            (units[0], -1)
        }
        Key::Enter => (0, KEY_RETURN),
        Key::Backspace => (0, KEY_BACK),
        Key::Delete => (0, KEY_DELETE),
        Key::Tab => (0, KEY_TAB),
        Key::Left => (0, KEY_LEFT),
        Key::Right => (0, KEY_RIGHT),
        Key::Up => (0, KEY_UP),
        Key::Down => (0, KEY_DOWN),
        Key::Home => (0, KEY_HOME),
        Key::End => (0, KEY_END),
        Key::PageUp => (0, KEY_PAGEUP),
        Key::PageDown => (0, KEY_PAGEDOWN),
        Key::Escape => (0, KEY_ESCAPE),
    }
}

pub fn encode_mods(mods: Mods) -> i16 {
    let mut m = 0;
    if mods.ctrl {
        m |= MOD_COMMAND;
    }
    if mods.shift {
        m |= MOD_SHIFT;
    }
    if mods.alt {
        m |= MOD_ALT;
    }
    m
}

/// `PClassInfoW` uses unsigned UTF-16 buffers.
fn utf16_to_string(buf: &[u16]) -> String {
    let units: Vec<u16> = buf.iter().take_while(|c| **c != 0).copied().collect();
    String::from_utf16_lossy(&units)
}

/// VST3 uses fixed-size UTF-16 buffers for user-visible strings.
fn wstring_to_string(buf: &[TChar]) -> String {
    let units: Vec<u16> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u16)
        .collect();
    String::from_utf16_lossy(&units)
}

fn cstr_to_string(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        std::ffi::CStr::from_ptr(ptr)
            .to_string_lossy()
            .into_owned()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A directory this test owns, cleaned up by the caller's `drop`.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "mini-host-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn file(&self, path: &str) -> PathBuf {
            let full = self.0.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, b"not really a plugin").unwrap();
            full
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_plain_file_is_the_binary() {
        let dir = TempDir::new("file");
        let file = dir.file("Notepad.vst3");
        assert_eq!(resolve_binary(&file).unwrap(), file);
    }

    #[test]
    fn a_macos_bundle_resolves_to_the_binary_inside() {
        let dir = TempDir::new("macos");
        let inner = dir.file("Notepad.vst3/Contents/MacOS/Notepad");
        let bundle = dir.0.join("Notepad.vst3");
        assert_eq!(resolve_binary(&bundle).unwrap(), inner);
    }

    #[test]
    fn a_windows_bundle_resolves_to_the_binary_inside() {
        let dir = TempDir::new("win");
        let inner = dir.file("Notepad.vst3/Contents/x86_64-win/Notepad.vst3");
        let bundle = dir.0.join("Notepad.vst3");
        assert_eq!(resolve_binary(&bundle).unwrap(), inner);
    }

    #[test]
    fn a_bundle_built_for_another_architecture_still_resolves() {
        // Not this machine's directory, and not one of the named fallbacks.
        let dir = TempDir::new("other");
        let inner = dir.file("Notepad.vst3/Contents/riscv64-linux/Notepad.so");
        let bundle = dir.0.join("Notepad.vst3");
        assert_eq!(resolve_binary(&bundle).unwrap(), inner);
    }

    #[test]
    fn a_directory_with_no_binary_is_an_error() {
        let dir = TempDir::new("empty");
        std::fs::create_dir_all(dir.0.join("Notepad.vst3/Contents")).unwrap();
        let bundle = dir.0.join("Notepad.vst3");
        assert!(matches!(
            resolve_binary(&bundle),
            Err(HostError::NoBinary(_))
        ));
    }

    #[test]
    fn a_path_that_does_not_exist_is_an_error() {
        let missing = std::env::temp_dir().join("mini-host-does-not-exist.vst3");
        assert!(matches!(
            resolve_binary(&missing),
            Err(HostError::NoBinary(_))
        ));
    }

    #[test]
    fn characters_are_sent_as_text_and_named_keys_as_codes() {
        // VST3 splits these: a character carries its UTF-16 unit and no key
        // code, a named key carries a code and no character.
        assert_eq!(encode_key(Key::Char('a')), (b'a' as u16, -1));
        assert_eq!(encode_key(Key::Char(' ')), (b' ' as u16, -1));
        assert_eq!(encode_key(Key::Enter), (0, KEY_RETURN));
        assert_eq!(encode_key(Key::Backspace), (0, KEY_BACK));
        assert_eq!(encode_key(Key::Escape), (0, KEY_ESCAPE));
    }

    #[test]
    fn a_non_ascii_character_keeps_its_utf16_unit() {
        let (unit, code) = encode_key(Key::Char('é'));
        assert_eq!(unit, 'é' as u16);
        assert_eq!(code, -1);
    }

    #[test]
    fn modifiers_combine() {
        assert_eq!(encode_mods(Mods::NONE), 0);
        let ctrl = encode_mods(Mods::CTRL);
        let shift = encode_mods(Mods::SHIFT);
        assert_ne!(ctrl, 0);
        assert_ne!(shift, 0);
        assert_ne!(ctrl, shift);
        assert_eq!(encode_mods(Mods::CTRL_SHIFT), ctrl | shift);
    }

    #[test]
    fn an_error_says_which_file_and_why() {
        let text = HostError::NoBinary("C:/plugins/Thing.vst3".into()).to_string();
        assert!(text.contains("Thing.vst3"), "{text}");
    }
}
