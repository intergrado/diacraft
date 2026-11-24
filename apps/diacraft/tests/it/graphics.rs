//! Which graphics adapter draws the window (`graphics`): the order the adapters are tried in, the probe's verdict,
//! the choice past an adapter that fails it, the configuration macOS and Windows get, and the probe on this machine's own GPUs.

use std::cell::RefCell;

use diacraft_desktop::graphics::{
    Candidate, GraphicsEnv, PROBE_COLOUR, PROBE_TIMEOUT, PROBE_WIDTH, Pick, System, adapter_order, configure, default_power, instance_flags, pick,
    probe, probe_verdict, selector,
};
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
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, None), vec![1, 0]);
    assert_eq!(adapter_order(&mac, PowerPreference::HighPerformance, None), vec![0, 1]);
    assert_eq!(adapter_order(&mac, PowerPreference::None, None), vec![0, 1], "no preference keeps wgpu's order");
}

#[test]
fn an_adapter_that_cannot_show_the_window_is_never_tried() {
    let mut mac = the_mac();
    if let Some(intel) = mac.get_mut(1) {
        intel.presents = false;
    }
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, None), vec![0]);
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some("intel")), vec![0], "not even when it is named");
    for c in &mut mac {
        c.presents = false;
    }
    assert!(adapter_order(&mac, PowerPreference::LowPower, None).is_empty());
}

#[test]
fn the_named_adapter_comes_first_then_hardware_then_native_back_ends() {
    let mac = the_mac();
    assert_eq!(adapter_order(&mac, PowerPreference::HighPerformance, Some("  UHD ")), vec![1, 0], "part of the name, any case, trimmed");
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some("radeon")), vec![0, 1]);
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some("")), vec![1, 0], "an empty name names nothing");
    assert_eq!(adapter_order(&mac, PowerPreference::LowPower, Some("nvidia")), vec![1, 0], "a name nothing has changes nothing");

    let mixed = vec![
        gpu("llvmpipe", Backend::Vulkan, DeviceType::Cpu),
        gpu("Radeon (GL)", Backend::Gl, DeviceType::DiscreteGpu),
        gpu("Virtual", Backend::Vulkan, DeviceType::VirtualGpu),
        gpu("Radeon", Backend::Vulkan, DeviceType::DiscreteGpu),
        gpu("Iris", Backend::Vulkan, DeviceType::IntegratedGpu),
    ];
    assert_eq!(adapter_order(&mixed, PowerPreference::LowPower, None), vec![4, 3, 2, 1, 0]);
    assert_eq!(adapter_order(&mixed, PowerPreference::HighPerformance, None), vec![3, 4, 2, 1, 0]);
    assert_eq!(adapter_order(&mixed, PowerPreference::None, None), vec![2, 3, 4, 1, 0], "software last, OpenGL after the native ones");
    assert_eq!(adapter_order(&mixed, PowerPreference::LowPower, Some("llvm")), vec![0, 4, 3, 2, 1], "a name wins over software");
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

    let told = GraphicsEnv { power: Some(PowerPreference::HighPerformance), named: Some("AMD".to_owned()), indirect_set: true };
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

    let told = GraphicsEnv { power: Some(PowerPreference::LowPower), named: None, indirect_set: false };
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
    assert!(!System::Other.probes(), "Linux and the BSDs keep eframe's choice");
}

#[test]
fn a_windows_laptop_passes_over_a_gpu_that_fails_through_both_its_back_ends() {
    // wgpu lists the Vulkan adapters before the DirectX 12 ones, each GPU once in each, then OpenGL, and the software rasteriser.
    let laptop = vec![
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU", Backend::Vulkan, DeviceType::DiscreteGpu),
        gpu("Intel(R) Iris(R) Xe Graphics", Backend::Vulkan, DeviceType::IntegratedGpu),
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU", Backend::Dx12, DeviceType::DiscreteGpu),
        gpu("Intel(R) Iris(R) Xe Graphics", Backend::Dx12, DeviceType::IntegratedGpu),
        gpu("Microsoft Basic Render Driver", Backend::Dx12, DeviceType::Cpu),
        gpu("NVIDIA GeForce RTX 4060 Laptop GPU/PCIe/SSE2", Backend::Gl, DeviceType::Other),
    ];
    let order = adapter_order(&laptop, default_power(System::Windows), None);
    assert_eq!(order, vec![0, 2, 1, 3, 5, 4], "the fast GPU through Vulkan, then DirectX 12, then the other GPU; OpenGL and software last");
    // The discrete GPU fails its first commands whichever back end asks: the window draws on the integrated one through Vulkan.
    let tried = RefCell::new(Vec::new());
    let picked = pick(&order, |i| {
        tried.borrow_mut().push(i);
        if laptop.get(i).is_some_and(|c| c.name.starts_with("NVIDIA")) { Err("ran none of the commands".to_owned()) } else { Ok(()) }
    });
    assert_eq!(picked.map(|p| p.chosen), Some(1));
    assert_eq!(*tried.borrow(), vec![0, 2, 1]);
    // When the discrete GPU works, it is the one eframe would have picked, through the same back end.
    assert_eq!(pick(&order, |_| Ok(())).map(|p| p.chosen), Some(0));
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
