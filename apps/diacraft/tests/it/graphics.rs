//! Which graphics adapter draws the window (`graphics`): the order the adapters are tried in, the probe's verdict,
//! the choice past an adapter that fails it, the configuration macOS and Windows get, and the probe on this machine's own GPUs.

use std::cell::RefCell;
use std::time::Duration;

use diacraft_desktop::graphics::{
    Candidate, FAULT_ENV, Fault, GraphicsEnv, Memory, PROBE_COLOUR, PROBE_GIVE_UP, PROBE_TIMEOUT, PROBE_WIDTH, Pick, Ran, Rules, Setup, System,
    adapter_order, choose, configure, default_power, fault_for, instance_flags, key_of, pick, probe, probe_verdict, run_within, selector,
    selector_with,
};
use diacraft_desktop::marker::{self, Store};
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use eframe::wgpu::{self, Backend, DeviceType, InstanceFlags, PowerPreference};

fn gpu(name: &str, backend: Backend, device_type: DeviceType) -> Candidate {
    Candidate { name: name.to_owned(), backend, device_type, presents: true }
}

/// The MacBook Pro the project tested on: wgpu lists the discrete GPU first.
fn the_mac() -> Vec<Candidate> {
    vec![
        gpu("AMD Radeon Pro 5500M", Backend::Metal, DeviceType::DiscreteGpu),
        gpu("Intel(R) UHD Graphics 630", Backend::Metal, DeviceType::IntegratedGpu),
    ]
}

#[test]
fn a_mac_tries_the_power_saving_gpu_first_unless_told_otherwise() {
    assert_eq!(default_power(System::MacOs), PowerPreference::LowPower);
    assert_eq!(default_power(System::Windows), PowerPreference::HighPerformance, "Windows keeps eframe's preference");
    assert_eq!(default_power(System::Other), PowerPreference::HighPerformance, "and so do the other systems");
    let mac = the_mac();
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, None, false), vec![1, 0]);
    assert_eq!(adapter_order(&mac, PowerPreference::HighPerformance, None, false), vec![0, 1]);
    assert_eq!(adapter_order(&mac, PowerPreference::None, None, false), vec![0, 1], "no preference keeps wgpu's order");
}

#[test]
fn an_adapter_that_cannot_show_the_window_is_never_tried() {
    let mut mac = the_mac();
    if let Some(intel) = mac.get_mut(1) {
        intel.presents = false;
    }
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, None, false), vec![0]);
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some("intel"), false), vec![0], "not even when it is named");
    for c in &mut mac {
        c.presents = false;
    }
    assert!(adapter_order(&mac, PowerPreference::LowPower, None, false).is_empty());
}

#[test]
fn the_named_adapter_comes_first_then_hardware_then_native_back_ends() {
    let mac = the_mac();
    assert_eq!(adapter_order(&mac, PowerPreference::HighPerformance, Some("  UHD "), false), vec![1, 0], "part of the name, any case, trimmed");
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some("radeon"), false), vec![0, 1]);
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some(""), false), vec![1, 0], "an empty name names nothing");
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some("nvidia"), false), vec![1, 0], "a name nothing has changes nothing");

    let mixed = vec![
        gpu("llvmpipe", Backend::Vulkan, DeviceType::Cpu),
        gpu("Radeon (GL)", Backend::Gl, DeviceType::DiscreteGpu),
        gpu("Virtual", Backend::Vulkan, DeviceType::VirtualGpu),
        gpu("Radeon", Backend::Vulkan, DeviceType::DiscreteGpu),
        gpu("Iris", Backend::Vulkan, DeviceType::IntegratedGpu),
    ];
    assert_eq!(adapter_order(&mixed, PowerPreference::LowPower, None, false), vec![4, 3, 2, 1, 0]);
    assert_eq!(adapter_order(&mixed, PowerPreference::HighPerformance, None, false), vec![3, 4, 2, 1, 0]);
    assert_eq!(adapter_order(&mixed, PowerPreference::None, None, false), vec![2, 3, 4, 1, 0], "software last, OpenGL after the native ones");
    assert_eq!(adapter_order(&mixed, PowerPreference::LowPower, Some("llvm"), false), vec![0, 4, 3, 2, 1], "a name wins over software");
}

#[test]
fn the_window_passes_over_a_gpu_that_runs_nothing() {
    // The Mac's AMD GPU fails the probe and the Intel one passes: in either order, the Intel one draws.
    let tried = RefCell::new(Vec::new());
    let probe = |i: usize| {
        tried.borrow_mut().push(i);
        if i == 0 { Err("ran none of the commands".to_owned()) } else { Ok(()) }
    };
    let picked = pick(&[0, 1], probe);
    assert_eq!(picked, Some(Pick { chosen: 1, passed_over: vec![(0, "ran none of the commands".to_owned())], none_passed: false }));
    assert_eq!(*tried.borrow(), vec![0, 1]);

    tried.borrow_mut().clear();
    assert_eq!(pick(&[1, 0], probe), Some(Pick { chosen: 1, passed_over: Vec::new(), none_passed: false }));
    assert_eq!(*tried.borrow(), vec![1], "the first that passes ends the search: no other GPU is woken");
}

#[test]
fn when_no_gpu_passes_the_first_is_used_anyway_and_none_is_no_choice() {
    let mut calls = 0;
    let picked = pick(&[2, 0, 1], |i| {
        calls += 1;
        Err(format!("failed {i}"))
    });
    let expected = vec![(2, "failed 2".to_owned()), (0, "failed 0".to_owned()), (1, "failed 1".to_owned())];
    assert_eq!(picked, Some(Pick { chosen: 2, passed_over: expected, none_passed: true }));
    assert_eq!(calls, 3, "each adapter is tried once");
    assert_eq!(pick(&[], |_| Ok(())), None);
}

/// The bytes the probe reads back when every pixel is `pixel`.
fn row(pixel: [u8; 4]) -> Vec<u8> {
    (0..PROBE_WIDTH).flat_map(|_| pixel).collect()
}

#[test]
fn the_probe_passes_only_the_colour_it_cleared_to() {
    assert_eq!(PROBE_WIDTH * 4 % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0, "one row copies without padding");
    assert_eq!(probe_verdict(&row(PROBE_COLOUR)), Ok(()));
    let [r, g, b, a] = PROBE_COLOUR;
    assert_eq!(probe_verdict(&row([r + 1, g - 1, b + 1, a])), Ok(()), "a step of rounding either way is the same colour");

    // What the Mac's AMD GPU gave: a buffer it never wrote.
    let zeros = probe_verdict(&row([0, 0, 0, 0])).unwrap_err();
    assert!(zeros.contains("ran none of the commands") && zeros.contains("[0, 0, 0, 0]"), "{zeros}");

    let mut one_off = row(PROBE_COLOUR);
    if let Some(last_blue) = one_off.get_mut(usize::try_from(PROBE_WIDTH * 4 - 2).unwrap()) {
        *last_blue = b + 2;
    }
    let off = probe_verdict(&one_off).unwrap_err();
    assert!(off.contains("read back as [51, 102, 155, 255]") && !off.contains("ran none"), "{off}");

    let short = probe_verdict(&row(PROBE_COLOUR)[..12]).unwrap_err();
    assert!(short.contains("12 bytes of the 256"), "{short}");
    assert!(probe_verdict(&[]).is_err());
}

#[test]
fn the_check_of_indirect_draws_is_left_out_unless_its_variable_is_set() {
    let flags = InstanceFlags::VALIDATION | InstanceFlags::VALIDATION_INDIRECT_CALL;
    assert_eq!(instance_flags(flags, false), InstanceFlags::VALIDATION);
    assert_eq!(instance_flags(flags, true), flags, "wgpu read the variable itself");
    assert_eq!(instance_flags(InstanceFlags::empty(), false), InstanceFlags::empty());
}

fn create_new(config: &WgpuConfiguration) -> &eframe::egui_wgpu::WgpuSetupCreateNew {
    match &config.wgpu_setup {
        WgpuSetup::CreateNew(create_new) => create_new,
        WgpuSetup::Existing(_) => panic!("eframe's default makes its own device"),
    }
}

#[test]
fn macos_gets_the_selector_and_the_other_systems_but_windows_keep_eframes_choice() {
    let base = || eframe::NativeOptions::default().wgpu_options;
    let base_flags = create_new(&base()).instance_descriptor.flags;
    let base_power = create_new(&base()).power_preference;

    let mac = configure(base(), System::MacOs, &GraphicsEnv::default());
    let set = create_new(&mac);
    assert!(set.native_adapter_selector.is_some());
    assert_eq!(set.power_preference, PowerPreference::LowPower);
    assert_eq!(set.instance_descriptor.flags, base_flags.difference(InstanceFlags::VALIDATION_INDIRECT_CALL));

    let told =
        GraphicsEnv { power: Some(PowerPreference::HighPerformance), named: Some("AMD".to_owned()), indirect_set: true, ..GraphicsEnv::default() };
    let mac = configure(base(), System::MacOs, &told);
    let set = create_new(&mac);
    assert!(set.native_adapter_selector.is_some());
    assert_eq!(set.power_preference, PowerPreference::HighPerformance, "WGPU_POWER_PREF wins");
    assert_eq!(set.instance_descriptor.flags, base_flags);

    let other = configure(base(), System::Other, &told);
    let set = create_new(&other);
    assert!(set.native_adapter_selector.is_none());
    assert_eq!(set.power_preference, base_power);
    assert_eq!(set.instance_descriptor.flags, base_flags);
}

#[test]
fn windows_checks_its_gpus_too_and_keeps_eframes_preference_and_flags() {
    // A Windows laptop with two GPUs has the same risk as the Mac, a GPU that takes the window's commands and runs none, so
    // its adapters are probed too. It keeps eframe's high-performance preference and wgpu's check of indirect draws.
    let base = || eframe::NativeOptions::default().wgpu_options;
    let base_flags = create_new(&base()).instance_descriptor.flags;

    let windows = configure(base(), System::Windows, &GraphicsEnv::default());
    let set = create_new(&windows);
    assert!(set.native_adapter_selector.is_some(), "Windows probes its adapters");
    assert_eq!(set.power_preference, PowerPreference::HighPerformance);
    assert_eq!(set.instance_descriptor.flags, base_flags, "the check of indirect draws stays on: it was the Mac's Intel drivers that failed it");

    let told = GraphicsEnv { power: Some(PowerPreference::LowPower), named: None, indirect_set: false, ..GraphicsEnv::default() };
    let windows = configure(base(), System::Windows, &told);
    let set = create_new(&windows);
    assert!(set.native_adapter_selector.is_some());
    assert_eq!(set.power_preference, PowerPreference::LowPower, "WGPU_POWER_PREF wins");
    assert_eq!(set.instance_descriptor.flags, base_flags);
}

#[test]
fn the_system_is_the_one_built_for() {
    let expected = if cfg!(target_os = "macos") {
        System::MacOs
    } else if cfg!(target_os = "windows") {
        System::Windows
    } else {
        System::Other
    };
    assert_eq!(System::this(), expected);
    assert!(System::MacOs.probes() && System::Windows.probes());
    assert!(!System::Other.probes(), "Linux keeps eframe's choice");
}

#[test]
fn a_windows_laptop_passes_over_a_gpu_that_fails_through_both_its_back_ends() {
    // wgpu lists the Vulkan adapters before the DirectX 12 ones, each GPU once in each, then OpenGL, and the software rasteriser; Windows puts DirectX 12 first.
    let laptop = vec![
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU", Backend::Vulkan, DeviceType::DiscreteGpu),
        gpu("Intel(R) Iris(R) Xe Graphics", Backend::Vulkan, DeviceType::IntegratedGpu),
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU", Backend::Dx12, DeviceType::DiscreteGpu),
        gpu("Intel(R) Iris(R) Xe Graphics", Backend::Dx12, DeviceType::IntegratedGpu),
        gpu("Microsoft Basic Render Driver", Backend::Dx12, DeviceType::Cpu),
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU/PCIe/SSE2", Backend::Gl, DeviceType::Other),
    ];
    let order = adapter_order(&laptop, default_power(System::Windows), None, true);
    assert_eq!(
        order,
        vec![2, 0, 3, 1, 5, 4],
        "the fast GPU through DirectX 12, then through Vulkan, then the other GPU the same way; OpenGL and software last"
    );
    // The discrete GPU fails its first commands whichever back end asks: the window draws on the integrated one through DirectX 12.
    let tried = RefCell::new(Vec::new());
    let picked = pick(&order, |i| {
        tried.borrow_mut().push(i);
        if laptop.get(i).is_some_and(|c| c.name.starts_with("NVIDIA")) { Err("ran none of the commands".to_owned()) } else { Ok(()) }
    });
    assert_eq!(picked.map(|p| p.chosen), Some(3));
    assert_eq!(*tried.borrow(), vec![2, 0, 3]);
    // When the discrete GPU works, it is the one eframe would have picked, through DirectX 12 now.
    assert_eq!(pick(&order, |_| Ok(())).map(|p| p.chosen), Some(2));
    // The order wgpu lists them in is what the other systems and a named back end keep.
    assert_eq!(adapter_order(&laptop, default_power(System::Windows), None, false), vec![0, 2, 1, 3, 5, 4]);
}

/// The Windows 10 laptop of public issue #17, as its log lists the adapters (DiaCraft 0.2.3): Intel UHD through Vulkan was the first test, and the start ended there.
fn the_reporters_laptop() -> Vec<Candidate> {
    vec![
        gpu("Intel(R) UHD Graphics", Backend::Vulkan, DeviceType::IntegratedGpu),
        gpu("Intel(R) UHD Graphics", Backend::Dx12, DeviceType::IntegratedGpu),
        gpu("Microsoft Basic Render Driver", Backend::Dx12, DeviceType::Cpu),
        gpu("Intel(R) UHD Graphics", Backend::Gl, DeviceType::IntegratedGpu),
    ]
}

#[test]
fn on_windows_the_first_test_of_the_reporters_laptop_is_the_intel_gpu_through_directx_12() {
    let laptop = the_reporters_laptop();
    let power = default_power(System::Windows);
    assert_eq!(adapter_order(&laptop, power, None, false), vec![0, 1, 3, 2], "wgpu's order: Vulkan first, which is where 0.2.3 ended");
    let order = adapter_order(&laptop, power, None, true);
    assert_eq!(order, vec![1, 0, 3, 2], "DirectX 12 before Vulkan for the same GPU; OpenGL after the native back ends; the software adapter last");
    assert_eq!(laptop.get(order[0]).map(Candidate::label).as_deref(), Some("Intel(R) UHD Graphics (Dx12, IntegratedGpu)"));
    // The test goes DirectX 12, then Vulkan: with the first one passing, Vulkan is never woken.
    let tried = RefCell::new(Vec::new());
    let verdict = choose(&laptop, power, None, Rules::USUAL.with_dx12_first(true), &[], |i| {
        tried.borrow_mut().push(i);
        Ok(())
    });
    assert_eq!(verdict.map(|p| p.chosen), Ok(1));
    assert_eq!(*tried.borrow(), vec![1], "the Vulkan driver that ended the start is not called at all");
    // When DirectX 12 fails the test, Vulkan is next, then OpenGL.
    tried.borrow_mut().clear();
    let verdict = choose(&laptop, power, None, Rules::USUAL.with_dx12_first(true), &[], |i| {
        tried.borrow_mut().push(i);
        if i == 1 { Err("ran none of the commands".to_owned()) } else { Ok(()) }
    });
    assert_eq!(verdict.map(|p| p.chosen), Ok(0));
    assert_eq!(*tried.borrow(), vec![1, 0]);
}

#[test]
fn directx_12_comes_first_only_for_the_same_kind_of_gpu_and_only_after_what_ranks_higher() {
    let mixed = vec![
        gpu("Radeon", Backend::Vulkan, DeviceType::DiscreteGpu),
        gpu("Iris", Backend::Dx12, DeviceType::IntegratedGpu),
        gpu("Radeon", Backend::Dx12, DeviceType::DiscreteGpu),
        gpu("WARP", Backend::Dx12, DeviceType::Cpu),
        gpu("Iris", Backend::Gl, DeviceType::IntegratedGpu),
    ];
    let high = PowerPreference::HighPerformance;
    assert_eq!(
        adapter_order(&mixed, high, None, true),
        vec![2, 0, 1, 4, 3],
        "the power preference outranks the back end: the discrete GPU through Vulkan is before the integrated one through DirectX 12"
    );
    assert_eq!(adapter_order(&mixed, PowerPreference::LowPower, None, true), vec![1, 2, 0, 4, 3]);
    // A name the person gave comes first, DirectX 12 or not; software and OpenGL keep their places.
    assert_eq!(adapter_order(&mixed, high, Some("radeon"), true), vec![2, 0, 1, 4, 3]);
    assert_eq!(adapter_order(&mixed, high, Some("iris"), true), vec![1, 4, 2, 0, 3], "named: DirectX 12 before OpenGL; then the others");
    // A system with no DirectX 12 has nothing to put first.
    assert_eq!(adapter_order(&mixed[..1], high, None, true), vec![0]);
}

#[test]
fn only_windows_without_a_named_back_end_puts_directx_12_first() {
    let none = GraphicsEnv::default();
    assert!(Rules::dx12_first_for(System::Windows, &none));
    assert!(!Rules::dx12_first_for(System::MacOs, &none));
    assert!(!Rules::dx12_first_for(System::Other, &none));
    for named in ["vulkan", "dx12", "vulkan,dx12", "dx12,vulkan", " gl "] {
        let env = GraphicsEnv { backend: Some(named.to_owned()), ..GraphicsEnv::default() };
        assert!(!Rules::dx12_first_for(System::Windows, &env), "WGPU_BACKEND={named} is the person's choice: wgpu's order stays");
    }
    for blank in ["", "  "] {
        let env = GraphicsEnv { backend: Some(blank.to_owned()), ..GraphicsEnv::default() };
        assert!(Rules::dx12_first_for(System::Windows, &env), "a variable that holds nothing names nothing");
    }
    assert_eq!(
        [Rules::USUAL.dx12_first, Rules::SOFTWARE.dx12_first, Rules::UNTESTED.dx12_first],
        [false; 3],
        "no rule of its own puts it first: the system and the environment decide"
    );
    assert!(Rules::USUAL.with_dx12_first(true).dx12_first);
    assert_eq!(Rules::SOFTWARE.with_dx12_first(true), Rules { software_only: true, probe: true, dx12_first: true });
}

// ---- a test that never returns ----

#[test]
fn the_fault_variable_names_an_adapter_by_a_part_of_its_name_and_what_to_do_to_it() {
    assert_eq!(FAULT_ENV, "DIACRAFT_GPU_PROBE_FAIL");
    let uhd = "Intel(R) UHD Graphics";
    assert_eq!(fault_for("UHD:abort", uhd, Backend::Vulkan), Some(Fault::Abort));
    assert_eq!(fault_for("uhd:HANG", uhd, Backend::Dx12), Some(Fault::Hang), "any case");
    assert_eq!(fault_for("  uhd : hang  ", uhd, Backend::Dx12), Some(Fault::Hang), "blanks around the parts do not count");
    assert_eq!(fault_for("radeon:abort", uhd, Backend::Vulkan), None, "another adapter");
    // The back end, in brackets, as the log writes the adapter: the same GPU through one of its two back ends.
    assert_eq!(fault_for("Intel(R) UHD Graphics (vulkan):abort", uhd, Backend::Vulkan), Some(Fault::Abort));
    assert_eq!(fault_for("Intel(R) UHD Graphics (vulkan):abort", uhd, Backend::Dx12), None);
    assert_eq!(fault_for("(dx12):hang", uhd, Backend::Dx12), Some(Fault::Hang));
    // Nothing that is not understood does anything: no part, no word, no colon, another word.
    for bad in ["", ":abort", " :hang", "UHD", "UHD:", "UHD:crash", "UHD:abort:", "abort", "UHD abort"] {
        assert_eq!(fault_for(bad, uhd, Backend::Vulkan), None, "{bad:?}");
    }
}

#[test]
fn work_that_returns_in_time_gives_its_answer_and_work_that_panics_is_caught_on_its_thread() {
    assert_eq!(run_within(Duration::from_secs(30), || 7), Ran::Done(7));
    match run_within(Duration::from_secs(30), || -> u32 { panic!("a driver panicked") }) {
        Ran::Failed(why) => assert!(why.contains("a driver panicked"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn work_that_blocks_is_given_up_on_at_the_time_limit_and_the_start_goes_on() {
    // The fake probe: a call that does not return until it is told to (a driver call that blocks).
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let begun = std::time::Instant::now();
    let ran = run_within(Duration::from_millis(200), move || {
        let _ = blocked.recv();
        "late"
    });
    let waited = begun.elapsed();
    assert_eq!(ran, Ran::TimedOut);
    assert!(waited >= Duration::from_millis(200) && waited < Duration::from_secs(10), "waited {waited:?}");
    // Let the thread end, so that no test leaves it behind. Its answer goes nowhere.
    drop(release);
    // The next piece of work is not held up by the first.
    assert_eq!(run_within(Duration::from_secs(30), || "next"), Ran::Done("next"));
}

#[test]
fn a_blocked_probe_in_a_choice_passes_the_adapter_over_and_leaves_it_out_of_every_later_try() {
    // `choose` with a fake probe: the first adapter's test blocks, and is given up on as `selector_with` does; the second passes. What is given up on is left out of the next try.
    let laptop = the_reporters_laptop();
    let memory = Memory::default();
    let power = default_power(System::Windows);
    let rules = Rules::USUAL.with_dx12_first(true);
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let blocked = std::sync::Mutex::new(Some(blocked));
    let picked = choose(&laptop, power, None, rules, &memory.excluded(), |i| {
        let first = i == 1;
        let held = if first { blocked.lock().unwrap().take() } else { None };
        match run_within(Duration::from_millis(150), move || {
            if let Some(held) = held {
                let _ = held.recv();
            }
        }) {
            Ran::TimedOut => {
                memory.leave_out(&laptop[i].key());
                Err("it did not answer".to_owned())
            }
            Ran::Done(()) => Ok(()),
            Ran::Failed(why) => Err(why),
        }
    })
    .unwrap();
    assert_eq!(picked.chosen, 0, "the next adapter of the order is chosen");
    assert_eq!(picked.passed_over, vec![(1, "it did not answer".to_owned())]);
    assert_eq!(memory.left_out(), vec![key_of("Intel(R) UHD Graphics", Backend::Dx12)]);
    // The next try does not offer it again, even when the same GPU is listed through another instance.
    let next = diacraft_desktop::graphics::rung_order(&laptop, power, None, rules, &memory.excluded());
    assert_eq!(next, vec![0, 3, 2]);
    drop(release);
}

fn real_adapters() -> Vec<wgpu::Adapter> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    descriptor.backends = wgpu::Backends::PRIMARY | wgpu::Backends::GL;
    let instance = wgpu::Instance::new(descriptor);
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY | wgpu::Backends::GL))
}

/// A scratch folder that goes away with the test.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(what: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("diacraft-gpu-{what}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn named_setup(name: &str, fault: Option<String>, memory: &std::sync::Arc<Memory>, give_up: Duration) -> Setup {
    Setup {
        label: "usual",
        power: PowerPreference::HighPerformance,
        named: Some(name.to_owned()),
        rules: Rules::USUAL,
        memory: std::sync::Arc::clone(memory),
        device: window_device(),
        fault,
        give_up,
    }
}

/// With a hang made in the first adapter of this machine's order (`DIACRAFT_GPU_PROBE_FAIL=<name>:hang`), the selector gives up on its test, says so, leaves it out for the rest of
/// the start and for the next one (the list file), removes its marker, and goes on to the other adapters. A machine with no GPU wgpu can use has nothing to check.
#[test]
fn the_selector_gives_up_on_an_adapter_whose_test_hangs_and_remembers_it() {
    crate::capture_logs();
    let adapters = real_adapters();
    let Some(first) = adapters.first() else {
        eprintln!("no GPU wgpu can use here: nothing to check");
        return;
    };
    let info = first.get_info();
    let scratch = Scratch::new("hang");
    let memory = std::sync::Arc::new(Memory::new(Store::new(Some(scratch.0.clone()), "0.2.4"), Vec::new()));
    let setup = named_setup(&info.name, Some(format!("{}:hang", info.name)), &memory, Duration::from_millis(400));
    let begun = std::time::Instant::now();
    let verdict = selector_with(setup)(&adapters, None);
    let waited = begun.elapsed();
    eprintln!("{} adapters; chose {:?} after {waited:?}", adapters.len(), verdict.as_ref().map(|a| a.get_info().name));
    assert!(waited < Duration::from_secs(60), "the start must not wait for ever: {waited:?}");
    let left_out = memory.left_out();
    // Every adapter of that name that was tested was given up on, and the choice went on past them: whatever is chosen is not one of them.
    assert!(!left_out.is_empty() && left_out.iter().all(|k| k.0.contains(&key_of(&info.name, info.backend).0)), "{left_out:?}");
    if let Ok(chosen) = &verdict {
        let chosen = chosen.get_info();
        assert!(!left_out.contains(&key_of(&chosen.name, chosen.backend)), "chose an adapter that was left out");
    }
    let marker_files: Vec<_> = std::fs::read_dir(&scratch.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(marker_files.iter().all(|n| !n.starts_with(marker::MARKER_PREFIX)), "the marker is removed: {marker_files:?}");
    assert!(marker_files.contains(&marker::LEFT_OUT_FILE.to_owned()), "the next start leaves it out: {marker_files:?}");
    assert_eq!(Store::new(Some(scratch.0.clone()), "0.2.4").open(), left_out);
    let log = crate::logged();
    assert!(log.iter().any(|l| l.contains("testing ") && l.contains(&info.name) && l.contains("(try usual, test 1)")), "{log:?}");
    assert!(log.iter().any(|l| l.contains("a driver call is blocked")), "{log:?}");
}

/// A crash is not a result a test of ours can read, so the test binary runs itself: the child below makes the first adapter's test abort the process (`DIACRAFT_GPU_PROBE_FAIL=<name>:abort`), and this test
/// reads what the dead start left in the settings folder, as the next start does. The adapter is left out by it, which is what the reporter's laptop needed. Unix only: the abort of a Windows process
/// opens its own report dialog on some machines.
#[cfg(unix)]
#[test]
fn a_start_that_aborts_during_a_test_leaves_a_marker_and_the_next_start_leaves_that_adapter_out() {
    crate::capture_logs();
    if real_adapters().is_empty() {
        eprintln!("no GPU wgpu can use here: nothing to check");
        return;
    }
    let scratch = Scratch::new("abort");
    // `ulimit -c 0` so that the abort leaves no core file of a process that has a GPU driver in it.
    let status = std::process::Command::new("sh")
        .args([
            "-c",
            "ulimit -c 0; exec \"$0\" \"$@\"",
            &std::env::current_exe().unwrap().to_string_lossy(),
            "--exact",
            "graphics::the_child_of_the_abort_test",
            "--nocapture",
        ])
        .env("DIACRAFT_TEST_ABORT_DIR", &scratch.0)
        .env("DIACRAFT_NO_PREFS", "1")
        .status()
        .unwrap();
    assert!(!status.success(), "the child was to abort: {status:?}");
    let markers: Vec<_> = std::fs::read_dir(&scratch.0).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(markers.len(), 1, "the dead start left its marker: {markers:?}");
    let text = std::fs::read_to_string(&markers[0]).unwrap();
    let said = marker::parse_file(&text, "testing").unwrap();
    assert_eq!((said.version.as_str(), said.keys.len()), ("9.9.9", 1), "{text}");
    // The next start.
    let left_out = Store::new(Some(scratch.0.clone()), "9.9.9").open();
    assert_eq!(left_out, said.keys, "the adapter the dead start was testing is left out");
    assert!(
        crate::logged().iter().any(|l| l.contains("the last start ended while testing") && l.contains(&marker::describe(&said.keys[0]))),
        "{:?}",
        crate::logged()
    );
    // And it stays left out for the starts after it, until `--reset-graphics`.
    assert_eq!(Store::new(Some(scratch.0.clone()), "9.9.9").open(), said.keys);
    assert_eq!(Store::new(Some(scratch.0.clone()), "9.9.9").reset(), said.keys);
    assert!(Store::new(Some(scratch.0.clone()), "9.9.9").open().is_empty());
}

/// The child of the test above: does nothing unless the parent asks for it with `DIACRAFT_TEST_ABORT_DIR`, and then aborts in the middle of the first adapter's test.
#[test]
fn the_child_of_the_abort_test() {
    let Some(dir) = std::env::var_os("DIACRAFT_TEST_ABORT_DIR") else { return };
    let adapters = real_adapters();
    let Some(first) = adapters.first() else { return };
    let name = first.get_info().name;
    let memory = std::sync::Arc::new(Memory::new(Store::new(Some(dir.into()), "9.9.9"), Vec::new()));
    let setup = named_setup(&name, Some(format!("{name}:abort")), &memory, PROBE_GIVE_UP);
    let _ = selector_with(setup)(&adapters, None);
    // Reached only when the test did not abort: the parent reads that as a start that did not end.
}

/// The device descriptor the window asks for: eframe's.
fn window_device() -> std::sync::Arc<dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> + Send + Sync> {
    std::sync::Arc::clone(&create_new(&eframe::NativeOptions::default().wgpu_options).device_descriptor)
}

/// Runs on this machine's own GPUs: each is probed, and the selector, for either preference, draws on one that passes
/// whenever one does. On the Mac the project tested on, the AMD GPU fails the probe (the window was black on it) and the Intel
/// one passes, so the window draws on the Intel one even when the high-performance GPU is asked for. A machine with no GPU
/// wgpu can use (a CI runner without a Vulkan driver) has nothing to check, and says so.
#[test]
fn the_window_draws_on_a_gpu_that_runs_commands() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY));
    let device = window_device();
    let mut passing = Vec::new();
    for adapter in &adapters {
        let info = adapter.get_info();
        let verdict = probe(adapter, &device(adapter), PROBE_TIMEOUT);
        eprintln!("{} ({:?}, {:?}): {verdict:?}", info.name, info.backend, info.device_type);
        if verdict.is_ok() {
            passing.push(info.name.clone());
        }
    }
    if adapters.is_empty() {
        eprintln!("no GPU wgpu can use here: nothing to check");
        return;
    }
    for power in [PowerPreference::LowPower, PowerPreference::HighPerformance, PowerPreference::None] {
        let chosen = selector(power, None, window_device())(&adapters, None).unwrap();
        let name = chosen.get_info().name;
        eprintln!("{power:?}: draws on {name}");
        if !passing.is_empty() {
            assert!(passing.contains(&name), "{power:?} chose {name}, which failed the probe; these passed: {passing:?}");
        }
    }
}
