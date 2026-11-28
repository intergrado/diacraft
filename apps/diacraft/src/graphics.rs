//! Which graphics adapter draws the window, on macOS and Windows.
//!
//! eframe asks wgpu for the high-performance GPU. On the dual-GPU MacBook Pro the project tested on (an AMD Radeon Pro 5500M
//! beside an Intel UHD Graphics 630, macOS 26.7.3), that GPU took every command and ran none: Metal ended each command buffer
//! with an internal error (`e00002bd`, the kernel driver could not allocate memory), wgpu counts a command buffer that ended in
//! error as finished, and nothing reached the log. The window stayed black, and `ui.screenshot` read back a buffer the GPU never
//! wrote: every pixel (0, 0, 0, 0). A plain Metal program with no wgpu in it failed the same way on that GPU, started from a
//! terminal and from the desktop session, and drew on the Intel one; so did the window.
//!
//! So on macOS the window does two things eframe does not:
//! - It prefers the power-saving GPU, unless `WGPU_POWER_PREF` (`low`, `high`, `none`) says otherwise. That is the GPU that
//!   drives the built-in display under automatic graphics switching, and all a diagram needs: the canvas is drawn on the CPU,
//!   and the GPU only composites it and draws the interface.
//! - Before it settles on an adapter it checks that the GPU runs commands ([`probe`]): a device made as the window will make
//!   it clears a small texture to [`PROBE_COLOUR`] and reads it back. An adapter that reads back anything else is passed over,
//!   with a warning in the log, and the next one is tried ([`pick`]). When none passes, the first is used, as before, with an
//!   error in the log.
//!
//! The order ([`adapter_order`]): an adapter that cannot present to the window's surface never; the one `WGPU_ADAPTER_NAME`
//! names (part of its name, any case) first; hardware before software; native back ends before OpenGL; then by the power
//! preference; then, on Windows, DirectX 12 before Vulkan for the same kind of GPU (see below); keeping wgpu's order among equals.
//! Every adapter and the choice are logged: which GPU draws is the first question of a black window.
//!
//! wgpu's check of indirect draw calls is off on macOS too: egui draws nothing indirectly, and the sibling craft apps found the
//! check's compute shader failing to compile on older Intel Macs' Metal drivers, which lost the device before the window
//! showed. `WGPU_VALIDATION_INDIRECT_CALL=1` turns it back on.
//!
//! **Windows** has the same risk on a laptop with two GPUs, so its adapters are probed and picked the same way, through
//! DirectX 12, Vulkan and OpenGL alike: a GPU that fails its first commands is passed over for the next. It keeps eframe's
//! high-performance preference. wgpu lists each GPU once for Vulkan and once for DirectX 12, Vulkan first; **the order puts DirectX 12 first for the same
//! kind of GPU** ([`Rules::dx12_first`]), because Windows' own graphics stack is the one every machine has working, and a Vulkan driver that is installed and broken
//! (the public report was Intel UHD Graphics through Vulkan, which ended the start with nothing in the log) is then tested last, and only when DirectX 12 did not
//! pass. A back end the person names with `WGPU_BACKEND` is their choice: the order is wgpu's then. It keeps the check of indirect draws,
//! which only the Mac's drivers were found to fail. None of this has run on a Windows machine yet.
//!
//! **A test that never returns**: a driver that crashes the process or blocks inside a call during the test ends a start that the tries of [`fallback`](crate::fallback)
//! cannot save, because they only help when a try returns an error. So each test is logged first (`testing <adapter> (try …)`, flushed to the disk), is made on a thread of its own
//! that the start gives up on after [`PROBE_GIVE_UP`] (the adapter is passed over and left out of every later try, and the start goes on to the next), and leaves a marker file in the
//! settings folder while it runs ([`marker`](crate::marker)): the next start leaves out an adapter whose test ended its predecessor, until DiaCraft's version changes or the person
//! starts `diacraft --reset-graphics`. `DIACRAFT_GPU_PROBE_FAIL` ([`FAULT_ENV`]) makes the test of an adapter abort or hang, so that all of this can be tried on a machine whose drivers work.
//!
//! Linux keeps eframe's choice: [`configuration`] changes nothing there. The probe makes a device
//! of its own before the window's, which costs time at start, and no GPU there has been found to need it.
//!
//! **When no adapter passes** the window does not stop, and does not draw on one that failed: the start goes on to the next try of [`fallback`](crate::fallback)
//! (DirectX 12 alone, the software adapter, OpenGL alone, and last an adapter that failed the test, used with no test). Each try chooses by [`Rules`], leaves out the adapters
//! a window could not be made on before, those whose test did not return and those an earlier start left out ([`Memory`]), and says in words why it found none ([`choose`]).

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use eframe::egui_wgpu::{NativeAdapterSelectorMethod, WgpuConfiguration, WgpuSetup};
use eframe::wgpu::{self, Backend, DeviceType, InstanceFlags, PowerPreference};

use diacraft_engine::guard;

use crate::marker::{self, Store};

/// wgpu's variable for choosing an adapter by (part of) its name, any case.
pub const NAME_ENV: &str = "WGPU_ADAPTER_NAME";

/// wgpu's variable for choosing the graphics back ends (`vulkan`, `dx12`, `metal`, `gl`, or a list of them with commas). A back end named there is the person's choice: no try of
/// the start swaps it for another.
pub const BACKEND_ENV: &str = "WGPU_BACKEND";

/// wgpu's variable that turns its check of indirect draw calls on (`1`) or off (`0`).
pub const INDIRECT_ENV: &str = "WGPU_VALIDATION_INDIRECT_CALL";

/// The colour the probe clears its texture to, as 8-bit RGBA. Each channel is a multiple of 51, so it is exact as a fraction
/// of 255 and survives the conversion to and from the GPU's floats.
pub const PROBE_COLOUR: [u8; 4] = [51, 102, 153, 255];

/// The probe texture's width in pixels: one row of 256 bytes, wgpu's alignment for a copy into a buffer.
pub const PROBE_WIDTH: u32 = 64;

/// How long the probe waits for the GPU. A GPU that has not cleared 64 pixels by then is not one to draw a window on.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// How long a start waits for the test of one adapter before it gives up on it, whatever it is doing: the GPU's own wait ([`PROBE_TIMEOUT`]) covers only the commands it was given, and a
/// driver call that blocks (making the device, say) has no timeout of its own. The test runs on a thread of its own, which is left behind when this runs out.
pub const PROBE_GIVE_UP: Duration = Duration::from_secs(12);

/// The stack of the thread a test runs on: the main thread's (`.cargo/config.toml` gives Windows 8 MiB), which is what the test had before it had a thread of its own. Drivers load libraries and
/// enumerate devices in the first calls, and some use a lot of stack. The memory is reserved, not used.
const TEST_STACK_BYTES: usize = 8 * 1024 * 1024;

/// The diagnostic variable that makes the test of an adapter fail the way a broken driver does, so that the start's defences can be tried on a machine whose drivers work:
/// `<part of the adapter's name>:<abort|hang>` (`DIACRAFT_GPU_PROBE_FAIL=UHD:abort`). See [`fault_for`].
pub const FAULT_ENV: &str = "DIACRAFT_GPU_PROBE_FAIL";

/// What an adapter is, for [`adapter_order`]: read from wgpu, or written by a test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub backend: Backend,
    pub device_type: DeviceType,
    /// Whether it can present to the window's surface (true when there is no surface to ask).
    pub presents: bool,
}

impl Candidate {
    /// The candidate `adapter` is, for the window's `surface`.
    pub fn of(adapter: &wgpu::Adapter, surface: Option<&wgpu::Surface<'_>>) -> Candidate {
        let info = adapter.get_info();
        Candidate {
            name: info.name,
            backend: info.backend,
            device_type: info.device_type,
            presents: surface.is_none_or(|s| adapter.is_surface_supported(s)),
        }
    }

    /// `Intel(R) UHD Graphics 630 (Metal, IntegratedGpu)`, for the log.
    pub fn label(&self) -> String {
        format!("{} ({:?}, {:?})", self.name.trim(), self.backend, self.device_type)
    }

    /// What names the adapter in the [`Memory`] of a start: its name and back end (the same GPU through another back end is another adapter).
    pub fn key(&self) -> Key {
        key_of(&self.name, self.backend)
    }
}

/// An adapter as the [`Memory`] of a start, and the files of [`marker`], name it.
pub type Key = (String, Backend);

/// The most characters of an adapter's name a [`Key`] keeps: the name is a driver's text, and goes into a file of the settings folder.
const MOST_NAME_CHARS: usize = 160;

/// The [`Key`] of an adapter called `name` through `backend`: the name with each control character (a line break, a NUL a driver ends it with) made a space, trimmed and cut to
/// [`MOST_NAME_CHARS`]. Every place that names an adapter builds its key here, so that the same adapter is the same key in the log, in the memory of a start and in a file.
pub fn key_of(name: &str, backend: Backend) -> Key {
    let flat: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    (flat.trim().chars().take(MOST_NAME_CHARS).collect::<String>().trim_end().to_owned(), backend)
}

/// How a try chooses among the adapters (`fallback::Rung::rules`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Only the adapters that run on the processor (wgpu's fallback adapter: WARP on Windows, llvmpipe or lavapipe on Linux).
    pub software_only: bool,
    /// Whether each adapter must pass the probe ([`probe`]) before the window draws on it. Without it the first adapter of the order is taken as it is.
    pub probe: bool,
    /// Whether DirectX 12 comes before Vulkan for the same kind of GPU ([`adapter_order`]): on Windows, unless the person named a back end.
    pub dx12_first: bool,
}

impl Rules {
    /// The rule of the usual try: any adapter, each checked.
    pub const USUAL: Rules = Rules { software_only: false, probe: true, dx12_first: false };
    /// The software adapter, checked.
    pub const SOFTWARE: Rules = Rules { software_only: true, probe: true, dx12_first: false };
    /// Any adapter, not checked: the last try, for a machine where none passed.
    pub const UNTESTED: Rules = Rules { software_only: false, probe: false, dx12_first: false };

    /// These rules, with DirectX 12 first for the same kind of GPU when `on`.
    pub const fn with_dx12_first(self, on: bool) -> Rules {
        Rules { dx12_first: on, ..self }
    }

    /// Whether the order puts DirectX 12 first on `system` when the person's environment is `env`: on Windows, and only when `WGPU_BACKEND` names nothing (a back end the person names is their choice).
    pub fn dx12_first_for(system: System, env: &GraphicsEnv) -> bool {
        system == System::Windows && env.explicit_backend().is_none()
    }
}

/// What the tries of one start remember between them: the adapter the last try chose, those a window could not be made on, and those left out. A later try leaves the failed ones and the
/// left-out ones out. **Left out** are the adapters an earlier start ended while testing ([`marker`], read when the start began), and those whose test did not return in this start
/// ([`Memory::leave_out`], which also puts them in the file for the next start).
#[derive(Debug, Default)]
pub struct Memory {
    state: Mutex<MemoryState>,
    store: Store,
}

#[derive(Debug, Default)]
struct MemoryState {
    chosen: Option<Key>,
    failed: Vec<Key>,
    left_out: Vec<Key>,
}

impl Memory {
    /// The memory of a start whose files are in `store` and which begins with `left_out` left out (what [`Store::open`] read).
    pub fn new(store: Store, left_out: Vec<Key>) -> Memory {
        Memory { state: Mutex::new(MemoryState { left_out, ..MemoryState::default() }), store }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        // A thread that panicked while holding it leaves the lists usable: take them back.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The files of earlier starts: where a test's marker is written.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The adapters left out: those an earlier start ended while testing, and those whose test did not return in this start.
    pub fn left_out(&self) -> Vec<Key> {
        self.state().left_out.clone()
    }

    /// The adapters every later try of this start leaves out: [`Memory::failed`] and [`Memory::left_out`].
    pub fn excluded(&self) -> Vec<Key> {
        let state = self.state();
        let mut all = state.failed.clone();
        all.extend(state.left_out.iter().filter(|k| !state.failed.contains(k)).cloned());
        all
    }

    /// The test of `key` did not return in time: it is left out of every later try of this start, and of the next starts (the list file of [`marker`]).
    pub fn leave_out(&self, key: &Key) {
        {
            let mut state = self.state();
            if !state.left_out.contains(key) {
                state.left_out.push(key.clone());
            }
        }
        self.store.leave_out(key);
    }

    /// The selector chose `key`: the window is being made on it.
    pub fn chose(&self, key: Key) {
        self.state().chosen = Some(key);
    }

    /// The window could not be made on the adapter just chosen: it is failed from now on. Returns it; `None` when none was chosen (or it was told already).
    pub fn fail_chosen(&self) -> Option<Key> {
        let mut state = self.state();
        let key = state.chosen.take()?;
        if !state.failed.contains(&key) {
            state.failed.push(key.clone());
        }
        Some(key)
    }

    /// The adapters a window could not be made on, in the order they failed.
    pub fn failed(&self) -> Vec<Key> {
        self.state().failed.clone()
    }
}

/// The system the window runs on, as far as the choice of a GPU goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    MacOs,
    Windows,
    /// Linux and any other.
    Other,
}

impl System {
    /// The system this program is built for.
    pub const fn this() -> System {
        if cfg!(target_os = "macos") {
            System::MacOs
        } else if cfg!(target_os = "windows") {
            System::Windows
        } else {
            System::Other
        }
    }

    /// Whether the window checks that a GPU runs commands before it draws on it ([`selector`]): on macOS and Windows.
    pub const fn probes(self) -> bool {
        matches!(self, System::MacOs | System::Windows)
    }
}

/// The power preference when `WGPU_POWER_PREF` sets none: the power-saving GPU on macOS, eframe's high-performance one elsewhere.
pub fn default_power(system: System) -> PowerPreference {
    if system == System::MacOs { PowerPreference::LowPower } else { PowerPreference::HighPerformance }
}

/// The adapters in the order the window tries them, as indexes into `candidates`: those that can't present to the window
/// never; the one whose name contains `named` (any case) first; hardware before software; native back ends before OpenGL; then
/// by `power`; then, when `dx12_first`, DirectX 12 before the other back ends (the same GPU through DirectX 12 before Vulkan: [`Rules::dx12_first`]). The sort is stable, so
/// equals keep wgpu's order.
pub fn adapter_order(candidates: &[Candidate], power: PowerPreference, named: Option<&str>, dx12_first: bool) -> Vec<usize> {
    let named = named.map(str::trim).filter(|n| !n.is_empty()).map(str::to_lowercase);
    let mut order: Vec<(usize, &Candidate)> = candidates.iter().enumerate().filter(|(_, c)| c.presents).collect();
    order.sort_by_key(|(_, c)| {
        let unnamed = named.as_ref().is_some_and(|n| !c.name.to_lowercase().contains(n.as_str()));
        let not_dx12 = dx12_first && c.backend != Backend::Dx12;
        (unnamed, c.device_type == DeviceType::Cpu, c.backend == Backend::Gl, power_rank(c.device_type, power), not_dx12)
    });
    order.into_iter().map(|(i, _)| i).collect()
}

/// wgpu's own ranking for a power preference: the preferred kind of GPU, the other kind, unknown, virtual, software.
fn power_rank(kind: DeviceType, power: PowerPreference) -> u8 {
    match (power, kind) {
        (PowerPreference::None, _) => 0,
        (PowerPreference::LowPower, DeviceType::IntegratedGpu) | (PowerPreference::HighPerformance, DeviceType::DiscreteGpu) => 0,
        (_, DeviceType::IntegratedGpu | DeviceType::DiscreteGpu) => 1,
        (_, DeviceType::Other) => 2,
        (_, DeviceType::VirtualGpu) => 3,
        (_, DeviceType::Cpu) => 4,
    }
}

/// What [`pick`] chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    /// The index of the adapter the window draws on.
    pub chosen: usize,
    /// The adapters tried before it, and why each was passed over.
    pub passed_over: Vec<(usize, String)>,
    /// No adapter passed: `chosen` is the first of the order, used anyway.
    pub none_passed: bool,
}

/// The first adapter of `order` that passes `probe`, each tried once in turn; the first of `order` when none passes. `None`
/// for an empty order.
pub fn pick(order: &[usize], mut probe: impl FnMut(usize) -> Result<(), String>) -> Option<Pick> {
    let first = *order.first()?;
    let mut passed_over = Vec::new();
    for &index in order {
        match probe(index) {
            Ok(()) => return Some(Pick { chosen: index, passed_over, none_passed: false }),
            Err(why) => passed_over.push((index, why)),
        }
    }
    Some(Pick { chosen: first, passed_over, none_passed: true })
}

/// [`adapter_order`] for one try: the adapters `rules` allow (only software ones for [`Rules::SOFTWARE`]) that are not in `failed` (those a window was not made on before, and those left
/// out), in the order [`adapter_order`] gives (with DirectX 12 first when `rules` say so). The indexes are those of `candidates`.
pub fn rung_order(candidates: &[Candidate], power: PowerPreference, named: Option<&str>, rules: Rules, failed: &[Key]) -> Vec<usize> {
    let allowed: Vec<Candidate> = candidates
        .iter()
        .map(|c| Candidate {
            presents: c.presents && !failed.contains(&c.key()) && (!rules.software_only || c.device_type == DeviceType::Cpu),
            ..c.clone()
        })
        .collect();
    adapter_order(&allowed, power, named, rules.dx12_first)
}

/// What one try chooses among `candidates`: the adapter [`rung_order`] puts first that passes `probe` (each tried once, in turn; none is tried when `rules.probe` is off, and
/// the first is taken). `Err` says in words why there is none: no adapter to choose from, or none passed the test (and why each failed).
pub fn choose(
    candidates: &[Candidate],
    power: PowerPreference,
    named: Option<&str>,
    rules: Rules,
    failed: &[Key],
    probe: impl FnMut(usize) -> Result<(), String>,
) -> Result<Pick, String> {
    let order = rung_order(candidates, power, named, rules, failed);
    let picked = if rules.probe { pick(&order, probe) } else { pick(&order, |_| Ok(())) };
    let Some(picked) = picked else {
        return Err(no_adapter(candidates, power, rules, failed));
    };
    if picked.none_passed {
        let reasons: Vec<String> = picked.passed_over.iter().map(|(i, why)| format!("{}: {why}", label_of(candidates, *i))).collect();
        return Err(format!("no graphics adapter passed the test ({})", reasons.join("; ")));
    }
    Ok(picked)
}

/// `Radeon (Vulkan, DiscreteGpu)` for the adapter at `index`, for the log.
fn label_of(candidates: &[Candidate], index: usize) -> String {
    candidates.get(index).map_or_else(|| format!("adapter {index}"), Candidate::label)
}

/// The sentence for a try that has no adapter to choose from, with the numbers that say why.
fn no_adapter(candidates: &[Candidate], power: PowerPreference, rules: Rules, failed: &[Key]) -> String {
    let mut notes = vec![format!("{} listed", candidates.len())];
    let hidden = candidates.iter().filter(|c| !c.presents).count();
    if hidden > 0 {
        notes.push(format!("{hidden} can't show the window"));
    }
    let earlier = candidates.iter().filter(|c| failed.contains(&c.key())).count();
    if earlier > 0 {
        notes.push(format!("{earlier} failed on an earlier try or start"));
    }
    if rules.software_only {
        notes.push("this try wants software adapters only".to_owned());
    }
    format!("no graphics adapter can show the window ({}; power preference {power:?})", notes.join("; "))
}

/// Whether `bytes`, read back from the probe's texture, hold [`PROBE_COLOUR`] in each of its [`PROBE_WIDTH`] pixels (a step
/// of one either way is allowed for a channel). `Err` says what came back instead.
pub fn probe_verdict(bytes: &[u8]) -> Result<(), String> {
    let pixels = usize::try_from(PROBE_WIDTH).unwrap_or(usize::MAX);
    let wanted = pixels.saturating_mul(4);
    if bytes.len() < wanted {
        return Err(format!("it read back {} bytes of the {wanted} it drew", bytes.len()));
    }
    let (read, _) = bytes.as_chunks::<4>();
    let off = read.iter().take(pixels).find(|pixel| pixel.iter().zip(PROBE_COLOUR).any(|(got, want)| got.abs_diff(want) > 1));
    match off {
        None => Ok(()),
        Some(pixel) if pixel.iter().all(|c| *c == 0) => {
            Err(format!("a texture it cleared to {PROBE_COLOUR:?} read back as {pixel:?}: the GPU ran none of the commands"))
        }
        Some(pixel) => Err(format!("a texture it cleared to {PROBE_COLOUR:?} read back as {pixel:?}")),
    }
}

/// Checks that `adapter` runs commands: makes a device with `descriptor`, as the window would, clears a texture of
/// [`PROBE_WIDTH`] by 1 pixels to [`PROBE_COLOUR`], copies it into a buffer and reads it back, waiting at most `timeout` for
/// the GPU. `Err` says why the window should not draw on it. The device is dropped at the end; an error of it is the probe's
/// answer, never a panic.
pub fn probe(adapter: &wgpu::Adapter, descriptor: &wgpu::DeviceDescriptor<'_>, timeout: Duration) -> Result<(), String> {
    let (device, queue) = pollster::block_on(adapter.request_device(descriptor)).map_err(|e| format!("it gave no device: {e}"))?;
    // wgpu's own handler panics on an error no scope catches: this device's errors are logged, and the scope below is the answer.
    device.on_uncaptured_error(Arc::new(|e| log::warn!("graphics probe: {e}")));
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let outcome = clear_and_read(&device, &queue, timeout);
    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(format!("wgpu refused the test: {error}"));
    }
    outcome
}

/// The probe's commands, on a device made for it.
fn clear_and_read(device: &wgpu::Device, queue: &wgpu::Queue, timeout: Duration) -> Result<(), String> {
    let size = wgpu::Extent3d { width: PROBE_WIDTH, height: 1, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("diacraft graphics probe"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = PROBE_WIDTH.saturating_mul(4);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("diacraft graphics probe"),
        size: u64::from(row),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("diacraft graphics probe") });
    {
        let [r, g, b, a] = PROBE_COLOUR.map(|c| f64::from(c) / 255.0);
        // A pass that only clears: dropped at once, which ends it.
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("diacraft graphics probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        size,
    );
    let submitted = queue.submit([encoder.finish()]);
    let mapped: Arc<Mutex<Option<Result<(), wgpu::BufferAsyncError>>>> = Arc::default();
    let slot = Arc::clone(&mapped);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
    });
    device
        .poll(wgpu::PollType::Wait { submission_index: Some(submitted), timeout: Some(timeout) })
        .map_err(|e| format!("the GPU did not finish the test in {} s: {e}", timeout.as_secs()))?;
    match mapped.lock().unwrap_or_else(PoisonError::into_inner).take() {
        Some(Ok(())) => {}
        Some(Err(e)) => return Err(format!("the test's buffer could not be read back: {e}")),
        None => return Err("the test's buffer was not read back in time".to_owned()),
    }
    let verdict = match buffer.slice(..).get_mapped_range() {
        Ok(bytes) => probe_verdict(&bytes),
        Err(e) => Err(format!("the test's buffer could not be read back: {e}")),
    };
    buffer.unmap();
    verdict
}

/// The window's device descriptor for an adapter: eframe's, the one the probe makes its device with too.
pub type DeviceFn = Arc<dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> + Send + Sync>;

/// What a try's [`selector_with`] needs to know.
pub struct Setup {
    /// The try's name in the log (`fallback::Rung::label`).
    pub label: &'static str,
    pub power: PowerPreference,
    /// `WGPU_ADAPTER_NAME`: part of the name of the adapter to try first.
    pub named: Option<String>,
    pub rules: Rules,
    /// Shared by the tries of the start: the adapters that failed or were left out are left out, the adapter chosen is recorded, and a test's marker is written in its store.
    pub memory: Arc<Memory>,
    pub device: DeviceFn,
    /// What `DIACRAFT_GPU_PROBE_FAIL` holds ([`FAULT_ENV`]): the diagnostic that makes the test of an adapter abort or hang.
    pub fault: Option<String>,
    /// How long a test may take before the start gives up on it ([`PROBE_GIVE_UP`]).
    pub give_up: Duration,
}

/// What a test is made to do by [`FAULT_ENV`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// End the process on the spot, as a driver that crashes does.
    Abort,
    /// Block for ever, as a driver call that never returns does.
    Hang,
}

/// The fault `spec` (`<part of the adapter's name>:<abort|hang>`, [`FAULT_ENV`]) asks of the test of the adapter called `name` through `backend`. The part is looked for, in any case,
/// in the adapter as the log writes it (`Intel(R) UHD Graphics (dx12)`), so `UHD` is every adapter of that name and `UHD Graphics (dx12)` the one through DirectX 12. An empty part
/// matches nothing, and so does a word after the colon that is neither `abort` nor `hang`.
pub fn fault_for(spec: &str, name: &str, backend: Backend) -> Option<Fault> {
    let (part, what) = spec.rsplit_once(':')?;
    let fault = match what.trim().to_lowercase().as_str() {
        "abort" => Fault::Abort,
        "hang" => Fault::Hang,
        _ => return None,
    };
    let part = part.trim().to_lowercase();
    (!part.is_empty() && marker::describe(&key_of(name, backend)).to_lowercase().contains(&part)).then_some(fault)
}

/// Does what `fault` asks, on the thread of the test: it never returns. The line that says so is on the disk first.
fn act_on(fault: Fault, label: &str) -> ! {
    let what = if fault == Fault::Abort { "ends the process" } else { "blocks" };
    log::warn!("{FAULT_ENV} is set: the test of {label} {what} here (a diagnostic; the adapter is not at fault)");
    log::logger().flush();
    match fault {
        Fault::Abort => std::process::abort(),
        Fault::Hang => loop {
            std::thread::sleep(Duration::from_secs(3600));
        },
    }
}

/// How work run by [`run_within`] ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ran<T> {
    /// It returned this in time.
    Done(T),
    /// It panicked, or its thread could not be made: the words.
    Failed(String),
    /// It was still running when the time was up. Its thread is left behind.
    TimedOut,
}

/// Runs `work` on a thread of its own and waits for it at most `limit`, so that a driver call that blocks cannot keep the start waiting for ever. A panic of `work` is caught on its
/// thread. A thread that is still running at the end of `limit` is left behind: it ends with the process, or whenever its call returns, and its answer is dropped.
pub fn run_within<T: Send + 'static>(limit: Duration, work: impl FnOnce() -> T + Send + 'static) -> Ran<T> {
    let (sender, receiver) = channel();
    let spawned = std::thread::Builder::new().name("graphics test".to_owned()).stack_size(TEST_STACK_BYTES).spawn(move || {
        // The receiver is gone when the start gave up on this work: nobody is left to tell.
        let _ = sender.send(guard::catch_panic(work));
    });
    if let Err(e) = spawned {
        return Ran::Failed(format!("the test could not start a thread: {e}"));
    }
    match receiver.recv_timeout(limit) {
        Ok(Ok(done)) => Ran::Done(done),
        Ok(Err(panicked)) => Ran::Failed(format!("the test panicked: {panicked}")),
        Err(RecvTimeoutError::Timeout) => Ran::TimedOut,
        Err(RecvTimeoutError::Disconnected) => Ran::Failed("the test's thread ended with no answer".to_owned()),
    }
}

/// The test of the adapter at `index` of the try's order (`number` counts the tests of this try), if it is listed: [`test_adapter`].
fn test_listed(setup: &Setup, adapters: &[wgpu::Adapter], candidates: &[Candidate], index: usize, number: usize) -> Result<(), String> {
    match (adapters.get(index), candidates.get(index)) {
        (Some(adapter), Some(candidate)) => test_adapter(setup, adapter, candidate, number),
        _ => Err("not listed".to_owned()),
    }
}

/// Tests `adapter` ([`probe`]) the way that survives a driver that does not answer. The log says which adapter is being tested and is flushed to the disk before
/// the driver is called; the marker of the test is written in the settings folder ([`marker`]) and removed when the test returns; the test runs on a thread of its own
/// ([`run_within`]), and a start that has waited [`Setup::give_up`] for it passes the adapter over: it is left out of every later try of this start and of the next starts
/// ([`Memory::leave_out`]), and the choice goes on to the next adapter.
fn test_adapter(setup: &Setup, adapter: &wgpu::Adapter, candidate: &Candidate, number: usize) -> Result<(), String> {
    let (key, label) = (candidate.key(), candidate.label());
    log::info!("testing {label} (try {}, test {number})", setup.label);
    // On the disk before the driver is called: a crash, or a machine that goes down, leaves this line as the last of the log.
    log::logger().flush();
    let marker = setup.memory.store().begin(&key);
    let fault = setup.fault.as_deref().and_then(|spec| fault_for(spec, &candidate.name, candidate.backend));
    let (adapter, device, shown) = (adapter.clone(), Arc::clone(&setup.device), label.clone());
    let ran = run_within(setup.give_up, move || {
        if let Some(fault) = fault {
            act_on(fault, &shown);
        }
        probe(&adapter, &device(&adapter), PROBE_TIMEOUT)
    });
    let verdict = match ran {
        Ran::Done(verdict) => verdict,
        Ran::Failed(why) => Err(why),
        Ran::TimedOut => {
            let seconds = setup.give_up.as_secs();
            log::warn!(
                "the test of {label} did not return in {seconds} s: a driver call is blocked. It is left out of every try, and of the starts after this one until DiaCraft's version changes or it is started with --reset-graphics"
            );
            setup.memory.leave_out(&key);
            Err(format!("it did not answer in {seconds} s: a driver call is blocked"))
        }
    };
    // The test returned (or was given up on and is in the list now): the marker has told what it had to.
    marker.done();
    verdict
}

/// How an adapter is listed in the log of a try.
fn list_entry(candidate: &Candidate, excluded: &[Key]) -> String {
    if !candidate.presents {
        format!("{}, can't show the window", candidate.label())
    } else if excluded.contains(&candidate.key()) {
        format!("{}, left out", candidate.label())
    } else {
        candidate.label()
    }
}

/// The function eframe gives the adapters to, for one try: ranks them ([`rung_order`]) for the power preference and the name, leaving out those a window could not be made on
/// before and those left out ([`Memory::excluded`]), probes them in that order with the window's own device descriptor ([`choose`], [`test_adapter`]) when the rules say so, logs every adapter and the
/// choice, records the choice in the memory and returns it. When there is none to choose, the error says why in words (eframe shows it, and the start goes on to the next try).
pub fn selector_with(setup: Setup) -> NativeAdapterSelectorMethod {
    Arc::new(move |adapters, surface| {
        let candidates: Vec<Candidate> = adapters.iter().map(|a| Candidate::of(a, surface)).collect();
        let excluded = setup.memory.excluded();
        let listed: Vec<String> = candidates.iter().map(|c| list_entry(c, &excluded)).collect();
        log::info!("graphics adapters (try {}): {}", setup.label, listed.join("; "));
        let mut tests = 0_usize;
        let verdict = choose(&candidates, setup.power, setup.named.as_deref(), setup.rules, &excluded, |i| {
            tests += 1;
            test_listed(&setup, adapters, &candidates, i, tests)
        });
        let chosen = verdict.inspect_err(|why| log::warn!("graphics try {}: {why}", setup.label))?;
        for (i, why) in &chosen.passed_over {
            log::warn!("the window does not draw on {}: {why}", label_of(&candidates, *i));
        }
        let name = label_of(&candidates, chosen.chosen);
        if setup.rules.probe {
            log::info!("drawing on {name} (try {}, power preference {:?})", setup.label, setup.power);
        } else {
            log::warn!("drawing on {name} with no test of it (try {}): no adapter passed the test", setup.label);
        }
        let adapter = adapters.get(chosen.chosen).cloned().ok_or_else(|| "the chosen graphics adapter is not listed".to_owned())?;
        let info = adapter.get_info();
        setup.memory.chose(key_of(&info.name, info.backend));
        Ok(adapter)
    })
}

/// [`selector_with`] for the usual try, with a memory of its own: the rules the window had before it had tries (each adapter checked), except that no adapter passing is an
/// error and not a window on the first adapter (the start goes on to its other tries instead).
pub fn selector(power: PowerPreference, named: Option<String>, device: DeviceFn) -> NativeAdapterSelectorMethod {
    selector_with(Setup { label: "usual", power, named, rules: Rules::USUAL, memory: Arc::default(), device, fault: None, give_up: PROBE_GIVE_UP })
}

/// The instance flags the window starts wgpu with on macOS: `flags` without the check of indirect draw calls, unless
/// [`INDIRECT_ENV`] is set (wgpu then reads it itself).
pub fn instance_flags(flags: InstanceFlags, indirect_env_set: bool) -> InstanceFlags {
    if indirect_env_set { flags } else { flags.difference(InstanceFlags::VALIDATION_INDIRECT_CALL) }
}

/// What the window asks of the environment for its graphics: `WGPU_POWER_PREF`, `WGPU_ADAPTER_NAME`, `WGPU_BACKEND`, `DIACRAFT_GPU_PROBE_FAIL` and whether
/// `WGPU_VALIDATION_INDIRECT_CALL` is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphicsEnv {
    pub power: Option<PowerPreference>,
    pub named: Option<String>,
    pub indirect_set: bool,
    /// What `WGPU_BACKEND` holds, as it was set.
    pub backend: Option<String>,
    /// What `DIACRAFT_GPU_PROBE_FAIL` holds ([`FAULT_ENV`]), as it was set.
    pub fault: Option<String>,
}

impl GraphicsEnv {
    /// The process's environment.
    pub fn read() -> GraphicsEnv {
        GraphicsEnv {
            power: PowerPreference::from_env(),
            named: std::env::var(NAME_ENV).ok(),
            indirect_set: std::env::var_os(INDIRECT_ENV).is_some(),
            backend: std::env::var(BACKEND_ENV).ok(),
            fault: std::env::var(FAULT_ENV).ok(),
        }
    }

    /// The back ends the person named in `WGPU_BACKEND`, or `None` when it is not set or holds nothing: their choice, which no try swaps for another.
    pub fn explicit_backend(&self) -> Option<&str> {
        self.backend.as_deref().map(str::trim).filter(|b| !b.is_empty())
    }
}

/// eframe's wgpu configuration `config` as the window uses it ([`configure`] for [`System::this`]): on macOS and Windows with
/// the [`selector`], on macOS also with the [`instance_flags`]; elsewhere unchanged.
pub fn configuration(config: WgpuConfiguration) -> WgpuConfiguration {
    configure(config, System::this(), &GraphicsEnv::read())
}

/// [`configuration`] for a given system and environment, so every branch can be tested anywhere.
pub fn configure(mut config: WgpuConfiguration, system: System, env: &GraphicsEnv) -> WgpuConfiguration {
    if !system.probes() {
        return config;
    }
    if let WgpuSetup::CreateNew(create_new) = &mut config.wgpu_setup {
        let power = env.power.unwrap_or_else(|| default_power(system));
        create_new.power_preference = power;
        if system == System::MacOs {
            create_new.instance_descriptor.flags = instance_flags(create_new.instance_descriptor.flags, env.indirect_set);
        }
        let setup = Setup {
            label: "usual",
            power,
            named: env.named.clone(),
            rules: Rules::USUAL.with_dx12_first(Rules::dx12_first_for(system, env)),
            memory: Arc::default(),
            device: Arc::clone(&create_new.device_descriptor),
            fault: env.fault.clone(),
            give_up: PROBE_GIVE_UP,
        };
        create_new.native_adapter_selector = Some(selector_with(setup));
    }
    config
}
