//! The graphics tries a start goes through when the usual one fails (`fallback`, `graphics`; the Windows start that did nothing): the order of the tries for each
//! system, which adapters each try takes, how a failed run is told from one that ran, and the climb from one try to the next.

use std::sync::Arc;

use diacraft_desktop::fallback::{Climb, Rung, Step, climb, configure_rung, ladder, step_of};
use diacraft_desktop::graphics::{Candidate, GraphicsEnv, Memory, Rules, Setup, System, choose, rung_order, selector_with};
use eframe::egui_wgpu::{WgpuConfiguration, WgpuError, WgpuSetup, WgpuSetupCreateNew};
use eframe::wgpu::{self, Backend, Backends, DeviceType, InstanceFlags, PowerPreference};

fn gpu(name: &str, backend: Backend, device_type: DeviceType) -> Candidate {
    Candidate { name: name.to_owned(), backend, device_type, presents: true }
}

/// A Windows laptop as wgpu lists it: each GPU through Vulkan and through DirectX 12, the software renderer, and OpenGL.
fn laptop() -> Vec<Candidate> {
    vec![
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU", Backend::Vulkan, DeviceType::DiscreteGpu),
        gpu("Intel(R) Iris(R) Xe Graphics", Backend::Vulkan, DeviceType::IntegratedGpu),
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU", Backend::Dx12, DeviceType::DiscreteGpu),
        gpu("Intel(R) Iris(R) Xe Graphics", Backend::Dx12, DeviceType::IntegratedGpu),
        gpu("Microsoft Basic Render Driver", Backend::Dx12, DeviceType::Cpu),
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU/PCIe/SSE2", Backend::Gl, DeviceType::Other),
    ]
}

fn asks_for(backend: &str) -> GraphicsEnv {
    GraphicsEnv { backend: Some(backend.to_owned()), ..GraphicsEnv::default() }
}

// ---- the order of the tries ----

#[test]
fn windows_tries_directx_12_then_software_then_opengl_then_any_adapter() {
    assert_eq!(
        ladder(System::Windows, &GraphicsEnv::default()),
        vec![Rung::Usual, Rung::Dx12, Rung::Software, Rung::Gl, Rung::Anyway],
        "the usual try, DirectX 12 alone, wgpu's fallback adapter (WARP on Windows), OpenGL alone, and last an adapter the test passed over"
    );
}

#[test]
fn macos_and_linux_have_the_tries_their_graphics_can_make() {
    // A Mac has Metal only: no DirectX, no software adapter, no OpenGL. An adapter the test passed over is the one thing left to try.
    assert_eq!(ladder(System::MacOs, &GraphicsEnv::default()), vec![Rung::Usual, Rung::Anyway]);
    // Linux keeps eframe's own choice first, then the software adapter (llvmpipe, lavapipe) and OpenGL.
    assert_eq!(ladder(System::Other, &GraphicsEnv::default()), vec![Rung::Usual, Rung::Software, Rung::Gl]);
}

#[test]
fn a_back_end_the_person_asked_for_is_never_swapped_for_another() {
    // WGPU_BACKEND=metal on Windows: no DirectX 12, no software, no OpenGL behind the person's back. The adapter the test passed over is still the person's back end.
    assert_eq!(ladder(System::Windows, &asks_for("metal")), vec![Rung::Usual, Rung::Anyway]);
    assert_eq!(ladder(System::MacOs, &asks_for("metal")), vec![Rung::Usual, Rung::Anyway]);
    assert_eq!(ladder(System::Other, &asks_for("vulkan")), vec![Rung::Usual], "Linux's own choice is the only try there");
    // A variable that holds nothing is no choice.
    assert_eq!(ladder(System::Windows, &asks_for("  ")).len(), 5);
    assert_eq!(ladder(System::Windows, &asks_for("")).len(), 5);
}

#[test]
fn every_try_has_a_name_for_the_log() {
    let all = [Rung::Usual, Rung::Dx12, Rung::Software, Rung::Gl, Rung::Anyway];
    let names: Vec<&str> = all.iter().map(|r| r.describe()).collect();
    for (rung, name) in all.iter().zip(&names) {
        assert!(!name.is_empty(), "{rung:?}");
    }
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "{names:?}");
    assert_eq!(Rung::Usual.label(), "usual");
}

// ---- which adapters a try takes ----

#[test]
fn the_software_try_takes_only_the_adapters_that_run_on_the_cpu() {
    let rules = Rules { software_only: true, probe: true, dx12_first: false };
    assert_eq!(rung_order(&laptop(), PowerPreference::HighPerformance, None, rules, &[]), vec![4], "WARP, and nothing else");
    let gpus_only = vec![gpu("Radeon", Backend::Vulkan, DeviceType::DiscreteGpu), gpu("Iris", Backend::Gl, DeviceType::IntegratedGpu)];
    assert!(rung_order(&gpus_only, PowerPreference::HighPerformance, None, rules, &[]).is_empty(), "no software adapter, no choice");
}

#[test]
fn the_usual_rules_keep_the_order_of_the_first_try() {
    // A machine whose adapters pass keeps this order, unchanged.
    let order = rung_order(&laptop(), PowerPreference::HighPerformance, None, Rules::USUAL, &[]);
    assert_eq!(order, vec![0, 2, 1, 3, 5, 4]);
    assert_eq!(order, diacraft_desktop::graphics::adapter_order(&laptop(), PowerPreference::HighPerformance, None, false));
}

#[test]
fn an_adapter_that_failed_after_it_was_chosen_is_not_chosen_again() {
    let failed = vec![("NVIDIA GeForce RTX 4060 Laptop GPU".to_owned(), Backend::Vulkan)];
    let order = rung_order(&laptop(), PowerPreference::HighPerformance, None, Rules::USUAL, &failed);
    assert_eq!(order, vec![2, 1, 3, 5, 4], "the same GPU through DirectX 12 is another try, and the other GPU stays");
    // A name given by the person does not bring it back.
    let named = rung_order(&laptop(), PowerPreference::HighPerformance, Some("nvidia"), Rules::USUAL, &failed);
    assert_eq!(named.first(), Some(&2));
    assert!(!named.contains(&0));
}

// ---- the choice, with a probe ----

#[test]
fn a_strict_try_gives_up_when_no_adapter_passes_the_test_and_says_why_each_failed() {
    let probes = std::cell::RefCell::new(Vec::new());
    let verdict = choose(&laptop(), PowerPreference::HighPerformance, None, Rules::USUAL, &[], |i| {
        probes.borrow_mut().push(i);
        Err(format!("it ran nothing ({i})"))
    });
    let why = verdict.unwrap_err();
    assert_eq!(*probes.borrow(), vec![0, 2, 1, 3, 5, 4], "each adapter is tried once, in order");
    assert!(why.contains("no graphics adapter passed the test"), "{why}");
    assert!(why.contains("Microsoft Basic Render Driver") && why.contains("it ran nothing (4)"), "{why}");
}

#[test]
fn a_try_that_passes_over_the_test_takes_the_first_adapter_without_probing() {
    let verdict =
        choose(&laptop(), PowerPreference::HighPerformance, None, Rules { software_only: false, probe: false, dx12_first: false }, &[], |_| {
            Err("a probe that must not run".to_owned())
        });
    let picked = verdict.unwrap();
    assert_eq!((picked.chosen, picked.passed_over.len(), picked.none_passed), (0, 0, false));
}

#[test]
fn the_first_adapter_that_passes_is_chosen_past_those_that_fail() {
    let picked =
        choose(&laptop(), PowerPreference::HighPerformance, None, Rules::USUAL, &[], |i| if i == 0 { Err("no".to_owned()) } else { Ok(()) }).unwrap();
    assert_eq!(picked.chosen, 2);
    assert_eq!(picked.passed_over, vec![(0, "no".to_owned())]);
}

#[test]
fn no_adapter_to_choose_from_is_said_in_words() {
    let none = choose(&[], PowerPreference::HighPerformance, None, Rules::USUAL, &[], |_| Ok(())).unwrap_err();
    assert!(none.contains("no graphics adapter can show the window") && none.contains("0 listed"), "{none}");
    let gpus_only = vec![gpu("Radeon", Backend::Vulkan, DeviceType::DiscreteGpu)];
    let software =
        choose(&gpus_only, PowerPreference::HighPerformance, None, Rules { software_only: true, probe: true, dx12_first: false }, &[], |_| Ok(()))
            .unwrap_err();
    assert!(software.contains("1 listed") && software.contains("software"), "{software}");
    let failed = vec![("Radeon".to_owned(), Backend::Vulkan)];
    let left_out = choose(&gpus_only, PowerPreference::HighPerformance, None, Rules::USUAL, &failed, |_| Ok(())).unwrap_err();
    assert!(left_out.contains("failed on an earlier try"), "{left_out}");
}

// ---- what a try remembers ----

#[test]
fn an_adapter_is_remembered_as_failed_once_a_window_could_not_be_made_on_it() {
    let memory = Memory::default();
    assert_eq!(memory.fail_chosen(), None, "nothing was chosen: nothing failed");
    let key = ("Radeon".to_owned(), Backend::Vulkan);
    memory.chose(key.clone());
    assert!(memory.failed().is_empty(), "chosen is not failed yet: the window may open on it");
    assert_eq!(memory.fail_chosen(), Some(key.clone()));
    assert_eq!(memory.failed(), vec![key.clone()]);
    assert_eq!(memory.fail_chosen(), None, "one choice fails once");
    memory.chose(key.clone());
    memory.fail_chosen();
    assert_eq!(memory.failed(), vec![key], "the same adapter is listed once");
}

// ---- the configuration of each try ----

fn create_new(config: &WgpuConfiguration) -> &WgpuSetupCreateNew {
    match &config.wgpu_setup {
        WgpuSetup::CreateNew(create_new) => create_new,
        WgpuSetup::Existing(_) => panic!("eframe's default makes its own device"),
    }
}

fn base() -> WgpuConfiguration {
    eframe::NativeOptions::default().wgpu_options
}

#[test]
fn each_windows_try_asks_wgpu_for_the_back_ends_it_means() {
    let memory = Arc::new(Memory::default());
    let env = GraphicsEnv::default();
    let dx12 = configure_rung(base(), System::Windows, &env, Rung::Dx12, Arc::clone(&memory));
    assert_eq!(create_new(&dx12).instance_descriptor.backends, Backends::DX12);
    let gl = configure_rung(base(), System::Windows, &env, Rung::Gl, Arc::clone(&memory));
    assert_eq!(create_new(&gl).instance_descriptor.backends, Backends::GL);
    for rung in [Rung::Software, Rung::Anyway] {
        let config = configure_rung(base(), System::Windows, &env, rung, Arc::clone(&memory));
        assert_eq!(create_new(&config).instance_descriptor.backends, Backends::PRIMARY | Backends::GL, "{rung:?}: every back end");
    }
    for rung in [Rung::Usual, Rung::Dx12, Rung::Software, Rung::Gl, Rung::Anyway] {
        let config = configure_rung(base(), System::Windows, &env, rung, Arc::clone(&memory));
        assert!(create_new(&config).native_adapter_selector.is_some(), "{rung:?} chooses its own adapter");
        assert_eq!(create_new(&config).power_preference, PowerPreference::HighPerformance, "{rung:?}");
    }
}

#[test]
fn the_usual_try_is_the_configuration_the_window_always_had() {
    let usual = configure_rung(base(), System::Windows, &GraphicsEnv::default(), Rung::Usual, Arc::default());
    let before = diacraft_desktop::graphics::configure(base(), System::Windows, &GraphicsEnv::default());
    assert_eq!(
        create_new(&usual).instance_descriptor.backends,
        create_new(&before).instance_descriptor.backends,
        "the back ends are eframe's and the environment's"
    );
    assert_eq!(create_new(&usual).instance_descriptor.flags, create_new(&before).instance_descriptor.flags);
    assert_eq!(create_new(&usual).power_preference, create_new(&before).power_preference);
    // Linux's usual try is eframe's own choice.
    let linux = configure_rung(base(), System::Other, &GraphicsEnv::default(), Rung::Usual, Arc::default());
    assert!(create_new(&linux).native_adapter_selector.is_none());
    let software = configure_rung(base(), System::Other, &GraphicsEnv::default(), Rung::Software, Arc::default());
    assert!(create_new(&software).native_adapter_selector.is_some(), "a later try on Linux chooses for itself");
}

#[test]
fn a_back_end_the_person_asked_for_stays_the_back_end_of_every_try() {
    let wanted = base();
    let wanted_backends = create_new(&wanted).instance_descriptor.backends;
    let env = asks_for("metal");
    for rung in [Rung::Usual, Rung::Anyway] {
        let config = configure_rung(base(), System::Windows, &env, rung, Arc::default());
        assert_eq!(create_new(&config).instance_descriptor.backends, wanted_backends, "{rung:?} does not replace what WGPU_BACKEND set");
    }
}

#[test]
fn a_mac_leaves_out_the_check_of_indirect_draws_in_every_try() {
    let flags = create_new(&base()).instance_descriptor.flags;
    for rung in [Rung::Usual, Rung::Anyway] {
        let config = configure_rung(base(), System::MacOs, &GraphicsEnv::default(), rung, Arc::default());
        assert_eq!(create_new(&config).instance_descriptor.flags, flags.difference(InstanceFlags::VALIDATION_INDIRECT_CALL), "{rung:?}");
    }
}

// ---- how a run ended ----

fn no_formats() -> eframe::Error {
    eframe::Error::Wgpu(WgpuError::NoSurfaceFormatsAvailable)
}

#[test]
fn a_graphics_error_before_the_app_was_built_is_worth_another_try() {
    assert_eq!(step_of(Ok(Ok(())), false), Step::Done);
    assert_eq!(step_of(Ok(Ok(())), true), Step::Done);
    match step_of(Ok(Err(no_formats())), false) {
        Step::Retry(why) => assert!(why.contains("no valid format for the surface"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_error_after_the_app_was_built_or_not_from_graphics_is_not_retried() {
    assert!(matches!(step_of(Ok(Err(no_formats())), true), Step::Stop(_)), "the window ran: this is not a start that failed");
    match step_of(Ok(Err(eframe::Error::AppCreation("the creator said no".into()))), false) {
        Step::Stop(why) => assert!(why.contains("the creator said no"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_panic_is_never_retried() {
    assert_eq!(step_of(Err("a driver panicked".to_owned()), false), Step::Panic("a driver panicked".to_owned()));
    assert_eq!(step_of(Err("later".to_owned()), true), Step::Panic("later".to_owned()));
}

// ---- the climb ----

#[test]
fn the_climb_ends_at_the_first_window_that_opens() {
    let mut seen = Vec::new();
    let result = climb(&[Rung::Usual, Rung::Dx12, Rung::Gl], |rung| {
        seen.push(rung);
        if rung == Rung::Dx12 { Step::Done } else { Step::Retry(format!("{rung:?} climb-test-failed")) }
    });
    assert_eq!(result, Climb::Opened);
    assert_eq!(seen, vec![Rung::Usual, Rung::Dx12], "the third try is never made");
}

#[test]
fn the_climb_keeps_every_reason_when_nothing_opens() {
    let result = climb(&[Rung::Usual, Rung::Dx12, Rung::Software], |rung| Step::Retry(format!("{rung:?} reason-test")));
    let Climb::Failed(reasons) = result else { panic!("{result:?}") };
    assert_eq!(reasons.len(), 3);
    assert!(reasons[0].contains(Rung::Usual.describe()) && reasons[0].contains("Usual reason-test"), "{reasons:?}");
    assert!(reasons[1].contains(Rung::Dx12.describe()) && reasons[1].contains("Dx12 reason-test"), "{reasons:?}");
    assert!(reasons[2].contains(Rung::Software.describe()), "{reasons:?}");
}

#[test]
fn a_stop_ends_the_climb_at_once() {
    let mut tries = 0;
    let result = climb(&[Rung::Usual, Rung::Dx12], |_| {
        tries += 1;
        Step::Stop("winit could not make a window".to_owned())
    });
    assert_eq!(tries, 1);
    let Climb::Failed(reasons) = result else { panic!("{result:?}") };
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("winit could not make a window"), "{reasons:?}");
}

#[test]
fn a_panic_ends_the_climb_with_the_panic() {
    let mut tries = 0;
    let result = climb(&[Rung::Usual, Rung::Dx12], |_| {
        tries += 1;
        Step::Panic("driver panic".to_owned())
    });
    assert_eq!((tries, result), (1, Climb::Panicked("driver panic".to_owned())));
}

#[test]
fn the_climb_says_each_try_and_each_failure_in_the_log() {
    crate::capture_logs();
    let _ = climb(&[Rung::Usual, Rung::Gl], |rung| Step::Retry(format!("{rung:?} logged-in-the-climb")));
    let log = crate::logged();
    let ours: Vec<&String> = log.iter().filter(|l| l.contains("logged-in-the-climb")).collect();
    assert!(ours.iter().any(|l| l.contains("Usual logged-in-the-climb") && l.contains("going on to")), "{ours:?}");
    assert!(ours.iter().any(|l| l.contains("Gl logged-in-the-climb")), "{ours:?}");
    assert!(log.iter().any(|l| l.contains("graphics try 1 of 2")), "{log:?}");
}

// ---- on this machine's own adapters ----

/// The software try takes a CPU adapter when this machine has one, and says so in words when it has none. Either way it remembers its choice, so a window that
/// fails on it is not offered it again. A machine with no adapter wgpu can use has nothing to check.
#[test]
fn the_software_try_on_this_machines_adapters() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    descriptor.backends = Backends::PRIMARY | Backends::GL;
    let instance = wgpu::Instance::new(descriptor);
    let adapters = pollster::block_on(instance.enumerate_adapters(Backends::PRIMARY | Backends::GL));
    if adapters.is_empty() {
        eprintln!("no GPU wgpu can use here: nothing to check");
        return;
    }
    let device = Arc::clone(&create_new(&base()).device_descriptor);
    let memory = Arc::new(Memory::default());
    let setup = Setup {
        label: "software",
        power: PowerPreference::HighPerformance,
        named: None,
        rules: Rules { software_only: true, probe: true, dx12_first: false },
        memory: Arc::clone(&memory),
        device,
        fault: None,
        give_up: std::time::Duration::from_secs(12),
    };
    let chosen = selector_with(setup)(&adapters, None);
    let has_software = adapters.iter().any(|a| a.get_info().device_type == DeviceType::Cpu);
    match chosen {
        Ok(adapter) => {
            assert_eq!(adapter.get_info().device_type, DeviceType::Cpu);
            let info = adapter.get_info();
            assert_eq!(memory.fail_chosen(), Some((info.name, info.backend)), "the choice is remembered");
        }
        Err(why) => {
            // Software adapters the machine lists may still fail the probe: the sentence says which of the two it was.
            assert!(why.contains("software") || why.contains("no graphics adapter passed the test"), "{why}");
            assert!(has_software || why.contains("software"), "{why}");
            assert_eq!(memory.fail_chosen(), None, "nothing was chosen");
        }
    }
}

#[test]
fn the_status_bar_says_why_the_window_is_slow_when_a_later_try_drew_it() {
    assert_eq!(diacraft_desktop::fallback::notice(Rung::Usual, Some("Radeon (dx12)")), None, "the usual try says nothing");
    let said = diacraft_desktop::fallback::notice(Rung::Software, Some("Microsoft Basic Render Driver (dx12)")).unwrap();
    assert!(
        said.contains("usual graphics did not start") && said.contains("Microsoft Basic Render Driver (dx12)") && said.contains("slow"),
        "{said}"
    );
    let unknown = diacraft_desktop::fallback::notice(Rung::Gl, None).unwrap();
    assert!(unknown.contains("another graphics adapter"), "{unknown}");
    assert!(said.chars().count() < 140, "short enough for a line of the status bar: {said}");
}
