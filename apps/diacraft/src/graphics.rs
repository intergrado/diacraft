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
//! preference, keeping wgpu's order among equals. Every adapter and the choice are logged: which GPU draws is the first
//! question of a black window.
//!
//! wgpu's check of indirect draw calls is off on macOS too: egui draws nothing indirectly, and the sibling craft apps found the
//! check's compute shader failing to compile on older Intel Macs' Metal drivers, which lost the device before the window
//! showed. `WGPU_VALIDATION_INDIRECT_CALL=1` turns it back on.
//!
//! **Windows** has the same risk on a laptop with two GPUs, so its adapters are probed and picked the same way, through
//! DirectX 12, Vulkan and OpenGL alike: a GPU that fails its first commands is passed over for the next. It keeps eframe's
//! high-performance preference (wgpu lists each GPU once for Vulkan and once for DirectX 12, Vulkan first, and the order keeps
//! that), so a machine whose GPUs all work draws on the GPU and the back end eframe would have chosen. It keeps the check of
//! indirect draws, which only the Mac's drivers were found to fail. None of this has run on a Windows machine yet.
//!
//! The other systems (Linux, the BSDs) keep eframe's choice: [`configuration`] changes nothing there. The probe makes a device
//! of its own before the window's, which costs time at start, and no GPU there has been found to need it.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use eframe::egui_wgpu::{NativeAdapterSelectorMethod, WgpuConfiguration, WgpuSetup};
use eframe::wgpu::{self, Backend, DeviceType, InstanceFlags, PowerPreference};

/// wgpu's variable for choosing an adapter by (part of) its name, any case.
pub const NAME_ENV: &str = "WGPU_ADAPTER_NAME";

/// wgpu's variable that turns its check of indirect draw calls on (`1`) or off (`0`).
pub const INDIRECT_ENV: &str = "WGPU_VALIDATION_INDIRECT_CALL";

/// The colour the probe clears its texture to, as 8-bit RGBA. Each channel is a multiple of 51, so it is exact as a fraction
/// of 255 and survives the conversion to and from the GPU's floats.
pub const PROBE_COLOUR: [u8; 4] = [51, 102, 153, 255];

/// The probe texture's width in pixels: one row of 256 bytes, wgpu's alignment for a copy into a buffer.
pub const PROBE_WIDTH: u32 = 64;

/// How long the probe waits for the GPU. A GPU that has not cleared 64 pixels by then is not one to draw a window on.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

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
}

/// The system the window runs on, as far as the choice of a GPU goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    MacOs,
    Windows,
    /// Linux, the BSDs and any other.
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
/// by `power`. The sort is stable, so equals keep wgpu's order.
pub fn adapter_order(candidates: &[Candidate], power: PowerPreference, named: Option<&str>) -> Vec<usize> {
    let named = named.map(str::trim).filter(|n| !n.is_empty()).map(str::to_lowercase);
    let mut order: Vec<(usize, &Candidate)> = candidates.iter().enumerate().filter(|(_, c)| c.presents).collect();
    order.sort_by_key(|(_, c)| {
        let unnamed = named.as_ref().is_some_and(|n| !c.name.to_lowercase().contains(n.as_str()));
        (unnamed, c.device_type == DeviceType::Cpu, c.backend == Backend::Gl, power_rank(c.device_type, power))
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

/// The function eframe gives the adapters to: ranks them ([`adapter_order`]) for `power` and `named`, probes them in that
/// order with the window's own `device` descriptor ([`pick`], [`probe`]), logs every adapter and the choice, and returns it.
pub fn selector(
    power: PowerPreference,
    named: Option<String>,
    device: Arc<dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> + Send + Sync>,
) -> NativeAdapterSelectorMethod {
    Arc::new(move |adapters, surface| {
        let candidates: Vec<Candidate> = adapters.iter().map(|a| Candidate::of(a, surface)).collect();
        let listed: Vec<String> =
            candidates.iter().map(|c| if c.presents { c.label() } else { format!("{}, can't show the window", c.label()) }).collect();
        log::info!("graphics adapters: {}", listed.join("; "));
        let order = adapter_order(&candidates, power, named.as_deref());
        let chosen = pick(&order, |i| adapters.get(i).map_or_else(|| Err("not listed".to_owned()), |a| probe(a, &device(a), PROBE_TIMEOUT)));
        let label = |i: usize| candidates.get(i).map_or_else(|| format!("adapter {i}"), Candidate::label);
        let Some(chosen) = chosen else {
            return Err(format!("no graphics adapter can show the window ({} listed, power preference {power:?})", adapters.len()));
        };
        for (i, why) in &chosen.passed_over {
            if chosen.none_passed {
                log::error!("{} failed the graphics test: {why}", label(*i));
            } else {
                log::warn!("the window does not draw on {}: {why}", label(*i));
            }
        }
        if chosen.none_passed {
            log::error!("no graphics adapter passed the test, so the window draws on {} anyway and may stay black", label(chosen.chosen));
        } else {
            log::info!("drawing on {} (power preference {power:?})", label(chosen.chosen));
        }
        adapters.get(chosen.chosen).cloned().ok_or_else(|| "the chosen graphics adapter is not listed".to_owned())
    })
}

/// The instance flags the window starts wgpu with on macOS: `flags` without the check of indirect draw calls, unless
/// [`INDIRECT_ENV`] is set (wgpu then reads it itself).
pub fn instance_flags(flags: InstanceFlags, indirect_env_set: bool) -> InstanceFlags {
    if indirect_env_set { flags } else { flags.difference(InstanceFlags::VALIDATION_INDIRECT_CALL) }
}

/// What the window asks of the environment for its graphics: `WGPU_POWER_PREF`, `WGPU_ADAPTER_NAME` and whether
/// `WGPU_VALIDATION_INDIRECT_CALL` is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphicsEnv {
    pub power: Option<PowerPreference>,
    pub named: Option<String>,
    pub indirect_set: bool,
}

impl GraphicsEnv {
    /// The process's environment.
    pub fn read() -> GraphicsEnv {
        GraphicsEnv {
            power: PowerPreference::from_env(),
            named: std::env::var(NAME_ENV).ok(),
            indirect_set: std::env::var_os(INDIRECT_ENV).is_some(),
        }
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
        create_new.native_adapter_selector = Some(selector(power, env.named.clone(), Arc::clone(&create_new.device_descriptor)));
    }
    config
}
