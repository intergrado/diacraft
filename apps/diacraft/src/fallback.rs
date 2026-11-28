//! The graphics tries a start goes through before it gives up (the Windows start that did nothing).
//!
//! The window needs a graphics adapter it can draw on, and which one works depends on the machine: a computer in a virtual machine or a remote desktop, an old or missing
//! driver, a Vulkan driver that is installed and broken. A start that stopped at the first adapter that failed left the person with no window and, on Windows, no message
//! either. So a start that fails to make its window tries again, a [`Rung`] at a time, and only when every try has failed does it say so ([`failure`](crate::failure)):
//!
//! 1. **The usual try** ([`Rung::Usual`]): what the window always did (`graphics`): eframe's choice on Linux; on macOS and Windows the adapters ranked (on Windows DirectX 12 before
//!    Vulkan for the same kind of GPU), each checked to run commands, the first that passes drawn on.
//! 2. **DirectX 12 alone** (Windows): a Vulkan driver that is broken never gets in the way of the DirectX 12 one.
//! 3. **The software adapter** ([`Rung::Software`]): wgpu's fallback adapter, the one that runs on the processor (WARP on Windows, llvmpipe or lavapipe on Linux). Slow, and it
//!    works on a machine with no usable graphics card. A machine with only this adapter opens the window, slowly.
//! 4. **OpenGL alone** ([`Rung::Gl`], Windows and Linux).
//! 5. **Any adapter** ([`Rung::Anyway`], Windows and macOS): the first the window can show on, with no test of it. It is what the window did before it had tries, when no
//!    adapter passed the test, and it is last because it may leave a black window.
//!
//! Between tries the adapter the last one chose is remembered as failed ([`Memory`]), so the next does not choose it again, and so is an adapter whose test never returned (a driver
//! that blocks; the start gives up on it, and the next starts leave it out: [`marker`](crate::marker)). **A back end the person asked for with
//! `WGPU_BACKEND` is never swapped for another** ([`ladder`]): the tries are the usual one and the untested adapter of that back end, and the failure says that the variable
//! is set.
//!
//! A try is a whole run of `eframe::run_native`, whose event loop eframe keeps for the next (`run_and_return`). Only a graphics error before the app was built is tried
//! again ([`step_of`]): an error of a window that was running is not a start that failed, and a panic ends the start.

use std::sync::Arc;

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use eframe::wgpu::Backends;

use crate::graphics::{GraphicsEnv, Memory, PROBE_GIVE_UP, Rules, Setup, System, default_power, instance_flags, selector_with};

/// One try at making the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung {
    /// What the window always did.
    Usual,
    /// DirectX 12 only (Windows).
    Dx12,
    /// The adapters that run on the processor only: wgpu's fallback adapter.
    Software,
    /// OpenGL only.
    Gl,
    /// The first adapter the window can show on, with no test.
    Anyway,
}

impl Rung {
    /// A word for the log.
    pub const fn label(self) -> &'static str {
        match self {
            Rung::Usual => "usual",
            Rung::Dx12 => "dx12",
            Rung::Software => "software",
            Rung::Gl => "gl",
            Rung::Anyway => "anyway",
        }
    }

    /// What the try is, in words for the log and the message box.
    pub const fn describe(self) -> &'static str {
        match self {
            Rung::Usual => "the usual graphics",
            Rung::Dx12 => "DirectX 12 only",
            Rung::Software => "a software adapter",
            Rung::Gl => "OpenGL only",
            Rung::Anyway => "an adapter the test passed over",
        }
    }

    /// The back ends the try asks wgpu for; `None` leaves what eframe and the environment chose.
    fn backends(self) -> Option<Backends> {
        match self {
            Rung::Usual => None,
            Rung::Dx12 => Some(Backends::DX12),
            Rung::Gl => Some(Backends::GL),
            Rung::Software | Rung::Anyway => Some(Backends::PRIMARY | Backends::GL),
        }
    }

    /// How the try chooses among the adapters.
    pub const fn rules(self) -> Rules {
        match self {
            Rung::Usual | Rung::Dx12 | Rung::Gl => Rules::USUAL,
            Rung::Software => Rules::SOFTWARE,
            Rung::Anyway => Rules::UNTESTED,
        }
    }
}

/// The tries for `system`, in order. A back end the person set in `WGPU_BACKEND` is the only one tried: the usual try and, on macOS and Windows, the untested adapter of it.
pub fn ladder(system: System, env: &GraphicsEnv) -> Vec<Rung> {
    let asked = env.explicit_backend().is_some();
    match (system, asked) {
        (System::Windows, false) => vec![Rung::Usual, Rung::Dx12, Rung::Software, Rung::Gl, Rung::Anyway],
        (System::MacOs, _) | (System::Windows, true) => vec![Rung::Usual, Rung::Anyway],
        (System::Other, false) => vec![Rung::Usual, Rung::Software, Rung::Gl],
        (System::Other, true) => vec![Rung::Usual],
    }
}

/// eframe's wgpu configuration `config` for `rung` on `system`: the usual try is [`crate::graphics::configure`]; the others set the back ends they mean (not when the person
/// set `WGPU_BACKEND`), the same power preference and device, and choose their adapter by their own rules, remembering the choice in `memory`.
pub fn configure_rung(mut config: WgpuConfiguration, system: System, env: &GraphicsEnv, rung: Rung, memory: Arc<Memory>) -> WgpuConfiguration {
    if rung == Rung::Usual && !system.probes() {
        return config;
    }
    if let WgpuSetup::CreateNew(create_new) = &mut config.wgpu_setup {
        let power = env.power.unwrap_or_else(|| default_power(system));
        create_new.power_preference = power;
        if system == System::MacOs {
            create_new.instance_descriptor.flags = instance_flags(create_new.instance_descriptor.flags, env.indirect_set);
        }
        if let Some(backends) = rung.backends().filter(|_| env.explicit_backend().is_none()) {
            create_new.instance_descriptor.backends = backends;
        }
        let setup = Setup {
            label: rung.label(),
            power,
            named: env.named.clone(),
            rules: rung.rules().with_dx12_first(Rules::dx12_first_for(system, env)),
            memory,
            device: Arc::clone(&create_new.device_descriptor),
            fault: env.fault.clone(),
            give_up: PROBE_GIVE_UP,
        };
        create_new.native_adapter_selector = Some(selector_with(setup));
    }
    config
}

/// What the status bar says when the window is drawn by a try after the usual one (the person sees a window that can be slow, and is told why): `on` is the adapter it
/// draws on, if known. `None` for the usual try, which says nothing.
pub fn notice(rung: Rung, on: Option<&str>) -> Option<String> {
    if rung == Rung::Usual {
        return None;
    }
    Some(format!(
        "The usual graphics did not start: drawing with {}, which can be slow (the log says why).",
        on.unwrap_or("another graphics adapter")
    ))
}

/// How one try ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The window opened and was closed: the start is over, and well.
    Done,
    /// Graphics could not make the window: worth another try. The error's words.
    Retry(String),
    /// A failure another try cannot mend (no display, the app itself, a window that was running): the start ends. The error's words.
    Stop(String),
    /// A panic: the start ends. Its message.
    Panic(String),
}

/// How a run of `eframe::run_native` ended, as a [`Step`]. `app_started` is whether the app had been built by then: a graphics error after that is the end of a window that was
/// running, and no other try would bring it back.
pub fn step_of(outcome: Result<Result<(), eframe::Error>, String>, app_started: bool) -> Step {
    match outcome {
        Ok(Ok(())) => Step::Done,
        Ok(Err(error @ eframe::Error::Wgpu(_))) if !app_started => Step::Retry(error.to_string()),
        Ok(Err(error)) => Step::Stop(error.to_string()),
        Err(message) => Step::Panic(message),
    }
}

/// How the climb through the tries ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Climb {
    /// A window opened (and was closed).
    Opened,
    /// No window could be made: each try's reason, in order.
    Failed(Vec<String>),
    /// A panic ended the start.
    Panicked(String),
}

/// Makes each try of `rungs` in turn with `run`, until a window opens or a try ends the start. Each try and each failure is logged.
pub fn climb(rungs: &[Rung], mut run: impl FnMut(Rung) -> Step) -> Climb {
    let total = rungs.len();
    let mut reasons = Vec::new();
    for (n, rung) in rungs.iter().copied().enumerate() {
        log::info!("graphics try {} of {total}: {}", n + 1, rung.describe());
        match run(rung) {
            Step::Done => return Climb::Opened,
            Step::Retry(why) => {
                match rungs.get(n + 1) {
                    Some(next) => log::warn!("graphics try {} ({}) failed: {why}; going on to {}", n + 1, rung.describe(), next.describe()),
                    None => log::error!("graphics try {} ({}) failed: {why}; no try is left", n + 1, rung.describe()),
                }
                reasons.push(format!("{}: {why}", rung.describe()));
            }
            Step::Stop(why) => {
                log::error!("the start ended on graphics try {} ({}): {why}", n + 1, rung.describe());
                reasons.push(why);
                return Climb::Failed(reasons);
            }
            Step::Panic(message) => return Climb::Panicked(message),
        }
    }
    Climb::Failed(reasons)
}
